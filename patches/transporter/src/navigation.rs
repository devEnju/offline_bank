//! Which step comes next, after the original's router (`00242BA0`) has
//! chosen, and during which steps HOME and sleep are refused.
//!
//! Steps of the task factory (`0019CEE4`): 2 title screen, 7 game search,
//! 8 game list, 9 reading the chosen game, 0xb the original's Bank step,
//! 0xc the question, 6 the transfer, 0x10 disconnect, which leads back to
//! the title screen. 3 is the original's connect step; with 4, 5, 0xa, 0xd
//! and 0xe it belongs to the server and is never a destination.

pub const TITLE: u32 = 2;
pub const CONNECT: u32 = 3;
pub const TRANSFER: u32 = 6;
pub const SEARCH: u32 = 7;
pub const LIST: u32 = 8;
pub const READ: u32 = 9;
pub const BANK_STEP: u32 = 0xb;
pub const QUESTION: u32 = 0xc;
pub const DISCONNECT: u32 = 0x10;

/// `outcome` is the finished step's byte `+0x30`; `native` is what the
/// original router answered for it; `cancelled` is the manager's flag
/// (`+0x1a`), for which the original answers the disconnect step itself.
///
/// The offline flow differs from the original's in two places: a game
/// chosen on the list is read at once, where the original connected first;
/// and the Bank step follows the reading, where the original's server steps
/// came in between.
pub fn destination(current: u32, outcome: u8, native: u32, cancelled: bool) -> u32 {
    if cancelled {
        return native;
    }
    match (current, outcome) {
        (LIST, 4) => READ,
        (READ, _) => BANK_STEP,
        _ => native,
    }
}

/// Steps that read or write files: the search (the Bank check and the
/// cartridge scan), the game list while it is set up (it reads every listed
/// save; `hooks::transporter_list_ready` lets HOME through again once it is
/// on screen), reading the chosen game, the Bank step (it saves its record
/// into the game), and the transfer. HOME and sleep are refused from the
/// moment one of them is chosen. Every other step waits for the user.
pub const fn loads(next: u32) -> bool {
    matches!(next, SEARCH | LIST | READ | BANK_STEP | TRANSFER)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXIT: u32 = 0x13;

    /// The original router, case by case (`00242BCC`..`00242E04`).
    fn original(current: u32, outcome: u8) -> u32 {
        match (current, outcome) {
            (0, 4) => TITLE,
            (0, _) => EXIT,
            (1, 4) => 0x11,
            (1, _) => EXIT,
            (TITLE, 3) => SEARCH,
            (TITLE, 0x12) => 1,
            (TITLE, _) => EXIT,
            (CONNECT, 3) => 5,
            (CONNECT, 0xe) => 4,
            (5, 3) => READ,
            (TRANSFER, 0xf) => 0x12,
            (SEARCH, 3) => LIST,
            (LIST, 4) => CONNECT,
            (READ, 3) => 0xa,
            (0xa, 8) => 0xe,
            (0xa, 9) => 0xd,
            (BANK_STEP, 4) => QUESTION,
            (BANK_STEP, 0xf) => 0x12,
            (BANK_STEP, 0x10) => 0xf,
            (QUESTION, 4) => TRANSFER,
            (QUESTION, 0x10) => 0xf,
            (0xd, 3) => BANK_STEP,
            (0xd, 0xf) => 0x12,
            (0xe, 3) => 0xd,
            (DISCONNECT | 0x11, _) => TITLE,
            (0x13.., _) => EXIT,
            _ => DISCONNECT,
        }
    }

    /// The routing of the patch so far, which edited two instructions
    /// inside the router: `00242D10` and `00242D28`.
    fn edited_inside(current: u32, outcome: u8) -> u32 {
        match (current, outcome) {
            (LIST, 4) => READ,
            (READ, _) => BANK_STEP,
            _ => original(current, outcome),
        }
    }

    #[test]
    fn the_routing_is_the_one_the_two_edits_inside_the_router_gave() {
        for current in 0..=0x14 {
            for outcome in 0..=u8::MAX {
                let native = original(current, outcome);
                assert_eq!(
                    destination(current, outcome, native, false),
                    edited_inside(current, outcome),
                    "{current:#x} {outcome:#x}"
                );
                // A cancelled step is the original's to answer.
                assert_eq!(destination(current, outcome, DISCONNECT, true), DISCONNECT);
            }
        }
    }

    #[test]
    fn a_transfer_runs_from_the_title_screen_and_back_without_a_server_step() {
        let mut step = TITLE;
        let mut seen = [0; 8];
        for (index, outcome) in [3, 3, 4, 3, 4, 4, 3, 0].into_iter().enumerate() {
            step = destination(step, outcome, original(step, outcome), false);
            seen[index] = step;
        }
        assert_eq!(
            seen,
            [SEARCH, LIST, READ, BANK_STEP, QUESTION, TRANSFER, DISCONNECT, TITLE]
        );
        for current in 0..=0x14 {
            for outcome in 0..=u8::MAX {
                let next = destination(current, outcome, original(current, outcome), false);
                // The server's own steps are only ever left, never entered.
                if ![CONNECT, 4, 5, 0xa, 0xd, 0xe].contains(&current) {
                    assert!(
                        ![CONNECT, 4, 5, 0xa, 0xd, 0xe].contains(&next),
                        "{current:#x} {outcome:#x}"
                    );
                }
            }
        }
    }

    #[test]
    fn home_is_refused_exactly_while_files_are_read_or_written() {
        let loading = [SEARCH, LIST, READ, BANK_STEP, TRANSFER];
        for next in 0..=0x14 {
            assert_eq!(loads(next), loading.contains(&next), "{next:#x}");
        }
        // START to the game list, and a chosen game to the question, are
        // refused without a gap; the screens that wait are not.
        assert!(loads(destination(TITLE, 3, SEARCH, false)));
        assert!(loads(destination(SEARCH, 3, LIST, false)));
        assert!(loads(destination(LIST, 4, CONNECT, false)));
        assert!(loads(destination(READ, 3, 0xa, false)));
        assert!(!loads(destination(BANK_STEP, 4, QUESTION, false)));
        assert!(loads(destination(QUESTION, 4, TRANSFER, false)));
        // Every way back to the title screen ends the refusal: nothing
        // found or a message after START, Back on the list, a refusal of
        // the original, "No" at the question, and the end of a transfer.
        for (current, outcome) in [
            (SEARCH, 2),
            (LIST, 5),
            (LIST, 0x11),
            (BANK_STEP, 5),
            (QUESTION, 3),
            (TRANSFER, 2),
            (TRANSFER, 3),
        ] {
            let next = destination(current, outcome, original(current, outcome), false);
            assert_eq!(next, DISCONNECT, "{current:#x} {outcome:#x}");
            assert!(!loads(next));
        }
        assert!(!loads(TITLE) && !loads(QUESTION) && !loads(DISCONNECT) && !loads(EXIT));
    }
}
