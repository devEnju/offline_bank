//! Which task comes next, after the original's router (`00242BA0`) has
//! chosen, and during which tasks HOME and sleep are refused.
//!
//! Tasks of the factory (`0019CEE4`): 2 start screen, 7 game search,
//! 8 game list, 9 reading the chosen game, 0xb the original's Bank task,
//! 0xc the question, 6 the transfer, 0x10 disconnect, which leads back to
//! the start screen, and 0x11, which reloads the screens after the language
//! screen with disconnect's code. 3 is the original's connect task; with 4, 5, 0xa, 0xd
//! and 0xf it belongs to the server and is never a destination. Neither is
//! 0x12, the failure task. 0xe, the original's offer to restore a session,
//! follows the search for a save in progress Bank left to its question.

/// The language screen, from the start screen.
pub const LANGUAGE: u32 = 1;
pub const START_SCREEN: u32 = 2;
pub const CONNECT: u32 = 3;
pub const TRANSFER: u32 = 6;
pub const SEARCH: u32 = 7;
pub const LIST: u32 = 8;
pub const READ: u32 = 9;
pub const BANK_TASK: u32 = 0xb;
pub const QUESTION: u32 = 0xc;
pub const DISCONNECT: u32 = 0x10;
pub const RELOAD: u32 = 0x11;
/// The server task that cancels a transfer that was begun (`00248360`),
/// after the Bank task or the question ended without one (outcome `0x10`):
/// it asks the server and keeps its panel ("Communicating with the Pokémon
/// Bank server...", 23:0x0F) up for at least 1.5 s. Offline nothing was
/// begun; nothing is read or written there.
pub const CANCEL: u32 = 0xf;
/// The failure task (`002444BC`), after the Bank task or the transfer ended
/// with outcome `0xF`: it shows the original's message `0x0B`. Offline each
/// of those tasks has shown its own message before that outcome, the
/// failure message or the one that no Game Card is detected, so this one
/// would be a second.
pub const FAILURE: u32 = 0x12;
/// What the original answers when the application is to end.
pub const EXIT: u32 = 0x13;
/// The original's offer to restore a session the server could not finish
/// (`00243AF4`): its question "You can try restoring your data, but you may
/// lose the Pokémon currently being transported. Would you still like to
/// try restoring your data?" (23:0x1A). Offline it settles a save in
/// progress Bank left to it, and the disconnect task follows.
pub const RESTORE: u32 = 0xe;
/// The search's outcome for a save in progress Bank left to Transporter's
/// question (`hooks::transporter_search`); the original search never ends
/// with it.
pub const ASKED: u8 = 8;

/// The original router's answers the offline flow takes as they are. Every
/// other answer, a server task, the cancel task or the failure task among
/// them, ends the session through disconnect, as every other answer ends
/// Bank's through cleanup. The reading and the Bank task are no answer of
/// the router offline: `destination` gives them.
const TAKEN: [u32; 9] = [
    LANGUAGE,
    START_SCREEN,
    TRANSFER,
    SEARCH,
    LIST,
    QUESTION,
    DISCONNECT,
    RELOAD,
    EXIT,
];

/// `outcome` is the finished task's byte `+0x30`; `native` is what the
/// original router answered for it; `cancelled` is the manager's flag
/// (`+0x1a`), for which the disconnect task follows, as the original
/// answers it.
///
/// The offline flow differs from the original's in four places: a game
/// chosen on the list is read at once, where the original connected first;
/// the Bank task follows the reading, where the original's server tasks
/// came in between; a session that ends without a transfer goes straight to
/// the disconnect task, where the original cancelled it on the server
/// first; and so does a task that failed, which has shown its message,
/// where the original's failure task showed another. Any answer the flow
/// does not take (`TAKEN`) goes to the disconnect task as well.
pub fn destination(current: u32, outcome: u8, native: u32, cancelled: bool) -> u32 {
    if cancelled {
        return DISCONNECT;
    }
    match (current, outcome) {
        (LIST, 4) => READ,
        (READ, _) => BANK_TASK,
        (SEARCH, ASKED) => RESTORE,
        _ if TAKEN.contains(&native) => native,
        _ => DISCONNECT,
    }
}

/// Tasks that read or write files: the search (the Bank check and the
/// cartridge scan), the game list while it is set up (it reads every listed
/// save; `hooks::list_ready` lets HOME through again once it is
/// on screen), reading the chosen game, the Bank task (it saves its record
/// into the game), and the transfer; and the question, whose first state
/// takes down the panel of the reading (`hooks::waits`
/// lets HOME through on that frame). HOME and sleep are refused from the
/// moment one of them is chosen, until it ends or shows a message that
/// waits for an answer (`hooks::message_waits`). Every other
/// task waits for the user.
pub const fn loads(next: u32) -> bool {
    matches!(next, SEARCH | LIST | READ | BANK_TASK | QUESTION | TRANSFER)
}

/// The game list, which the search hands its loading panel to; the list
/// takes it down once it is on screen (`hooks::list_ready`). After every
/// other task the search ends in, the router entry ends it.
pub const fn takes_panel(next: u32) -> bool {
    matches!(next, LIST)
}

/// Tasks whose set-up puts up the general loading panel (`0019B340`):
/// reading the chosen game (`00246A6C`), the transfer (`0024A700`), and
/// disconnect and the reload (`00247268`). The panel of the reading stays
/// through the Bank task until the question ends it (`00247394`). The
/// search's panel is the patch's own (`panel`), and the server tasks that
/// have one are never a destination.
pub const fn spins(next: u32) -> bool {
    matches!(next, READ | TRANSFER | DISCONNECT | RELOAD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router;

    /// The routing the two edits inside the router gave (`00242D10`,
    /// `00242D28`), and the cancel and failure tasks left out.
    fn edited_inside(current: u32, outcome: u8) -> u32 {
        match (current, outcome) {
            (LIST, 4) => READ,
            (READ, _) => BANK_TASK,
            (BANK_TASK | QUESTION, 0x10) => DISCONNECT,
            (TRANSFER | BANK_TASK | 0xd, 0xf) => DISCONNECT,
            // The search's own outcome for Transporter's question, and the
            // restore task, which ends in disconnect.
            (SEARCH, ASKED) => RESTORE,
            (RESTORE, _) => DISCONNECT,
            _ => router::next(current, outcome),
        }
    }

    /// The tasks of the server, which the offline flow never enters.
    const SERVER: [u32; 7] = [CONNECT, 4, 5, 0xa, 0xd, CANCEL, FAILURE];

    #[test]
    fn the_routing_is_the_one_the_two_edits_inside_the_router_gave() {
        assert_eq!(router::EXIT, EXIT);
        for current in 0..=0x14 {
            for outcome in 0..=u8::MAX {
                let native = router::next(current, outcome);
                let routed = destination(current, outcome, native, false);
                // From every task the offline flow enters, the routing is
                // the edited router's; from the server's own, which it never
                // enters, the session ends.
                if SERVER.contains(&current) {
                    assert!(
                        matches!(routed, DISCONNECT | BANK_TASK),
                        "{current:#x} {outcome:#x}"
                    );
                } else {
                    assert_eq!(
                        routed,
                        edited_inside(current, outcome),
                        "{current:#x} {outcome:#x}"
                    );
                }
                // A cancelled task is the original's to answer.
                assert_eq!(destination(current, outcome, DISCONNECT, true), DISCONNECT);
            }
        }
    }

    #[test]
    fn a_transfer_runs_from_the_title_screen_and_back_without_a_server_task() {
        let mut task = START_SCREEN;
        let mut seen = [0; 8];
        for (index, outcome) in [3, 3, 4, 3, 4, 4, 3, 0].into_iter().enumerate() {
            task = destination(task, outcome, router::next(task, outcome), false);
            seen[index] = task;
        }
        assert_eq!(
            seen,
            [
                SEARCH,
                LIST,
                READ,
                BANK_TASK,
                QUESTION,
                TRANSFER,
                DISCONNECT,
                START_SCREEN
            ]
        );
        for current in 0..=0x14 {
            for outcome in 0..=u8::MAX {
                let next = destination(current, outcome, router::next(current, outcome), false);
                // The server's own tasks and the failure task are never
                // entered.
                assert!(!SERVER.contains(&next), "{current:#x} {outcome:#x}");
            }
        }
    }

    #[test]
    fn home_is_refused_exactly_while_files_are_read_or_written() {
        let loading = [SEARCH, LIST, READ, BANK_TASK, QUESTION, TRANSFER];
        for next in 0..=0x14 {
            assert_eq!(loads(next), loading.contains(&next), "{next:#x}");
        }
        // START to the game list, and a chosen game to the end of its panel
        // in the question, are refused without a gap; the screens that wait
        // are not.
        assert!(loads(destination(START_SCREEN, 3, SEARCH, false)));
        assert!(loads(destination(SEARCH, 3, LIST, false)));
        assert!(loads(destination(LIST, 4, CONNECT, false)));
        assert!(loads(destination(READ, 3, 0xa, false)));
        assert!(loads(destination(BANK_TASK, 4, QUESTION, false)));
        assert!(loads(destination(QUESTION, 4, TRANSFER, false)));
        // Every way back to the start screen ends the refusal: no game
        // found after START, Back on the list, a refusal of the original,
        // "No" at the question, nothing to move, a Bank task or a transfer
        // that failed, and the end of a transfer.
        for (current, outcome) in [
            (SEARCH, 2),
            (LIST, 5),
            (LIST, 0x11),
            (BANK_TASK, 5),
            (BANK_TASK, 0xf),
            (BANK_TASK, 0x10),
            (QUESTION, 3),
            (QUESTION, 0x10),
            (TRANSFER, 2),
            (TRANSFER, 3),
            (TRANSFER, 0xf),
        ] {
            let next = destination(current, outcome, router::next(current, outcome), false);
            assert_eq!(next, DISCONNECT, "{current:#x} {outcome:#x}");
            assert!(!loads(next));
        }
        assert!(!loads(START_SCREEN) && !loads(DISCONNECT) && !loads(EXIT));
    }

    #[test]
    fn a_save_left_to_the_question_goes_from_the_search_to_it_and_back() {
        let mut task = START_SCREEN;
        let mut seen = [0; 3];
        for (index, outcome) in [3, ASKED, 3].into_iter().enumerate() {
            task = destination(task, outcome, router::next(task, outcome), false);
            seen[index] = task;
        }
        assert_eq!(seen, [SEARCH, RESTORE, DISCONNECT]);
        // The question waits for the user; the answer's work refuses HOME
        // itself (`hooks::transporter_restore`).
        assert!(!loads(RESTORE));
        assert!(!spins(RESTORE));
        // The original search never ends with that outcome.
        assert_eq!(router::next(SEARCH, ASKED), DISCONNECT);
    }

    #[test]
    fn only_the_game_list_takes_the_search_panel() {
        assert!(takes_panel(LIST));
        assert!(!takes_panel(START_SCREEN));
        assert!(!takes_panel(DISCONNECT));
    }

    #[test]
    fn the_steps_whose_set_up_puts_up_the_loading_panel() {
        let panel = [TRANSFER, READ, DISCONNECT, RELOAD];
        for next in 0..=0x14 {
            assert_eq!(spins(next), panel.contains(&next), "{next:#x}");
        }
        // Every way into one of them is a destination the router entry
        // answers.
        assert!(spins(destination(LIST, 4, CONNECT, false)));
        assert!(spins(destination(QUESTION, 4, TRANSFER, false)));
        assert!(spins(destination(QUESTION, 0x10, CANCEL, false)));
        assert!(spins(destination(1, 4, RELOAD, false)));
    }
}
