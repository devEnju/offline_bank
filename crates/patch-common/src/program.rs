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

    /// `(context, group, pane, message)`: a message with its values into a
    /// pane, as the title screen writes its version line. It loads the
    /// message into the first of the context's two work strings
    /// (`[context + 4]`), fills in its values into the second, and hands
    /// the second to the pane.
    const PANE_MESSAGE: usize;
    /// `(layout, pane)`: finds a pane in a layout.
    const FIND_PANE: usize;
    /// `(pane, string, -1)`: sets a text pane's text.
    const PANE_TEXT: usize;
}
