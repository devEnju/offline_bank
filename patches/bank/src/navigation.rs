//! Task routing after native 002a5580 has chosen its destination.
//!
//! Task IDs are those of factory 002a5a7c: 2 start screen, 3 game scan, 4 main
//! menu, 9 Bank opening, 0xa game selection, 0xb scene set-up, 0x10 Bank
//! loading, 0xc/0xd reward guard and claim, 0x19 box screen, 7 save, 0x14
//! cleanup. The main menu is never a destination: opening leads straight to
//! game selection, and leaving selection takes the native cleanup route back
//! to the start screen. Only the console test build (docs/building.md) shows
//! the menu, between opening and game selection, to choose its rehearsal on.
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

/// Whether the main menu is shown: in the console test build only.
pub const MENU: bool = cfg!(feature = "test-build");
/// The main menu's outcome for "Use Pokémon Bank" (002a6d20), for which the
/// original router answers game selection (002a56d4).
pub const MENU_USE: u8 = 0xc;

/// `outcome` is the finished task's byte `+0x30`; `native` is what the original
/// router returned for it. `turn_back` is set when opening ended at a notice
/// that leads back to the start screen: a save in progress waits for its game.
pub fn destination(current: u32, outcome: u8, native: u32, turn_back: bool) -> u32 {
    match (current, outcome) {
        (3, 4) => OPEN,
        // The scan's HOME-transfer outcome must never open an offline Bank.
        (3, _) => CLEANUP,
        (OPEN, 4) if turn_back => CLEANUP,
        (OPEN, 4) if MENU => MAIN_MENU,
        (OPEN, 4) => SELECT,
        (MAIN_MENU, MENU_USE) if MENU => SELECT,
        (SELECT, 5) => SCENE,
        (SCENE, _) => LOAD,
        // Native Back returns to the menu, and so does the test build's,
        // where another rehearsal can be chosen. Without a menu, cleanup
        // destroys the shared UI so the next entry from the start screen
        // recreates it.
        (SELECT, 0x13) if MENU => MAIN_MENU,
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

    /// What follows an opened Bank: game selection, or in the console test
    /// build the main menu.
    const AFTER_OPEN: u32 = if MENU { MAIN_MENU } else { SELECT };
    /// What follows Back on game selection: the start screen through
    /// cleanup, or in the console test build the main menu again.
    const AFTER_BACK: u32 = if MENU { MAIN_MENU } else { CLEANUP };

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
    fn main_menu_is_a_destination_in_the_test_build_only_after_opening_and_after_back() {
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for native in 0..=0x1d {
                    for turn_back in [false, true] {
                        let menu = MENU
                            && ((current, outcome, turn_back) == (OPEN, 4, false)
                                || (current, outcome) == (SELECT, 0x13));
                        assert_eq!(
                            destination(current, outcome, native, turn_back) == MAIN_MENU,
                            menu,
                            "{current:#x} {outcome:#x} {native:#x}"
                        );
                    }
                }
            }
        }
        // From the menu only "Use Pokémon Bank" leads on, to game selection
        // as in the original; everything else ends the session.
        for outcome in 0..=u8::MAX {
            let next = destination(MAIN_MENU, outcome, 0xa, false);
            assert_eq!(next == SELECT, MENU && outcome == MENU_USE, "{outcome:#x}");
            assert!(next == SELECT || next == CLEANUP);
        }
    }

    #[test]
    fn opening_goes_to_selection_and_back_returns_through_cleanup() {
        // Native 002a5580 answers 4 (menu) for both of these.
        assert_eq!(destination(OPEN, 4, MAIN_MENU, false), AFTER_OPEN);
        assert_eq!(destination(SELECT, 0x13, MAIN_MENU, false), AFTER_BACK);
        // Native cleanup then selects the start screen, unchanged.
        assert_eq!(destination(CLEANUP, 3, START_SCREEN, false), START_SCREEN);
        assert_eq!(destination(START_SCREEN, 4, 3, false), 3);
        // Repeated cycles follow the same route every time.
        for _ in 0..3 {
            assert_eq!(destination(3, 4, 5, false), OPEN);
            assert_eq!(destination(3, 0x17, 5, false), CLEANUP);
            assert_eq!(destination(OPEN, 4, MAIN_MENU, false), AFTER_OPEN);
            assert_eq!(destination(SELECT, 0x13, MAIN_MENU, false), AFTER_BACK);
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
        // Save and Quit, reward cancellation, and box cancellation all clean
        // up; so does a Save and Quit that did not save (outcome 3, as the
        // original's after its message 0xB).
        assert_eq!(destination(7, 4, CLEANUP, false), CLEANUP);
        assert_eq!(destination(7, 3, CLEANUP, false), CLEANUP);
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
        // A menu task that somehow ran can only leave through cleanup; the
        // test build's menu also for the original's eShop and HOME results.
        for outcome in [3, 0xd, 0x17] {
            assert_eq!(destination(MAIN_MENU, outcome, 0xa, false), CLEANUP);
        }
        // Account, purchase, HOME, and service tasks stay unreachable.
        for native in [
            5, 6, 8, 0xb, 0xe, 0xf, 0x11, 0x12, 0x13, 0x17, 0x1a, 0x1b, 0x1c, 0x1d,
        ] {
            assert_eq!(destination(1, 0, native, false), CLEANUP);
        }
    }

    #[test]
    fn only_a_normal_scan_result_can_open_the_bank() {
        for outcome in 0..=u8::MAX {
            for native in 0..=0x1d {
                assert_eq!(
                    destination(3, outcome, native, false),
                    if outcome == 4 { OPEN } else { CLEANUP },
                    "scan outcome {outcome:#x}, native destination {native:#x}"
                );
            }
        }
        // No game or a HOME-transfer result returns through cleanup, then
        // START scans again so inserting a game does not require a restart.
        for outcome in [3, 0x17] {
            assert_eq!(destination(3, outcome, 5, false), CLEANUP);
            assert_eq!(destination(CLEANUP, 3, START_SCREEN, false), START_SCREEN);
            assert_eq!(destination(START_SCREEN, 4, 3, false), 3);
        }
    }
}
