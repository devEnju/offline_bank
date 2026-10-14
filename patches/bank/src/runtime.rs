//! Main-thread hooks for the reviewed Bank task machine.
//!
//! Native game writes are guarded by a durable local journal and exact image checks.
//! See docs/bank.md for the original application flow.

use crate::{
    native_game::{GameIoDescriptor, GameKind, NativeGameError, NativeGameSession, NativePoll},
    storage_worker::{BankWorker, GameEvidence, Job, Reply},
    transaction::Trainer,
};
use bank_common::{
    bank_files::Loaded,
    native_bank::NativeBank,
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
    rewards::{self, Currency, Dialog, Entry, Moment, Receipt, RewardError},
    sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS},
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
    /// A notice of the original is up that leads back to the start screen:
    /// no usable game, or a save in progress that waits for its game.
    TurnBack,
    Load,
    SaveInspect,
    SavePrepare,
    SaveWrite,
    SaveFinalize,
    /// The save is done and the original's message for it is up.
    Saved,
}
struct Runtime {
    worker: Option<BankWorker>,
    game: Option<NativeGameSession>,
    owner: usize,
    step: Step,
    job: Option<JobId>,
    /// A job was given to the worker since the files were last closed.
    files_used: bool,
    /// The job that closes the files at the end of a session, until its
    /// reply is collected.
    closing: Option<JobId>,
    baseline: Option<GameEvidence>,
    /// Whose game the loaded one is, read with `baseline`.
    trainer: Option<Trainer>,
    save_dex: Option<crate::dex::Evidence>,
    fault: Fault,
    native_result: u32,
    pending: bool,
    notice: Notice,
    /// The files are closed and the error screen is up.
    stopped: bool,
    /// Opening ended at a notice that leads back to the start screen.
    turn_back: bool,
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
    /// Save and Quit has written the Miles state into the Bank body.
    settled: bool,
    claimed: bool,
    welcome: Welcome,
}
impl RewardSession {
    const NONE: Self = Self {
        entry: None,
        settled: false,
        claimed: false,
        welcome: Welcome::None,
    };
}
/// The original's welcome gift in this session.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Welcome {
    /// The Bank was loaded with the gift already given, or this is the
    /// test build, which passes every reward screen silently.
    None,
    /// The loaded Bank has not had it: the original's reward task gives it.
    Pending,
    /// Given in this session, on top of the balance.
    Credited,
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
    files_used: false,
    closing: None,
    baseline: None,
    trainer: None,
    save_dex: None,
    fault: Fault::None,
    native_result: 0,
    pending: false,
    notice: Notice::new(PACKAGE),
    stopped: false,
    turn_back: false,
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
        write_word(coordinator, 0xf8, bank_count(task)?);
    }
    Ok(())
}
/// The Pokémon in the Bank's hundred boxes, as native 001d5ec0 counts them
/// for the transfer record and for the Miles.
fn bank_count(task: Task) -> Result<u32, Fault> {
    let bank = task.bank_pointer()?;
    let helper = checked_pointer(unsafe { read_word(bank, 0xbb520) })?;
    let count: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(0x001d_5ec0usize) };
    Ok(unsafe { count(helper) })
}
/// The console's date and time, to the minute.
fn clock() -> Result<Moment, Fault> {
    let mut words = [0u32; 2];
    // Native 0023a754 preserves upper flag bits, so the words start zeroed.
    let local_date: unsafe extern "aapcs" fn(*mut u32) = unsafe { transmute(0x0023_a754usize) };
    unsafe { local_date(words.as_mut_ptr()) };
    crate::local_clock::local_moment(words).ok_or(Fault::Clock)
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
    /// One frame of a task after an error: the session is over. Its files
    /// are closed first, as cleanup closes them for a session that ends
    /// there, since the error screen lets HOME through and nothing follows
    /// it. Until they are, the loading screen stays and HOME is refused.
    fn report_failure(&mut self, task: Task) -> u32 {
        if !self.stopped {
            if let (Some(id), Some(worker)) = (self.job, self.worker.as_mut()) {
                // A job that is still out holds what the task lent it.
                match worker.poll(id) {
                    Ok(None) => return 0,
                    Err(_) if worker.active() => return 0,
                    _ => self.job = None,
                }
            }
            self.close_files();
            if !self.files_closed() {
                return 0;
            }
            self.stopped = true;
        }
        self.notice
            .stop(task, self.fault as u32, self.native_result)
    }
    fn worker_error(&mut self, error: WorkerError) -> Fault {
        self.native_result = error.native_result();
        match error.fault_code() {
            2 => Fault::NativeObject,
            3 => Fault::StorageOpen,
            4 => Fault::StorageCreate,
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
        self.files_used = true;
        Ok(())
    }
    /// A session ends: the worker closes the Bank's files, so that nothing
    /// of them is open on the start screen, where HOME works and Bank can be
    /// closed. The next session opens them as a first one does.
    fn close_files(&mut self) {
        if !self.files_used || self.job.is_some() || self.closing.is_some() {
            return;
        }
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        if let Ok(id) = unsafe { worker.submit(Job::Close) } {
            self.closing = Some(id);
            self.files_used = false;
        }
    }
    /// Collects the reply of `close_files`. False while it is still out.
    fn files_closed(&mut self) -> bool {
        let (Some(id), Some(worker)) = (self.closing, self.worker.as_mut()) else {
            self.closing = None;
            return true;
        };
        match worker.poll(id) {
            Ok(None) => false,
            Err(_) if worker.active() => false,
            // What closing returned changes nothing here: a file that could
            // not be closed shows when the next session opens it.
            _ => {
                self.closing = None;
                true
            }
        }
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
    /// Fills a transport box no file supplied with what a new Bank has:
    /// blank records. A box that was read is complete, a delivery too.
    fn fill_defaults(&mut self, loaded: Loaded) -> Result<(), Fault> {
        if !loaded.transport_missing {
            return Ok(());
        }
        let (record, tag) = self.blank.ok_or(Fault::NativeObject)?;
        let staging = self
            .worker()?
            .staging_mut()
            .map_err(|_| Fault::StorageLoad)?;
        for slot in 0..TRANSPORT_SLOTS {
            let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
            staging[at..at + RECORD_SIZE].copy_from_slice(&record);
            staging[TRANSPORT_TAGS.start + slot] = tag;
        }
        Ok(())
    }
    /// Restores the assembled body into the native object.
    fn restore_bank(&mut self, task: Task, loaded: Loaded) -> Result<(), Fault> {
        self.fill_defaults(loaded)?;
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
    /// Starts the session's provisional reward state from what was loaded:
    /// the balance, the reward date and the record of the Bank body, and the
    /// Pokémon in its boxes. The hours since that date are paid into the
    /// native balance, where the original reward screens read it.
    fn begin_rewards(&mut self, task: Task) -> Result<(), Fault> {
        let count = bank_count(task)?;
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let stored = bank.reward_date().map_err(|_| Fault::NativeObject)?;
        let balance = bank.miles().map_err(|_| Fault::NativeObject)?;
        let record = bank.rewards().map_err(|_| Fault::NativeObject)?;
        let entry =
            rewards::enter(record, stored, balance, count, clock()?).map_err(reward_fault)?;
        bank.set_miles(entry.balance());
        let given = bank.first_present().map_err(|_| Fault::NativeObject)?;
        self.rewards = RewardSession {
            entry: Some(entry),
            settled: false,
            claimed: false,
            welcome: if given || cfg!(feature = "test-build") {
                Welcome::None
            } else {
                Welcome::Pending
            },
        };
        Ok(())
    }
    /// Reads the clock again at Save and Quit and fixes the Miles state that
    /// must be written with this save's Pokémon changes: the balance, the
    /// reward date and the record, all in the Bank body.
    fn settle_rewards(&mut self, task: Task) -> Result<(), Fault> {
        let entry = self.rewards.entry.ok_or(Fault::RewardState)?;
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let balance = bank.miles().map_err(|_| Fault::NativeObject)?;
        let settlement = rewards::settle(&entry, balance, clock()?, self.rewards.claimed);
        bank.set_miles(settlement.balance);
        bank.set_reward_date(settlement.date);
        bank.set_rewards(settlement.record);
        self.rewards.settled = true;
        Ok(())
    }
    /// The welcome gift, around the original's call. In state 5 the original
    /// sets the balance to 100 once message 4 is answered; the balance before
    /// is returned, and `welcome_after` adds the gift to it instead. Before
    /// the choice (state 6) the quote is the whole balance, as the ordinary
    /// way's state 0x15 leaves it.
    fn welcome_before(&mut self, task: Task) -> Result<Option<u32>, Fault> {
        let step = unsafe { read_word(task.raw(), 0x10) };
        if !matches!(step, 5 | 6) {
            return Ok(None);
        }
        let balance = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .and_then(|bank| bank.miles())
            .map_err(|_| Fault::NativeObject)?;
        if step == 6 {
            unsafe { write_word(task.raw(), 0x50, balance) };
            return Ok(None);
        }
        Ok(Some(balance))
    }
    /// After state 5: still waiting for the answer to message 4, or the gift
    /// was given and is added to the balance it found.
    fn welcome_after(&mut self, task: Task, before: u32) -> Result<(), Fault> {
        if unsafe { read_word(task.raw(), 0x10) } == 5 {
            return Ok(());
        }
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        bank.set_miles(rewards::welcome(before));
        self.rewards.welcome = Welcome::Credited;
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
        // A fresh Bank has no reward date yet: Miles are counted from its
        // first save. The two places of the patch's own Miles state start
        // empty too, whatever the new object holds there: the original
        // does not set the padding the remainder is kept in.
        unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?
            .set_rewards(offline_core::rewards::Record::NONE);
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
        if self.step == Step::TurnBack {
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
            // The files of the session before are closed first.
            if !self.files_closed() {
                return Ok(false);
            }
            local_date(task)?;
            let opening = unsafe { read_word(task.raw(), 0) } == TASK_OPEN;
            // Opening inherits the scan's live panel, SD-card warning,
            // sound and animation. Only loading for a chosen game sets up
            // its loading text; opening must not restart or replace the UI.
            if !opening {
                unsafe {
                    bank_common::ui::loading(task.ui()?, bank_common::ui::BANK_LOADING_MESSAGE)
                }
                .map_err(|_| Fault::NativeObject)?;
            }
            // The task's +1c continuation is patched to this same poller before
            // enabling busy protection. No blocking join or FS call occurs here.
            task.begin_busy();
            unsafe { write_word(task.raw(), 0x10, 1) };
            self.baseline = None;
            self.trainer = None;
            self.rewards = RewardSession::NONE;
            if opening {
                self.turn_back = false;
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
                // The record is the patch's own: a title that is none of
                // the eight is not valid stored data.
                let kind =
                    GameKind::of_title(pending.game.title_id).ok_or(Fault::StorageInvalid)?;
                // A game that is not there is not waited for when the save
                // can be settled without it; the worker decides. A game
                // that is there and is not what a load would read fails as
                // it would for a load.
                let game =
                    match unsafe { GameIoDescriptor::from_loaded_kind(task.pointer(8)?, kind) } {
                        Ok(game) => Some(game),
                        Err(NativeGameError::NotLoaded) => None,
                        Err(_) => return Err(Fault::GameRead),
                    };
                self.submit(task, Step::Recover, Job::Recover { game })?;
                Ok(false)
            }
            (Step::Recover, Reply::Waiting { title, trainer }) => {
                // The save in progress waits for its game, and nothing was
                // written. The original's message for another copy of a
                // game names it; the start screen follows, where the game
                // can be put in and START pressed again.
                let gen7 = GameKind::of_title(title)
                    .ok_or(Fault::StorageInvalid)?
                    .gen7();
                let ui = task.ui()?;
                let heap = checked_pointer(unsafe { read_word(task.raw(), 0xc) })?;
                unsafe { bank_common::ui::stop_loading(ui) }.map_err(|_| Fault::NativeObject)?;
                self.turn_back = true;
                self.step = Step::TurnBack;
                task.end_busy();
                unsafe {
                    bank_common::ui::other_copy(
                        ui,
                        heap,
                        &trainer.name,
                        trainer.id,
                        Trainer::digits(gen7),
                    )
                }
                .map_err(|_| Fault::NativeObject)?;
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
                    // first start.
                    None if self.step == Step::Initialize => {}
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
                    self.turn_back = true;
                    self.step = Step::TurnBack;
                    task.end_busy();
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
                // Whose game it is, kept with its fingerprint: a save in
                // progress is bound to both. The original's reader also
                // reloads the Pokédex views, so it runs before the restore,
                // which leaves them as it always has.
                let (_, gen7) = selected_game(task)?;
                let (name, id) =
                    unsafe { crate::dex::trainer(task.pointer(8)?, task.bank_pointer()?) }
                        .map_err(|_| Fault::NativeObject)?;
                self.restore_bank(task, loaded)?;
                self.baseline = Some(evidence);
                self.trainer = Some(Trainer::of_game(&name, id, gen7));
                self.pending = false;
                capture_counts(task)?;
                self.begin_rewards(task)?;
                Ok(self.complete(task))
            }
            _ => Err(Fault::StorageInvalid),
        }
    }
    unsafe fn save(&mut self, task: Task) -> Result<bool, Fault> {
        if self.step == Step::Idle {
            self.baseline.ok_or(Fault::GameRead)?;
            let coordinator = task.pointer(8)?;
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
            self.settle_rewards(task)?;
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
        if self.step == Step::Saved {
            return match unsafe { bank_common::ui::acknowledged(task.ui()?) } {
                Ok(false) => Ok(false),
                Ok(true) => Ok(self.complete(task)),
                Err(_) => Err(Fault::NativeObject),
            };
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
                if !self.rewards.settled {
                    return Err(Fault::RewardState);
                }
                let trainer = self.trainer.ok_or(Fault::GameRead)?;
                // A game the Bank knows already, with the same trainer,
                // brings its records and Pokédex along with the save.
                unsafe { crate::dex::refresh(task.pointer(8)?, task.bank_pointer()?) }
                    .map_err(|_| Fault::NativeObject)?;
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
                        trainer,
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
                self.begin_rewards(task)?;
                // The original's ending of a successful save: its sound and
                // message, which waits. Nothing is read or written now.
                self.step = Step::Saved;
                task.end_busy();
                storage_activity(false);
                unsafe { bank_common::ui::saved(task.ui()?) }.map_err(|_| Fault::NativeObject)?;
                Ok(false)
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
    let destination = 'route: {
        if cancelled {
            break 'route 0x14;
        }
        // After an error nothing is opened again in this session. Its
        // screen does not end, so a task only gets here if that screen
        // could not be shown: the game scan then still leads to task 9,
        // whose hook tries again, and everything else ends in cleanup.
        if state.fault != Fault::None && !matches!(current, 0 | 1 | 2 | 3 | 0x14 | 0x15) {
            break 'route 0x14;
        }
        if matches!((current, outcome), (3, 4 | 0x17)) {
            unsafe {
                manager.add(0x1c).write(0);
            }
        }
        // The main menu (task 4) is never created, but in the console test
        // build; see navigation.rs.
        let destination = crate::navigation::destination(current, outcome, next, state.turn_back);
        // Refuse HOME and sleep across a whole chain of loading tasks, not
        // only while one of their jobs runs.
        if crate::navigation::loads(destination) {
            storage_activity(true);
        } else if state.job.is_none() {
            storage_activity(false);
        }
        destination
    };
    // Every session ends in cleanup (task 0x14), and from there the start
    // screen follows. The files are closed on the way in; the way out
    // collects the reply, and closes them if that could not be asked before.
    state.files_closed();
    if destination == 0x14 || current == 0x14 {
        state.close_files();
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
    // top of the accounting applied when the Bank was loaded, which keeps
    // the remainder. Save and Quit writes the date of the save.
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

/// Retains native pending-gift guards, the welcome gift and balance
/// redemption while skipping service requests, historical distributions and
/// the native hourly accrual. The displayed balance already holds the
/// session's local earnings; each completed redemption is checked against its
/// game receipt.
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
        // The welcome's own states: 5 gives the gift, 4 and 6 lead from it to
        // the choice of state 7.
        let welcome = state.rewards.welcome;
        let on_welcome = match step {
            5 => welcome == Welcome::Pending,
            4 | 6 => welcome == Welcome::Credited,
            _ => false,
        };
        // Only the verified local subgraphs are permitted. Task C protects
        // existing Gen6 pending gifts and full Gen7 gift storage.
        if state.baseline.is_none()
            || state.pending
            || (is_guard && step > 7)
            || (!is_guard && !(matches!(step, 0 | 7..=0xd | 0x16 | 0x1b..=0x22) || on_welcome))
        {
            return Err(Fault::RewardState);
        }
        if step == 0 || (!is_guard && step == 0x1b) {
            freeze_accrual_date(task)?;
            // A Bank that has not had the welcome gift: the original's state
            // 0 reads its flag and gives it (message 4, then state 5).
            let welcome = step == 0 && welcome == Welcome::Pending;
            if !is_guard {
                unsafe {
                    write_word(raw, 0x4c, 1); // disables remote re-query
                    write_word(raw, 0x50, 0); // no server-awarded points
                    if !welcome {
                        write_word(raw, 0x10, 0x1b);
                    }
                }
            }
            // Nothing to say: both tasks go to states of their own that end
            // without a word. The guard's is "no present in the way"
            // (outcome 5), the claim's its end for a total of zero. Bank has
            // no gifts to hand out offline.
            let entry = state.rewards.entry.ok_or(Fault::RewardState)?;
            let balance = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
                .and_then(|bank| bank.miles())
                .map_err(|_| Fault::NativeObject)?;
            // A test build never stops at the rewards: it takes the same
            // way whatever there is to get, and claims nothing.
            let dialog = if cfg!(feature = "test-build") {
                Dialog::Silent
            } else if welcome {
                // The guard's notices stand before the gift as before any
                // redemption, whatever the balance.
                Dialog::Offer
            } else {
                entry.dialog(balance)
            };
            if is_guard {
                // The guard's notices, a present still waiting in the game
                // or a full gift list, stand in the way of a redemption
                // only: without an offer it has nothing to say either.
                if dialog != Dialog::Offer {
                    unsafe { write_word(raw, 0x10, 4) };
                }
            } else if dialog == Dialog::Silent {
                // The original ends the loading panel in state 0x1b
                // before it looks at the total (002aac80).
                unsafe { bank_common::ui::stop_loading(task.ui()?) }
                    .map_err(|_| Fault::NativeObject)?;
                unsafe { write_word(raw, 0x10, 0x22) };
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
    let before_gift = match state.welcome_before(task) {
        Ok(balance) => balance,
        Err(fault) => {
            state.fault = fault;
            return state.report_failure(task);
        }
    };
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
    if let Some(balance) = before_gift {
        if let Err(fault) = state.welcome_after(task, balance) {
            state.fault = fault;
            return state.report_failure(task);
        }
    }
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

/// What the error screen says behind the version: nothing for the offline
/// patch, the test build's name for the test build.
const PACKAGE: &[u8] = if cfg!(feature = "test-build") {
    b"-test"
} else {
    b""
};

/// Replaces the start screen's call that writes its version line
/// (002b49c4): the line says which patch runs.
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_version_shown(
    context: *mut u8,
    group: u32,
    pane: u32,
    message: u32,
) {
    const SUFFIX: &[u8] = if cfg!(feature = "test-build") {
        b"-test"
    } else {
        b"-offline"
    };
    unsafe { bank_common::ui::version(context, group, pane, message, SUFFIX) }
}

/// Test builds: stands in for the main menu's listener "an entry was
/// chosen" (table slot 003617dc, the original's 002a6c84(listener, entry)
/// with the listener at task 4's `+0x34`). While the menu waits for a choice
/// (state 1), every entry but the original's own way out opens the Bank as
/// "Use Pokémon Bank" does, and says which Save and Quit the session
/// rehearses. The entries in the order of the screen; `bank_test_menu_update`
/// labels the four rehearsals "Use Pokémon Bank (before)", "(after)",
/// "(record)", "(boxes)":
///
/// | entry | the original's | here |
/// | --- | --- | --- |
/// | 0 | Use Pokémon Bank | no rehearsal |
/// | 1 | Learn about Pokémon Bank | stop before the game is written |
/// | 5 | Move Pokémon to Pokémon HOME | stop after the game is written |
/// | 2 | View Your Support ID Number | write the journal record for a while |
/// | 3 | Download Poké Transporter | write the spare snapshot for a while |
/// | 4 | Back | the original's: its question, then the start screen |
///
/// What "Use Pokémon Bank" does is done here as the original does it for an
/// account without a block (002a6d20..002a6ddc): outcome 0xC, state 0x13
/// (the update's "finished"), the menu's buttons switched off. Its other
/// branch asked the server's reply.
/// # Safety
/// Called only through that table slot, with the listener of the live
/// main-menu task and the chosen entry.
#[cfg(feature = "test-build")]
#[no_mangle]
pub unsafe extern "aapcs" fn bank_test_menu_chosen(listener: *mut u8, entry: u32) {
    use bank_common::rehearsal::{choose, Rehearsal};
    let original: unsafe extern "aapcs" fn(*mut u8, u32) = unsafe { transmute(0x002a_6c84usize) };
    let task = unsafe { listener.sub(0x34) };
    let rehearsal = match entry {
        0 => Rehearsal::None,
        1 => Rehearsal::StopBeforeGame,
        5 => Rehearsal::StopAfterGame,
        2 => Rehearsal::TearRecord,
        3 => Rehearsal::TearBoxes,
        _ => return unsafe { original(listener, entry) },
    };
    if checked_pointer(task as u32).is_err() || unsafe { read_word(task, 0x10) } != 1 {
        return unsafe { original(listener, entry) };
    }
    let Ok(ui) = checked_pointer(unsafe { read_word(task, 0x38) }) else {
        return unsafe { original(listener, entry) };
    };
    choose(rehearsal);
    unsafe {
        task.add(0x30).write(crate::navigation::MENU_USE);
        write_word(task, 0x10, 0x13);
        let buttons = read_word(ui, 0x10);
        if buttons != 0 {
            (buttons as *mut u8).add(0x25).write(0);
        }
        let enable: unsafe extern "aapcs" fn(u32, u32) = transmute(0x001e_9748usize);
        enable(read_word(ui, 0x80), 0);
    }
}

/// Test builds: what the four rehearsals' buttons say behind the original's
/// "Use Pokémon Bank" (message 0x29, in the console's language), by the
/// text pane the original labels each with (001d62f0: panes 0x1f, 0x66,
/// 0x24, 0x29 of layout group 1, for its messages 0x2A, 0x56, 0x2B, 0x2C),
/// in the order of the screen. The first button, pane 0x13, has that message
/// already, and "Back" keeps its own.
#[cfg(feature = "test-build")]
const MENU_SUFFIXES: [(u32, &[u8]); 4] = [
    (0x1f, b" (before)"),
    (0x66, b" (after)"),
    (0x24, b" (record)"),
    (0x29, b" (boxes)"),
];
#[cfg(feature = "test-build")]
const MENU_USE_MESSAGE: u32 = 0x29;

/// Test builds: stands in for the main menu's update (table slot 003617bc,
/// the original's 002a6750). In its first state, before the menu waits for
/// a choice and after its initializer has labelled the buttons, the four
/// rehearsals' buttons get the first button's text with the name of what
/// they rehearse behind it; then the original runs.
/// # Safety
/// Called only through that table slot, with the live main-menu task.
#[cfg(feature = "test-build")]
#[no_mangle]
pub unsafe extern "aapcs" fn bank_test_menu_update(task: *mut u8) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(0x002a_6750usize) };
    if unsafe { read_word(task, 0x10) } == 0 {
        if let Ok(ui) = checked_pointer(unsafe { read_word(task, 0x38) }) {
            for (pane, suffix) in MENU_SUFFIXES {
                unsafe { bank_common::ui::button(ui, 1, pane, MENU_USE_MESSAGE, suffix) };
            }
        }
    }
    unsafe { original(task) }
}
