//! The original's router `get_next_state(manager, current)` (`00242BA0`),
//! transcribed case by case (`00242BCC`..`00242E04`) for the PC tests of the
//! patch's routing (`navigation::destination`). Each case looks at the ended
//! step's outcome byte (`+0x30`). With the manager's flag `+0x1a`, a
//! cancelled step, it answers the disconnect step itself; the patch reads
//! that flag itself.

use crate::navigation::*;

/// The step the original answers when the application is to end.
pub const EXIT: u32 = 0x13;

/// The next step for step `current` that ended with `outcome`.
pub fn next(current: u32, outcome: u8) -> u32 {
    match (current, outcome) {
        (0, 4) => TITLE,
        (0, _) => EXIT,
        (1, 4) => RELOAD,
        (1, _) => EXIT,
        (TITLE, 3) => SEARCH,
        (TITLE, 0x12) => 1,
        (TITLE, _) => EXIT,
        (CONNECT, 3) => 5,
        (CONNECT, 0xe) => 4,
        (5, 3) => READ,
        (TRANSFER, 0xf) => FAILURE,
        (SEARCH, 3) => LIST,
        (LIST, 4) => CONNECT,
        (READ, 3) => 0xa,
        (0xa, 8) => 0xe,
        (0xa, 9) => 0xd,
        (BANK_STEP, 4) => QUESTION,
        (BANK_STEP, 0xf) => FAILURE,
        (BANK_STEP, 0x10) => 0xf,
        (QUESTION, 4) => TRANSFER,
        (QUESTION, 0x10) => 0xf,
        (0xd, 3) => BANK_STEP,
        (0xd, 0xf) => FAILURE,
        (0xe, 3) => 0xd,
        (DISCONNECT | RELOAD, _) => TITLE,
        (0x13.., _) => EXIT,
        _ => DISCONNECT,
    }
}
