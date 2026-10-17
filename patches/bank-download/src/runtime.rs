//! The hooks of the download patch, on Bank's main thread.
//!
//! Task 3 first checks local data before any native scan runs, then keeps
//! archive and first-start setup without detecting games. Task 9 runs
//! online and shows the final result after cleanup. The load is the one
//! the original made for Pokémon HOME, which needs no game. The reward
//! step, task 0xc, is the patch's: it writes what the original loaded and
//! ends as cancelled, which the original answers by leaving without saving.

use crate::{
    download::{self, DONE, NO_BANK_ON_SERVER, PACKAGE, PRESENT, REFUSED, WRITTEN},
    navigation::{self, Stage},
};
use bank_common::{
    bank_files::{self, ExtFiles},
    fault::Fault,
    fs,
    native_bank::NativeBank,
    task::{
        checked_pointer, read_word, storage_activity, write_word, Ended, Notice, Task, FS_SESSION,
        TASK_OPEN, TASK_REWARD_GUARD, TASK_SCAN,
    },
    ui,
    worker::{Bank15, WorkerError},
};
use core::mem::transmute;
use offline_core::native_blob::BLOB_SIZE;
use patch_common::{
    exclusive::Exclusive,
    worker::{JobId, Service, Shared, Worker},
};

/// The numbers when a hook itself could not go on: a native object was not
/// what it expects.
const NATIVE: [u32; 2] = [Fault::NativeObject as u32, 0];

/// The original's update and busy poll of task 9 (the two slots this patch
/// takes, `bank15::OPEN_TASK`).
const OPEN_UPDATE: usize = 0x002a_e568;
const OPEN_BUSY: usize = 0x002b_4a20;
const SCAN_UPDATE: usize = 0x002a_be08;
const SCAN_BUSY: usize = 0x002b_4a20;
/// What the server's reply says about this console's Bank, a byte of the
/// reply object every task holds at `+0x28`: with 0x35 the original's task 9
/// greets the user and creates a Bank on the server (002ae620).
const REPLY: usize = 0x28;
const REPLY_BANK: usize = 0x20;
const NO_BANK: u8 = 0x35;

#[derive(Clone, Copy)]
enum Job {
    /// Is a Bank of the offline patch on the card?
    Look { session: u32 },
    /// Write the native body in the staging buffer as a new Bank.
    Write { session: u32 },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reply {
    Present,
    Absent,
    Written,
}
struct Files;
static SHARED: Shared<Files> = Shared::new([0; BLOB_SIZE]);
impl Service for Files {
    type Program = Bank15;
    type Job = Job;
    type Reply = Reply;
    type Error = WorkerError;
    type Staging = [u8; BLOB_SIZE];
    // The worker alone; this patch starts no other thread.
    const THREADS: i64 = 1;
    fn shared() -> &'static Shared<Self> {
        &SHARED
    }
    fn start() -> Self {
        Self
    }
    fn run(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
        let (Job::Look { session } | Job::Write { session }) = job;
        if session == 0 {
            // Bank's own FS session is not initialized yet.
            return Err(Fault::StorageOpen.with(0x7800_0000));
        }
        let files = ExtFiles { session };
        match job {
            Job::Look { .. } => {
                if !unsafe { fs::ExtdataStorage::archive_exists(session) }
                    .map_err(|error| Fault::StorageOpen.io(error))?
                {
                    return Ok(Reply::Absent);
                }
                match download::present(files) {
                    Ok(true) => Ok(Reply::Present),
                    Ok(false) => Ok(Reply::Absent),
                    Err(bank_files::Error::Io(error)) => Err(Fault::StorageOpen.io(error)),
                    Err(error) => Err(Fault::StorageInvalid.files(error)),
                }
            }
            Job::Write { .. } => match download::write(files, staging) {
                Ok(true) => Ok(Reply::Written),
                Ok(false) => Ok(Reply::Present),
                Err(error) => Err(Fault::StorageCreate.files(error)),
            },
        }
    }
}

struct Runtime {
    worker: Option<Worker<Files>>,
    owner: usize,
    job: Option<JobId>,
    stage: Stage,
    /// What the run ended with, from then on.
    numbers: Option<[u32; 2]>,
    /// Whether the task 9 that shows the result has brought up the loading
    /// panel, as the task that looked at the card did.
    panel: bool,
    notice: Notice,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread, which owns the original's objects the state points to.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        worker: None,
        owner: 0,
        job: None,
        stage: Stage::Looking,
        numbers: None,
        panel: false,
        notice: Notice::new(PACKAGE),
    })
};

impl Runtime {
    fn worker(&mut self) -> Result<&mut Worker<Files>, [u32; 2]> {
        unsafe { Worker::get(&mut self.worker) }.map_err(WorkerError::numbers)
    }
    fn decide(&mut self, numbers: [u32; 2]) {
        self.numbers = Some(numbers);
        self.stage = Stage::Decided;
    }
    /// One frame of a job of `task`: started by `begin` on the first, asked
    /// for from then on. `Ok(None)` while it runs.
    unsafe fn job(
        &mut self,
        task: Task,
        begin: impl FnOnce(&mut Self) -> Result<Job, [u32; 2]>,
    ) -> Result<Option<Reply>, [u32; 2]> {
        if self.job.is_none() {
            let job = begin(self)?;
            let submitted = unsafe { self.worker()?.submit(job) };
            self.job = Some(submitted.map_err(WorkerError::numbers)?);
            self.owner = task.raw() as usize;
            return Ok(None);
        }
        if self.owner != task.raw() as usize {
            return Err(NATIVE);
        }
        let mut job = self.job;
        let polled = self.worker()?.collect(&mut job);
        self.job = job;
        polled.map_err(WorkerError::numbers)
    }
    /// Task 3 before anything is contacted: the loading panel, and the
    /// worker looks at the card.
    unsafe fn look(&mut self, task: Task) -> Result<Option<Reply>, [u32; 2]> {
        unsafe {
            self.job(task, |state| {
                let panel = task.ui().map_err(|_| NATIVE)?;
                ui::loading(panel, ui::CREATING_MESSAGE).map_err(|_| NATIVE)?;
                // The task's +1c continuation is this patch's poller while
                // it looks. No FS call occurs here.
                task.begin_busy();
                write_word(task.raw(), 0x10, 1);
                // Files are read until the reply.
                storage_activity(true);
                state.panel = true;
                Ok(Job::Look {
                    session: FS_SESSION.read(),
                })
            })
        }
    }
    /// The reward step: the Bank object the original has just loaded goes
    /// into the worker's buffer, and the worker writes it. The loading
    /// panel of the load is still up.
    unsafe fn write(&mut self, task: Task) -> Result<Option<Reply>, [u32; 2]> {
        unsafe {
            self.job(task, |state| {
                // The native server load left this panel active. Only its text
                // changes for local work; sound and animation keep running.
                ui::loading_message(task.ui().map_err(|_| NATIVE)?, ui::CREATING_MESSAGE);
                let object = task.bank_pointer().map_err(|_| NATIVE)?;
                let bank = NativeBank::from_raw(object).map_err(|_| NATIVE)?;
                let live = bank.snapshot().map_err(|_| NATIVE)?.as_bytes();
                let staging = state
                    .worker()?
                    .staging_mut()
                    .map_err(WorkerError::numbers)?;
                if staging.len() != live.len() {
                    return Err(NATIVE);
                }
                staging.copy_from_slice(live);
                Ok(Job::Write {
                    session: FS_SESSION.read(),
                })
            })
        }
    }
}

/// Replaces only BL 002A5580 at 002A5A2C. The original router is called
/// first and decides; utility routing is applied afterward (`navigation`).
/// # Safety
/// Called on Bank's main thread with the live initialized task manager. The
/// bootstrap, paired exheader and every hook must be installed for the exact
/// verified executable.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_next(manager: *mut u8, current: u32) -> u32 {
    let Some(mut state) = STATE.take() else {
        return navigation::CLEANUP;
    };
    let Some(Ended { native, .. }) = (unsafe { Ended::route(manager, current) }) else {
        return navigation::CLEANUP;
    };
    let destination = navigation::destination(current, native, state.stage);
    state.stage = navigation::next_stage(current, state.stage);
    if destination == navigation::SCAN {
        storage_activity(true);
    }
    destination
}

/// Local preflight runs before the native scan's first update. With no Bank,
/// preserve native archive/first-start setup and skip game detection.
unsafe fn scan(raw: *mut u8, original: usize) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SCAN]) }) else {
        return 1;
    };
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    if state.numbers.is_none() && state.stage == Stage::Looking {
        match unsafe { state.look(task) } {
            Ok(None) => return 0,
            Ok(Some(Reply::Absent)) => {
                state.stage = Stage::Preparing;
                state.owner = 0;
                state.panel = false;
                task.end_busy();
                unsafe { write_word(raw, 0x10, 0) };
                // The callback may be the worker's forced busy poll. Let
                // the next frame enter native state 0 through the update;
                // the native busy poll expects a real native file job.
                return 0;
            }
            Ok(Some(_)) => state.decide([DONE, PRESENT]),
            Err(numbers) => state.decide(numbers),
        }
    }
    if let Some([first, second]) = state.numbers {
        let shown = state.notice.done(task, first, second);
        if shown != 0 {
            state.stage = Stage::Looking;
        }
        return shown;
    }
    if state.stage != Stage::Preparing {
        return task.finish(false);
    }
    let native_state = unsafe { read_word(raw, 0x10) };
    unsafe { write_word(raw, 0x10, navigation::preparation_state(native_state)) };
    // The native busy poll can call the hooked update; release the state
    // first.
    drop(state);
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(original) };
    unsafe { original(raw) }
}

/// Replaces task 3's update before game detection.
/// # Safety
/// The native task driver supplies the live initialized scan task on the UI thread.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_scan(raw: *mut u8) -> u32 {
    unsafe { scan(raw, SCAN_UPDATE) }
}

/// Replaces task 3's busy poll.
/// # Safety
/// As `bank_download_scan`.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_scan_busy(raw: *mut u8) -> u32 {
    unsafe { scan(raw, SCAN_BUSY) }
}

/// Task 9, shared by its update and its busy poll. `original` is the
/// function the slot held.
unsafe fn open(raw: *mut u8, original: usize, update: bool) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(original) };
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_OPEN]) }) else {
        return unsafe { original(raw) };
    };
    let Some(mut state) = STATE.take() else {
        return unsafe { original(raw) };
    };
    if let Some([first, second]) = state.numbers {
        if !state.panel {
            // A task 9 that begins with the result, after cleanup: it is
            // first brought to where the one that looked at the card stood
            // when it had its answer, the state the result screen is known
            // from. The screen comes on the next frame.
            state.panel = true;
            if let Ok(panel) = task.ui() {
                let _ = unsafe { ui::loading(panel, ui::CREATING_MESSAGE) };
            }
            task.begin_busy();
            unsafe { write_word(raw, 0x10, 1) };
            return 0;
        }
        // The numbers are the result, under the original's line about the
        // HOME Menu. That screen does not end: Bank is closed from the HOME
        // Menu. The session, if there was one, has ended in cleanup before.
        let shown = state.notice.done(task, first, second);
        if shown != 0 {
            // The screen could not be shown and the task has ended as
            // failed, which leads to cleanup. The start screen follows, not
            // this task again; the next START shows the result.
            state.stage = Stage::Looking;
        }
        return shown;
    }
    // Online, the task is the original's. One thing is asked first: if the
    // server holds no Bank, the original would now greet the user and create
    // one. This patch creates nothing; it ends the task as failed, which
    // the original answers with cleanup and disconnect.
    if update && unsafe { read_word(raw, 0x10) } == 0 {
        let reply = checked_pointer(unsafe { read_word(raw, REPLY) });
        if reply.is_ok_and(|reply| unsafe { reply.add(REPLY_BANK).read() } == NO_BANK) {
            state.decide([REFUSED, NO_BANK_ON_SERVER]);
            return task.finish(false);
        }
    }
    drop(state);
    unsafe { original(raw) }
}

/// Replaces the update virtual of task 9; its constructor, init and end
/// stay native.
/// # Safety
/// The trusted Bank task driver supplies the live task object on the main
/// thread, after native init.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_open(raw: *mut u8) -> u32 {
    unsafe { open(raw, OPEN_UPDATE, true) }
}

/// Replaces the busy-poll virtual of task 9.
/// # Safety
/// As `bank_download_open`.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_open_busy(raw: *mut u8) -> u32 {
    unsafe { open(raw, OPEN_BUSY, false) }
}

/// Replaces the update virtual of task 0xc, the reward step, which the
/// router hook enters after the load; its constructor, init and end stay
/// native, and they need neither a game nor the scene. Nothing
/// is asked about rewards and none is taken. The Bank the original loaded
/// is written, and the task ends with the original's outcome for "cancel",
/// which the original router answers with leaving without saving (0x13):
/// the session is closed on the server whether the files could be written
/// or not.
/// # Safety
/// As `bank_download_open`, for task 0xc.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_save(raw: *mut u8) -> u32 {
    let cancel = |raw: *mut u8| unsafe {
        raw.add(0x30).write(navigation::CANCELLED);
        1
    };
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_REWARD_GUARD]) }) else {
        return cancel(raw);
    };
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    if state.numbers.is_none() {
        match unsafe { state.write(task) } {
            Ok(None) => return 0,
            Ok(Some(Reply::Written)) => state.decide([DONE, WRITTEN]),
            Ok(Some(_)) => state.decide([DONE, PRESENT]),
            Err(numbers) => state.decide(numbers),
        }
    }
    state.owner = 0;
    cancel(raw)
}

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
    unsafe { ui::version(context, group, pane, message, PACKAGE) }
}
