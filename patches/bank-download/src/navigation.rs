//! Task routing after native 002a5580 has chosen its destination.
//!
//! The session is the original's from the first to the last task: game
//! scan (3), connect and sign-in (5, 8, 0xf), open Bank (9), main menu (4),
//! game selection and scene (0xa, 0xb), the checks of the last session
//! (0x11, 0x12, 0x17), load (0x10), the reward step (0xc), leaving without
//! saving (0x13), cleanup (0x14). The patch changes three answers:
//!
//! - After the game scan task 9 runs once before anything is contacted, to
//!   look for a Bank on the card (`Stage::Looking`).
//! - When it found none, the original's connect follows (`Stage::Found`
//!   says the look is over and nothing was there).
//! - After cleanup, once a result exists, task 9 runs again to show it.
//!
//! The reward step's update is the patch's own. It ends with the original's
//! outcome for "cancel", 0x15, which the original router answers with 0x13
//! itself (002a57f4).

pub const SCAN: u32 = 3;
pub const CONNECT: u32 = 5;
pub const OPEN: u32 = 9;
pub const CLEANUP: u32 = 0x14;

/// The outcome of the original's reward step when the user cancels, for
/// which the original router answers "leave without saving".
pub const CANCELLED: u8 = 0x15;

/// What the patch knows when a task has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The card has not been looked at.
    Looking,
    /// The look has just ended and found no Bank: go online.
    NoneFound,
    /// Online; the original decides.
    Online,
    /// A result exists and is to be shown.
    Decided,
}

/// `native` is what the original router returned for the task that ended.
pub fn destination(current: u32, native: u32, stage: Stage) -> u32 {
    match (current, stage) {
        // The original answers 5, its connect, or cleanup when its scan
        // found nothing it can go on with; only the first is changed.
        (SCAN, Stage::Looking) if native == CONNECT => OPEN,
        (OPEN, Stage::NoneFound) => CONNECT,
        (CLEANUP, Stage::Decided) => OPEN,
        _ => native,
    }
}

/// The stage after `destination` was asked with `stage`: the one-frame
/// "none found" becomes "online" once the connect was chosen.
pub fn next_stage(current: u32, stage: Stage) -> Stage {
    match (current, stage) {
        (OPEN, Stage::NoneFound) => Stage::Online,
        _ => stage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original router, as far as this patch's session reaches
    /// (002a5624..002a5934). `home` is the manager's flag `+0x1c`, set when
    /// the scan found no game and the user chose to move to Pokémon HOME.
    fn original(current: u32, outcome: u8, home: bool) -> u32 {
        match (current, outcome) {
            (2, 3) => 0x18,
            (2, 4) => 3,
            (3, 3) => CLEANUP,
            (3, 4 | 0x17) => 5,
            (5, 4) => 8,
            (5, 0x12) => 6,
            (8, 4) => 0xf,
            (0xf, 4) => 9,
            (9, 4) if home => 0x1c,
            (9, 4) => 4,
            (4, 0xc) => 0xa,
            (4, 0xd) => 0xe,
            (4, 0x17) => 0x1c,
            (0xa, 5) => 0xb,
            (0xa, 0x13) => 4,
            (0xb, 9) => 0x12,
            (0xb, 0xa) => 0x11,
            (0x11, 4) => 0x10,
            (0x11 | 0x10 | 7, 0x14) => 0x16,
            (0x12 | 0x17, 4) => 0x11,
            (0x12, 0x16) => 0x17,
            (0x17, 9) => 0x12,
            (0x17, 0xe) => 0x18,
            (0x10, 4) => 0xc,
            (0xc, 5) => 0xd,
            (0xc, 6 | 0xc) | (0xd, 5) => 0x19,
            (0xc | 0xd | 0x19, 0x15) => 0x13,
            (0x19, 4) => 7,
            (0x14 | 0x15, _) => 2,
            _ => CLEANUP,
        }
    }

    #[test]
    fn the_card_is_looked_at_before_anything_is_contacted() {
        assert_eq!(original(SCAN, 4, false), CONNECT);
        assert_eq!(destination(SCAN, CONNECT, Stage::Looking), OPEN);
        // A scan that failed ends as the original ends it.
        assert_eq!(destination(SCAN, CLEANUP, Stage::Looking), CLEANUP);
        // Nothing on the card: the original's connect, and from then on
        // every answer is the original's.
        assert_eq!(destination(OPEN, 4, Stage::NoneFound), CONNECT);
        assert_eq!(next_stage(OPEN, Stage::NoneFound), Stage::Online);
        assert_eq!(next_stage(SCAN, Stage::Looking), Stage::Looking);
    }

    #[test]
    fn online_every_answer_is_the_originals() {
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for home in [false, true] {
                    let native = original(current, outcome, home);
                    assert_eq!(destination(current, native, Stage::Online), native);
                    assert_eq!(next_stage(current, Stage::Online), Stage::Online);
                }
            }
        }
    }

    #[test]
    fn the_session_runs_from_sign_in_to_leaving_without_saving() {
        let mut current = OPEN;
        let mut stage = Stage::NoneFound;
        let mut seen = [0u32; 16];
        let mut count = 0;
        // The outcome each task of a session without trouble ends with; the
        // reward step ends as cancelled.
        loop {
            let outcome = match current {
                4 => 0xc,
                0xa => 5,
                0xb => 0xa,
                0xc => CANCELLED,
                _ => 4,
            };
            let native = original(current, outcome, false);
            let next = destination(current, native, stage);
            stage = next_stage(current, stage);
            seen[count] = next;
            count += 1;
            if next == CLEANUP {
                break;
            }
            current = next;
        }
        assert_eq!(
            seen[..count],
            [CONNECT, 8, 0xf, OPEN, 4, 0xa, 0xb, 0x11, 0x10, 0xc, 0x13, CLEANUP]
        );
        // The box screen, the reward claim and saving are never reached.
        assert!(seen[..count]
            .iter()
            .all(|task| ![0xd, 0x19, 7].contains(task)));
    }

    #[test]
    fn a_result_is_shown_after_cleanup_and_only_then() {
        assert_eq!(destination(CLEANUP, 2, Stage::Decided), OPEN);
        for stage in [Stage::Looking, Stage::NoneFound, Stage::Online] {
            assert_eq!(destination(CLEANUP, 2, stage), 2);
        }
        // With a result the session still ends the original's way first.
        for current in (0..=0x1d).filter(|&task| task != CLEANUP) {
            for outcome in 0..=u8::MAX {
                let native = original(current, outcome, false);
                assert_eq!(destination(current, native, Stage::Decided), native);
            }
        }
    }
}
