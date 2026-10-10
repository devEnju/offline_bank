//! The entry points called from the patched original code, the worker thread
//! that does the file work, and the file adapter over Transporter's own SDK
//! wrappers. The original task calls an entry every frame until it answers;
//! the main thread never waits for a file operation.

use crate::layout::*;
use core::{
    cell::UnsafeCell,
    mem::transmute,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};
use offline_core::{
    sections::TRANSPORT_SLOTS,
    sidecar::{Sidecar, SLOTS},
    transport::{self, KIND},
    Storage,
};

/// Transporter's `fs:USER` session handle.
pub(crate) const FS_SESSION: *const u32 = 0x0031_1f80 as *const u32;
pub(crate) const OPEN_FILE_DIRECTLY: usize = 0x001d_f448;
pub(crate) const FILE_READ: usize = 0x0015_930c;
pub(crate) const FILE_WRITE: usize = 0x0015_9390;
pub(crate) const FILE_SIZE: usize = 0x0015_93f0;
pub(crate) const FILE_CLOSE: usize = 0x0015_9364;
/// The original's "show this message and wait for it to be acknowledged":
/// `(ui, message, 1)`, as every one of its tasks calls it.
const SHOW_MESSAGE: usize = 0x0019_b50c;
/// Flush and update the file's time, as Bank's writer does.
pub(crate) const FLUSH_FLAGS: u32 = 0x0001_0001;
/// The original's `svcCreateThread(out, entry, arg, stack_top, priority,
/// processor)` and `svcExitThread()` wrappers.
const CREATE_THREAD: usize = 0x0010_e250;
const EXIT_THREAD: usize = 0x0011_f140;
/// One below the main thread, so the worker runs while the main thread waits
/// for the next frame.
const WORKER_PRIORITY: i32 = 0x31;

extern "aapcs" {
    /// `svc 0x23` behind an ordinary call boundary (see link.rs).
    pub(crate) fn transporter_close_handle(handle: u32) -> i32;
}

const IDLE: u32 = 0;
const RUNNING: u32 = 1;
const DONE: u32 = 2;
const JOB_CHECK: u32 = 0;
const JOB_DELIVER: u32 = 1;

/// Worker state. Only the main thread starts and collects a job; while one
/// runs, the worker reads `task` and `job` and writes the two atomics.
#[repr(C)]
struct Shared {
    phase: AtomicU32,
    result: AtomicU32,
    handle: UnsafeCell<u32>,
    task: UnsafeCell<*const u8>,
    job: UnsafeCell<u32>,
}
// SAFETY: see the access rule above; `phase` orders every hand-over.
unsafe impl Sync for Shared {}
static STATE: Shared = Shared {
    phase: AtomicU32::new(IDLE),
    result: AtomicU32::new(0),
    handle: UnsafeCell::new(0),
    task: UnsafeCell::new(core::ptr::null()),
    job: UnsafeCell::new(0),
};

#[repr(C, align(8))]
struct Stack(UnsafeCell<[u8; WORKER_STACK_SIZE]>);
// SAFETY: used only as the stack of the one worker thread.
unsafe impl Sync for Stack {}
static STACK: Stack = Stack(UnsafeCell::new([0; WORKER_STACK_SIZE]));

/// The transport box in Bank's extdata, open for reading and writing: its
/// two slots (`offline_core::sidecar`) are the files `/mover.bin` and
/// `/mover.alt.bin`, addressed as one range. Bank keeps every slot in a file
/// of its own, because the console leaves a file that was being written
/// unreadable when the power fails.
///
/// A file that is missing or cannot be opened makes every read of its slot
/// fail, and so the check and the delivery refuse: Transporter neither
/// creates nor replaces a file. Bank's next Save and Quit does.
struct File {
    handles: [Option<u32>; SLOTS],
    written: [bool; SLOTS],
}
impl File {
    /// `None` when Bank's extdata or both files do not exist, access is
    /// refused, or a file is not exactly the size Bank creates.
    unsafe fn open() -> Option<Self> {
        let mut handles = [None; SLOTS];
        for (handle, path) in handles.iter_mut().zip(MOVER_PATHS) {
            *handle = unsafe { Self::open_one(path) }.ok()?;
        }
        let file = Self {
            handles,
            written: [false; SLOTS],
        };
        file.handles.iter().any(Option::is_some).then_some(file)
    }
    /// `Ok(None)` for a file that is not there or cannot be opened, `Err`
    /// for one of another size: that is not a file of this layout.
    unsafe fn open_one(path: &[u8]) -> Result<Option<u32>, ()> {
        type Open = unsafe extern "aapcs" fn(
            *const u32,
            *mut u32,
            u32,
            u32,
            u32,
            *const u8,
            u32,
            u32,
            *const u8,
            u32,
            u32,
            u32,
        ) -> i32;
        type Size = unsafe extern "aapcs" fn(*const u32, *mut u64) -> i32;
        let open: Open = unsafe { transmute(OPEN_FILE_DIRECTLY) };
        // media type SD, extdata id low and high.
        let archive = [1u32, BANK_EXTDATA, 0];
        let mut handle = 0;
        let code = unsafe {
            open(
                FS_SESSION,
                &mut handle,
                0,
                6,
                2,
                archive.as_ptr().cast(),
                12,
                3,
                path.as_ptr(),
                path.len() as u32,
                3,
                0,
            )
        };
        if code < 0 || handle == 0 {
            return Ok(None);
        }
        let size: Size = unsafe { transmute(FILE_SIZE) };
        let mut length = 0;
        if unsafe { size(&handle, &mut length) } < 0 || length != KIND.slot_len() {
            unsafe { close(handle) };
            return Err(());
        }
        Ok(Some(handle))
    }
    /// The slot file that holds `offset..offset + length`, and where in it.
    fn locate(&self, offset: u64, length: usize) -> Result<(usize, u32, u64), ()> {
        let index = (offset / KIND.slot_len()) as usize;
        let within = offset % KIND.slot_len();
        if index >= SLOTS || within + length as u64 > KIND.slot_len() {
            return Err(());
        }
        Ok((index, self.handles[index].ok_or(())?, within))
    }
    fn put(&mut self, handle: u32, offset: u64, bytes: &[u8], flags: u32) -> Result<(), ()> {
        type Write =
            unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *const u8, u32, u32) -> i32;
        let write: Write = unsafe { transmute(FILE_WRITE) };
        let mut count = 0;
        let code = unsafe {
            write(
                &handle,
                &mut count,
                offset,
                bytes.as_ptr(),
                bytes.len() as u32,
                flags,
            )
        };
        if code < 0 || count as usize != bytes.len() {
            return Err(());
        }
        Ok(())
    }
}
unsafe fn close(handle: u32) {
    type Close = unsafe extern "aapcs" fn(*const u32) -> i32;
    let close: Close = unsafe { transmute(FILE_CLOSE) };
    unsafe {
        close(&handle);
        transporter_close_handle(handle);
    }
}
impl Drop for File {
    fn drop(&mut self) {
        for handle in self.handles.into_iter().flatten() {
            unsafe { close(handle) };
        }
    }
}
impl Storage for File {
    type Error = ();
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), ()> {
        type Read = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *mut u8, u32) -> i32;
        let (_, handle, within) = self.locate(offset, bytes.len())?;
        let read: Read = unsafe { transmute(FILE_READ) };
        let mut count = 0;
        let code = unsafe {
            read(
                &handle,
                &mut count,
                within,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
            )
        };
        if code < 0 || count as usize != bytes.len() {
            return Err(());
        }
        Ok(())
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), ()> {
        let (index, handle, within) = self.locate(offset, bytes.len())?;
        self.written[index] = true;
        self.put(handle, within, bytes, 0)
    }
    /// Only a file that was written is flushed: the one that holds Bank's
    /// own box is never sent a write of any kind.
    fn sync(&mut self) -> Result<(), ()> {
        let dummy = [0u8; 1];
        for index in 0..SLOTS {
            if let (Some(handle), true) = (self.handles[index], self.written[index]) {
                self.put(handle, 0, &dummy[..0], FLUSH_FLAGS)?;
                self.written[index] = false;
            }
        }
        Ok(())
    }
    /// Transporter replaces no file of Bank's.
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), ()> {
        Err(())
    }
}

/// A pointer the original would itself dereference: word aligned and inside
/// the process image or heap.
unsafe fn follow(base: *const u8, offset: usize) -> Option<*const u8> {
    let value = unsafe { base.add(offset).cast::<u32>().read() };
    if value & 3 != 0 || !(0x0010_0000..0x1000_0000).contains(&value) {
        return None;
    }
    Some(value as *const u8)
}

/// The transport box the original filled while reading the source game.
unsafe fn native_box(task: *const u8) -> Option<(&'static [u8], &'static [u8])> {
    unsafe {
        let manager = follow(task, TASK_MANAGER)?;
        let object = follow(manager, MANAGER_BANK_OBJECT)?;
        let accessor = follow(object, BANK_OBJECT_ACCESSOR)?;
        let body = follow(accessor, ACCESSOR_BODY)?;
        Some((
            slice::from_raw_parts(body.add(RECORDS_OFFSET), RECORDS_LEN),
            slice::from_raw_parts(body.add(TAGS_OFFSET), TRANSPORT_SLOTS),
        ))
    }
}

/// The file work of one job. Runs on the worker thread.
unsafe fn work(job: u32, task: *const u8) -> u32 {
    let Some(file) = (unsafe { File::open() }) else {
        return if job == JOB_CHECK {
            refusal_message(None).unwrap_or(0)
        } else {
            0
        };
    };
    let mut file = Sidecar::new(file, KIND);
    if job == JOB_CHECK {
        // The message to show, or 0 for none.
        return refusal_message(file.slots().ok().as_ref()).unwrap_or(0);
    }
    let Some((records, tags)) = (unsafe { native_box(task) }) else {
        return 0;
    };
    u32::from(transport::deliver(&mut file, records, tags).is_ok())
}

unsafe extern "aapcs" fn worker(_: u32) {
    let result = unsafe { work(*STATE.job.get(), *STATE.task.get()) };
    STATE.result.store(result, Ordering::Relaxed);
    STATE.phase.store(DONE, Ordering::Release);
    let exit: unsafe extern "aapcs" fn() = unsafe { transmute(EXIT_THREAD) };
    unsafe { exit() };
}

/// Starts `job` on the first call and answers `None` until it is finished.
/// If no thread can be created the job runs here, as it did before.
unsafe fn step(task: *const u8, job: u32) -> Option<u32> {
    type Create = unsafe extern "aapcs" fn(
        *mut u32,
        unsafe extern "aapcs" fn(u32),
        u32,
        usize,
        i32,
        i32,
    ) -> i32;
    match STATE.phase.load(Ordering::Acquire) {
        IDLE => {
            unsafe {
                *STATE.task.get() = task;
                *STATE.job.get() = job;
            }
            STATE.phase.store(RUNNING, Ordering::Release);
            let create: Create = unsafe { transmute(CREATE_THREAD) };
            let top = STACK.0.get() as usize + WORKER_STACK_SIZE;
            let code = unsafe { create(STATE.handle.get(), worker, 0, top, WORKER_PRIORITY, -2) };
            if code < 0 {
                STATE.phase.store(IDLE, Ordering::Release);
                return Some(unsafe { work(job, task) });
            }
            None
        }
        DONE => {
            unsafe { transporter_close_handle(*STATE.handle.get()) };
            STATE.phase.store(IDLE, Ordering::Release);
            Some(STATE.result.load(Ordering::Relaxed))
        }
        _ => None,
    }
}

/// Asks, when START was pressed and before the original searches for games,
/// whether Bank can take a delivery: the question the original put to the
/// server after a game was chosen. If not, the original's message for it is
/// shown here and the search ends as it does for "no game found": back to
/// the title screen.
/// # Safety
/// Called only from the stub of the patched site at 00246F48 (link.rs) with
/// the live game-search task in `r0`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_check(task: *mut u8) -> u32 {
    match unsafe { step(task, JOB_CHECK) } {
        None => SEARCH_PENDING,
        Some(0) => SEARCH_GO,
        Some(message) => {
            if unsafe { show(task, message) } {
                SEARCH_SHOWN
            } else {
                SEARCH_END
            }
        }
    }
}

/// Shows an original message the way the game search shows its own.
/// `false` when the task's dialog owner is not a pointer.
unsafe fn show(task: *const u8, message: u32) -> bool {
    let Some(ui) = (unsafe { follow(task, TASK_UI) }) else {
        return false;
    };
    let show: unsafe extern "aapcs" fn(*const u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
    unsafe { show(ui, message, 1) };
    true
}

/// Replaces the upload. Returns `DELIVER_DONE` when the delivery is written,
/// flushed, and read back; `DELIVER_PENDING` while that is in progress; and
/// `DELIVER_FAILED` when nothing was delivered, in which case the original
/// must not remove anything from the source game.
/// # Safety
/// Called only from the stub of the patched site at 0024A150 (link.rs) with
/// the live task in `r0`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_deliver(task: *mut u8) -> u32 {
    match unsafe { step(task, JOB_DELIVER) } {
        None => DELIVER_PENDING,
        Some(1) => DELIVER_DONE,
        Some(_) => DELIVER_FAILED,
    }
}
