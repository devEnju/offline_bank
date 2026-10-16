//! The original's task router `002a5580(manager, current)`, transcribed
//! case by case for the PC tests of the patches' routing
//! (`navigation::destination` in each). The router is a jump table over the
//! task IDs `0..0x1e` (`002a55ac`); each case looks at the ended task's
//! outcome byte (`+0x30`).
//!
//! It also reads two flags of the manager: `+0x1b`, the language is known,
//! which every patch sets at start (docs/internals.md, "First start"), and
//! `+0x1c`, the scan found no game and the user chose Pokémon HOME. With
//! the manager's flag `+0x1a`, a cancelled task, it clears that flag and
//! answers cleanup (`0x14`) before it looks at the task; the patches read
//! that flag themselves.

/// The next task for task `current` that ended with `outcome`. `home` is
/// the manager's flag `+0x1c` as the router reads it; where the router sets
/// it (task 3), the answer does not depend on it.
pub fn next(current: u32, outcome: u8, home: bool) -> u32 {
    const CLEANUP: u32 = 0x14;
    match (current, outcome) {
        // 002a5624: language known, so not the language screen (1).
        (0, 5) => 2,
        (0, _) => 0x18,
        // 002a5650: sets the language flag.
        (1, 5) => 0x15,
        (1, _) => 0x18,
        // 002a5674: the start screen.
        (2, 4) => 3,
        (2, _) => 0x18,
        // 002a56a0: the game scan; 4 clears `home`, 0x17 sets it.
        (3, 4 | 0x17) => 5,
        (3, _) => CLEANUP,
        // 002a56cc: the main menu.
        (4, 0xc) => 0xa,
        (4, 0xd) => 0xe,
        (4, 0x17) => 0x1c,
        (4, _) => CLEANUP,
        // 002a56f8
        (5, 4) => 8,
        (5, 0x12) => 6,
        (5, _) => CLEANUP,
        // 002a571c
        (6, _) => CLEANUP,
        // 002a572c: saving.
        (7, 0x14) => 0x16,
        (7, _) => CLEANUP,
        // 002a5744
        (8, 4) => 0xf,
        (8, _) => CLEANUP,
        // 002a575c: opening the Bank.
        (9, 4) if home => 0x1c,
        (9, 4) => 4,
        (9, _) => CLEANUP,
        // 002a5780: game selection.
        (0xa, 5) => 0xb,
        (0xa, 0x13) => 4,
        (0xa, _) => CLEANUP,
        // 002a57a0: the scene set-up.
        (0xb, 9) => 0x12,
        (0xb, 0xa) => 0x11,
        (0xb, _) => CLEANUP,
        // 002a57dc: the reward guard.
        (0xc, 5) => 0xd,
        (0xc, 6 | 0xc) => 0x19,
        (0xc, _) => 0x13,
        // 002a5800: the reward claim.
        (0xd, 5) => 0x19,
        (0xd, _) => 0x13,
        // 002a581c
        (0xe, _) => 4,
        // 002a5824
        (0xf, 4) => 9,
        (0xf, _) => CLEANUP,
        // 002a57bc: loading the Bank.
        (0x10, 4) => 0xc,
        (0x10, 0x14) => 0x16,
        (0x10, _) => CLEANUP,
        // 002a583c
        (0x11, 4) => 0x10,
        (0x11, 0x14) => 0x16,
        (0x11, _) => CLEANUP,
        // 002a5860
        (0x12, 4) => 0x11,
        (0x12, 0x16) => 0x17,
        (0x12, _) => CLEANUP,
        // 002a5934
        (0x13 | 0x16, _) => CLEANUP,
        // 002a5648: after cleanup, the start screen.
        (0x14 | 0x15, _) => 2,
        // 002a5880
        (0x17, 4) => 0x11,
        (0x17, 9) => 0x12,
        (0x17, 0xe) => 0x18,
        (0x17, _) => CLEANUP,
        // 002a58bc: the box screen.
        (0x19, 4) => 7,
        (0x19, _) => 0x13,
        // 002a58e8, 002a58f0
        (0x1a, _) => 0,
        (0x1b, _) => 0x13,
        // 002a58f8, 002a5910
        (0x1c, 4) => 0x1d,
        (0x1d, 4) => 0x1b,
        (0x1c | 0x1d, _) => CLEANUP,
        // 002a5688: 0x18, and any ID past the table.
        _ => 0x18,
    }
}
