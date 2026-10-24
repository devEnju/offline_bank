//! Bank 1.5 as the shared code reaches it (`patch_common::program`).

use patch_common::program::Program;

/// Bank 1.5's SDK and engine:
/// - its code, which ends where the start-up hook begins (`00313910`);
/// - thread creation `002320d4`, which enters the thread through `001211e4`
///   and initializes native TLS there, its delegate, and the event
///   (`00235c6c(event, 0)` makes one, `00234a00` signals it, `00231e70`
///   waits for it);
/// - the activity mask (`001d4d90(bit)` sets a bit, `00229eb4(bit)` clears
///   it), which the original sets during its own transfers, and the byte
///   its main loop (`0010ba04`) sets for a HOME press it accepts
///   (`0010ba7c`);
/// - the start screen's call for its version line, message 0x4E
///   (`002b49c4`), and the two calls it hands the line to the pane with;
/// - its task router `002a5580` and its factory's bound
///   (`task::TASK_COUNT`), and the vtable `003626FC` the Bank data object's
///   constructor `0010EA48` installs (`native_bank::VTABLE_ADDRESS`);
/// - the end of its loading panel (pane 0x97): `001d6200` stops a sound,
///   `001e7084` hides a pane, `001e69ac` unbinds an animation;
/// - its clock, as its minimum time `001d5bb0` reads it: `00229dc4` copies
///   the time at `00372fe0`, `002c8ea4` gives it in ms;
/// - its file system calls (docs/internals.md, "File access") and
///   its `fs:USER` session.
pub struct Bank15;
impl Program for Bank15 {
    const TEXT: core::ops::Range<u32> = 0x0010_0000..0x0031_3910;
    const CREATE_THREAD: usize = 0x0023_20d4;
    const DELEGATE: [u32; 4] = [4, 0x0012_117c, 0x0012_1198, 0x0012_118c];
    const EVENT_INIT: usize = 0x0023_5c6c;
    const EVENT_SIGNAL: usize = 0x0023_4a00;
    const EVENT_WAIT: usize = 0x0023_1e70;
    const ACTIVITY_SET: usize = 0x001d_4d90;
    const ACTIVITY_CLEAR: usize = 0x0022_9eb4;
    const HOME_ACCEPTED: usize = 0x0037_2989;
    const ROUTER: usize = 0x002a_5580;
    const TASK_COUNT: u32 = crate::task::TASK_COUNT;
    const BANK_VTABLE: u32 = crate::native_bank::VTABLE_ADDRESS;
    const PANE_MESSAGE: usize = 0x001e_21c8;
    const FIND_PANE: usize = 0x001e_264c;
    const PANE_TEXT: usize = 0x0029_a628;
    const STOP_SOUND: usize = 0x001d_6200;
    const PANE_SHOWN: usize = 0x001e_7084;
    const UNBIND_ANIMATION: usize = 0x001e_69ac;
    const LOADING_PANE: u32 = 0x97;
    const CLOCK_NOW: usize = 0x0022_9dc4;
    const CLOCK_MILLISECONDS: usize = 0x002c_8ea4;
    const OPEN_ARCHIVE: usize = 0x0020_a468;
    const CLOSE_ARCHIVE: usize = 0x0020_a438;
    const CREATE_FILE: Option<usize> = Some(0x0016_54f8);
    const DELETE_FILE: Option<usize> = Some(0x0016_5554);
    const OPEN_FILE: usize = 0x0016_57ec;
    const FILE_SIZE: usize = 0x0016_59ac;
    const FILE_READ: usize = 0x0016_58c8;
    const FILE_WRITE: usize = 0x0016_594c;
    const FILE_CLOSE: usize = 0x0016_5920;
    const FS_SESSION: usize = 0x0039_0100;
}
