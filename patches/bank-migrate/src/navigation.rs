//! Task routing after native 002a5580 has chosen its destination.
//!
//! START enters task 3, whose update runs only the conversion. No game scan
//! runs. Its result screen stays until Bank is closed. If the task cannot
//! show that screen, cleanup (0x14) leads back to the start screen.

pub const SCAN: u32 = 3;
pub const CLEANUP: u32 = 0x14;

/// `outcome` is the finished task's byte `+0x30`; `native` is what the
/// original router returned for it.
pub fn destination(current: u32, outcome: u8, native: u32) -> u32 {
    match (current, outcome) {
        (SCAN, _) => CLEANUP,
        _ => match native {
            0 | 1 | 2 | 3 | 0x14 | 0x15 | 0x16 | 0x18 => native,
            _ => CLEANUP,
        },
    }
}

/// Tasks that show a loading screen from their first frame to their last.
/// HOME and sleep are refused from the moment one of them is chosen.
pub const fn loads(destination: u32) -> bool {
    destination == SCAN
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank_common::router;

    #[test]
    fn start_runs_the_conversion_without_scanning_games_then_returns() {
        assert_eq!(destination(2, 4, 3), 3);
        for outcome in 0..=u8::MAX {
            for native in 0..=0x1d {
                assert_eq!(destination(SCAN, outcome, native), CLEANUP);
            }
        }
        // Native cleanup then selects the start screen, unchanged.
        assert_eq!(destination(CLEANUP, 3, 2), 2);
        assert!(loads(SCAN) && !loads(9) && !loads(CLEANUP) && !loads(2));
    }

    #[test]
    fn a_start_converts_and_returns_to_the_start_screen() {
        // With the original router's answers (`bank_common::router`).
        for outcome in 0..=u8::MAX {
            assert_eq!(destination(2, 4, router::next(2, 4, false)), SCAN);
            assert_eq!(
                destination(SCAN, outcome, router::next(SCAN, outcome, false)),
                CLEANUP
            );
            assert_eq!(
                destination(CLEANUP, outcome, router::next(CLEANUP, outcome, false)),
                2
            );
        }
    }

    #[test]
    fn no_online_or_game_task_is_ever_a_destination() {
        // Main menu, sign-in, save, selection, scene set-up, rewards,
        // loading, the box screen, and the account and service tasks.
        let never = [
            4, 5, 6, 7, 8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x13, 0x17, 0x19,
            0x1a, 0x1b, 0x1c, 0x1d,
        ];
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for native in 0..=0x1d {
                    let chosen = destination(current, outcome, native);
                    assert!(
                        !never.contains(&chosen),
                        "{current:#x} {outcome:#x} {native:#x}"
                    );
                }
            }
        }
    }
}
