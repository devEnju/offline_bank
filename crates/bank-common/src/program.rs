//! Bank 1.5 as the shared code reaches it (`patch_common::program`).

use patch_common::program::Program;

/// Bank 1.5's SDK and engine:
/// - thread creation `002320d4`, which enters the thread through `001211e4`
///   and initializes native TLS there, its delegate, and the event
///   (`00235c6c(event, 0)` makes one, `00234a00` signals it, `00231e70`
///   waits for it);
/// - the activity mask (`001d4d90(bit)` sets a bit, `00229eb4(bit)` clears
///   it), which the original sets during its own transfers, and the byte
///   its main loop (`0010ba04`) sets for a HOME press it accepts
///   (`0010ba7c`);
/// - the start screen's call for its version line, message 0x4E
///   (`002b49c4`), and the two calls it hands the line to the pane with.
pub struct Bank15;
impl Program for Bank15 {
    const CREATE_THREAD: usize = 0x0023_20d4;
    const DELEGATE: [u32; 4] = [4, 0x0012_117c, 0x0012_1198, 0x0012_118c];
    const EVENT_INIT: usize = 0x0023_5c6c;
    const EVENT_SIGNAL: usize = 0x0023_4a00;
    const EVENT_WAIT: usize = 0x0023_1e70;
    const ACTIVITY_SET: usize = 0x001d_4d90;
    const ACTIVITY_CLEAR: usize = 0x0022_9eb4;
    const HOME_ACCEPTED: usize = 0x0037_2989;
    const PANE_MESSAGE: usize = 0x001e_21c8;
    const FIND_PANE: usize = 0x001e_264c;
    const PANE_TEXT: usize = 0x0029_a628;
}
