//! The two hooks of the migration patch, on Bank's main thread.
//!
//! The scan task runs the conversion before any game is checked and then
//! shows the preparation screen while the worker runs the conversion. Every
//! result, including success, stays under the original's line about the HOME
//! Menu with the version and two numbers. Reopening Bank and pressing START
//! runs it again, which is safe, and finds a converted Bank.

use crate::{
    migrate::{self, Card, Done, PACKAGE},
    navigation,
    source::Source,
};
use bank_common::{
    bank_files::{ExtFiles, UnitFile},
    fault::Fault,
    fs::{self, ExtdataStorage},
    numbers::VERSION,
    program::Bank15,
    task::{refuse_home, BankTask, Notice, Task, TASK_SCAN},
    ui,
    worker::WorkerError,
};
use offline_core::native_blob::BLOB_SIZE;
use patch_common::{
    exclusive::Exclusive,
    native::{write_word, Ended},
    program::Program,
    spinner::{self, Spinner},
    worker::{Pending, Polled, Service, Shared, Worker},
};

/// The numbers when the hook itself could not go on: a native object was
/// not what it expects.
const NATIVE: [u32; 2] = [Fault::NativeObject as u32, 0];

/// The files in Bank's extdata, by name, through the borrowed session.
impl Card for ExtFiles {
    type Error = fs::Error;
    type Storage = ExtdataStorage;
    fn exists(&mut self, files: &'static [UnitFile]) -> Result<bool, fs::Error> {
        unsafe { ExtdataStorage::<Bank15>::exists(self.session, files) }
    }
    fn open(&mut self, files: &'static [UnitFile]) -> Result<Option<ExtdataStorage>, fs::Error> {
        unsafe { ExtdataStorage::open_existing(self.session, files) }
    }
    fn create(&mut self, files: &'static [UnitFile]) -> Result<ExtdataStorage, fs::Error> {
        unsafe { ExtdataStorage::create_new(self.session, files) }
    }
    fn remove(&mut self, files: &'static [UnitFile]) -> Result<(), fs::Error> {
        unsafe { ExtdataStorage::<Bank15>::remove(self.session, files) }
    }
}

/// The one job: convert what is in the extdata of this session. Its reply
/// is the two numbers to show, and the way the Bank was stored that it
/// found.
#[derive(Clone, Copy)]
struct Convert {
    session: u32,
}
struct Converter;
static SHARED: Shared<Converter> = Shared::new([0; BLOB_SIZE]);
impl Service for Converter {
    type Program = Bank15;
    type Job = Convert;
    type Reply = ([u32; 2], Option<Source>);
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
    fn run(&mut self, job: Convert, staging: &mut [u8]) -> Result<Self::Reply, WorkerError> {
        if job.session == 0 {
            // Bank's own FS session is not initialized yet.
            return Err(Fault::StorageOpen.with(0x7800_0000));
        }
        // The old files cannot be read: the numbers of that step.
        match unsafe { ExtdataStorage::<Bank15>::archive_exists(job.session) } {
            Ok(true) => {}
            Ok(false) => return Ok(([migrate::DONE, Done::NoBank as u32], None)),
            Err(error) => {
                return Ok((
                    [
                        migrate::FAILED + migrate::Step::ReadOld as u32,
                        error.diagnostic(),
                    ],
                    None,
                ))
            }
        }
        let mut files = ExtFiles {
            session: job.session,
        };
        let report = migrate::run(&mut files, staging);
        let numbers = migrate::numbers(report.result, fs::Error::diagnostic);
        Ok((numbers, report.source))
    }
}

struct Runtime {
    worker: Option<Worker<Converter>>,
    /// The conversion out, for the task that submitted it.
    job: Pending<usize>,
    /// What the run ended with, from then on.
    numbers: Option<[u32; 2]>,
    /// When the loading panel went up.
    spinner: Spinner,
    notice: Notice,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread, which owns the original's objects the state points to.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        worker: None,
        job: Pending::new(),
        numbers: None,
        spinner: Spinner::new(),
        notice: Notice::new(PACKAGE),
    })
};

/// The original's clock in ms (`spinner::now`).
fn now() -> u32 {
    spinner::now::<Bank15>()
}

impl Runtime {
    fn worker(&mut self) -> Result<&mut Worker<Converter>, [u32; 2]> {
        unsafe { Worker::get(&mut self.worker) }.map_err(WorkerError::numbers)
    }
    /// One frame of the utility task. `Ok(None)` while the conversion runs,
    /// which begins once the loading panel has stood for 0.5 s.
    unsafe fn open(
        &mut self,
        task: Task,
    ) -> Result<Option<<Converter as Service>::Reply>, [u32; 2]> {
        if !self.job.is_out() {
            let now = now();
            if self.spinner.elapsed(now).is_none() {
                let panel = task.ui().map_err(|_| NATIVE)?;
                unsafe { ui::loading(panel, ui::CREATING_MESSAGE) }.map_err(|_| NATIVE)?;
                self.spinner.up(now);
                return Ok(None);
            }
            if self.spinner.idle(now) {
                return Ok(None);
            }
            // The task's +1c continuation is patched to this same poller
            // before enabling busy protection. No FS call occurs here.
            task.begin_busy();
            unsafe { write_word(task.raw(), 0x10, 1) };
            let session = unsafe { Bank15::fs_session() };
            let submitted = unsafe { self.worker()?.submit(Convert { session }) };
            let id = submitted.map_err(WorkerError::numbers)?;
            // Files are being read and written until the reply.
            refuse_home(true);
            self.job.submitted(id, task.raw() as usize);
            return Ok(None);
        }
        let worker = self.worker.as_mut().ok_or(NATIVE)?;
        match self
            .job
            .poll(task.raw() as usize, |job| worker.collect(job))
        {
            Polled::Waiting => Ok(None),
            Polled::Reply(reply) => Ok(Some(reply)),
            Polled::Failed(error) => Err(WorkerError::numbers(error)),
            // Another task's job has ended; this task's work is not there.
            Polled::Foreign => Err(NATIVE),
        }
    }
}

/// Replaces only BL 002A5580 at 002A5A2C. The original router is still
/// called; its answer is narrowed to the utility task and cleanup.
/// # Safety
/// Called on Bank's main thread with the live initialized task manager. The
/// bootstrap, paired exheader and both hooks must be installed for the exact
/// verified executable.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_migrate_next(manager: *mut u8, current: u32) -> u32 {
    let Some(state) = STATE.take() else {
        return navigation::CLEANUP;
    };
    let Some(Ended {
        outcome,
        cancelled,
        native: next,
    }) = (unsafe { Ended::route::<Bank15>(manager, current) })
    else {
        return navigation::CLEANUP;
    };
    let destination = navigation::destination(current, outcome, next, cancelled);
    // Refuse HOME and sleep from the scan to the end of the conversion.
    if navigation::loads(destination) {
        refuse_home(true);
    } else if !state.job.is_out() {
        refuse_home(false);
    }
    destination
}

/// Replaces the update and busy-poll virtuals of task 3; its constructor,
/// init and end stay native.
/// # Safety
/// The trusted Bank task driver supplies the live task object on the main
/// thread, after native init.
#[no_mangle]
pub unsafe extern "aapcs" fn bank_migrate_scan(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SCAN]) }) else {
        return 1;
    };
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    if state.numbers.is_none() {
        match unsafe { state.open(task) } {
            Ok(None) => return 0,
            Ok(Some((numbers, source))) => {
                // The line above the numbers names the way converted.
                if let Some(label) = migrate::label(source, VERSION) {
                    state.notice.relabel(label);
                }
                state.numbers = Some(numbers);
            }
            Err(numbers) => state.numbers = Some(numbers),
        }
    }
    let [first, second] = state.numbers.unwrap_or(NATIVE);
    // A conversion that is done shows its result once the loading panel has
    // stood for 1.5 s; an error at once.
    if first == migrate::DONE && state.spinner.held(now()) {
        return 0;
    }
    // Every result is numbers under the original's line about the HOME
    // Menu. That screen does not end: nothing can go on in this patch. Bank
    // is closed from the HOME Menu.
    state.notice.done(task, first, second)
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
    unsafe { patch_common::ui::version::<Bank15>(context, group, pane, message, PACKAGE) }
}
