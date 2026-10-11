//! Where the original keeps the transport box, and what the hooks answer.
//!
//! `task + 8` is the session manager, `manager + 0xCC` the Bank data object,
//! `object + 0xBB524` its transport accessor, and `accessor + 4` the body
//! pointer the original slot functions (`0019A6F4`, `0019A7E0`, `0019A858`,
//! `0024D624`) index from: record `i` at `+0xAAF1C + i * 0xE8`, tag `i` at
//! `+0xAD604 + i`. Both are the Bank body offsets plus 8.

use offline_core::{
    sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS},
    sidecar::{Slot, SLOTS},
    transport,
};

pub const TASK_MANAGER: usize = 0x8;
pub const MANAGER_BANK_OBJECT: usize = 0xCC;
pub const BANK_OBJECT_ACCESSOR: usize = 0xBB524;
pub const ACCESSOR_BODY: usize = 0x4;
/// The accessor's body pointer is 8 bytes before the body Bank stores.
pub const BODY_BIAS: usize = 8;
pub const RECORDS_OFFSET: usize = TRANSPORT_RECORDS.start + BODY_BIAS;
pub const TAGS_OFFSET: usize = TRANSPORT_TAGS.start + BODY_BIAS;
pub const RECORDS_LEN: usize = TRANSPORT_SLOTS * RECORD_SIZE;

/// The dialog owner of the game-search task (`00246F04`), set by its init
/// (`0024704C`): the first argument of the original's "show message and
/// wait", `0019B50C(ui, message, 1)`, as the search calls it for message 0.
pub const TASK_UI: usize = 0x34;

/// Answers of the check entry, as the stub of the patched site at `00246F48`
/// tests them (link.rs): go on with the game search; come back next frame;
/// end the search at once, which leads back to the title screen.
pub const SEARCH_GO: u32 = 0;
pub const SEARCH_PENDING: u32 = 1;
pub const SEARCH_END: u32 = 2;

/// The original's message 3: "Your communication with Pokémon Bank did not
/// complete correctly during your last session. Please open Pokémon Bank
/// and perform the cleanup process before using Poké Transporter." The
/// original shows it where the server asked for that (`0025C964`).
pub const MESSAGE_OPEN_BANK: u32 = 3;
/// The original's message 5: "At least one Pokémon remains in the Transport
/// Box from your previous session. Please empty the Transport Box by using
/// Pokémon Bank before using Poké Transporter."
pub const MESSAGE_BOX_NOT_EMPTY: u32 = 5;

/// The original's message 0x27, the title screen's own line: "Press HOME to
/// return to the HOME Menu". One line in every language of the original.
pub const MESSAGE_HOME: u32 = 0x27;

/// The message that stops a session before it begins, for the two slots of
/// Bank's transport box; `None` when a transfer may go ahead. Slots that are
/// `None` could not be read at all: a file is missing, of another size or
/// unreadable, or Bank's extdata is not there. Opening Bank shows or solves
/// each of these, so the message carries no numbers of its own.
pub fn refusal_message(slots: Option<&[Option<Slot>; SLOTS]>) -> Option<u32> {
    let Some(slots) = slots else {
        return Some(MESSAGE_OPEN_BANK);
    };
    if transport::may_deliver(slots) {
        None
    } else if slots.iter().flatten().any(|slot| slot.count != 0) {
        // Pokémon are in the way: in Bank's box, or in a waiting delivery.
        Some(MESSAGE_BOX_NOT_EMPTY)
    } else {
        // Two valid empty slots: a Bank save was not finished.
        Some(MESSAGE_OPEN_BANK)
    }
}

/// What is on screen once the check has refused. Nothing leads back from
/// here: the session cannot begin until Bank was opened, so the last screen
/// stays, as the original's own message with that line does (`00245274`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Refused {
    /// The check has not refused.
    No = 0,
    /// The message, until it is acknowledged.
    Message = 1,
    /// The line about the HOME Menu.
    Home = 2,
}
impl Refused {
    pub const fn of(value: u32) -> Self {
        match value {
            1 => Self::Message,
            2 => Self::Home,
            _ => Self::No,
        }
    }
    /// The screen of the next frame; `acknowledged` is the dialog's answer
    /// of this one. A change to `Home` is when that line is shown.
    pub const fn next(self, acknowledged: bool) -> Self {
        match self {
            Self::Message if acknowledged => Self::Home,
            other => other,
        }
    }
}

/// Answers of the delivery entry, as the stub of the patched site at
/// `0024A150` tests them (link.rs).
pub const DELIVER_FAILED: u32 = 0;
pub const DELIVER_DONE: u32 = 1;
pub const DELIVER_PENDING: u32 = 2;

/// Stack of the worker thread, in the payload's own zero-initialised data.
/// Its deepest call chain needs well under 1 KiB.
pub const WORKER_STACK_SIZE: usize = 0x4000;

/// Bank's extdata and the two files of the transport box inside it, one per
/// slot of the box, in slot order.
pub const BANK_EXTDATA: u32 = 0x0000_0C9B;
pub const MOVER_PATHS: [&[u8]; 2] = [b"/mover.bin\0", b"/mover.alt.bin\0"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_the_ones_the_original_slot_functions_use() {
        // 0019A7E0: record = body - 0xFFF550E4 + i * 0xE8; tag at body + 0xAD604.
        assert_eq!(RECORDS_OFFSET as u32, 0u32.wrapping_sub(0xFFF5_50E4));
        assert_eq!(TAGS_OFFSET, 0xAD604);
        assert_eq!(RECORDS_LEN, 6_960);
        assert_eq!(RECORDS_LEN + TRANSPORT_SLOTS, 6_990);
    }

    fn slot(count: u32) -> Option<Slot> {
        Some(Slot {
            tag: None,
            aux: 1,
            count,
            len: 0,
            crc: 0,
        })
    }
    fn message(slots: [Option<Slot>; SLOTS]) -> Option<u32> {
        refusal_message(Some(&slots))
    }

    #[test]
    fn the_check_tells_pokemon_in_the_way_from_a_bank_that_is_not_ready() {
        assert_eq!(message([None, None]), None);
        assert_eq!(message([slot(0), None]), None);
        assert_eq!(message([None, slot(0)]), None);
        // The "remains in the Transport Box" message only when one does.
        for slots in [
            [slot(2), None],
            [slot(0), slot(5)],
            [slot(3), slot(0)],
            [slot(1), slot(1)],
        ] {
            assert_eq!(message(slots), Some(MESSAGE_BOX_NOT_EMPTY));
        }
        // An unfinished Bank save, and files that are not there or do not
        // read: Bank before its first start, or one not yet converted.
        assert_eq!(message([slot(0), slot(0)]), Some(MESSAGE_OPEN_BANK));
        assert_eq!(refusal_message(None), Some(MESSAGE_OPEN_BANK));
        // A refusal is exactly where the delivery itself would refuse.
        for slots in [
            [None, None],
            [slot(0), None],
            [slot(2), None],
            [slot(0), slot(0)],
        ] {
            assert_eq!(message(slots).is_none(), transport::may_deliver(&slots));
        }
        // The worker reports "go on" as 0, which no message may be.
        assert_eq!((MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY), (3, 5));
    }

    #[test]
    fn a_refusal_ends_on_the_line_about_the_home_menu_and_stays_there() {
        // The message until it is acknowledged, then the line.
        assert_eq!(Refused::Message.next(false), Refused::Message);
        assert_eq!(Refused::Message.next(true), Refused::Home);
        // Whatever the dialog answers from then on, the line stays, and no
        // screen ever leads back to a check that has not refused.
        for acknowledged in [false, true] {
            assert_eq!(Refused::Home.next(acknowledged), Refused::Home);
            assert_eq!(Refused::No.next(acknowledged), Refused::No);
        }
        for refused in [Refused::No, Refused::Message, Refused::Home] {
            assert_eq!(Refused::of(refused as u32), refused);
        }
        assert_eq!(Refused::of(7), Refused::No);
        assert_eq!(MESSAGE_HOME, 0x27);
    }

    #[test]
    fn the_paths_are_the_ones_bank_uses_and_fit_the_name_limit() {
        assert_eq!(MOVER_PATHS, [&b"/mover.bin\0"[..], b"/mover.alt.bin\0"]);
        for path in MOVER_PATHS {
            assert_eq!(path.last(), Some(&0));
            assert!(path.len() - 2 <= 16);
        }
    }
}
