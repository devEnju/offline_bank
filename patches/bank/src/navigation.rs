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
pub const SCAN: u32 = 3;
pub const MAIN_MENU: u32 = 4;
pub const OPEN: u32 = 9;
pub const SELECT: u32 = 0xa;
pub const SCENE: u32 = 0xb;
pub const LOAD: u32 = 0x10;
pub const REWARD_GUARD: u32 = 0xc;
pub const REWARD_CLAIM: u32 = 0xd;
pub const BOXES: u32 = 0x19;
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
        (SCAN, 4) => OPEN,
        // The scan's HOME-transfer outcome must never open an offline Bank.
        (SCAN, _) => CLEANUP,
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
        (LOAD, 4) => REWARD_GUARD,
        (MAIN_MENU, _) => CLEANUP,
        // Cancellation leaves the durable Bank unchanged; task3 reloads the
        // game before another session. No cloud rollback task is entered.
        (BOXES, 0x15) => CLEANUP,
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
    matches!(destination, SCAN | OPEN | SCENE | LOAD | SAVE)
}

/// The reward guard and claim, which Bank loading hands its loading panel
/// to; one of them takes it down. HOME and sleep stay refused into them
/// while the panel is still up (`hooks::panel_left`), until it is down.
pub const fn takes_panel(destination: u32) -> bool {
    matches!(destination, REWARD_GUARD | REWARD_CLAIM)
}

/// Tasks whose set-up puts up the loading panel (`001d5b44`): the scene
/// set-up (`002af1c8`), whose panel stays through Bank loading into the
/// reward step, saving (`002b2540`), and cleanup (`002abd6c`). The game
/// scan's panel is the patch's own (`panel`), and opening keeps it.
pub const fn spins(destination: u32) -> bool {
    matches!(destination, SCENE | SAVE | CLEANUP)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use bank_common::router;
    use std::vec::Vec;

    /// What follows an opened Bank: game selection, or in the console test
    /// build the main menu.
    const AFTER_OPEN: u32 = if MENU { MAIN_MENU } else { SELECT };
    /// What follows Back on game selection: the start screen through
    /// cleanup, or in the console test build the main menu again.
    const AFTER_BACK: u32 = if MENU { MAIN_MENU } else { CLEANUP };

    /// One session from the start screen, each task ending with `outcome`,
    /// each answer the original router's (`bank_common::router`) as the
    /// patch changes it: the tasks that follow, up to the start screen.
    fn session(outcome: impl Fn(u32) -> u8) -> Vec<u32> {
        let mut current = START_SCREEN;
        let mut seen = Vec::new();
        while seen.len() < 20 {
            let ended = outcome(current);
            let next = destination(current, ended, router::next(current, ended, false), false);
            seen.push(next);
            if next == START_SCREEN {
                break;
            }
            current = next;
        }
        seen
    }

    #[test]
    fn a_session_is_the_originals_without_its_server_steps() {
        // START, a game found, opened; "Use Pokémon Bank" in the test
        // build's menu; a game chosen; loaded; the reward guard with nothing
        // in the way, the claim; the boxes; Save and Quit; cleanup.
        let seen = session(|task| match task {
            MAIN_MENU => MENU_USE,
            SELECT | REWARD_GUARD | REWARD_CLAIM => 5,
            _ => 4,
        });
        let mut expected = std::vec![SCAN, OPEN];
        if MENU {
            expected.push(MAIN_MENU);
        }
        expected.extend([
            SELECT,
            SCENE,
            LOAD,
            REWARD_GUARD,
            REWARD_CLAIM,
            BOXES,
            SAVE,
            CLEANUP,
            START_SCREEN,
        ]);
        assert_eq!(seen, expected);
        // Back on game selection, without the menu: cleanup, then the
        // start screen.
        let back = session(|task| if task == SELECT { 0x13 } else { 4 });
        if !MENU {
            assert_eq!(back, [SCAN, OPEN, SELECT, CLEANUP, START_SCREEN]);
        }
        // A save that failed (outcome 3, as after the original's message
        // 0xB) ends the same way.
        let failed = session(|task| match task {
            SELECT | REWARD_GUARD | REWARD_CLAIM => 5,
            MAIN_MENU => MENU_USE,
            SAVE => 3,
            _ => 4,
        });
        assert_eq!(failed[failed.len() - 3..], [SAVE, CLEANUP, START_SCREEN]);
    }

    #[test]
    fn no_answer_of_the_original_router_leads_to_its_server() {
        // Connect, sign-in, account and service tasks, the second load and
        // the checks of a game's last session, the release and HOME screens.
        let never = [
            5, 6, 8, 0xe, 0xf, 0x11, 0x12, 0x13, 0x17, 0x1a, 0x1b, 0x1c, 0x1d,
        ];
        for current in 0..=0x1d {
            for outcome in 0..=u8::MAX {
                for home in [false, true] {
                    let native = router::next(current, outcome, home);
                    for turn_back in [false, true] {
                        let next = destination(current, outcome, native, turn_back);
                        assert!(
                            !never.contains(&next) && (MENU || next != MAIN_MENU),
                            "{current:#x} {outcome:#x} {native:#x} -> {next:#x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn only_scan_opening_loading_and_saving_refuse_home() {
        let loading: [u32; 5] = [SCAN, OPEN, SCENE, LOAD, SAVE];
        for destination in 0..=0x1d {
            assert_eq!(loads(destination), loading.contains(&destination));
        }
        // The whole way from the start screen to game selection, and from a
        // chosen game to the reward check, is covered without a gap.
        assert!(loads(destination(START_SCREEN, 4, SCAN, false)));
        assert!(loads(destination(SCAN, 4, 5, false)));
        assert!(!loads(destination(OPEN, 4, MAIN_MENU, false)));
        assert!(loads(destination(SELECT, 5, 0xb, false)));
        assert!(loads(destination(SCENE, 3, CLEANUP, false)));
        assert!(!loads(destination(LOAD, 4, REWARD_GUARD, false)));
        assert!(loads(destination(BOXES, 4, SAVE, false)));
        assert!(!loads(destination(SAVE, 4, CLEANUP, false)));
    }

    #[test]
    fn loading_hands_its_panel_to_the_reward_step() {
        assert!(takes_panel(destination(LOAD, 4, REWARD_GUARD, false)));
        assert!(takes_panel(REWARD_CLAIM));
        for destination in 0..=0x1d {
            assert_eq!(
                takes_panel(destination),
                matches!(destination, REWARD_GUARD | REWARD_CLAIM)
            );
            // The reward step is no loading task of its own.
            assert!(!(takes_panel(destination) && loads(destination)));
        }
    }

    #[test]
    fn the_tasks_whose_set_up_puts_up_the_loading_panel() {
        let panel: [u32; 3] = [SCENE, SAVE, CLEANUP];
        for destination in 0..=0x1d {
            assert_eq!(spins(destination), panel.contains(&destination));
        }
        assert!(spins(destination(SELECT, 5, 0xb, false)));
        assert!(spins(destination(BOXES, 4, SAVE, false)));
        assert!(spins(destination(SAVE, 4, CLEANUP, false)));
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
        assert_eq!(destination(START_SCREEN, 4, SCAN, false), SCAN);
        // From the start screen the Bank opens again, or the session ends.
        assert_eq!(destination(SCAN, 4, 5, false), OPEN);
        assert_eq!(destination(SCAN, 0x17, 5, false), CLEANUP);
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
        assert_eq!(destination(LOAD, 4, REWARD_GUARD, false), REWARD_GUARD);
        assert_eq!(
            destination(REWARD_GUARD, 5, REWARD_CLAIM, false),
            REWARD_CLAIM
        );
        assert_eq!(destination(REWARD_GUARD, 6, BOXES, false), BOXES);
        assert_eq!(destination(REWARD_CLAIM, 5, BOXES, false), BOXES);
        assert_eq!(destination(BOXES, 4, SAVE, false), SAVE);
        // Save and Quit, reward cancellation, and box cancellation all clean
        // up; so does a Save and Quit that did not save (outcome 3, as the
        // original's after its message 0xB).
        assert_eq!(destination(SAVE, 4, CLEANUP, false), CLEANUP);
        assert_eq!(destination(SAVE, 3, CLEANUP, false), CLEANUP);
        assert_eq!(destination(REWARD_GUARD, 0x15, 0x13, false), CLEANUP);
        assert_eq!(destination(REWARD_CLAIM, 0x15, 0x13, false), CLEANUP);
        assert_eq!(destination(BOXES, 0x15, 0x13, false), CLEANUP);
    }

    #[test]
    fn after_a_load_no_way_leads_to_another_load_before_the_start_screen() {
        // Loading keeps the Bank its opening restored (`Job::LoadGame`), and
        // the Miles of the hours since the reward date are paid once per
        // load: a second load needs an opening before it.
        let mut seen = std::vec![LOAD];
        let mut next_task = 0;
        while let Some(&current) = seen.get(next_task) {
            next_task += 1;
            for outcome in 0..=u8::MAX {
                for home in [false, true] {
                    for turn_back in [false, true] {
                        let native = router::next(current, outcome, home);
                        let next = destination(current, outcome, native, turn_back);
                        assert!(
                            ![SELECT, SCENE, LOAD, MAIN_MENU, OPEN].contains(&next),
                            "{current:#x} {outcome:#x} -> {next:#x}"
                        );
                        if next != START_SCREEN && !seen.contains(&next) {
                            seen.push(next);
                        }
                    }
                }
            }
        }
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
                    destination(SCAN, outcome, native, false),
                    if outcome == 4 { OPEN } else { CLEANUP },
                    "scan outcome {outcome:#x}, native destination {native:#x}"
                );
            }
        }
        // No game or a HOME-transfer result returns through cleanup, then
        // START scans again so inserting a game does not require a restart.
        for outcome in [3, 0x17] {
            assert_eq!(destination(SCAN, outcome, 5, false), CLEANUP);
            assert_eq!(destination(CLEANUP, 3, START_SCREEN, false), START_SCREEN);
            assert_eq!(destination(START_SCREEN, 4, SCAN, false), SCAN);
        }
    }
}
