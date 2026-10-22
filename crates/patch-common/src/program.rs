//! What the shared code needs of one original program: the addresses of
//! its SDK and engine functions, found by static disassembly of that
//! program's exact image. Bank 1.5 and Poké Transporter 1.5 are the same
//! SDK and engine at other addresses; each patch crate describes its
//! program once (`bank_common::program::Bank15`,
//! `transporter_payload::program::Transporter15`).

pub trait Program: 'static {
    /// The SDK's thread creation, `(thread, delegate, entry, &context,
    /// stack_top, priority, processor, 0)`. It enters the thread through
    /// the SDK trampoline, which initializes TLS and ends the thread with
    /// `svcExitThread`.
    const CREATE_THREAD: usize;
    /// The delegate it is given: its size, then its functions that copy the
    /// one context pointer onto the new stack, invoke `entry(context)`, and
    /// destroy nothing.
    const DELEGATE: [u32; 4];
    /// The SDK's event: `init(event, 0)` makes one, `signal(event)` signals
    /// it, `wait(event)` waits for it.
    const EVENT_INIT: usize;
    const EVENT_SIGNAL: usize;
    const EVENT_WAIT: usize;

    /// The original's activity mask: `set(bit)` sets a bit of it,
    /// `clear(bit)` clears it. While the mask is not zero the original's
    /// main loop refuses HOME and sleep.
    const ACTIVITY_SET: usize;
    const ACTIVITY_CLEAR: usize;
    /// The byte the main loop sets when HOME is pressed while the mask is
    /// zero, with nothing else done for the press then. The loop carries
    /// the press out some frames later, whatever the mask says by then.
    const HOME_ACCEPTED: usize;

    /// The original's task router, `ROUTER(manager, current)`: the next
    /// task for the one that has ended (`native::Ended`). Its factory makes
    /// the tasks `0..TASK_COUNT`.
    const ROUTER: usize;
    const TASK_COUNT: u32;
    /// The vtable the constructor of the Bank data object installs, the
    /// object a task reaches through its manager (`native::Task`). Both
    /// programs build it with the same constructor.
    const BANK_VTABLE: u32;

    /// `(context, group, pane, message)`: a message with its values into a
    /// pane, as the start screen writes its version line. It loads the
    /// message into the first of the context's two work strings
    /// (`[context + 4]`), fills in its values into the second, and hands
    /// the second to the pane.
    const PANE_MESSAGE: usize;
    /// `(layout, pane)`: finds a pane in a layout.
    const FIND_PANE: usize;
    /// `(pane, string, -1)`: sets a text pane's text.
    const PANE_TEXT: usize;

    /// The end of the shared UI's loading panel (`ui::end_loading`), the
    /// same code in both programs: `STOP_SOUND(sound, 0, -1)` stops a
    /// sound, `PANE_SHOWN(context, group, pane, shown)` shows or hides a
    /// pane, `UNBIND_ANIMATION(context, group, animation, 0)` unbinds an
    /// animation. `LOADING_PANE` is the panel's pane in group 0.
    const STOP_SOUND: usize;
    const PANE_SHOWN: usize;
    const UNBIND_ANIMATION: usize;
    const LOADING_PANE: u32;

    /// The original's clock, as its minimum times read it (`spinner::now`):
    /// `CLOCK_NOW(&moment)` copies the time the main loop keeps into an
    /// 8-byte moment, `CLOCK_MILLISECONDS(&moment)` gives it in ms.
    const CLOCK_NOW: usize;
    const CLOCK_MILLISECONDS: usize;

    /// The SDK's file system calls. The first argument is the `fs:USER`
    /// session for the archive calls and the open, the file handle for the
    /// others: `OPEN_ARCHIVE(session, &archive, id, path)`,
    /// `CLOSE_ARCHIVE(session, archive)`, `CREATE_FILE(session, 0, archive,
    /// path, 0, size)`, `DELETE_FILE(session, 0, archive, path)`,
    /// `OPEN_FILE(session, &file, 0, archive, path, flags, 0)`,
    /// `FILE_SIZE(file, &size)`, `FILE_READ(file, &count, offset, bytes,
    /// length)`, `FILE_WRITE(file, &count, offset, bytes, length, flags)`,
    /// `FILE_CLOSE(file)`. A program without a call has `None`.
    const OPEN_ARCHIVE: usize;
    const CLOSE_ARCHIVE: usize;
    const CREATE_FILE: Option<usize>;
    const DELETE_FILE: Option<usize>;
    const OPEN_FILE: usize;
    const FILE_SIZE: usize;
    const FILE_READ: usize;
    const FILE_WRITE: usize;
    const FILE_CLOSE: usize;
    /// The word that holds the program's `fs:USER` session, which its
    /// start-up sets up.
    const FS_SESSION: usize;

    /// The program's `fs:USER` session (`FS_SESSION`).
    /// # Safety
    /// Only in the program `Self` describes, after its start-up.
    unsafe fn fs_session() -> u32 {
        unsafe { (Self::FS_SESSION as *const u32).read_volatile() }
    }
}
