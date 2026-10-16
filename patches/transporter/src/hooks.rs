//! The entry points called from the patched original code, the worker thread
//! that does the file work, and Bank's files as the delivery rules reach
//! them. The original task calls an entry every frame until it answers;
//! the main thread waits for a file operation only if no worker thread can be
//! created, and the job then runs on it.

use crate::{
    fs::{terminated, transporter_close_handle, Archive, File, OPEN_READ_WRITE},
    layout::*,
    navigation,
    thread::{self, DONE, IDLE, RUNNING},
};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};
use offline_core::{files, sections::TRANSPORT_SLOTS, transport, Storage, METADATA_SIZE};

/// The original's "show this message and wait": `(ui, message, 1)`, as the
/// game search calls it for its own message 0 (`00246FFC`).
const SHOW_MESSAGE: usize = 0x0019_b50c;
/// The title screen's "put this message into its pane": `(context, group,
/// pane, message)`, as it calls it for its version line, message 0x29
/// (`0024BAE8`). It loads the message into the first of the context's two
/// work strings (`[context + 4]`), fills in its values into the second, and
/// hands the second to the pane (`0019F834` finds it in the layout of the
/// group, `[[context + 0x1C] + 8 * group]`; `002375D0` sets its text).
const PANE_MESSAGE: usize = 0x001d_33cc;
const PANE_FIND: usize = 0x0019_f834;
const PANE_TEXT: usize = 0x0023_75d0;
const CONTEXT_STRINGS: usize = 0x4;
const CONTEXT_LAYOUTS: usize = 0x1c;
/// What the version line says behind "Ver. 1.5".
const VERSION_SUFFIX: &[u8] = b"-offline";
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
/// The original's `svcCreateThread(out, entry, arg, stack_top, priority,
/// processor)` and `svcExitThread()` wrappers.
const CREATE_THREAD: usize = 0x0010_e250;
const EXIT_THREAD: usize = 0x0011_f140;
/// One below the main thread, so the worker runs while the main thread waits
/// for the next frame.
const WORKER_PRIORITY: i32 = 0x31;

extern "aapcs" {
    /// `svc 0x24` for one handle, behind an ordinary call boundary (see
    /// link.rs).
    fn transporter_wait_thread(handle: u32, timeout: i64) -> i32;
}

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

/// A pair of files in Bank's extdata, open for reading and writing and
/// addressed as one range: the two journal records, or the two copies of
/// the transport box. Bank keeps each in a file of its own, because the
/// console leaves a file that was being written unreadable when the power
/// fails.
///
/// A file that is missing or cannot be opened makes every read of it fail,
/// and so the check and the delivery refuse: Transporter neither creates
/// nor replaces a file. Bank's next Save and Quit does.
struct Pair {
    files: [Option<File>; 2],
    written: [bool; 2],
    /// Length of each of the two files.
    len: u64,
}

/// Bank's files as the delivery rules reach them (`transport::Files`): one
/// pair at a time, so that never more than two files are open.
struct BankFiles;
impl transport::Files for BankFiles {
    type Storage = Pair;
    fn journal(&mut self) -> Option<Pair> {
        unsafe { Pair::open(files::JOURNAL, METADATA_SIZE as u64) }
    }
    fn mover(&mut self) -> Option<Pair> {
        unsafe { Pair::open(files::MOVER, u64::from(transport::LEN)) }
    }
}
impl Pair {
    /// `None` when Bank's extdata or both files do not exist, access is
    /// refused, or a file is not exactly `len` bytes, the size Bank creates.
    unsafe fn open(names: [&str; 2], len: u64) -> Option<Self> {
        // Whatever was opened is closed again if this ends early.
        let mut pair = Self {
            files: [None, None],
            written: [false; 2],
            len,
        };
        for (file, name) in pair.files.iter_mut().zip(names) {
            let (path, length) = terminated(name);
            *file = unsafe { File::open(Archive::BankExtdata, &path[..length], OPEN_READ_WRITE) };
            // One of another size is not a file of this layout.
            if file.as_ref().is_some_and(|file| file.size() != Some(len)) {
                return None;
            }
        }
        pair.files.iter().any(Option::is_some).then_some(pair)
    }
    /// The file that holds `offset..offset + length`, and where in it.
    fn locate(&self, offset: u64, length: usize) -> Result<(usize, &File, u64), ()> {
        let index = (offset / self.len) as usize;
        let within = offset % self.len;
        if index >= self.files.len() || within + length as u64 > self.len {
            return Err(());
        }
        Ok((index, self.files[index].as_ref().ok_or(())?, within))
    }
}
impl Storage for Pair {
    type Error = ();
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), ()> {
        let (_, file, within) = self.locate(offset, bytes.len())?;
        file.read(within, bytes.as_mut_ptr(), bytes.len() as u32)
            .then_some(())
            .ok_or(())
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), ()> {
        let (index, file, within) = self.locate(offset, bytes.len())?;
        let written = file.write(within, bytes.as_ptr(), bytes.len() as u32, 0);
        self.written[index] = true;
        written.then_some(()).ok_or(())
    }
    /// Only a file that was written is flushed: the one that holds Bank's
    /// own box is never sent a write of any kind.
    fn sync(&mut self) -> Result<(), ()> {
        for (file, written) in self.files.iter().zip(&mut self.written) {
            if let (Some(file), true) = (file, *written) {
                if !file.flush() {
                    return Err(());
                }
                *written = false;
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
/// the message to show, or 0 for none; the delivery answers 1 when it is
/// written.
unsafe fn work(job: u32, task: *const u8) -> u32 {
    if job == JOB_CHECK {
        return transport::check(&mut BankFiles)
            .err()
            .map_or(0, refusal_message);
    }
    let Some((records, tags)) = (unsafe { native_box(task) }) else {
        return 0;
    };
    u32::from(transport::deliver(&mut BankFiles, records, tags).is_ok())
}

unsafe extern "aapcs" fn worker(_: u32) {
    let result = unsafe { work(*STATE.job.get(), *STATE.task.get()) };
    STATE.result.store(result, Ordering::Relaxed);
    STATE.phase.store(DONE, Ordering::Release);
    let exit: unsafe extern "aapcs" fn() = unsafe { transmute(EXIT_THREAD) };
    unsafe { exit() };
}

/// Starts `job` on the first call and answers `None` until it is finished.
/// Carries out `thread::step`; if no thread can be created, the job runs
/// here.
unsafe fn step(task: *const u8, job: u32) -> Option<u32> {
    type Create = unsafe extern "aapcs" fn(
        *mut u32,
        unsafe extern "aapcs" fn(u32),
        u32,
        usize,
        i32,
        i32,
    ) -> i32;
    let handle = unsafe { *STATE.handle.get() };
    let phase = STATE.phase.load(Ordering::Acquire);
    match thread::step(phase, || unsafe { transporter_wait_thread(handle, 0) }) {
        thread::Step::Start => {
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
        thread::Step::Collect(next) => {
            unsafe { transporter_close_handle(handle) };
            STATE.phase.store(next, Ordering::Release);
            Some(STATE.result.load(Ordering::Relaxed))
        }
        thread::Step::Here => Some(unsafe { work(job, task) }),
        thread::Step::Pending => None,
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

/// Shows an original message the way the game search shows its own. It
/// waits for an answer, so HOME and sleep work on it
/// (`transporter_message_waits`). `false` when the task's dialog owner is
/// not a pointer.
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

/// Replaces the title screen's call that writes its version line
/// (`0024BAE8`): the original call, then the same line with the patch's name
/// behind it, added to the original's own string in place and handed to the
/// pane with the original's two calls. Without room for it the line stays
/// the original's.
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_version(
    context: *const u8,
    group: u32,
    pane: u32,
    message: u32,
) {
    unsafe {
        let original: unsafe extern "aapcs" fn(*const u8, u32, u32, u32) = transmute(PANE_MESSAGE);
        original(context, group, pane, message);
        let Some(text) = follow(context, CONTEXT_STRINGS).and_then(|strings| follow(strings, 4))
        else {
            return;
        };
        let (Some(data), Some(layouts)) = (follow(text, 4), follow(context, CONTEXT_LAYOUTS))
        else {
            return;
        };
        let data = data.cast_mut().cast::<u16>();
        let room = usize::from(text.add(8).cast::<u16>().read());
        if text.add(0xc).read() == 0 {
            return;
        }
        let mut len = 0;
        while len < room && data.add(len).read() != 0 {
            len += 1;
        }
        if len == 0 || len + VERSION_SUFFIX.len() >= room {
            return;
        }
        for (index, &unit) in VERSION_SUFFIX.iter().enumerate() {
            data.add(len + index).write(u16::from(unit));
        }
        len += VERSION_SUFFIX.len();
        data.add(len).write(0);
        text.cast_mut().add(0xa).cast::<u16>().write(len as u16);
        let find: unsafe extern "aapcs" fn(u32, u32) -> *const u8 = transmute(PANE_FIND);
        let set: unsafe extern "aapcs" fn(*const u8, *const u8, i32) = transmute(PANE_TEXT);
        let layout = layouts.add(group as usize * 8).cast::<u32>().read();
        let found = find(layout, pane);
        if !found.is_null() {
            set(found, text, -1);
        }
    }
}
