//! Task routing after native 002a5580 has chosen its destination.
//!
//! The session keeps local preflight and native setup (3), connect and sign-in (5, 8,
//! 0xf), open Bank (9), the load the original made for Pokémon HOME (0x1c),
//! which reads and saves no game, the reward step (0xc), leaving without
//! saving (0x13), cleanup (0x14):
//!
//! - Task 3 first looks for a local Bank (`Stage::Looking`). With none,
//!   only archive preparation and first-start bookkeeping run
//!   (`Stage::Preparing`); game detection is skipped before connecting.
//! - After task 9 has opened the Bank online, the HOME load follows, also
//!   where the original would show its main menu: no game is chosen. The
//!   original goes there itself when its scan found no game (002a576c).
//! - After that load the reward step follows, where the original would
//!   release the session (0x1d) and show its HOME screens (0x1b).
//! - After cleanup, once a result exists, task 9 runs again to show it.
//!   A written Bank is shown with the original's message for a successful
//!   save, after which cleanup leads to the start screen, and the next START
//!   looks at the card again.
//!
//! The reward step's update is the patch's own. It ends with the original's
//! outcome for "cancel", 0x15, which the original router answers with 0x13
//! itself (002a57f4): the same release the original's 0x1d makes.

pub const SCAN: u32 = 3;
pub const CONNECT: u32 = 5;
pub const OPEN: u32 = 9;
pub const MENU: u32 = 4;
/// The reward step, whose update writes the files.
pub const WRITE: u32 = 0xc;
pub const CLEANUP: u32 = 0x14;
/// The load without a game (factory 002a63a8: the load's task with its
/// mode byte set), and the release the original follows it with.
pub const HOME_LOAD: u32 = 0x1c;
pub const HOME_RELEASE: u32 = 0x1d;

/// The outcome of the original's reward step when the user cancels, for
/// which the original router answers "leave without saving".
pub const CANCELLED: u8 = 0x15;

/// What the patch knows when a task has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The card has not been looked at.
    Looking,
    /// No local Bank: run native archive/first-start setup without scanning games.
    Preparing,
    /// Online; the original decides.
    Online,
    /// A result exists and is to be shown.
    Decided,
    /// The original's message for a successful save was shown: back to the
    /// start screen.
    Saved,
}

/// `native` is what the original router returned for the task that ended.
pub fn destination(current: u32, native: u32, stage: Stage) -> u32 {
    match (current, stage) {
        // No native continuation can connect before the local check finishes.
        (SCAN, Stage::Looking) => CLEANUP,
        // Opened online: the original answers its menu, or this load
        // itself when its scan found no game.
        (OPEN, Stage::Online) if native == MENU => HOME_LOAD,
        // Loaded: the files are written before the session is released.
        (HOME_LOAD, Stage::Online) if native == HOME_RELEASE => WRITE,
        (CLEANUP, Stage::Decided) => OPEN,
        (OPEN, Stage::Saved) => CLEANUP,
        _ => native,
    }
}

/// The stage after `destination` was asked with `stage`:
/// preparation becomes "online" once the scan task ends, and "saved"
/// becomes "looking" once cleanup was.
pub fn next_stage(current: u32, stage: Stage) -> Stage {
    match (current, stage) {
        (SCAN, Stage::Preparing) => Stage::Online,
        (CLEANUP, Stage::Preparing | Stage::Online) => Stage::Looking,
        (OPEN, Stage::Saved) => Stage::Looking,
        _ => stage,
    }
}

/// Native task 3 state 0 prepares the archive. States 1..=0x11 detect games;
/// state 0x12 starts the native save/account bookkeeping needed online.
pub const fn preparation_state(state: u32) -> u32 {
    match state {
        1..=0x11 => 0x12,
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

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
            (0x1c, 4) => 0x1d,
            (0x1d, 4) => 0x1b,
            (0x1b, _) => 0x13,
            (0x14 | 0x15, _) => 2,
            _ => CLEANUP,
        }
    }

    #[test]
    fn the_card_is_looked_at_before_anything_is_contacted() {
        assert_eq!(original(SCAN, 4, false), CONNECT);
        assert_eq!(destination(SCAN, CONNECT, Stage::Looking), CLEANUP);
        // A scan that failed ends as the original ends it.
        assert_eq!(destination(SCAN, CLEANUP, Stage::Looking), CLEANUP);
        // Nothing on the card: the original's connect, and from then on
        // every answer is the original's.
        assert_eq!(destination(SCAN, CONNECT, Stage::Preparing), CONNECT);
        assert_eq!(next_stage(SCAN, Stage::Preparing), Stage::Online);
        assert_eq!(next_stage(SCAN, Stage::Looking), Stage::Looking);
    }

    #[test]
    fn online_every_answer_but_two_is_the_originals() {
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for home in [false, true] {
                    let native = original(current, outcome, home);
                    let expected = match (current, native) {
                        (OPEN, MENU) => HOME_LOAD,
                        (HOME_LOAD, HOME_RELEASE) => WRITE,
                        _ => native,
                    };
                    assert_eq!(destination(current, native, Stage::Online), expected);
                    assert_eq!(
                        next_stage(current, Stage::Online),
                        if current == CLEANUP {
                            Stage::Looking
                        } else {
                            Stage::Online
                        }
                    );
                }
            }
        }
    }

    /// The tasks of a session after the look at the card. `failed` is a
    /// task that ends with the original's outcome for a failure, 3; every
    /// other ends without trouble, the reward step as cancelled.
    fn session(home: bool, failed: Option<u32>) -> Vec<u32> {
        let mut current = SCAN;
        let mut stage = Stage::Preparing;
        let mut seen = Vec::new();
        loop {
            let outcome = match current {
                task if Some(task) == failed => 3,
                WRITE => CANCELLED,
                _ => 4,
            };
            let native = original(current, outcome, home);
            let next = destination(current, native, stage);
            stage = next_stage(current, stage);
            seen.push(next);
            if next == CLEANUP {
                return seen;
            }
            current = next;
        }
    }

    #[test]
    fn the_session_loads_without_a_game_and_leaves_without_saving() {
        // The same whether the scan found a game or not.
        for home in [false, true] {
            let seen = session(home, None);
            assert_eq!(
                seen,
                [CONNECT, 8, 0xf, OPEN, HOME_LOAD, WRITE, 0x13, CLEANUP]
            );
            // Never the menu, the game selection and its scene, the checks
            // of a game's last session, the load that saves a game, the
            // HOME screens, the box screen, the reward claim or saving.
            let never = [
                MENU, 0xa, 0xb, 0x10, 0x11, 0x12, 0x17, 0x1b, 0x1d, 0xd, 0x19, 7,
            ];
            assert!(seen.iter().all(|task| !never.contains(task)));
        }
    }

    #[test]
    fn a_task_that_fails_ends_the_session_as_the_original_ends_it() {
        for home in [false, true] {
            // A load that failed wrote nothing: the reward step is not
            // reached.
            assert_eq!(
                session(home, Some(HOME_LOAD)),
                [CONNECT, 8, 0xf, OPEN, HOME_LOAD, CLEANUP]
            );
            assert_eq!(session(home, Some(OPEN))[3..], [OPEN, CLEANUP]);
            assert_eq!(session(home, Some(8)), [CONNECT, 8, CLEANUP]);
        }
    }

    #[test]
    fn a_result_is_shown_after_cleanup_and_only_then() {
        assert_eq!(destination(CLEANUP, 2, Stage::Decided), OPEN);
        for stage in [Stage::Looking, Stage::Preparing, Stage::Online] {
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

    #[test]
    fn after_the_success_message_the_start_screen_follows_and_the_card_is_looked_at_again() {
        // Task 9 ends without trouble; the original would answer its menu.
        for home in [false, true] {
            let native = original(OPEN, 4, home);
            assert_eq!(destination(OPEN, native, Stage::Saved), CLEANUP);
        }
        assert_eq!(next_stage(OPEN, Stage::Saved), Stage::Looking);
        // Cleanup then selects the start screen, and START the scan, which
        // leads to the look at the card.
        assert_eq!(
            destination(CLEANUP, original(CLEANUP, 4, false), Stage::Looking),
            2
        );
        assert_eq!(destination(2, original(2, 4, false), Stage::Looking), SCAN);
        assert_eq!(
            destination(SCAN, original(SCAN, 4, false), Stage::Looking),
            CLEANUP
        );
    }

    #[test]
    fn preparation_skips_every_game_detection_state_but_keeps_native_setup() {
        assert_eq!(preparation_state(0), 0);
        for state in 1..=0x11 {
            assert_eq!(preparation_state(state), 0x12);
        }
        for state in 0x12..=0x25 {
            assert_eq!(preparation_state(state), state);
        }
        // No local data allows archive setup; a native setup failure still
        // cleans up and the next START performs the local check again.
        assert_eq!(destination(SCAN, CLEANUP, Stage::Preparing), CLEANUP);
        let stage = next_stage(SCAN, Stage::Preparing);
        assert_eq!(next_stage(CLEANUP, stage), Stage::Looking);
    }
}
