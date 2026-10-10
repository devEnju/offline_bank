//! The two hooks of the migration patch, on Bank's main thread.
//!
//! The router hook leads from the game scan to task 9, "open Bank", and from
//! there back to the start screen. The task 9 hook shows the loading
//! screen, has the worker run the conversion, and shows its result as the
//! screen with two numbers. Starting again runs it again, which is safe.

use crate::{
    migrate::{self, Card},
    navigation,
};
use bank_common::{
    bank_files::UnitFile,
    fs::{self, ExtdataStorage},
    task::{
        checked_pointer, read_word, storage_activity, write_word, Notice, Task, FS_SESSION,
        TASK_OPEN,
    },
    ui,
    worker::{JobId, Service, Shared, Worker, WorkerError},
};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    sync::atomic::{AtomicBool, Ordering},
};

/// First number when the hook itself could not go on: a native object was
/// not what it expects, or the worker could not be used. The offline patch
/// shows the same number for the same.
const NATIVE_OBJECT: u32 = 2;
/// First number when Bank's file system session is not there yet.
const STORAGE_OPEN: u32 = 3;

/// The files in Bank's extdata, by name, through the borrowed session.
struct ExtFiles {
    session: u32,
}
impl Card for ExtFiles {
    type Error = fs::Error;
    type Storage = ExtdataStorage;
    fn exists(&mut self, files: &'static [UnitFile]) -> Result<bool, fs::Error> {
        unsafe { ExtdataStorage::exists(self.session, files) }
    }
    fn open(&mut self, files: &'static [UnitFile]) -> Result<Option<ExtdataStorage>, fs::Error> {
        unsafe { ExtdataStorage::open_existing(self.session, files) }
    }
    fn create(&mut self, files: &'static [UnitFile]) -> Result<ExtdataStorage, fs::Error> {
        unsafe { ExtdataStorage::create_new(self.session, files) }
    }
    fn remove(&mut self, files: &'static [UnitFile]) -> Result<(), fs::Error> {
        unsafe { ExtdataStorage::remove(self.session, files) }
    }
}

/// The one job: convert what is in the extdata of this session. Its reply
/// is the two numbers to show.
#[derive(Clone, Copy)]
struct Convert {
    session: u32,
}
struct Converter;
static SHARED: Shared<Converter> = Shared::new();
impl Service for Converter {
    type Job = Convert;
    type Reply = [u32; 2];
    // The worker alone; this patch starts no other thread.
    const THREADS: i64 = 1;
    fn shared() -> &'static Shared<Self> {
        &SHARED
    }
    fn start() -> Self {
        Self
    }
    fn run(&mut self, job: Convert, staging: &mut [u8]) -> Result<[u32; 2], WorkerError> {
        if job.session == 0 {
            // Bank's own FS session is not initialized yet.
            return Err(WorkerError::operation(STORAGE_OPEN, 0x7800_0000));
        }
        let mut files = ExtFiles {
            session: job.session,
        };
        Ok(migrate::numbers(
            migrate::run(&mut files, staging),
            fs::Error::diagnostic,
        ))
    }
    // The conversion finishes the step it began.
    fn cancellable(_: &Convert) -> bool {
        false
    }
}

struct Runtime {
    worker: Option<Worker<Converter>>,
    owner: usize,
    job: Option<JobId>,
    /// What the last run ended with, while its screen is up.
    numbers: Option<[u32; 2]>,
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
    numbers: None,
    notice: Notice::new(),
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
    fn worker(&mut self) -> Result<&mut Worker<Converter>, [u32; 2]> {
        if self.worker.is_none() {
            self.worker = Some(unsafe { Worker::start() }.map_err(numbers)?);
        }
        self.worker.as_mut().ok_or([NATIVE_OBJECT, 0])
    }
    /// One frame of task 9. `Ok(None)` while the conversion runs.
    unsafe fn open(&mut self, task: Task) -> Result<Option<[u32; 2]>, [u32; 2]> {
        let Some(id) = self.job else {
            let panel = task.ui().map_err(|_| [NATIVE_OBJECT, 0])?;
            unsafe { ui::loading(panel, ui::BANK_LOADING_MESSAGE) }
                .map_err(|_| [NATIVE_OBJECT, 0])?;
            // The task's +1c continuation is patched to this same poller
            // before enabling busy protection. No FS call occurs here.
            task.begin_busy();
            unsafe { write_word(task.raw(), 0x10, 1) };
            let session = unsafe { FS_SESSION.read() };
            let submitted = unsafe { self.worker()?.submit(Convert { session }) };
            let id = submitted.map_err(numbers)?;
            // Files are being read and written until the reply.
            storage_activity(true);
            self.owner = task.raw() as usize;
            self.job = Some(id);
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
                    // An uncertain poll never ends a running conversion.
                    // Keep rendering and ask again.
                    return Ok(None);
                }
                self.job = None;
                Err(numbers(error))
            }
        }
    }
}

/// Replaces only BL 002A5580 at 002A5A2C. The original router is still
/// called; its answer is narrowed to the way to task 9 and back.
/// # Safety
/// Called on Bank's main thread with the live initialized task manager. The
/// bootstrap, paired exheader and both hooks must be installed for the exact
/// verified executable.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_migrate_next(manager: *mut u8, current: u32) -> u32 {
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
    if cancelled {
        return navigation::CLEANUP;
    }
    if matches!((current, outcome), (3, 4 | 0x17)) {
        unsafe {
            manager.add(0x1c).write(0);
        }
    }
    let destination = navigation::destination(current, outcome, next);
    // Refuse HOME and sleep from the scan to the end of the conversion.
    if navigation::loads(destination) {
        storage_activity(true);
    } else if state.job.is_none() {
        storage_activity(false);
    }
    destination
}

/// Replaces the update and busy-poll virtuals of task 9; its constructor,
/// init and end stay native.
/// # Safety
/// The trusted Bank task driver supplies the live task object on the main
/// thread, after native init.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_migrate_open(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_OPEN]) }) else {
        return 1;
    };
    let Some(mut guard) = Guard::acquire() else {
        return task.finish(false);
    };
    let state = guard.state();
    if state.numbers.is_none() {
        match unsafe { state.open(task) } {
            Ok(None) => return 0,
            Ok(Some(numbers)) | Err(numbers) => state.numbers = Some(numbers),
        }
    }
    let [first, second] = state.numbers.unwrap_or([NATIVE_OBJECT, 0]);
    let finished = state.notice.report(task, first, second);
    if finished != 0 {
        // Acknowledged: the next start converts again, which finds the
        // work done and says so.
        state.numbers = None;
        state.owner = 0;
    }
    finished
}
