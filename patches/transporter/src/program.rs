//! Poké Transporter 1.5 as the shared code reaches it
//! (`patch_common::program`).

use patch_common::program::Program;

/// Poké Transporter 1.5's SDK and engine, the same code as Bank's:
/// - thread creation `001E3F58`, which enters the thread through `0011F100`
///   and initializes native TLS there, its delegate, and the event
///   (`001E6404(event, 0)` makes one, `001E5220` signals it, `001E3CF4`
///   waits for it);
/// - the activity mask (`0022AEEC(bit)` sets a bit, `0011A5FC(bit)` clears
///   it), refused from the original's connect task to its disconnect task,
///   and the byte its HOME check (`0010AD84`) sets for the main loop
///   (`001044D0`);
/// - the start screen's call for its version line, message 0x29
///   (`0024BAE8`), and the two calls it hands the line to the pane with;
/// - its router `get_next_state` `00242BA0` and its factory's bound
///   (`task::TASK_COUNT`), and the vtable `002E6DD0` the Bank data object's
///   constructor `0010D8EC` installs (`task::BANK_VTABLE`);
/// - the end of its loading panel (pane 0x1D), the same code as Bank's:
///   `0019B6B0` stops a sound, `0019C884` hides a pane, `0019B604` unbinds
///   an animation;
/// - its clock, as its minimum time `0019B758` reads it: `00233854`
///   copies the time at `002F4E38`, `0025A77C` gives it in ms;
/// - its file system calls, the same code as Bank's (found by comparing
///   their instructions with Bank's), without create and delete: the patch
///   neither creates nor deletes a file; and its `fs:USER` session.
pub struct Transporter15;
impl Program for Transporter15 {
    const TEXT: core::ops::Range<u32> = 0x0010_0000..0x0028_d1ac;
    const CREATE_THREAD: usize = 0x001e_3f58;
    const DELEGATE: [u32; 4] = [4, 0x0011_f098, 0x0011_f0b4, 0x0011_f0a8];
    const EVENT_INIT: usize = 0x001e_6404;
    const EVENT_SIGNAL: usize = 0x001e_5220;
    const EVENT_WAIT: usize = 0x001e_3cf4;
    const ACTIVITY_SET: usize = 0x0022_aeec;
    const ACTIVITY_CLEAR: usize = 0x0011_a5fc;
    const HOME_ACCEPTED: usize = 0x002f_49e9;
    const ROUTER: usize = 0x0024_2ba0;
    const TASK_COUNT: u32 = crate::task::TASK_COUNT;
    const BANK_VTABLE: u32 = crate::task::BANK_VTABLE;
    const PANE_MESSAGE: usize = 0x001d_33cc;
    const FIND_PANE: usize = 0x0019_f834;
    const PANE_TEXT: usize = 0x0023_75d0;
    const STOP_SOUND: usize = 0x0019_b6b0;
    const PANE_SHOWN: usize = 0x0019_c884;
    const UNBIND_ANIMATION: usize = 0x0019_b604;
    const LOADING_PANE: u32 = 0x1d;
    const CLOCK_NOW: usize = 0x0023_3854;
    const CLOCK_MILLISECONDS: usize = 0x0025_a77c;
    const OPEN_ARCHIVE: usize = 0x001d_b7dc;
    const CLOSE_ARCHIVE: usize = 0x001d_b7ac;
    const CREATE_FILE: Option<usize> = None;
    const DELETE_FILE: Option<usize> = None;
    const OPEN_FILE: usize = 0x0015_9230;
    const FILE_SIZE: usize = 0x0015_93f0;
    const FILE_READ: usize = 0x0015_930c;
    const FILE_WRITE: usize = 0x0015_9390;
    const FILE_CLOSE: usize = 0x0015_9364;
    const FS_SESSION: usize = 0x0031_1f80;
}
