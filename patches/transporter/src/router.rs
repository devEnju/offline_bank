//! The original's router `get_next_state(manager, current)` (`00242BA0`),
//! transcribed case by case (`00242BCC`..`00242E04`) for the PC tests of the
//! patch's routing (`navigation::destination`). Each case looks at the ended
//! step's outcome byte (`+0x30`). With the manager's flag `+0x1a`, a
//! cancelled step, it answers the disconnect step itself; the patch reads
//! that flag itself.

/// The step the original answers when the application is to end.
pub const EXIT: u32 = 0x13;

// Steps of the factory (`0019CEE4`), as numbers so that this reference does
// not share the patch's names: 2 title screen, 3 connect, 6 transfer,
// 7 game search, 8 game list, 9 reading the chosen game, 0xb the Bank step,
// 0xc the question, 0xf cancel, 0x10 disconnect, 0x11 reload, 0x12 failure.

/// The next step for step `current` that ended with `outcome`.
pub fn next(current: u32, outcome: u8) -> u32 {
    match (current, outcome) {
        (0, 4) => 2,
        (0, _) => 0x13,
        (1, 4) => 0x11,
        (1, _) => 0x13,
        (2, 3) => 7,
        (2, 0x12) => 1,
        (2, _) => 0x13,
        (3, 3) => 5,
        (3, 0xe) => 4,
        (5, 3) => 9,
        (6, 0xf) => 0x12,
        (7, 3) => 8,
        (8, 4) => 3,
        (9, 3) => 0xa,
        (0xa, 8) => 0xe,
        (0xa, 9) => 0xd,
        (0xb, 4) => 0xc,
        (0xb, 0xf) => 0x12,
        (0xb, 0x10) => 0xf,
        (0xc, 4) => 6,
        (0xc, 0x10) => 0xf,
        (0xd, 3) => 0xb,
        (0xd, 0xf) => 0x12,
        (0xe, 3) => 0xd,
        (0x10 | 0x11, _) => 2,
        (0x13.., _) => 0x13,
        _ => 0x10,
    }
}
