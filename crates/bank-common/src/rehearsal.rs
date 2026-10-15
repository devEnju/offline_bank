//! Test builds only: which rehearsal of an interrupted Save and Quit the
//! session makes (docs/building.md). A test build is one package; the
//! rehearsal is chosen in it before a game is picked, and a release holds
//! none of this.

use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rehearsal {
    /// Save and Quit as in a release.
    None = 0,
    /// Stops after the Bank has marked its save as in progress and before
    /// the game is written.
    StopBeforeGame = 1,
    /// Stops after the game is written and before the mark is cleared.
    StopAfterGame = 2,
    /// First writes the journal record again and again, then saves.
    TearRecord = 3,
    /// First writes the spare copy of the Bank file again and again, then
    /// saves.
    TearBoxes = 4,
}

/// How often `TearRecord` writes the journal record before the save goes on.
pub const TEAR_RECORD_PASSES: u32 = 150;
/// How often `TearBoxes` writes the spare copy of the Bank file. Even, so
/// that the payload ends as it began.
pub const TEAR_BOXES_PASSES: u32 = 30;

static CHOSEN: AtomicU8 = AtomicU8::new(Rehearsal::None as u8);

/// Chosen on the main thread before a session's game is picked; the worker
/// reads it at Save and Quit.
pub fn choose(rehearsal: Rehearsal) {
    CHOSEN.store(rehearsal as u8, Ordering::Release);
}

pub fn chosen() -> Rehearsal {
    match CHOSEN.load(Ordering::Acquire) {
        1 => Rehearsal::StopBeforeGame,
        2 => Rehearsal::StopAfterGame,
        3 => Rehearsal::TearRecord,
        4 => Rehearsal::TearBoxes,
        _ => Rehearsal::None,
    }
}
