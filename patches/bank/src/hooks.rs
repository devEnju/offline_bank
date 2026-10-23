//! Main-thread hooks for the reviewed Bank task machine.
//!
//! Native game writes are guarded by a durable local journal and exact image checks.
//! See docs/bank.md for the original application flow.

use crate::{
    native_game::{
        self, GameIoDescriptor, GameKind, NativeGameError, NativeGameSession, NativePoll,
    },
    panel::{self, Panel},
    save_steps,
    storage_worker::{BankRequests, BankWorker, Ending, GameEvidence, Job, Reply},
    transaction::Trainer,
};
use bank_common::{
    bank_files::Loaded,
    fault::Fault,
    native_bank::NativeBank,
    program::Bank15,
    task::{
        refuse_home, BankTask, Notice, Task, LOCAL_DATE, TASK_LOAD, TASK_OPEN, TASK_REWARDS,
        TASK_REWARD_GUARD, TASK_SAVE, TASK_SCAN,
    },
    worker::WorkerError,
};
use core::mem::transmute;
use offline_core::{
    native_blob::NativeBlobView,
    rewards::{self, Currency, Dialog, Entry, Moment, Receipt, RewardError},
    sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS},
    Phase,
};
use patch_common::{
    exclusive::Exclusive,
    native::{code, follow, pointer, read_word, write_word, Ended, NotNative},
    program::Program,
    spinner::{self, Spinner},
    worker::{Polled, Requests},
};

/// `001d5ec0(helper)`: the Pokémon in the Bank's hundred boxes, with the
/// helper at `bank + 0xbb520`.
const BANK_COUNT: usize = 0x001d_5ec0;
/// `00235bfc(id)`: a heap of the original, as its counts are given one.
const HEAP: usize = 0x0023_5bfc;
/// The original's update of the game scan, task 3, `002abe08`.
const SCAN_UPDATE: usize = 0x002a_be08;
/// The original's update of task 9, `002ae568`: its welcome and the request
/// for a new Bank.
const OPEN_UPDATE: usize = 0x002a_e568;
/// `002ae8b0(task)`: the defaults and translated names of a new Bank.
const NEW_BANK_DEFAULTS: usize = 0x002a_e8b0;
/// The updates of the reward guard (task 0xc, `002ad1bc`) and of the claim
/// (task 0xd, `002a9750`).
const REWARD_GUARD_UPDATE: usize = 0x002a_d1bc;
const REWARD_CLAIM_UPDATE: usize = 0x002a_9750;
/// `001d5880(header)`: the reward date in the Bank's header; the five
/// functions below read year, month, day, hour and minute of a packed
/// timestamp.
const REWARD_DATE: usize = 0x001d_5880;
const TIMESTAMP_FIELDS: [usize; 5] = [
    0x001d_5864,
    0x001d_5848,
    0x001d_582c,
    0x001d_5810,
    0x001d_57f4,
];
/// Test builds: the main menu's listener `002a6c84(listener, entry)` and its
/// update `002a6750(task)`, and `001e9748(buttons, on)`.
#[cfg(feature = "test-build")]
const MENU_CHOSEN: usize = 0x002a_6c84;
#[cfg(feature = "test-build")]
const MENU_UPDATE: usize = 0x002a_6750;
#[cfg(feature = "test-build")]
const ENABLE_BUTTONS: usize = 0x001e_9748;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Idle,
    Open,
    Introduce,
    Initialize,
    Recover,
    /// The opening restored the Bank; its Pokédex views are reloaded on
    /// this frame and the opening ends.
    Restored,
    /// The session ends at a message of the original that waits for A: a
    /// save in progress that waits for its game, or a job that could not
    /// finish (`Ending`). Once it is answered the task ends as failed, which
    /// the router answers with cleanup and the start screen.
    TurnBack,
    Load,
    /// The chosen game is loaded and its trainer read; its Pokémon are
    /// counted on this frame.
    Loaded,
    /// The game's Pokémon are counted (`None` without a game count); the
    /// Bank's are counted on this frame and the load ends.
    Counted {
        game: Option<u32>,
    },
    /// The work of the opening or the load is done: it ends once its
    /// loading screen has stood for 1.5 s (`spinner`).
    Done,
    /// Save and Quit (`save_steps`).
    Save(save_steps::Step),
}
struct Runtime {
    game: Option<NativeGameSession>,
    owner: usize,
    step: Step,
    /// The worker and the job out, for the task that submitted it.
    requests: BankRequests,
    baseline: Option<Baseline>,
    fault: Fault,
    native_result: u32,
    pending: bool,
    notice: Notice,
    /// The files are closed and the error screen is up.
    stopped: bool,
    rewards: RewardSession,
    /// The native blank transport-box record and its format tag, captured from
    /// the freshly constructed Bank before the first restore.
    blank: Option<([u8; RECORD_SIZE], u8)>,
    /// The game scan's loading panel (`panel`).
    panel: Panel,
    /// When the loading screen that is up went up.
    spinner: Spinner,
}
/// The game the Bank was loaded with, as it was then: a save in progress is
/// bound to both.
#[derive(Clone, Copy)]
struct Baseline {
    evidence: GameEvidence,
    /// Whose game it is.
    trainer: Trainer,
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
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread, which owns the original's objects the state points to.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        game: None,
        owner: 0,
        step: Step::Idle,
        requests: Requests::new(),
        baseline: None,
        fault: Fault::None,
        native_result: 0,
        pending: false,
        notice: Notice::new(PACKAGE),
        stopped: false,
        rewards: RewardSession::NONE,
        blank: None,
        panel: Panel::new(),
        spinner: Spinner::new(),
    })
};

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
/// The Pokémon of the selected game, if one is selected, counted as the
/// original counts them for its transfer record. The coordinator keeps the
/// count at `+0xf4`, and the Bank's (`bank_count`) at `+0xf8`. Each count is
/// a frame's work of its own.
fn game_count(task: Task) -> Result<Option<u32>, Fault> {
    match unsafe { native_game::selected(task.pointer(8)?) } {
        Ok((game, kind)) => unsafe {
            let boxes = game.add(if kind.gen7() { 0x769dc } else { 0x1e51c });
            let vtable = follow(boxes, 0).ok_or(NotNative)?;
            let address = code::<Bank15>(read_word(vtable, 0xc)).ok_or(Fault::NativeObject)?;
            let heap: unsafe extern "aapcs" fn(u32) -> u32 = transmute(HEAP);
            let count: unsafe extern "aapcs" fn(*mut u8, u32, u32) -> u32 = transmute(address);
            Ok(Some(count(boxes, heap(0x17), 1)))
        },
        Err(NativeGameError::NotLoaded) => Ok(None),
        Err(_) => Err(Fault::NativeObject),
    }
}
/// The Pokémon in the Bank's hundred boxes, as native 001d5ec0 counts them
/// for the transfer record and for the Miles.
fn bank_count(task: Task) -> Result<u32, Fault> {
    let bank = task.bank_pointer()?;
    let helper = unsafe { follow(bank, 0xbb520) }.ok_or(NotNative)?;
    let count: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(BANK_COUNT) };
    Ok(unsafe { count(helper) })
}
/// The original's clock in ms (`spinner::now`).
fn now() -> u32 {
    spinner::now::<Bank15>()
}
/// The console's date and time, to the minute.
fn clock() -> Result<Moment, Fault> {
    let mut words = [0u32; 2];
    let local_date: unsafe extern "aapcs" fn(*mut u32) = unsafe { transmute(LOCAL_DATE) };
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
    let (game, kind) =
        unsafe { native_game::selected(task.pointer(8)?) }.map_err(|_| Fault::NativeObject)?;
    Ok((game, kind.gen7()))
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
    let vtable = unsafe { follow(owner, 0) }.ok_or(NotNative)?;
    let address = code::<Bank15>(unsafe { read_word(vtable, 8) }).ok_or(Fault::NativeObject)?;
    let buffer: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 = unsafe { transmute(address) };
    let gift = unsafe { buffer(owner) };
    pointer(gift as u32).ok_or(NotNative)?;
    Ok(gift)
}
impl Runtime {
    /// One frame of a task after an error: the session is over. A job that
    /// is still out is waited for first, since the error screen lets HOME
    /// through: it lets go of the files when it ends (`rest`). Until then
    /// the loading screen stays and HOME is refused.
    fn report_failure(&mut self, task: Task) -> u32 {
        if !self.stopped {
            // A job that is still out holds what the task lent it.
            if !self.requests.settle() {
                return 0;
            }
            self.stopped = true;
        }
        self.notice
            .stop(task, self.fault as u32, self.native_result)
    }
    /// The fault of a worker error, whose second number is kept for the
    /// error screen.
    fn worker_error(&mut self, error: WorkerError) -> Fault {
        self.native_result = error.native_result();
        error.fault()
    }
    fn worker(&mut self) -> Result<&mut BankWorker, Fault> {
        match unsafe { self.requests.worker() } {
            Ok(worker) => Ok(worker),
            Err(error) => {
                self.native_result = error.native_result();
                Err(error.fault())
            }
        }
    }
    fn submit(&mut self, task: Task, step: Step, job: Job) -> Result<(), Fault> {
        if self.requests.is_out() {
            return Err(Fault::Reentrant);
        }
        // Busy protection retains every archive and frozen native block until poll.
        let result = unsafe { self.requests.submit(task.raw() as usize, job) };
        result.map_err(|e| self.worker_error(e))?;
        // Files are being read or written until `complete`.
        refuse_home(true);
        self.owner = task.raw() as usize;
        self.step = step;
        Ok(())
    }
    fn poll(&mut self, task: Task) -> Result<Option<Reply>, Fault> {
        match self.requests.poll(task.raw() as usize) {
            Polled::Waiting => Ok(None),
            Polled::Reply(reply) => Ok(Some(reply)),
            Polled::Failed(error) => Err(self.worker_error(error)),
            // Another task's job has ended; this task's work is not
            // there.
            Polled::Foreign => Err(Fault::NativeObject),
        }
    }
    /// Stages the live native body for the worker, which cuts it into files.
    /// For a save, the worker keeps its checksum to check the Bank it reads
    /// back.
    fn stage_bank(&mut self, task: Task) -> Result<(), Fault> {
        let bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let snapshot = bank.snapshot().map_err(|_| Fault::NativeObject)?;
        let staging = self
            .worker()?
            .staging_mut()
            .map_err(|_| Fault::StorageLoad)?;
        staging.copy_from_slice(snapshot.as_bytes());
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
    /// Restores the assembled body into the native object, at opening or
    /// after a recovery. Its Pokédex views follow on the next frame
    /// (`reload_dex`).
    fn restore_bank(&mut self, task: Task, loaded: Loaded) -> Result<(), Fault> {
        self.fill_defaults(loaded)?;
        let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
            .map_err(|_| Fault::NativeObject)?;
        let bytes = self.staged_body()?;
        bank.restore_preserving_detected_games(bytes)
            .map_err(|_| Fault::StorageLoad)?;
        Ok(())
    }
    /// The work of an opening or a load is done. The task ends once its
    /// loading screen has stood for 1.5 s, and waits in `Step::Done` until
    /// then.
    fn done(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        if self.spinner.held(now()) {
            self.step = Step::Done;
            return Ok(None);
        }
        if unsafe { read_word(task.raw(), 0) } == TASK_OPEN {
            self.opened(task)
        } else {
            // Loading hands its panel to the reward step: HOME and sleep
            // stay refused until that takes it down (`panel_taken_down`).
            Ok(Some(self.end_task(task)))
        }
    }
    /// The opening's end, once the live object is the Bank: no save is in
    /// progress any more, and the loading panel ends.
    fn opened(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        self.pending = false;
        // The original's main-menu initializer, which is skipped, ends the
        // loading panel; game selection does not.
        let ui = task.ui()?;
        unsafe { bank_common::ui::stop_loading(ui) }.map_err(|_| Fault::NativeObject)?;
        Ok(Some(self.complete(task)))
    }
    /// The session ends at a message that waits for A, `show` on the task's
    /// UI, and the start screen follows (`Step::TurnBack`): the loading
    /// screen ends at once, as for an error, and nothing more is read or
    /// written, so HOME and sleep go through on the message.
    fn turn_back(
        &mut self,
        task: Task,
        show: impl FnOnce(*mut u8) -> Result<(), bank_common::ui::Error>,
    ) -> Result<Option<bool>, Fault> {
        let ui = task.ui()?;
        unsafe { bank_common::ui::stop_loading(ui) }.map_err(|_| Fault::NativeObject)?;
        self.step = Step::TurnBack;
        task.end_busy();
        show(ui).map_err(|_| Fault::NativeObject)?;
        Ok(None)
    }
    /// After `restore_bank`, with staging still the body it restored: the
    /// Pokédex views are reloaded from the live object and checked.
    fn reload_dex(&mut self, task: Task) -> Result<(), Fault> {
        let raw = task.bank_pointer()?;
        let expected = crate::dex::evidence(self.staged_body()?).map_err(|_| Fault::StorageLoad)?;
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
    /// Pokémon in its boxes, `count`. The hours since that date are paid
    /// into the native balance, where the original reward screens read it.
    fn begin_rewards(&mut self, task: Task, count: u32) -> Result<(), Fault> {
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
    /// A reward step has taken down the loading panel Bank loading handed it,
    /// as the guard's state 0, the claim's states 0 and `0x1b`, or the silent
    /// entry do, whatever it shows next: HOME and sleep are let through.
    fn panel_taken_down(&self, task: Task) {
        let down = task
            .ui()
            .is_ok_and(|ui| !unsafe { bank_common::ui::loading_shown(ui) }.unwrap_or(false));
        if down && !self.requests.is_out() {
            refuse_home(false);
        }
    }
    /// The task's work is done, and HOME and sleep are let through.
    fn complete(&mut self, task: Task) -> bool {
        self.end_task(task);
        refuse_home(false);
        true
    }
    /// The task's work is done; nothing of it is with the worker any more.
    fn end_task(&mut self, task: Task) -> bool {
        self.owner = 0;
        self.step = Step::Idle;
        self.game = None;
        task.end_busy();
        true
    }
    /// The welcome the original gives before it creates a Bank. Its own
    /// update runs states 2 and 3 (three messages, each acknowledged) and
    /// leaves state 4, where it would ask the server for the new Bank.
    /// Preparation returns between the panel, native defaults and staging,
    /// giving the main loop a chance to update the UI and audio.
    unsafe fn introduce(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        if self.owner != task.raw() as usize {
            return Err(Fault::NativeObject);
        }
        match unsafe { read_word(task.raw(), 0x10) } {
            2 | 3 => {
                let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
                    unsafe { transmute(OPEN_UPDATE) };
                unsafe { original(task.raw()) };
                Ok(None)
            }
            4 => {
                refuse_home(true);
                unsafe { bank_common::ui::loading(task.ui()?, bank_common::ui::CREATING_MESSAGE) }
                    .map_err(|_| Fault::NativeObject)?;
                self.spinner.up(now());
                task.begin_busy();
                unsafe { write_word(task.raw(), 0x10, 6) };
                Ok(None)
            }
            // The panel stands for 0.5 s before the work begins.
            6 if self.spinner.idle(now()) => Ok(None),
            6 => {
                // Only a missing private Bank permits fresh initialization.
                // Native defaults/translated names stay on the UI thread.
                let defaults: unsafe extern "aapcs" fn(*mut u8) =
                    unsafe { transmute(NEW_BANK_DEFAULTS) };
                unsafe { defaults(task.raw()) };
                // Miles start at the first save. Clear our reward fields,
                // including the padding the original does not initialize.
                unsafe { NativeBank::from_raw(task.bank_pointer()?) }
                    .map_err(|_| Fault::NativeObject)?
                    .set_rewards(offline_core::rewards::Record::NONE);
                unsafe { write_word(task.raw(), 0x10, 7) };
                Ok(None)
            }
            7 => {
                self.stage_bank(task)?;
                unsafe { write_word(task.raw(), 0x10, 1) };
                self.submit(task, Step::Initialize, Job::Initialize)?;
                Ok(None)
            }
            _ => Err(Fault::NativeObject),
        }
    }
    unsafe fn load(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        if self.step == Step::Introduce {
            return unsafe { self.introduce(task) };
        }
        if self.step == Step::TurnBack {
            if self.owner != task.raw() as usize {
                return Err(Fault::NativeObject);
            }
            return match unsafe { bank_common::ui::acknowledged(task.ui()?) } {
                Ok(false) => Ok(None),
                Ok(true) => {
                    self.complete(task);
                    Ok(Some(false))
                }
                Err(_) => Err(Fault::NativeObject),
            };
        }
        if matches!(
            self.step,
            Step::Restored | Step::Loaded | Step::Counted { .. } | Step::Done
        ) && self.owner != task.raw() as usize
        {
            return Err(Fault::NativeObject);
        }
        match self.step {
            Step::Restored => {
                self.reload_dex(task)?;
                return self.done(task);
            }
            // The counts of the game and the Bank as they were loaded, a
            // frame each.
            Step::Loaded => {
                self.step = Step::Counted {
                    game: game_count(task)?,
                };
                return Ok(None);
            }
            Step::Counted { game } => {
                let bank = bank_count(task)?;
                let coordinator = task.pointer(8)?;
                if let Some(game) = game {
                    unsafe { write_word(coordinator, 0xf4, game) };
                }
                unsafe { write_word(coordinator, 0xf8, bank) };
                self.begin_rewards(task, bank)?;
                return self.done(task);
            }
            Step::Done => return self.done(task),
            _ => {}
        }
        if self.step == Step::Idle {
            // The loading screen stands for 0.5 s before the work begins:
            // the scene set-up's for a load; an opening's has stood since
            // the scan.
            if self.spinner.idle(now()) {
                return Ok(None);
            }
            let opening = unsafe { read_word(task.raw(), 0) } == TASK_OPEN;
            local_date(task)?;
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
            self.rewards = RewardSession::NONE;
            if opening {
                self.capture_blank(task)?;
                let session = unsafe { Bank15::fs_session() };
                self.submit(task, Step::Open, Job::Open { session })?;
            } else {
                let game = unsafe {
                    NativeGameSession::from_raw(task.pointer(8)?, read_word(task.raw(), 0xc))
                }
                .map_err(|_| Fault::GameRead)?;
                let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                self.game = Some(game);
                self.submit(task, Step::Load, Job::LoadGame { game: descriptor })?;
            }
            return Ok(None);
        }
        let Some(reply) = self.poll(task)? else {
            return Ok(None);
        };
        match (self.step, reply) {
            (Step::Open, Reply::Missing) => {
                // No Bank yet: the original's welcome comes first, as where
                // its server reported none. Nothing is read or written while
                // it is shown, so HOME works.
                self.step = Step::Introduce;
                task.end_busy();
                refuse_home(false);
                unsafe { bank_common::ui::welcome(task.ui()?) }.map_err(|_| Fault::NativeObject)?;
                unsafe {
                    task.raw().add(0x54).write(0);
                    write_word(task.raw(), 0x10, 2);
                }
                Ok(None)
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
                Ok(None)
            }
            (Step::Recover, Reply::Waiting { title, trainer }) => {
                // The save in progress waits for its game, and nothing was
                // written. The original's message for another copy of a
                // game names it; the start screen follows, where the game
                // can be put in and START pressed again.
                let gen7 = GameKind::of_title(title)
                    .ok_or(Fault::StorageInvalid)?
                    .gen7();
                let heap = task.pointer(0xc)?;
                self.turn_back(task, |ui| unsafe {
                    bank_common::ui::other_copy(
                        ui,
                        heap,
                        &trainer.name,
                        trainer.id,
                        Trainer::digits(gen7),
                    )
                })
            }
            // The Game Card is gone: nothing of the session was saved, and
            // what the journal holds is the next START's to settle
            // (docs/internals.md, "When a job cannot finish").
            (Step::Recover | Step::Load, Reply::Ended(Ending::Retry)) => {
                self.turn_back(task, |ui| unsafe { bank_common::ui::try_again(ui) })
            }
            // No room for the new Bank's files: the scan's own message for
            // a card without room for Bank's extdata. The next START
            // creates the Bank again.
            (Step::Initialize, Reply::Ended(Ending::NoSpace)) => self
                .turn_back(task, |ui| unsafe {
                    bank_common::ui::notice(ui, bank_common::ui::NO_ROOM_TO_START)
                }),
            (
                Step::Open,
                Reply::Opened {
                    phase: Phase::Clean(_),
                    loaded,
                },
            )
            | (Step::Initialize | Step::Recover, Reply::BankReady { loaded }) => {
                match loaded {
                    // Its Pokédex views follow on the next frame.
                    Some(loaded) => {
                        self.restore_bank(task, loaded)?;
                        self.step = Step::Restored;
                        Ok(None)
                    }
                    // A newly created Bank: the live object is the body that
                    // was just written. Copying it back would only stall the
                    // first start.
                    None if self.step == Step::Initialize => self.done(task),
                    None => Err(Fault::StorageInvalid),
                }
            }
            (Step::Load, Reply::Inspected(evidence)) => {
                // The Bank is the body opening restored: nothing between
                // opening and here writes it. Whose game it is, kept with
                // its fingerprint: a save in progress is bound to both. The
                // original's reader reloads the Pokédex views from that body.
                let (_, gen7) = selected_game(task)?;
                let (name, id) =
                    unsafe { crate::dex::trainer(task.pointer(8)?, task.bank_pointer()?) }
                        .map_err(|_| Fault::NativeObject)?;
                self.baseline = Some(Baseline {
                    evidence,
                    trainer: Trainer::of_game(&name, id, gen7),
                });
                self.pending = false;
                // The counts follow on the next frame.
                self.step = Step::Loaded;
                Ok(None)
            }
            _ => Err(Fault::StorageInvalid),
        }
    }
    /// The worker has checked the Bank it read back; the live object is the
    /// body it saved, and the session ends here once the loading screen has
    /// stood for 1.5 s. The original's ending of a successful save: its
    /// sound and message, which waits. Nothing is read or written now.
    fn saved(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        if self.spinner.held(now()) {
            return Ok(None);
        }
        self.step = Step::Save(save_steps::Step::Ended);
        task.end_busy();
        refuse_home(false);
        unsafe { bank_common::ui::saved(task.ui()?) }.map_err(|_| Fault::NativeObject)?;
        Ok(None)
    }
    unsafe fn save(&mut self, task: Task) -> Result<Option<bool>, Fault> {
        use save_steps::Step as Save;
        let step = match self.step {
            Step::Idle => {
                // The set-up's loading screen stands for 0.5 s before the
                // work begins.
                if self.spinner.idle(now()) {
                    return Ok(None);
                }
                // Whether the Game Card is there comes before anything else.
                self.baseline.ok_or(Fault::GameRead)?;
                let coordinator = task.pointer(8)?;
                task.begin_busy();
                unsafe { write_word(task.raw(), 0x10, 1) };
                let game =
                    unsafe { NativeGameSession::from_raw(coordinator, read_word(task.raw(), 0xc)) }
                        .map_err(|_| Fault::GameRead)?;
                let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                self.game = Some(game);
                self.submit(
                    task,
                    Step::Save(Save::Detect),
                    Job::Detect { game: descriptor },
                )?;
                return Ok(None);
            }
            Step::Save(step) => step,
            _ => return Err(Fault::JournalRecovery),
        };
        if self.owner != task.raw() as usize {
            return Err(Fault::NativeObject);
        }
        match step {
            Save::Saved => return self.saved(task),
            Save::Ended | Save::Stopped(_) => {
                return match unsafe { bank_common::ui::acknowledged(task.ui()?) } {
                    Ok(false) => Ok(None),
                    Ok(true) => {
                        self.complete(task);
                        Ok(Some(step == Save::Ended))
                    }
                    Err(_) => Err(Fault::NativeObject),
                };
            }
            Save::Write => {
                let game = self.game.as_mut().ok_or(Fault::GameWrite)?;
                return match game.poll() {
                    Ok(NativePoll::Pending) => Ok(None),
                    // The writer has ended, whether it wrote the game or
                    // failed, a pulled Game Card among the reasons: the
                    // worker settles the save from what the game holds.
                    Ok(NativePoll::Complete) | Err(NativeGameError::NativeStatus(_)) => {
                        let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                        let finalize = Job::Finalize { game: descriptor };
                        self.submit(task, Step::Save(Save::Finalize), finalize)?;
                        Ok(None)
                    }
                    Err(_) => Err(Fault::GameWrite),
                };
            }
            Save::CountBank { game } => {
                let coordinator = task.pointer(8)?;
                // The counts of the load, and the ones now.
                let old_game = unsafe { read_word(coordinator, 0xf4) };
                let old_bank = unsafe { read_word(coordinator, 0xf8) };
                let new_game = game.unwrap_or(old_game);
                let new_bank = bank_count(task)?;
                // An earlier clock earns nothing; only an invalid one stops here.
                self.settle_rewards(task)?;
                let mut bank = unsafe { NativeBank::from_raw(task.bank_pointer()?) }
                    .map_err(|_| Fault::NativeObject)?;
                bank.record_transfer_counts(old_game, new_game, old_bank, new_bank);
                self.step = Step::Save(Save::StageGame);
                return Ok(None);
            }
            Save::StageGame => {
                let game = self.game.as_mut().ok_or(Fault::GamePrepare)?;
                game.prepare().map_err(|_| Fault::GamePrepare)?;
                self.step = Step::Save(Save::StageDex);
                return Ok(None);
            }
            Save::StageDex => {
                // A game the Bank knows already, with the same trainer,
                // brings its records and Pokédex along with the save.
                unsafe { crate::dex::refresh(task.pointer(8)?, task.bank_pointer()?) }
                    .map_err(|_| Fault::NativeObject)?;
                self.step = Step::Save(Save::StageBank);
                return Ok(None);
            }
            Save::StageBank => {
                let Baseline {
                    evidence: baseline,
                    trainer,
                } = self.baseline.ok_or(Fault::GameRead)?;
                let game = self.game.as_mut().ok_or(Fault::GamePrepare)?;
                let descriptor = game.io_descriptor().map_err(|_| Fault::GameRead)?;
                let prepared = game
                    .prepared()
                    .map_err(|_| Fault::GamePrepare)?
                    .freeze()
                    .map_err(|_| Fault::GamePrepare)?;
                if !self.rewards.settled {
                    return Err(Fault::RewardState);
                }
                self.stage_bank(task)?;
                self.pending = Save::Prepare.pending();
                // The worker checks the prepared game, reads the game once
                // and compares it with the game as it was loaded.
                self.submit(
                    task,
                    Step::Save(Save::Prepare),
                    Job::Prepare {
                        game: descriptor,
                        prepared,
                        baseline,
                        trainer,
                    },
                )?;
                return Ok(None);
            }
            // Its reply's frame goes on to `CountBank` by itself.
            Save::CountGame => return Err(Fault::JournalRecovery),
            Save::Detect | Save::Prepare | Save::Finalize => {}
        }
        let Some(reply) = self.poll(task)? else {
            return Ok(None);
        };
        let cartridge = self.baseline.ok_or(Fault::GameRead)?.evidence.cartridge;
        let next = step.after(reply, cartridge)?;
        self.step = Step::Save(next);
        self.pending = next.pending();
        match next {
            Save::CountGame => {
                // The Bank's count follows on the next frame.
                self.step = Step::Save(Save::CountBank {
                    game: game_count(task)?,
                });
            }
            Save::Write => {
                // Room for the retained native game-writer thread.
                let capacity = self.worker()?.ensure_thread_capacity(1);
                capacity.map_err(|e| self.worker_error(e))?;
                let game = self.game.as_mut().ok_or(Fault::GameWrite)?;
                unsafe { game.start_after_journal() }.map_err(|_| Fault::GameWrite)?;
            }
            Save::Stopped(ending) => {
                // Save and Quit ends without saving, as the original's own
                // failed save (task 7 state 0x10): loading ends at once, the
                // message for what it leaves waits, outcome 3, cleanup and
                // the start screen. The changes made in memory go with the
                // session; the next START settles what the journal holds.
                let ui = task.ui()?;
                unsafe { bank_common::ui::stop_loading(ui) }.map_err(|_| Fault::NativeObject)?;
                task.end_busy();
                refuse_home(false);
                unsafe {
                    match ending {
                        // Of the failure message only the advice about the
                        // card; its first part names a server.
                        Ending::Retry => bank_common::ui::try_again(ui),
                        Ending::NoSpace => bank_common::ui::sd_full(ui),
                        Ending::Torn => bank_common::ui::save_corrupted(ui),
                    }
                }
                .map_err(|_| Fault::NativeObject)?;
            }
            Save::Saved => return self.saved(task),
            Save::Detect
            | Save::CountBank { .. }
            | Save::StageGame
            | Save::StageDex
            | Save::StageBank
            | Save::Prepare
            | Save::Finalize
            | Save::Ended => return Err(Fault::JournalRecovery),
        }
        Ok(None)
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
    let Some(mut state) = STATE.take() else {
        return crate::navigation::CLEANUP;
    };
    let Some(Ended {
        outcome,
        cancelled,
        native: next,
    }) = (unsafe { Ended::route::<Bank15>(manager, current) })
    else {
        return crate::navigation::CLEANUP;
    };
    // A task that ended during welcome/preparation was cancelled; no file
    // initialization was submitted yet.
    if state.step == Step::Introduce {
        state.step = Step::Idle;
        state.owner = 0;
    }
    // After an error nothing is opened again in this session. Its
    // screen does not end, so a task only gets here if that screen
    // could not be shown: the game scan then still leads to task 9,
    // whose hook tries again, and everything else ends in cleanup.
    if state.fault != Fault::None && !matches!(current, 0 | 1 | 2 | 3 | 0x14 | 0x15) {
        return crate::navigation::CLEANUP;
    }
    // The main menu (task 4) is never created, but in the console test
    // build; see navigation.rs.
    let destination = crate::navigation::destination(current, outcome, next, cancelled);
    // The scan found a game, so the Bank opens: no transfer to Pokémon HOME
    // is under way.
    if (current, destination) == (crate::navigation::SCAN, crate::navigation::OPEN) {
        unsafe {
            manager.add(0x1c).write(0);
        }
    }
    // Refuse HOME and sleep across a whole chain of loading tasks, not
    // only while one of their jobs runs, and into the reward step while
    // the loading panel is still up.
    if crate::navigation::loads(destination)
        || (crate::navigation::takes_panel(destination) && unsafe { panel_left(manager) })
    {
        refuse_home(true);
    } else if !state.requests.is_out() {
        refuse_home(false);
    }
    // The task's set-up puts up its loading panel on a later frame; its
    // time is taken on the first frame of the patch behind it.
    if crate::navigation::spins(destination) {
        state.spinner.coming();
    }
    destination
}

// The original's "show a message" function, 001d6650(ui, kind, message,
// wait), its first instruction (was: push {r4-r8, lr}); r3 = whether the
// message waits for an answer (`patch_common`'s `message_stub!`).
patch_common::message_stub!(bank_message_stub, "r3", "0x001d6654", message_waits);
/// A message that waits for an answer lets HOME and sleep through, unless a
/// job is still out. A hook that holds the state shows one, or calls the
/// original's update that does (the welcome, the rewards), only while no job
/// is out. The original's own bit for a game save that is being written is
/// not this one and stays as it is.
extern "aapcs" fn message_waits() {
    if STATE.take().is_none_or(|state| !state.requests.is_out()) {
        refuse_home(false);
    }
}

/// Whether the task that has just ended leaves the loading panel on screen:
/// Bank loading hands its panel to the reward step (`navigation::takes_panel`).
unsafe fn panel_left(manager: *mut u8) -> bool {
    let Some(raw) = (unsafe { follow(manager, 0x10) }) else {
        return false;
    };
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_LOAD, TASK_REWARD_GUARD]) }) else {
        return false;
    };
    task.ui()
        .is_ok_and(|ui| unsafe { bank_common::ui::loading_shown(ui) }.unwrap_or(false))
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
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    // The scene set-up's panel, for a load.
    state.spinner.seen(now());
    if state.fault != Fault::None {
        return state.report_failure(task);
    }
    match unsafe { state.load(task) } {
        Ok(None) => 0,
        Ok(Some(success)) => task.finish(success),
        Err(fault) => {
            state.fault = fault;
            state.report_failure(task)
        }
    }
}

/// Replaces task 3's update (`00361ac4`). The scan's loading panel goes up
/// one frame before the original's first state, which mounts Bank's extdata
/// on this thread and no longer starts the panel itself (`panel`); every
/// later frame is the original's update.
/// # Safety
/// The trusted Bank task driver supplies the live scan task on the main
/// thread, after native init.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_scan(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SCAN]) }) else {
        return 1;
    };
    let before = unsafe { read_word(raw, 0x10) };
    {
        let Some(mut state) = STATE.take() else {
            return 0;
        };
        let idle = state.spinner.idle(now());
        match state.panel.frame(before, idle) {
            panel::Frame::Show => {
                let shown = task.ui().map_err(Fault::from).and_then(|ui| unsafe {
                    bank_common::ui::loading(ui, panel::MESSAGE).map_err(|_| Fault::NativeObject)
                });
                state.spinner.up(now());
                return match shown {
                    Ok(()) => 0,
                    Err(fault) => {
                        state.fault = fault;
                        state.report_failure(task)
                    }
                };
            }
            // The panel stands for 0.5 s before the first state.
            panel::Frame::Wait => return 0,
            panel::Frame::Original => {}
        }
    }
    // The state is free while the original runs: its no-game notice
    // (`no_game`) takes it.
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(SCAN_UPDATE) };
    let result = unsafe { original(raw) };
    let after = unsafe { read_word(raw, 0x10) };
    if let Some(mut state) = STATE.take() {
        if state.panel.ran(before, after) {
            // The mount failed and its message waits: as the original
            // showed it, without the panel, which ends in this same frame.
            if let Ok(ui) = task.ui() {
                let _ = unsafe { bank_common::ui::stop_loading(ui) };
            }
        }
    }
    result
}

// Scan state 0x18's call of "show a message" (002ac7a4) enters here. Its
// r4 is the live scan task (002abe0c: mov r4, r0); the stub hands it to
// `no_game` and keeps the caller's lr and callee-saved
// registers.
patch_common::stub!(
    bank_offline_no_game_stub,
    ["mov r0, r4", "b {notice}"],
    notice = sym no_game,
);
/// The scan found no game it can use: the original's notice, without its
/// question about Pokémon HOME.
unsafe extern "aapcs" fn no_game(raw: *mut u8) {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SCAN]) }) else {
        return;
    };
    let Some(mut state) = STATE.take() else {
        return;
    };
    // No offline files or worker job have been opened during the native
    // scan. On failure its acknowledgment state keeps the error screen up.
    let shown = task.ui().map_err(Fault::from).and_then(|ui| unsafe {
        bank_common::ui::notice_pages(ui, 0, 2).map_err(|_| Fault::NativeObject)
    });
    if let Err(fault) = shown {
        state.fault = fault;
        state.report_failure(task);
    }
}

/// # Safety
/// Same main-thread task contract as bank_offline_load, for task7 only.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_offline_save(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SAVE]) }) else {
        return 1;
    };
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    // Its set-up's panel.
    state.spinner.seen(now());
    if state.fault != Fault::None {
        return state.report_failure(task);
    }
    match unsafe { state.save(task) } {
        Ok(None) => 0,
        Ok(Some(saved)) => task.finish(saved),
        Err(fault) => {
            state.fault = fault;
            state.report_failure(task)
        }
    }
}
fn freeze_accrual_date(task: Task) -> Result<(), Fault> {
    let session = task.pointer(0x28)?;
    let bank = task.bank_pointer()?;
    let header = unsafe { follow(bank, 0xbb528) }.ok_or(NotNative)?;
    let date_pointer: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
        unsafe { transmute(REWARD_DATE) };
    let date = unsafe { date_pointer(header) };
    pointer(date as u32).ok_or(NotNative)?;
    // Native timestamp has minute precision. Matching the session date gives
    // zero whole elapsed hours, so the native hourly accrual adds nothing on
    // top of the accounting applied when the Bank was loaded, which keeps
    // the remainder. Save and Quit writes the date of the save.
    for (index, address) in TIMESTAMP_FIELDS.into_iter().enumerate() {
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
    let Some(mut state) = STATE.take() else {
        return 0;
    };
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
            unsafe { transmute(REWARD_GUARD_UPDATE) };
        let result = unsafe { original(raw) };
        state.panel_taken_down(task);
        return result;
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
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        unsafe { transmute(REWARD_CLAIM_UPDATE) };
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
    state.panel_taken_down(task);
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
            let local_date: unsafe extern "aapcs" fn(*mut u32) = transmute(LOCAL_DATE);
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
    unsafe { patch_common::ui::version::<Bank15>(context, group, pane, message, SUFFIX) }
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
    let original: unsafe extern "aapcs" fn(*mut u8, u32) = unsafe { transmute(MENU_CHOSEN) };
    let task = unsafe { listener.sub(0x34) };
    let rehearsal = match entry {
        0 => Rehearsal::None,
        1 => Rehearsal::StopBeforeGame,
        5 => Rehearsal::StopAfterGame,
        2 => Rehearsal::TearRecord,
        3 => Rehearsal::TearBoxes,
        _ => return unsafe { original(listener, entry) },
    };
    if pointer(task as u32).is_none() || unsafe { read_word(task, 0x10) } != 1 {
        return unsafe { original(listener, entry) };
    }
    let Some(ui) = (unsafe { follow(task, 0x38) }) else {
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
        let enable: unsafe extern "aapcs" fn(u32, u32) = transmute(ENABLE_BUTTONS);
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
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(MENU_UPDATE) };
    if unsafe { read_word(task, 0x10) } == 0 {
        if let Some(ui) = unsafe { follow(task, 0x38) } {
            for (pane, suffix) in MENU_SUFFIXES {
                unsafe { bank_common::ui::button(ui, 1, pane, MENU_USE_MESSAGE, suffix) };
            }
        }
    }
    unsafe { original(task) }
}
