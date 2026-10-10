//! Task routing after native 002a5580 has chosen its destination.
//!
//! The way to "open Bank" is the offline patch's: start screen (2), game
//! scan (3), then task 9 in place of the original's sign-in. Task 9 runs the
//! conversion and ends in cleanup (0x14), which leads back to the start
//! screen. Nothing behind task 9 is ever a destination.

pub const OPEN: u32 = 9;
pub const CLEANUP: u32 = 0x14;

/// `outcome` is the finished task's byte `+0x30`; `native` is what the
/// original router returned for it.
pub fn destination(current: u32, outcome: u8, native: u32) -> u32 {
    match (current, outcome) {
        (3, 4 | 0x17) => OPEN,
        (OPEN, _) => CLEANUP,
        _ => match native {
            0 | 1 | 2 | 3 | 0x14 | 0x15 | 0x16 | 0x18 => native,
            _ => CLEANUP,
        },
    }
}

/// Tasks that show a loading screen from their first frame to their last.
/// HOME and sleep are refused from the moment one of them is chosen.
pub const fn loads(destination: u32) -> bool {
    matches!(destination, 3 | OPEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_leads_to_the_conversion_and_that_to_the_start_screen() {
        // Native 002a5580 answers 5, the sign-in, after the scan.
        assert_eq!(destination(2, 4, 3), 3);
        assert_eq!(destination(3, 4, 5), OPEN);
        assert_eq!(destination(3, 0x17, 5), OPEN);
        for outcome in 0..=u8::MAX {
            for native in 0..=0x1d {
                assert_eq!(destination(OPEN, outcome, native), CLEANUP);
            }
        }
        // Native cleanup then selects the start screen, unchanged.
        assert_eq!(destination(CLEANUP, 3, 2), 2);
        assert!(loads(3) && loads(OPEN) && !loads(CLEANUP) && !loads(2));
    }

    #[test]
    fn nothing_behind_opening_is_ever_a_destination() {
        // Main menu, sign-in, save, selection, scene set-up, rewards,
        // loading, the box screen, and the account and service tasks.
        let never = [
            4, 5, 6, 7, 8, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 0x11, 0x12, 0x13, 0x17, 0x19, 0x1a,
            0x1b, 0x1c, 0x1d,
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
