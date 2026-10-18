//! Poké Transporter 1.5 as the shared code reaches it
//! (`patch_common::program`).

use patch_common::program::Program;

/// Poké Transporter 1.5's SDK and engine, the same code as Bank's:
/// - thread creation `001E3F58`, which enters the thread through `0011F100`
///   and initializes native TLS there, its delegate, and the event
///   (`001E6404(event, 0)` makes one, `001E5220` signals it, `001E3CF4`
///   waits for it);
/// - the activity mask (`0022AEEC(bit)` sets a bit, `0011A5FC(bit)` clears
///   it), refused from the original's connect step to its disconnect step,
///   and the byte its HOME check (`0010AD84`) sets for the main loop
///   (`001044D0`);
/// - the title screen's call for its version line, message 0x29
///   (`0024BAE8`), and the two calls it hands the line to the pane with.
pub struct Transporter15;
impl Program for Transporter15 {
    const CREATE_THREAD: usize = 0x001e_3f58;
    const DELEGATE: [u32; 4] = [4, 0x0011_f098, 0x0011_f0b4, 0x0011_f0a8];
    const EVENT_INIT: usize = 0x001e_6404;
    const EVENT_SIGNAL: usize = 0x001e_5220;
    const EVENT_WAIT: usize = 0x001e_3cf4;
    const ACTIVITY_SET: usize = 0x0022_aeec;
    const ACTIVITY_CLEAR: usize = 0x0011_a5fc;
    const HOME_ACCEPTED: usize = 0x002f_49e9;
    const PANE_MESSAGE: usize = 0x001d_33cc;
    const FIND_PANE: usize = 0x0019_f834;
    const PANE_TEXT: usize = 0x0023_75d0;
}
