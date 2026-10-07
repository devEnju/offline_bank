//! Task routing after native 002a5580 has chosen its destination.
//!
//! Task IDs are those of factory 002a5a7c: 2 start screen, 3 game scan, 4 main
//! menu, 9 Bank opening, 0xa game selection, 0xb scene set-up, 0x10 Bank
//! loading, 0xc/0xd reward guard and claim, 0x19 box screen, 7 save, 0x14
//! cleanup. The main menu is never a destination: opening leads straight to
//! game selection, and leaving selection takes the native cleanup route back
//! to the start screen.
//!
//! Task 0xb runs only for its native start-up routine 002af12c, which destroys
//! the text window, creates the character scene, and creates a new text
//! window showing the loading panel. That order puts the reward dialogs in
//! front of the scene. Its update (a server check) is replaced in the patch
//! profile by the native "finished" stub, so it ends on its first frame.

pub const START_SCREEN: u32 = 2;
pub const MAIN_MENU: u32 = 4;
pub const OPEN: u32 = 9;
pub const SELECT: u32 = 0xa;
pub const SCENE: u32 = 0xb;
pub const LOAD: u32 = 0x10;
pub const SAVE: u32 = 7;
pub const CLEANUP: u32 = 0x14;

/// `outcome` is the finished task's byte `+0x30`; `native` is what the original
/// router returned for it. `no_games` is set when opening found no usable game.
pub fn destination(current: u32, outcome: u8, native: u32, no_games: bool) -> u32 {
    match (current, outcome) {
        (3, 4 | 0x17) => OPEN,
        (OPEN, 4) if no_games => CLEANUP,
        (OPEN, 4) => SELECT,
        (SELECT, 5) => SCENE,
        (SCENE, _) => LOAD,
        // Native Back returns to the menu; cleanup destroys the shared UI so
        // the next entry from the start screen recreates it.
        (SELECT, 0x13) => CLEANUP,
        (LOAD, 4) => 0xc,
        (MAIN_MENU, _) => CLEANUP,
        // Cancellation leaves the durable Bank unchanged; task3 reloads the
        // game before another session. No cloud rollback task is entered.
        (0x19, 0x15) => CLEANUP,
        // Every other destination, including account, purchase, HOME, and
        // service tasks, ends the session through cleanup.
        _ => match native {
            0 | 1 | 2 | 3 | 7 | 9 | 0xa | 0xc | 0xd | 0x10 | 0x14 | 0x15 | 0x16 | 0x18 | 0x19 => {
                native
            }
            _ => CLEANUP,
        },
    }
}

/// Tasks that show a loading screen from their first frame to their last:
/// the game scan, Bank opening, the scene set-up that puts up the loading
/// panel for Bank loading, Bank loading, and saving. HOME and sleep are
/// refused from the moment one of them is chosen, so no frame between two of
/// them (scan then opening, scene set-up then loading) is left open.
pub const fn loads(destination: u32) -> bool {
    matches!(destination, 3 | OPEN | SCENE | LOAD | SAVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scan_opening_loading_and_saving_refuse_home() {
        let loading: [u32; 5] = [3, OPEN, SCENE, LOAD, SAVE];
        for destination in 0..=0x1d {
            assert_eq!(loads(destination), loading.contains(&destination));
        }
        // The whole way from the start screen to game selection, and from a
        // chosen game to the reward check, is covered without a gap.
        assert!(loads(destination(START_SCREEN, 4, 3, false)));
        assert!(loads(destination(3, 4, 5, false)));
        assert!(!loads(destination(OPEN, 4, MAIN_MENU, false)));
        assert!(loads(destination(SELECT, 5, 0xb, false)));
        assert!(loads(destination(SCENE, 3, CLEANUP, false)));
        assert!(!loads(destination(LOAD, 4, 0xc, false)));
        assert!(loads(destination(0x19, 4, 7, false)));
        assert!(!loads(destination(SAVE, 4, CLEANUP, false)));
    }

    #[test]
    fn main_menu_is_never_a_destination() {
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for native in 0..=0x1d {
                    for no_games in [false, true] {
                        assert_ne!(
                            destination(current, outcome, native, no_games),
                            MAIN_MENU,
                            "{current:#x} {outcome:#x} {native:#x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn opening_goes_to_selection_and_back_returns_through_cleanup() {
        // Native 002a5580 answers 4 (menu) for both of these.
        assert_eq!(destination(OPEN, 4, MAIN_MENU, false), SELECT);
        assert_eq!(destination(SELECT, 0x13, MAIN_MENU, false), CLEANUP);
        // Native cleanup then selects the start screen, unchanged.
        assert_eq!(destination(CLEANUP, 3, START_SCREEN, false), START_SCREEN);
        assert_eq!(destination(START_SCREEN, 4, 3, false), 3);
        // Repeated cycles follow the same route every time.
        for _ in 0..3 {
            assert_eq!(destination(3, 4, 5, false), OPEN);
            assert_eq!(destination(3, 0x17, 5, false), OPEN);
            assert_eq!(destination(OPEN, 4, MAIN_MENU, false), SELECT);
            assert_eq!(destination(SELECT, 0x13, MAIN_MENU, false), CLEANUP);
        }
    }

    #[test]
    fn selected_game_follows_load_rewards_boxes_and_save() {
        // The native scene set-up runs between selection and loading, as in
        // the original; whatever its outcome byte holds, loading follows.
        assert_eq!(destination(SELECT, 5, 0xb, false), SCENE);
        for outcome in 0..=u8::MAX {
            for native in 0..=0x1d {
                assert_eq!(destination(SCENE, outcome, native, false), LOAD);
            }
        }
        assert_eq!(destination(LOAD, 4, 0xc, false), 0xc);
        assert_eq!(destination(0xc, 5, 0xd, false), 0xd);
        assert_eq!(destination(0xc, 6, 0x19, false), 0x19);
        assert_eq!(destination(0xd, 5, 0x19, false), 0x19);
        assert_eq!(destination(0x19, 4, 7, false), 7);
        // Save and Quit, reward cancellation, and box cancellation all clean up.
        assert_eq!(destination(7, 4, CLEANUP, false), CLEANUP);
        assert_eq!(destination(0xc, 0x15, 0x13, false), CLEANUP);
        assert_eq!(destination(0xd, 0x15, 0x13, false), CLEANUP);
        assert_eq!(destination(0x19, 0x15, 0x13, false), CLEANUP);
    }

    #[test]
    fn failures_missing_games_and_online_destinations_end_in_cleanup() {
        // Failed opening or loading (outcome 3) uses the native cleanup route.
        assert_eq!(destination(OPEN, 3, CLEANUP, false), CLEANUP);
        assert_eq!(destination(LOAD, 3, CLEANUP, false), CLEANUP);
        // A rejected or invalid game in selection (outcome 6).
        assert_eq!(destination(SELECT, 6, CLEANUP, false), CLEANUP);
        assert_eq!(destination(OPEN, 4, MAIN_MENU, true), CLEANUP);
        // A menu task that somehow ran can only leave through cleanup.
        for outcome in [3, 0xc, 0xd, 0x17] {
            assert_eq!(destination(MAIN_MENU, outcome, 0xa, false), CLEANUP);
        }
        // Account, purchase, HOME, and service tasks stay unreachable.
        for native in [
            5, 6, 8, 0xb, 0xe, 0xf, 0x11, 0x12, 0x13, 0x17, 0x1a, 0x1b, 0x1c, 0x1d,
        ] {
            assert_eq!(destination(1, 0, native, false), CLEANUP);
        }
    }
}
