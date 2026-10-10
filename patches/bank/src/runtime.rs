//! Main-thread hooks for the reviewed Bank task machine.
//!
//! Native game writes are guarded by a durable local journal and exact image checks.
//! See docs/bank.md for the original application flow.

use crate::{
    native_bank::NativeBank,
    native_game::{GameIoDescriptor, GameKind, NativeGameError, NativeGameSession, NativePoll},
    storage_worker::{BankWorker, GameEvidence, Job, Reply},
};
use bank_common::{
    bank_files::Loaded,
    task::{
        checked_pointer, read_word, storage_activity, write_word, NotNative, Notice, Task,
        FS_SESSION, TASK_LOAD, TASK_OPEN, TASK_REWARDS, TASK_REWARD_GUARD, TASK_SAVE,
    },
    worker::{JobId, WorkerError},
};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    sync::atomic::{AtomicBool, Ordering},
};
use offline_core::{
    native_blob::NativeBlobView,
    rewards::{self, Currency, Date, Entry, Receipt, RewardError, Stored},
    sections::{DEX, MILES, RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS},
    Phase, RecoveryDecision,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Fault {
    None = 0,
    Reentrant = 1,
    NativeObject = 2,
    StorageOpen = 3,
    StorageCreate = 4,
    StorageInvalid = 5,
    Clock = 6,
    RecoveryRequired = 7,
    StorageLoad = 9,
    GameRead = 10,
    GameChanged = 11,
    GamePrepare = 12,
    GameWrite = 13,
    GameSecureValue = 14,
    JournalPrepare = 15,
    JournalRecovery = 16,
    TransferRolledBack = 17,
    RewardState = 18,
    RewardClaim = 19,
    /// Test builds: a Save and Quit stopped on purpose.
    #[cfg(feature = "test-build")]
    TestStop = crate::storage_worker::TEST_STOP,
}

impl From<NotNative> for Fault {
    fn from(_: NotNative) -> Self {
        Self::NativeObject
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Idle,
    Open,
    Introduce,
    Initialize,
    Recover,
    NoGames,
    Load,
    SaveInspect,
    SavePrepare,
    SaveWrite,
    SaveFinalize,
}
struct Runtime {
    worker: Option<BankWorker>,
    game: Option<NativeGameSession>,
    owner: usize,
    step: Step,
    job: Option<JobId>,
    baseline: Option<GameEvidence>,
    save_dex: Option<crate::dex::Evidence>,
    fault: Fault,
    native_result: u32,
    pending: bool,
    notice: Notice,
    no_games: bool,
    rewards: RewardSession,
    /// The native blank transport-box record and its format tag, captured from
    /// the freshly constructed Bank before the first restore.
    blank: Option<([u8; RECORD_SIZE], u8)>,
}
/// Provisional reward state of the loaded session. Only a verified Save and
/// Quit makes any of it durable; a new load or a cancellation replaces it.
#[derive(Clone, Copy)]
struct RewardSession {
    entry: Option<Entry>,
    settlement: Option<Stored>,
    claimed: bool,
}
impl RewardSession {
    const NONE: Self = Self {
        entry: None,
        settlement: None,
        claimed: false,
    };
}
/// State of the native claim task immediately before its redemption step.
#[derive(Clone, Copy)]
struct ClaimProbe {
    quoted: u32,
    choice: u32,
    balance: u32,
    gen7: bool,
    gifts: u32,
}
struct Shared(UnsafeCell<Runtime>);
// SAFETY: only the main task thread acquires Guard. The storage worker has a
// separate mailbox/state and never receives Runtime or any live UI reference.
unsafe impl Sync for Shared {}
static BUSY: AtomicBool = AtomicBool::new(false);
static STATE: Shared = Shared(UnsafeCell::new(Runtime {
    worker: None,
    game: None,
    owner: 0,
    step: Step::Idle,
    job: None,
    baseline: None,
    save_dex: None,
    fault: Fault::None,
    native_result: 0,
    pending: false,
    notice: Notice::new(),
    no_games: false,
    rewards: RewardSession::NONE,
    blank: None,
}));
struct Guard;
impl Guard {
    fn acquire() -> Option<Self> {
        BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self)
    }
    fn state(&mut self) -> &mut Runtime {
        // SAFETY: this guard is the only access path while BUSY is held.
        unsafe { &mut *STATE.0.get() }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// Puts the local date where the task's session keeps the server's.
fn local_date(task: Task) -> Result<(), Fault> {
    let session = task.pointer(0x28)?;
    unsafe {
        if bank_offline_timestamp(core::ptr::null_mut(), session.add(0x28)) == 0 {
            return Err(Fault::Clock);
        }
        // Existing native free-access display path. Local time is used for
        // visible creation dates; rewards use the shared local accounting.
        session.add(0x44).write(1);
        session.add(0x4f).write(1);
        write_word(session, 0x3c, 0);
        write_word(session, 0x40, 0);
    }
    Ok(())
}
/// Counts the Pokémon of the selected game and of the Bank into the
/// coordinator, as the original does for its transfer record.
fn capture_counts(task: Task) -> Result<(), Fault> {
    let coordinator = task.pointer(8)?;
    let bank = task.bank_pointer()?;
    unsafe {
        if coordinator.add(0xc8).read() != 0 {
            let selected: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
                transmute(0x0023_3a6cusize);
            let game = selected(coordinator);
            checked_pointer(game as u32)?;
            let kind = game.add(8).read();
            if !(1..=8).contains(&kind) {
                return Err(Fault::NativeObject);
            }
            let boxes = game.add(if kind >= 5 { 0x769dc } else { 0x1e51c });
            let vtable = checked_pointer(read_word(boxes, 0))?;
            let address = read_word(vtable, 0xc);
            if address & 3 != 0 || !(0x100000..0x313910).contains(&address) {
                return Err(Fault::NativeObject);
            }
            let heap: unsafe extern "aapcs" fn(u32) -> u32 = transmute(0x0023_5bfcusize);
            let count: unsafe extern "aapcs" fn(*mut u8, u32, u32) -> u32 =
                transmute(address as usize);
            let game_count = count(boxes, heap(0x17), 1);
            write_word(coordinator, 0xf4, game_count);
        }
        let helper = checked_pointer(read_word(bank, 0xbb520))?;
        let count: unsafe extern "aapcs" fn(*mut u8) -> u32 = transmute(0x001d_5ec0usize);
        write_word(coordinator, 0xf8, count(helper));
    }
    Ok(())
}
fn today() -> Result<Date, Fault> {
    let mut words = [0u32; 2];
    // Native 0023a754 preserves upper flag bits, so the words start zeroed.
    let local_date: unsafe extern "aapcs" fn(*mut u32) = unsafe { transmute(0x0023_a754usize) };
    unsafe { local_date(words.as_mut_ptr()) };
    crate::local_clock::local_day(words).ok_or(Fault::Clock)
}
fn reward_fault(error: RewardError) -> Fault {
    match error {
        RewardError::InvalidDate => Fault::Clock,
        _ => Fault::RewardState,
    }
}
/// The selected game object and whether it is a Gen 7 title.
fn selected_game(task: Task) -> Result<(*mut u8, bool), Fault> {
    let coordinator = task.pointer(8)?;
    if unsafe { coordinator.add(0xc8).read() } == 0 {
        return Err(Fault::NativeObject);
    }
    let selected: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
        unsafe { transmute(0x0023_3a6cusize) };
    let game = unsafe { selected(coordinator) };
    checked_pointer(game as u32)?;
    let kind = unsafe { game.add(8).read() };
    if !(1..=8).contains(&kind) {
        return Err(Fault::NativeObject);
    }
    Ok((game, kind >= 5))
}
// Native 001d58b0: 48 slots of 0x108 bytes; a nonzero title halfword is used.
const GEN7_GIFTS: usize = 0x000a_d43c;
fn gen7_slot(game: *mut u8, index: u32) -> *mut u8 {
    unsafe { game.add(GEN7_GIFTS + index as usize * 0x108 + 0x104) }
}
fn gen7_gift_count(game: *mut u8) -> u32 {
    (0..rewards::GEN7_GIFT_SLOTS)
        .filter(
            |&index| unsafe { gen7_slot(game, index).add(2).cast::<u16>().read_unaligned() } != 0,
        )
        .count() as u32
}
/// The Gen 6 pending-gift buffer returned by the game's own accessor, as in
/// native 002a9ed8..002a9ef0.
fn gen6_gift(game: *mut u8) -> Result<*mut u8, Fault> {
    let owner = unsafe { game.add(0x1c384) };
    let vtable = checked_pointer(unsafe { read_word(owner, 0) })?;
    let address = unsafe { read_word(vtable, 8) };
    if address & 3 != 0 || !(0x100000..0x313910).contains(&address) {
        return Err(Fault::NativeObject);
    }
    let buffer: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
        unsafe { transmute(address as usize) };
    let gift = unsafe { buffer(owner) };
    checked_pointer(gift as u32)?;
    Ok(gift)
}
impl Runtime {
    fn report_failure(&mut self, task: Task) -> u32 {
        self.notice
            .report(task, self.fault as u32, self.native_result)
    }
    fn worker_error(&mut self, error: WorkerError) -> Fault {
        self.native_result = error.native_result();
        match error.fault_code() {
            2 => Fault::NativeObject,
            3 => Fault::StorageOpen,
            4 => Fault::StorageCreate,
            7 => Fault::RecoveryRequired,
            9 => Fault::StorageLoad,
            10 => Fault::GameRead,
            11 => Fault::GameChanged,
            12 => Fault::GamePrepare,
            13 => Fault::GameWrite,
            14 => Fault::GameSecureValue,
            15 => Fault::JournalPrepare,
            16 => Fault::JournalRecovery,
            17 => Fault::TransferRolledBack,
            #[cfg(feature = "test-build")]
            crate::storage_worker::TEST_STOP => Fault::TestStop,
            _ => Fault::StorageInvalid,
        }
    }
    fn worker(&mut self) -> Result<&mut BankWorker, Fault> {
        if self.worker.is_none() {
            self.worker = Some(unsafe { BankWorker::start() }.map_err(|e| self.worker_error(e))?);
        }
        self.worker.as_mut().ok_or(Fault::StorageOpen)
    }
    fn submit(&mut self, task: Task, step: Step, job: Job) -> Result<(), Fault> {
        if self.job.is_some() {
            return Err(Fault::Reentrant);
        }
        // Busy protection retains every archive and frozen native block until poll.
        let result = unsafe { self.worker()?.submit(job) };
        let id = result.map_err(|e| self.worker_error(e))?;
        // Files are being read or written until `complete`.
        storage_activity(true);
        self.owner = task.raw() as usize;
        self.step = step;
        self.job = Some(id);
        Ok(())
    }
    fn poll(&mut self, task: Task) -> Result<Option<Reply>, Fault> {
        if self.owner != task.raw() as usize {
            return Err(Fault::NativeObject);
        }
        let id = self.job.ok_or(Fault::Reentrant)?;
        let result = self.worker()?.poll(id);
        match result {
            Ok(Some(reply)) => {
                self.job = None;
                Ok(Some(reply))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                if self.worker()?.active() {
                    // An uncertain/stale poll never releases an in-flight native
                    // borrow. Keep rendering and retry the same generation.
                    self.native_result = error.native_result();
                    return Ok(None);
                }
                self.job = None;
                Err(self.worker_error(error))
            }
        }
    }
    /// Stages the live native body for the worker, which cuts it into files.
    /// The Miles balance travels separately and is not part of the Bank file.
    fn stage_bank(&mut self, task: Task) -> Result<(), Fault> {
        let bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let snapshot = bank.snapshot().map_err(|_| Fault::NativeObject)?;
        let dex = crate::dex::evidence(snapshot.as_bytes()).map_err(|_| Fault::NativeObject)?;
        let staging = self
            .worker()?
            .staging_mut()
            .map_err(|_| Fault::StorageLoad)?;
        staging.copy_from_slice(snapshot.as_bytes());
        staging[MILES].fill(0);
        if self.step == Step::SaveInspect {
            self.save_dex = Some(dex);
        }
        Ok(())
    }
    /// The native body the worker assembled in staging.
    fn staged_body(&mut self) -> Result<&[u8], Fault> {
        let staging = self.worker()?.staging().map_err(|_| Fault::StorageLoad)?;
        Ok(NativeBlobView::parse_layout(staging)
            .map_err(|_| Fault::StorageLoad)?
            .as_bytes())
    }
    /// Remembers the blank transport record of the freshly constructed Bank.
    fn capture_blank(&mut self, task: Task) -> Result<(), Fault> {
        if self.blank.is_some() {
            return Ok(());
        }
        let bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let body = bank.snapshot().map_err(|_| Fault::NativeObject)?.as_bytes();
        let mut record = [0; RECORD_SIZE];
        record
            .copy_from_slice(&body[TRANSPORT_RECORDS.start..TRANSPORT_RECORDS.start + RECORD_SIZE]);
        self.blank = Some((record, body[TRANSPORT_TAGS.start]));
        Ok(())
    }
    /// Fills regions no file supplied with what a new Bank has: the live
    /// Pokédex defaults and blank transport records.
    fn fill_defaults(&mut self, task: Task, loaded: Loaded) -> Result<(), Fault> {
        // A delivery is the complete box Transporter built and needs no fill.
        let first = if loaded.transport_missing {
            0
        } else {
            TRANSPORT_SLOTS
        };
        if !loaded.dex_missing && first == TRANSPORT_SLOTS {
            return Ok(());
        }
        let (record, tag) = self.blank.ok_or(Fault::NativeObject)?;
        let bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let live = bank.snapshot().map_err(|_| Fault::NativeObject)?.as_bytes();
        let staging = self
            .worker()?
            .staging_mut()
            .map_err(|_| Fault::StorageLoad)?;
        if loaded.dex_missing {
            staging[DEX].copy_from_slice(&live[DEX]);
        }
        for slot in first..TRANSPORT_SLOTS {
            let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
            staging[at..at + RECORD_SIZE].copy_from_slice(&record);
            staging[TRANSPORT_TAGS.start + slot] = tag;
        }
        Ok(())
    }
    /// Restores the assembled body into the native object.
    fn restore_bank(&mut self, task: Task, loaded: Loaded) -> Result<(), Fault> {
        self.fill_defaults(task, loaded)?;
        let raw = task.bank_pointer()?;
        let expected = crate::dex::evidence(self.staged_body()?).map_err(|_| Fault::StorageLoad)?;
        if self.step == Step::SaveFinalize {
            let saved = self.save_dex.ok_or(Fault::StorageLoad)?;
            if saved.validity_mask != expected.validity_mask
                || saved.family_crc32 != expected.family_crc32
            {
                return Err(Fault::StorageLoad);
            }
        }
        {
            let mut bank = unsafe { NativeBank::from_raw(raw) }.map_err(|_| Fault::NativeObject)?;
            let bytes = self.staged_body()?;
            bank.restore_preserving_detected_games(bytes)
                .map_err(|_| Fault::StorageLoad)?;
        }
        // Native helper buffers live outside the serialized body. Reload all
        // eight family views after the body changes, then invalidate aggregates.
        unsafe { crate::dex::refresh_after_restore(raw) }.map_err(|_| Fault::NativeObject)?;
        let bank = unsafe { NativeBank::from_raw(raw) }.map_err(|_| Fault::NativeObject)?;
        let restored =
            crate::dex::evidence(bank.snapshot().map_err(|_| Fault::NativeObject)?.as_bytes())
                .map_err(|_| Fault::StorageLoad)?;
        // Cache reload may change native display flags, but never the eight
        // family snapshots or their validity mask. Verify this actual boundary.
        if restored.validity_mask != expected.validity_mask
            || restored.family_crc32 != expected.family_crc32
        {
            return Err(Fault::StorageLoad);
        }
        Ok(())
    }
    /// Starts the session's provisional reward state from the stored Miles
    /// record. The new balance is calculated first; only the result is put
    /// into the native object for the original reward screens.
    fn begin_rewards(&mut self, task: Task, stored: Stored) -> Result<(), Fault> {
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let entry =
            rewards::enter(stored.accounting, stored.balance, today()?).map_err(reward_fault)?;
        bank.set_miles(entry.balance());
        self.rewards = RewardSession {
            entry: Some(entry),
            settlement: None,
            claimed: false,
        };
        Ok(())
    }
    /// Reads the clock again at Save and Quit and fixes the Miles state that
    /// must be written with this save's Pokémon changes.
    fn settle_rewards(&mut self, task: Task, saved_count: u32) -> Result<(), Fault> {
        let entry = self.rewards.entry.ok_or(Fault::RewardState)?;
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let balance = bank.miles().map_err(|_| Fault::NativeObject)?;
        let settlement =
            rewards::settle(&entry, balance, saved_count, today()?).map_err(reward_fault)?;
        bank.set_miles(settlement.balance);
        self.rewards.settlement = Some(Stored {
            balance: settlement.balance,
            accounting: Some(settlement.accounting),
        });
        Ok(())
    }
    fn probe_claim(&mut self, task: Task) -> Result<ClaimProbe, Fault> {
        let (game, gen7) = selected_game(task)?;
        let bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        Ok(ClaimProbe {
            quoted: unsafe { read_word(task.raw(), 0x50) },
            choice: unsafe { read_word(task.raw(), 0x60) },
            balance: bank.miles().map_err(|_| Fault::NativeObject)?,
            gen7,
            gifts: if gen7 { gen7_gift_count(game) } else { 0 },
        })
    }
    /// Checks what native claim state 0xb did. Only its completed redemption
    /// (state 0xc) may change the balance, and then exactly as verified.
    fn check_claim(&mut self, task: Task, probe: ClaimProbe, step: u32) -> Result<(), Fault> {
        if step == 0xb {
            return Ok(()); // still fading the choice dialog; nothing ran yet
        }
        let (game, gen7) = selected_game(task)?;
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let balance = bank.miles().map_err(|_| Fault::NativeObject)?;
        if step != 0xc {
            // Decline, unsupported currency, or a full Gen 7 gift store.
            if balance != probe.balance {
                bank.set_miles(probe.balance);
                return Err(Fault::RewardClaim);
            }
            return Ok(());
        }
        let verified = (|| {
            if self.rewards.claimed || gen7 != probe.gen7 {
                return Err(RewardError::ReceiptMismatch);
            }
            let currency = match probe.choice {
                0 => Currency::Miles,
                1 => Currency::BattlePoints,
                _ => return Err(RewardError::UnsupportedCurrency),
            };
            let receipt = if gen7 {
                let (mut kind, mut amount) = (0, 0);
                if probe.gifts < rewards::GEN7_GIFT_SLOTS {
                    let slot = gen7_slot(game, probe.gifts);
                    kind = unsafe { slot.add(0x51).read() };
                    amount = unsafe { slot.add(0x68).cast::<u32>().read_unaligned() };
                }
                Receipt::Gen7 {
                    count_before: probe.gifts,
                    count_after: gen7_gift_count(game),
                    kind,
                    amount,
                }
            } else {
                let gift = gen6_gift(game).map_err(|_| RewardError::ReceiptMismatch)?;
                let field = match currency {
                    Currency::Miles => 0x6a2,
                    Currency::BattlePoints => 0x6a0,
                };
                Receipt::Gen6 {
                    flagged: unsafe { gift.add(0x1ff).read() } & 0x80 != 0,
                    amount: unsafe { gift.add(field).cast::<u16>().read_unaligned() },
                }
            };
            rewards::verify_claim(currency, probe.quoted, probe.balance, balance, receipt)
        })();
        match verified {
            Ok(_) => {
                self.rewards.claimed = true;
                Ok(())
            }
            Err(_) => {
                // No confirmed receipt: the debit must not survive in memory.
                bank.set_miles(probe.balance);
                Err(Fault::RewardClaim)
            }
        }
    }
    fn complete(&mut self, task: Task) -> bool {
        self.owner = 0;
        self.step = Step::Idle;
        self.job = None;
        self.game = None;
        self.save_dex = None;
        task.end_busy();
        storage_activity(false);
        true
    }
    /// Creates the Bank file from the original's defaults for a new Bank.
    fn create_bank(&mut self, task: Task) -> Result<(), Fault> {
        // Only an explicit NotFound for our private file permits fresh
        // initialization. Native defaults/translated names stay on UI.
        let defaults: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002a_e8b0usize) };
        unsafe { defaults(task.raw()) };
        // A fresh Bank has no Miles record until its first save.
        self.stage_bank(task)?;
        self.submit(task, Step::Initialize, Job::Initialize)
    }
    /// The welcome the original gives before it creates a Bank. Its own
    /// update runs states 2 and 3 (three messages, each acknowledged) and
    /// leaves state 4, where it would ask the server for the new Bank.
    unsafe fn introduce(&mut self, task: Task) -> Result<bool, Fault> {
        if self.owner != task.raw() as usize {
            return Err(Fault::NativeObject);
        }
        match unsafe { read_word(task.raw(), 0x10) } {
            2 | 3 => {
                let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
                    unsafe { transmute(0x002a_e568usize) };
                unsafe { original(task.raw()) };
                Ok(false)
            }
            4 => {
                storage_activity(true);
                unsafe { bank_common::ui::loading(task.ui()?, bank_common::ui::CREATING_MESSAGE) }
                    .map_err(|_| Fault::NativeObject)?;
                task.begin_busy();
                unsafe { write_word(task.raw(), 0x10, 1) };
                self.create_bank(task)?;
                Ok(false)
            }
            _ => Err(Fault::NativeObject),
        }
    }
    unsafe fn load(&mut self, task: Task) -> Result<bool, Fault> {
        if self.step == Step::Introduce {
            return unsafe { self.introduce(task) };
        }
        if self.step == Step::NoGames {
            if self.owner != task.raw() as usize {
                return Err(Fault::NativeObject);
            }
            return match unsafe { bank_common::ui::acknowledged(task.ui()?) } {
                Ok(false) => Ok(false),
                Ok(true) => Ok(self.complete(task)),
                Err(_) => Err(Fault::NativeObject),
            };
        }
        if self.step == Step::Idle {
            local_date(task)?;
            let opening = unsafe { read_word(task.raw(), 0) } == TASK_OPEN;
            // The original shows this message for both: opening an existing
            // Bank and loading it for the chosen game.
            unsafe { bank_common::ui::loading(task.ui()?, bank_common::ui::BANK_LOADING_MESSAGE) }
                .map_err(|_| Fault::NativeObject)?;
            // The task's +1c continuation is patched to this same poller before
            // enabling busy protection. No blocking join or FS call occurs here.
            task.begin_busy();
            unsafe { write_word(task.raw(), 0x10, 1) };
            self.baseline = None;
            self.rewards = RewardSession::NONE;
            if opening {
                self.no_games = false;
                self.capture_blank(task)?;
                let session = unsafe { FS_SESSION.read() };
                self.submit(task, Step::Open, Job::Open { session })?;
            } else {
                let game = unsafe {
                    NativeGameSession::from_raw(task.pointer(8)?, read_word(task.raw(), 0xc))
                }
                .map_err(|_| Fault::GameRead)?;
                let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                self.game = Some(game);
                self.submit(task, Step::Load, Job::InspectAndLoad { game: descriptor })?;
            }
            return Ok(false);
        }
        let Some(reply) = self.poll(task)? else {
            return Ok(false);
        };
        match (self.step, reply) {
            (Step::Open, Reply::Missing) => {
                // No Bank yet: the original's welcome comes first, as where
                // its server reported none. Nothing is read or written while
                // it is shown, so HOME works.
                self.step = Step::Introduce;
                task.end_busy();
                storage_activity(false);
                unsafe { bank_common::ui::welcome(task.ui()?) }.map_err(|_| Fault::NativeObject)?;
                unsafe {
                    task.raw().add(0x54).write(0);
                    write_word(task.raw(), 0x10, 2);
                }
                Ok(false)
            }
            (
                Step::Open,
                Reply::Opened {
                    phase: Phase::Prepared(pending),
                    ..
                },
            ) => {
                self.pending = true;
                let kind = (1..=8)
                    .filter_map(|id| GameKind::try_from(id).ok())
                    .find(|kind| kind.title_id() == pending.game.title_id)
                    .ok_or(Fault::RecoveryRequired)?;
                // A game that is not there is not waited for when the save
                // can be settled without it; the worker decides.
                let game =
                    match unsafe { GameIoDescriptor::from_loaded_kind(task.pointer(8)?, kind) } {
                        Ok(game) => Some(game),
                        Err(NativeGameError::NotLoaded) => None,
                        Err(_) => return Err(Fault::RecoveryRequired),
                    };
                self.submit(task, Step::Recover, Job::Recover { game })?;
                Ok(false)
            }
            (
                Step::Open,
                Reply::Opened {
                    phase: Phase::Clean(_),
                    loaded,
                },
            )
            | (Step::Initialize | Step::Recover, Reply::BankReady { loaded, .. }) => {
                match loaded {
                    Some(loaded) => self.restore_bank(task, loaded)?,
                    // A newly created Bank: the live object is the body that
                    // was just written. Copying it back would only stall the
                    // first start; forget stale Dex prompt state instead.
                    None if self.step == Step::Initialize => unsafe { crate::dex::reset_session() },
                    None => return Err(Fault::StorageInvalid),
                }
                self.pending = false;
                // The skipped main-menu initializer used to end the loading
                // panel. Game selection starts from the validated Bank instead.
                let ui = task.ui()?;
                unsafe { bank_common::ui::stop_loading(ui) }.map_err(|_| Fault::NativeObject)?;
                let coordinator = task.pointer(8)?;
                let available =
                    (1..=8).any(|kind| unsafe { coordinator.add(kind * 12 + 1).read() } != 0);
                if !available {
                    // Native selection would repeat this notice without an
                    // exit. Show it once, then return to the start screen.
                    self.no_games = true;
                    self.step = Step::NoGames;
                    task.end_busy();
                    storage_activity(false);
                    unsafe { bank_common::ui::notice(ui, bank_common::ui::NO_GAME_NOTICE) }
                        .map_err(|_| Fault::NativeObject)?;
                    return Ok(false);
                }
                Ok(self.complete(task))
            }
            (
                Step::Load,
                Reply::BankReady {
                    evidence: Some(evidence),
                    loaded: Some(loaded),
                    ..
                },
            ) => {
                self.restore_bank(task, loaded)?;
                self.baseline = Some(evidence);
                self.pending = false;
                capture_counts(task)?;
                self.begin_rewards(task, loaded.rewards)?;
                Ok(self.complete(task))
            }
            _ => Err(Fault::StorageInvalid),
        }
    }
    unsafe fn save(&mut self, task: Task) -> Result<bool, Fault> {
        if self.step == Step::Idle {
            self.baseline.ok_or(Fault::GameRead)?;
            let coordinator = task.pointer(8)?;
            // Save-request hooks already ran any required native trainer
            // confirmation. A declined replacement leaves that family intact.
            unsafe { crate::dex::sync_before_save(coordinator, task.bank_pointer()?) }
                .map_err(|_| Fault::NativeObject)?;
            task.begin_busy();
            unsafe { write_word(task.raw(), 0x10, 1) };
            let old_game = unsafe { read_word(coordinator, 0xf4) };
            let old_bank = unsafe { read_word(coordinator, 0xf8) };
            capture_counts(task)?;
            let new_game = unsafe { read_word(coordinator, 0xf4) };
            let new_bank = unsafe { read_word(coordinator, 0xf8) };
            unsafe {
                write_word(coordinator, 0xf4, old_game);
                write_word(coordinator, 0xf8, old_bank);
            }
            // An earlier clock earns nothing; only an invalid one stops here.
            self.settle_rewards(task, new_bank)?;
            let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
                .map_err(|_| Fault::NativeObject)?;
            bank.record_transfer_counts(old_game, new_game, old_bank, new_bank);
            let game =
                unsafe { NativeGameSession::from_raw(coordinator, read_word(task.raw(), 0xc)) }
                    .map_err(|_| Fault::GameRead)?;
            let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
            self.game = Some(game);
            self.submit(task, Step::SaveInspect, Job::Inspect { game: descriptor })?;
            return Ok(false);
        }
        if self.owner != task.raw() as usize {
            return Err(Fault::NativeObject);
        }
        if self.step == Step::SaveWrite {
            let game = self.game.as_mut().ok_or(Fault::GameWrite)?;
            match game.poll().map_err(|_| Fault::GameWrite)? {
                NativePoll::Pending => return Ok(false),
                NativePoll::Complete => {
                    let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                    self.submit(task, Step::SaveFinalize, Job::Finalize { game: descriptor })?;
                    return Ok(false);
                }
            }
        }
        let Some(reply) = self.poll(task)? else {
            return Ok(false);
        };
        match (self.step, reply) {
            (Step::SaveInspect, Reply::Inspected(original)) => {
                let baseline = self.baseline.ok_or(Fault::GameRead)?;
                if baseline.title != original.title || baseline.fingerprint != original.fingerprint
                {
                    return Err(Fault::GameChanged);
                }
                let game = self.game.as_mut().ok_or(Fault::GamePrepare)?;
                game.prepare().map_err(|_| Fault::GamePrepare)?;
                let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                let prepared = game
                    .prepared()
                    .map_err(|_| Fault::GamePrepare)?
                    .freeze()
                    .map_err(|_| Fault::GamePrepare)?;
                let rewards = self.rewards.settlement.ok_or(Fault::RewardState)?;
                self.stage_bank(task)?;
                // Conservative until Finalize succeeds: failed preparation may
                // already have published a durable pending journal replica.
                self.pending = true;
                self.submit(
                    task,
                    Step::SavePrepare,
                    Job::Prepare {
                        game: descriptor,
                        prepared,
                        baseline: original,
                        rewards,
                    },
                )?;
                Ok(false)
            }
            (Step::SavePrepare, Reply::ReadyToWrite) => {
                // Room for the retained native game-writer thread.
                let capacity = self.worker()?.ensure_thread_capacity(1);
                capacity.map_err(|e| self.worker_error(e))?;
                let game = self.game.as_mut().ok_or(Fault::GameWrite)?;
                unsafe { game.start_after_journal() }.map_err(|_| Fault::GameWrite)?;
                self.step = Step::SaveWrite;
                Ok(false)
            }
            (
                Step::SaveFinalize,
                Reply::BankReady {
                    evidence: Some(evidence),
                    decision: Some(RecoveryDecision::CommitAfter),
                    loaded: Some(loaded),
                },
            ) => {
                self.restore_bank(task, loaded)?;
                self.baseline = Some(evidence);
                self.pending = false;
                self.begin_rewards(task, loaded.rewards)?;
                Ok(self.complete(task))
            }
            _ => Err(Fault::JournalRecovery),
        }
    }
}
/// Replaces only BL002A5580 at002A5A2C. Original task selection is still called
/// for native local behavior, then cloud-only destinations are intercepted.
/// # Safety
/// Called on Bank's main thread with the live initialized task manager. The
/// bootstrap, paired exheader, archive-preservation edit, and all task hooks
/// must be installed for the exact verified executable.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_next(manager: *mut u8, current: u32) -> u32 {
    let Some(mut guard) = Guard::acquire() else {
        return 0x18;
    };
    let state = guard.state();
    if checked_pointer(manager as u32).is_err() || current > 0x1d {
        return 0x18;
    }
    let task = unsafe { read_word(manager, 0x10) } as *mut u8;
    if checked_pointer(task as u32).is_err() {
        return 0x18;
    }
    let cancelled = unsafe { manager.add(0x1a).read() } != 0;
    let outcome = unsafe { task.add(0x30).read() };
    let original: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
        unsafe { transmute(0x002a_5580usize) };
    let next = unsafe { original(manager, current) };
    // A task that ended during the welcome was cancelled; nothing was begun.
    if state.step == Step::Introduce {
        state.step = Step::Idle;
        state.owner = 0;
    }
    if cancelled {
        return 0x14;
    }
    // After an error nothing is opened again in this session. The game scan
    // still leads to task 9, whose hook shows the error again; everything
    // else ends in cleanup.
    if state.fault != Fault::None && !matches!(current, 0 | 1 | 2 | 3 | 0x14 | 0x15) {
        return 0x14;
    }
    if matches!((current, outcome), (3, 4 | 0x17)) {
        unsafe {
            manager.add(0x1c).write(0);
        }
    }
    // The main menu (task 4) is never created; see navigation.rs.
    let destination = crate::navigation::destination(current, outcome, next, state.no_games);
    // Refuse HOME and sleep across a whole chain of loading tasks, not only
    // while one of their jobs runs.
    if crate::navigation::loads(destination) {
        storage_activity(true);
    } else if state.job.is_none() {
        storage_activity(false);
    }
    destination
}

/// Replaces task9 and task10 update virtuals; constructors/init/end stay native.
/// # Safety
/// The trusted Bank task driver supplies the indicated live task object on the
/// main thread, after native init, with no concurrent access to its Bank object.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_load(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_OPEN, TASK_LOAD]) }) else {
        return 1;
    };
    let Some(mut guard) = Guard::acquire() else {
        return task.finish(false);
    };
    let state = guard.state();
    if state.fault != Fault::None {
        return state.report_failure(task);
    }
    match unsafe { state.load(task) } {
        Ok(false) => 0,
        Ok(true) => task.finish(true),
        Err(fault) => {
            state.fault = fault;
            state.report_failure(task)
        }
    }
}

/// # Safety
/// Same main-thread task contract as bank_offline_load, for task7 only.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_save(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SAVE]) }) else {
        return 1;
    };
    let Some(mut guard) = Guard::acquire() else {
        return 0;
    };
    let state = guard.state();
    if state.fault != Fault::None {
        return state.report_failure(task);
    }
    match unsafe { state.save(task) } {
        Ok(false) => 0,
        Ok(true) => task.finish(true),
        Err(fault) => {
            state.fault = fault;
            state.report_failure(task)
        }
    }
}
fn freeze_accrual_date(task: Task) -> Result<(), Fault> {
    let session = task.pointer(0x28)?;
    let bank = task.bank_pointer()?;
    let header = checked_pointer(unsafe { read_word(bank, 0xbb528) })?;
    let date_pointer: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
        unsafe { transmute(0x001d_5880usize) };
    let date = unsafe { date_pointer(header) };
    checked_pointer(date as u32)?;
    // Native timestamp has minute precision. Matching the session date gives
    // zero whole elapsed hours, so the native hourly accrual adds nothing on
    // top of the calendar-day accounting applied when the Bank was loaded.
    let addresses = [
        0x001d5864usize,
        0x001d5848,
        0x001d582c,
        0x001d5810,
        0x001d57f4,
    ];
    for (index, address) in addresses.into_iter().enumerate() {
        let get: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(address) };
        let value = unsafe { get(session.add(0x28)) };
        unsafe {
            if index == 0 {
                date.cast::<u16>().write(value as u16);
            } else {
                date.add(index + 1).write(value as u8);
            }
        }
    }
    Ok(())
}

/// Retains native pending-gift guards and balance redemption while skipping
/// service requests, historical distributions, first-use gifts, and the native
/// hourly accrual. The displayed balance already holds the session's local
/// earnings; each completed redemption is checked against its game receipt.
/// # Safety
/// The verified task C/D update vtables supply their live initialized objects
/// on the main thread after the selected game and Bank passed the load hook.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_rewards(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_REWARD_GUARD, TASK_REWARDS]) }) else {
        return 1;
    };
    let Some(mut guard) = Guard::acquire() else {
        return 0;
    };
    let state = guard.state();
    if state.fault != Fault::None {
        return state.report_failure(task);
    }
    let is_guard = unsafe { read_word(raw, 0) } == TASK_REWARD_GUARD;
    let setup = (|| -> Result<*mut u8, Fault> {
        let session = task.pointer(0x28)?;
        let step = unsafe { read_word(raw, 0x10) };
        // Only the verified local subgraphs are permitted. Task C protects
        // existing Gen6 pending gifts and full Gen7 gift storage.
        if state.baseline.is_none()
            || state.pending
            || (is_guard && step > 7)
            || (!is_guard && !matches!(step, 0 | 7..=0xd | 0x16 | 0x1b..=0x22))
        {
            return Err(Fault::RewardState);
        }
        if step == 0 || (!is_guard && step == 0x1b) {
            freeze_accrual_date(task)?;
            if !is_guard {
                unsafe {
                    write_word(raw, 0x4c, 1); // disables remote re-query
                    write_word(raw, 0x50, 0); // no server-awarded points
                    write_word(raw, 0x10, 0x1b);
                }
            }
            // Still the saved day and nothing to redeem: both tasks go to states of
            // their own that end without a word. The guard's is "no present
            // in the way" (outcome 5), the claim's its end for a total of
            // zero. Bank has no gifts to hand out offline.
            let entry = state.rewards.entry.ok_or(Fault::RewardState)?;
            let balance = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
                .and_then(|bank| bank.miles())
                .map_err(|_| Fault::NativeObject)?;
            if rewards::nothing_to_get(entry.new_day(), balance, false) {
                if !is_guard {
                    // The original ends the loading panel in state 0x1b
                    // before it looks at the total (002aac80).
                    unsafe { bank_common::ui::stop_loading(task.ui()?) }
                        .map_err(|_| Fault::NativeObject)?;
                }
                unsafe { write_word(raw, 0x10, if is_guard { 4 } else { 0x22 }) };
            }
        }
        Ok(session)
    })();
    let session = match setup {
        Ok(session) => session,
        Err(fault) => {
            state.fault = fault;
            return state.report_failure(task);
        }
    };
    if is_guard {
        let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
            unsafe { transmute(0x002a_d1bcusize) };
        return unsafe { original(raw) };
    }
    // Redemption happens only inside native state 0xb, after its dialog fade.
    let probe = if unsafe { read_word(raw, 0x10) } == 0xb {
        match state.probe_claim(task) {
            Ok(probe) => Some(probe),
            Err(fault) => {
                state.fault = fault;
                return state.report_failure(task);
            }
        }
    } else {
        None
    };
    let remote_count = unsafe { read_word(session, 0x38) };
    // Native state1b treats -1 as unavailable distribution metadata. Restore
    // this field before returning to other native UI code.
    unsafe { write_word(session, 0x38, u32::MAX) };
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(0x002a_9750usize) };
    let result = unsafe { original(raw) };
    unsafe { write_word(session, 0x38, remote_count) };
    if let Some(probe) = probe {
        let step = unsafe { read_word(raw, 0x10) };
        if let Err(fault) = state.check_claim(task, probe, step) {
            state.fault = fault;
            return state.report_failure(task);
        }
    }
    result
}

/// Replaces native timestamp helper 001d3bf4 for slot/group/clear operations.
/// The original caller's online client may be absent in an offline session.
/// # Safety
/// `output` must be null or point to eight live writable bytes as supplied by
/// the verified native callers. The native RTC API requires Bank's main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_timestamp(
    online_context: *mut u8,
    output: *mut u8,
) -> u32 {
    unsafe {
        crate::local_clock::timestamp_with(online_context, output, |words| {
            let local_date: unsafe extern "aapcs" fn(*mut u32) = transmute(0x0023_a754usize);
            local_date(words.as_mut_ptr());
        })
    }
}
