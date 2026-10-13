//! The hooks of the download patch, on Bank's main thread.
//!
//! The router hook asks the original and changes five of its answers
//! (`navigation`). Task 9, "open Bank", is the patch's own twice and the
//! original's in between: first it looks at the card before anything is
//! contacted, at the end it shows the result as two numbers under the
//! original's line about the HOME Menu, which stays. The load is the one
//! the original made for Pokémon HOME, which needs no game. The reward
//! step, task 0xc, is the patch's: it writes what the original loaded and
//! ends as cancelled, which the original answers by leaving without saving.

use crate::{
    download::{self, DONE, NO_BANK_ON_SERVER, PRESENT, REFUSED, WRITTEN},
    navigation::{self, Stage},
};
use bank_common::{
    bank_files::{self, ExtFiles},
    fs,
    native_bank::NativeBank,
    session,
    task::{
        checked_pointer, read_word, storage_activity, write_word, Notice, Task, FS_SESSION,
        TASK_OPEN, TASK_REWARD_GUARD,
    },
    ui,
    worker::{JobId, Service, Shared, Worker, WorkerError},
};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    sync::atomic::{AtomicBool, Ordering},
};
use offline_core::StoreError;

/// First number when a hook itself could not go on: a native object was
/// not what it expects, or the worker could not be used. The offline patch
/// shows the same number for the same.
const NATIVE_OBJECT: u32 = 2;
/// First numbers of a failed job, as the offline patch has them: Bank's
/// files could not be opened, a new Bank could not be written, a file is
/// not one of this layout.
const STORAGE_OPEN: u32 = 3;
const STORAGE_INITIALIZE: u32 = 4;
const STORAGE_INVALID: u32 = 5;

/// The original's update and busy poll of task 9 (the two slots this patch
/// takes, `bank15::OPEN_TASK`).
const OPEN_UPDATE: usize = 0x002a_e568;
const OPEN_BUSY: usize = 0x002b_4a20;
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
static SHARED: Shared<Files> = Shared::new();
impl Service for Files {
    type Job = Job;
    type Reply = Reply;
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
            return Err(WorkerError::operation(STORAGE_OPEN, 0x7800_0000));
        }
        let files = ExtFiles { session };
        match job {
            Job::Look { .. } => match download::present(files) {
                Ok(true) => Ok(Reply::Present),
                Ok(false) => Ok(Reply::Absent),
                Err(bank_files::Error::Io(error)) => Err(fs_error(error, STORAGE_OPEN)),
                Err(error) => Err(files_error(error, STORAGE_INVALID)),
            },
            Job::Write { .. } => match download::write(files, staging) {
                Ok(true) => Ok(Reply::Written),
                Ok(false) => Ok(Reply::Present),
                Err(error) => Err(files_error(error, STORAGE_INITIALIZE)),
            },
        }
    }
    // Neither job is given up half way.
    fn cancellable(_: &Job) -> bool {
        false
    }
}
fn fs_error(error: fs::Error, fault: u32) -> WorkerError {
    WorkerError::operation(fault, error.diagnostic())
}
/// The second number as the offline patch gives it for the same failure.
fn files_error(error: bank_files::Error<fs::Error>, fault: u32) -> WorkerError {
    match error {
        bank_files::Error::Io(error)
        | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(error))) => {
            fs_error(error, fault)
        }
        bank_files::Error::Bank(_) => WorkerError::operation(fault, 0),
        bank_files::Error::Side(file, problem) => {
            WorkerError::operation(fault, 0x7a00_0000 | (file as u32) << 8 | problem as u32)
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
struct State(UnsafeCell<Runtime>);
// SAFETY: only the main task thread acquires Guard. The worker has a
// separate mailbox and never receives Runtime or any live UI reference.
unsafe impl Sync for State {}
static BUSY: AtomicBool = AtomicBool::new(false);
static STATE: State = State(UnsafeCell::new(Runtime {
    worker: None,
    owner: 0,
    job: None,
    stage: Stage::Looking,
    numbers: None,
    panel: false,
    notice: Notice::new(PACKAGE),
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

fn numbers(error: WorkerError) -> [u32; 2] {
    [error.fault_code(), error.native_result()]
}
impl Runtime {
    fn worker(&mut self) -> Result<&mut Worker<Files>, [u32; 2]> {
        if self.worker.is_none() {
            self.worker = Some(unsafe { Worker::start() }.map_err(numbers)?);
        }
        self.worker.as_mut().ok_or([NATIVE_OBJECT, 0])
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
        let Some(id) = self.job else {
            let job = begin(self)?;
            let submitted = unsafe { self.worker()?.submit(job) };
            self.job = Some(submitted.map_err(numbers)?);
            self.owner = task.raw() as usize;
            return Ok(None);
        };
        if self.owner != task.raw() as usize {
            return Err([NATIVE_OBJECT, 0]);
        }
        let polled = self.worker()?.poll(id);
        match polled {
            Ok(Some(reply)) => {
                self.job = None;
                Ok(Some(reply))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                if self.worker()?.active() {
                    // An uncertain poll never ends a running job. Keep
                    // rendering and ask again.
                    return Ok(None);
                }
                self.job = None;
                Err(numbers(error))
            }
        }
    }
    /// Task 9 before anything is contacted: the loading panel, and the
    /// worker looks at the card.
    unsafe fn look(&mut self, task: Task) -> Result<Option<Reply>, [u32; 2]> {
        unsafe {
            self.job(task, |state| {
                let panel = task.ui().map_err(|_| [NATIVE_OBJECT, 0])?;
                ui::loading(panel, ui::BANK_LOADING_MESSAGE).map_err(|_| [NATIVE_OBJECT, 0])?;
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
                let object = task.bank_pointer().map_err(|_| [NATIVE_OBJECT, 0])?;
                let bank = NativeBank::from_raw(object).map_err(|_| [NATIVE_OBJECT, 0])?;
                let live = bank.snapshot().map_err(|_| [NATIVE_OBJECT, 0])?.as_bytes();
                let staging = state.worker()?.staging_mut().map_err(numbers)?;
                if staging.len() != live.len() {
                    return Err([NATIVE_OBJECT, 0]);
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
/// first and decides; three of its answers are changed (`navigation`).
/// # Safety
/// Called on Bank's main thread with the live initialized task manager. The
/// bootstrap, paired exheader and every hook must be installed for the exact
/// verified executable.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_download_next(manager: *mut u8, current: u32) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
        unsafe { transmute(0x002a_5580usize) };
    let native = unsafe { original(manager, current) };
    let Some(mut guard) = Guard::acquire() else {
        return native;
    };
    let state = guard.state();
    let destination = navigation::destination(current, native, state.stage);
    state.stage = navigation::next_stage(current, state.stage);
    destination
}

/// Task 9, shared by its update and its busy poll. `original` is the
/// function the slot held.
unsafe fn open(raw: *mut u8, original: usize, update: bool) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(original) };
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_OPEN]) }) else {
        return unsafe { original(raw) };
    };
    let Some(mut guard) = Guard::acquire() else {
        return unsafe { original(raw) };
    };
    let state = guard.state();
    if state.numbers.is_none() && state.stage == Stage::Looking {
        match unsafe { state.look(task) } {
            Ok(None) => return 0,
            Ok(Some(Reply::Absent)) => {
                // Nothing is on the card. The task ends, and the router
                // sends the session on to the original's connect.
                state.stage = Stage::NoneFound;
                state.owner = 0;
                state.panel = false;
                storage_activity(false);
                task.end_busy();
                return task.finish(true);
            }
            Ok(Some(_)) => state.decide([DONE, PRESENT]),
            Err(numbers) => state.decide(numbers),
        }
    }
    if let Some([first, second]) = state.numbers {
        if !state.panel {
            // A task 9 that begins with the result, after cleanup: it is
            // first brought to where the one that looked at the card stood
            // when it had its answer, the state the result screen is known
            // from. The screen comes on the next frame.
            state.panel = true;
            if let Ok(panel) = task.ui() {
                let _ = unsafe { ui::loading(panel, ui::BANK_LOADING_MESSAGE) };
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
    drop(guard);
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
    let Some(mut guard) = Guard::acquire() else {
        return cancel(raw);
    };
    let state = guard.state();
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

/// The patch's name: behind "Ver. 1.5" on the start screen, and behind the
/// version on the result screen.
const PACKAGE: &[u8] = b"-download";

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
