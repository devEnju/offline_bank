//! The entry points called from the patched original code, the worker thread
//! that does the file work, and the file adapter over Transporter's own SDK
//! wrappers. The original task calls an entry every frame until it answers;
//! the main thread never waits for a file operation.

use crate::{layout::*, navigation};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};
use offline_core::{
    numbers::{self, Rest},
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
/// The original's "show this message": `(ui, message, wait)`. With 1 the
/// dialog ends when its last page is acknowledged, as every one of its
/// tasks calls it; with 0 it stays, as its message 0x1F does (`002452DC`).
const SHOW_MESSAGE: usize = 0x0019_b50c;
/// One frame of a dialog, `(context)` with the context at `ui + 0x5C`: 4
/// once a waiting dialog was acknowledged (`0024700C`).
const DIALOG_POLL: usize = 0x0019_b4bc;
const UI_CONTEXT: usize = 0x5c;
const ACKNOWLEDGED: u32 = 4;
/// The dialog's own string object at `ui + 0x8C`, which the show function
/// fills and hands to the dialog through `0019C7D4(context, string)`. It is
/// the string class of Bank's dialogs: data at `+4`, room at `+8`, length at
/// `+0xA` (both in units), a flag at `+0xC` that it has a buffer.
const UI_TEXT: usize = 0x8c;
const DIALOG_TEXT: usize = 0x0019_c7d4;
/// The original's router, `get_next_state(manager, current)`, and what it
/// reads: the finished task at `manager + 0x10` with its outcome byte at
/// `+0x30`, and the manager's "cancelled" flag, which it clears.
const NEXT_STATE: usize = 0x0024_2ba0;
const MANAGER_TASK: usize = 0x10;
const MANAGER_CANCELLED: usize = 0x1a;
const TASK_OUTCOME: usize = 0x30;
/// The original's HOME and sleep block: `0022AEEC(bit)` sets a bit of its
/// activity mask and `0011A5FC(bit)` clears it. While the mask is not zero
/// the original refuses both, as it did from its connect step to its
/// disconnect step.
const BLOCK_SET: usize = 0x0022_aeec;
const BLOCK_CLEAR: usize = 0x0011_a5fc;
/// Set by the original (0010AD84) when HOME is pressed while the mask is
/// zero, and nothing else is done for the press then. Its main loop
/// (001044D0) carries the press out some frames later, whatever the mask
/// says by then.
const HOME_ACCEPTED: *mut u8 = 0x002f_49e9 as *mut u8;
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
/// runs, the worker reads `task` and `job` and writes the atomics.
#[repr(C)]
struct Shared {
    phase: AtomicU32,
    result: AtomicU32,
    /// The two numbers of a refusal that has them, behind `numbered`.
    numbers: [AtomicU32; 2],
    numbered: AtomicU32,
    handle: UnsafeCell<u32>,
    task: UnsafeCell<*const u8>,
    job: UnsafeCell<u32>,
}
// SAFETY: see the access rule above; `phase` orders every hand-over.
unsafe impl Sync for Shared {}
static STATE: Shared = Shared {
    phase: AtomicU32::new(IDLE),
    result: AtomicU32::new(0),
    numbers: [AtomicU32::new(0), AtomicU32::new(0)],
    numbered: AtomicU32::new(0),
    handle: UnsafeCell::new(0),
    task: UnsafeCell::new(core::ptr::null()),
    job: UnsafeCell::new(0),
};
/// What is on screen once the check has refused (`layout::Refused`). Only
/// the main thread reads and writes it.
static REFUSED: AtomicU32 = AtomicU32::new(Refused::No as u32);

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
    /// `Err` when Bank's extdata or both files do not exist or access is
    /// refused, with the result of the first open that failed, or when a
    /// file is not exactly the size Bank creates.
    unsafe fn open() -> Result<Self, Found> {
        // Whatever was opened is closed again if this ends early.
        let mut file = Self {
            handles: [None; SLOTS],
            written: [false; SLOTS],
        };
        let mut failed = None;
        for (handle, path) in file.handles.iter_mut().zip(MOVER_PATHS) {
            match unsafe { Self::open_one(path) } {
                Ok(opened) => *handle = Some(opened),
                Err(Found::Closed(code)) => {
                    failed.get_or_insert(code);
                }
                Err(found) => return Err(found),
            }
        }
        match failed {
            Some(code) if file.handles.iter().all(Option::is_none) => Err(Found::Closed(code)),
            _ => Ok(file),
        }
    }
    /// `Found::Closed` for a file that is not there or cannot be opened,
    /// `Found::Size` for one of another size: that is not a file of this
    /// layout.
    unsafe fn open_one(path: &[u8]) -> Result<u32, Found> {
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
        if code < 0 {
            return Err(Found::Closed(code as u32));
        }
        if handle == 0 {
            return Err(Found::Closed(NO_HANDLE));
        }
        let size: Size = unsafe { transmute(FILE_SIZE) };
        let mut length = 0;
        let code = unsafe { size(&handle, &mut length) };
        if code < 0 || length != KIND.slot_len() {
            unsafe { close(handle) };
            return Err(if code < 0 {
                Found::Closed(code as u32)
            } else {
                Found::Size(length)
            });
        }
        Ok(handle)
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

/// The file work of one job. Runs on the worker thread. The check answers
/// the message to show, or 0 for none, and leaves the numbers that go with
/// it; the delivery answers 1 when it is written.
unsafe fn work(job: u32, task: *const u8) -> u32 {
    let file = unsafe { File::open() };
    if job == JOB_CHECK {
        let found = match file {
            Ok(file) => Found::Slots(Sidecar::new(file, KIND).slots().ok()),
            Err(found) => found,
        };
        let Some(refusal) = refusal(&found) else {
            return 0;
        };
        let (first, second) = refusal.numbers.unwrap_or_default();
        STATE.numbers[0].store(first, Ordering::Relaxed);
        STATE.numbers[1].store(second, Ordering::Relaxed);
        STATE
            .numbered
            .store(u32::from(refusal.numbers.is_some()), Ordering::Relaxed);
        return refusal.message;
    }
    let (Ok(file), Some((records, tags))) = (file, unsafe { native_box(task) }) else {
        return 0;
    };
    let mut file = Sidecar::new(file, KIND);
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

/// Sets or clears bit 1 of the original's activity mask through the
/// original functions.
///
/// The mask only refuses new HOME presses. One accepted just before it was
/// set would still be carried out, so setting the mask also takes that press
/// back: accepting it did nothing but set the one byte cleared here. From
/// then on the original refuses every press itself, and the HOME Menu, and
/// closing Transporter from it, never meet a file being read or written.
unsafe fn refuse_home(refuse: bool) {
    let address = if refuse { BLOCK_SET } else { BLOCK_CLEAR };
    let mark: unsafe extern "aapcs" fn(u32) = unsafe { transmute(address) };
    unsafe { mark(1) };
    if refuse {
        unsafe { HOME_ACCEPTED.write_volatile(0) };
    }
}

/// Replaces the one call of the original's router (`00242EF8`). The
/// original is asked first; its answer is changed where the offline flow
/// differs, and HOME and sleep are refused or let through by the kind of
/// the step that comes next (`navigation`).
/// # Safety
/// Called only from the patched call site, on the main thread, with the
/// live manager whose current task has ended.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_next(manager: *mut u8, current: u32) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = unsafe { transmute(NEXT_STATE) };
    // Read before the original runs, which clears the flag.
    let cancelled = unsafe { manager.add(MANAGER_CANCELLED).read() } != 0;
    let outcome = unsafe { follow(manager, MANAGER_TASK) }
        .map(|task| unsafe { task.add(TASK_OUTCOME).read() });
    let native = unsafe { original(manager, current) };
    // A step has ended: no refusal of the check is on screen any more.
    REFUSED.store(Refused::No as u32, Ordering::Relaxed);
    let Some(outcome) = outcome else {
        return native;
    };
    let next = navigation::destination(current, outcome, native, cancelled);
    unsafe { refuse_home(navigation::loads(next)) };
    next
}

/// The original's "show a message" function was entered with a message that
/// waits for an answer (the stub of the patched site at 0019B50C, link.rs).
/// Such a message is where a step that reads or writes has nothing left to
/// do but end, so the refusal the router entry began for it ends here. The
/// original's own bit for a Virtual Console save that is being written is
/// not this one and stays as it is.
/// # Safety
/// Called only from that stub, on the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_message_waits() {
    unsafe { refuse_home(false) };
}

/// The game list is set up and on screen: every save has been read, and it
/// waits for the user.
/// # Safety
/// Called only from the stub of the patched site at 002445EC (link.rs), on
/// the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_ready() {
    unsafe { refuse_home(false) };
}

/// Asks, when START was pressed and before the original searches for games,
/// whether Bank can take a delivery: the question the original put to the
/// server after a game was chosen. If not, the original's message for it is
/// shown here, and once it was acknowledged the original's line about the
/// HOME Menu, which stays: no session can begin until Bank was opened. The
/// search is held in its first step for that, which calls this every frame.
/// The "open Pokémon Bank" message has several causes and carries two
/// numbers for them, on its first page and under the line.
/// # Safety
/// Called only from the stub of the patched site at 00246F48 (link.rs) with
/// the live game-search task in `r0`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_check(task: *mut u8) -> u32 {
    let numbers = (STATE.numbered.load(Ordering::Relaxed) != 0).then(|| {
        (
            STATE.numbers[0].load(Ordering::Relaxed),
            STATE.numbers[1].load(Ordering::Relaxed),
        )
    });
    let refused = Refused::of(REFUSED.load(Ordering::Relaxed));
    if refused != Refused::No {
        let Some(ui) = (unsafe { follow(task, TASK_UI) }) else {
            return SEARCH_END;
        };
        let Some(context) = (unsafe { follow(ui, UI_CONTEXT) }) else {
            return SEARCH_END;
        };
        // Each frame of a dialog is this call, also of one that stays.
        let poll: unsafe extern "aapcs" fn(*const u8) -> u32 = unsafe { transmute(DIALOG_POLL) };
        let next = refused.next(unsafe { poll(context) } == ACKNOWLEDGED);
        if next != refused {
            unsafe { show(ui, MESSAGE_HOME, false, numbers, Rest::Dropped) };
            REFUSED.store(next as u32, Ordering::Relaxed);
        }
        return SEARCH_PENDING;
    }
    match unsafe { step(task, JOB_CHECK) } {
        None => SEARCH_PENDING,
        Some(0) => SEARCH_GO,
        Some(message) => {
            let Some(ui) = (unsafe { follow(task, TASK_UI) }) else {
                return SEARCH_END;
            };
            // Nothing more is read: HOME and sleep work on the message.
            unsafe { refuse_home(false) };
            unsafe { show(ui, message, true, numbers, Rest::Kept) };
            REFUSED.store(Refused::Message as u32, Ordering::Relaxed);
            SEARCH_PENDING
        }
    }
}

/// Shows an original message the way the game search shows its own, and
/// puts `numbers` on the third line of its first page, in the dialog's own
/// string, which is handed to the dialog again as the show function does.
/// Without room for them the message stays as it is.
unsafe fn show(ui: *const u8, message: u32, wait: bool, numbers: Option<(u32, u32)>, rest: Rest) {
    let show: unsafe extern "aapcs" fn(*const u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
    unsafe { show(ui, message, u32::from(wait)) };
    let Some((first, second)) = numbers else {
        return;
    };
    unsafe {
        let (Some(context), Some(text)) = (follow(ui, UI_CONTEXT), follow(ui, UI_TEXT)) else {
            return;
        };
        let Some(data) = follow(text, 4) else {
            return;
        };
        let (text, data) = (text.cast_mut(), data.cast_mut().cast::<u16>());
        let room = usize::from(text.add(8).cast::<u16>().read());
        let len = usize::from(text.add(0xa).cast::<u16>().read());
        if text.add(0xc).read() == 0 || len == 0 || len >= room || data.add(len).read() != 0 {
            return;
        }
        let units = slice::from_raw_parts_mut(data, room);
        let Some(len) = numbers::put(units, len, first, second, rest) else {
            return;
        };
        units[len] = 0;
        text.add(0xa).cast::<u16>().write(len as u16);
        let begin: unsafe extern "aapcs" fn(*const u8, *mut u8) = transmute(DIALOG_TEXT);
        begin(context, text);
    }
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
