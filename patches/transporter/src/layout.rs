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

/// The first numbers of Bank's error screen that say the same here
/// (docs/bank.md, "Troubleshooting"): opening the files, loading them, and
/// an interrupted save that has to be finished.
pub const STEP_OPEN: u32 = 3;
pub const STEP_LOAD: u32 = 9;
pub const STEP_RECOVERY: u32 = 0x10;
/// Bank's second numbers: a call that succeeded without a handle for the
/// file, and a transport file that is damaged.
pub const NO_HANDLE: u32 = 0x7100_0003;
pub const TRANSPORT_DAMAGED: u32 = 0x7a00_0201;

/// What the check found of Bank's transport box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// Neither file of the box opens: Bank's extdata is not there, the
    /// offline Bank was never started, or its Bank is not converted. With
    /// the console's result code of the first open that failed.
    Closed(u32),
    /// A file of another size than Bank creates.
    Size(u64),
    /// The two slots; `None` if a slot could not be read.
    Slots(Option<[Option<Slot>; SLOTS]>),
}

/// The message that stops a session before it begins, and the two numbers
/// that go with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub message: u32,
    /// Only the "open Pokémon Bank" message has more than one cause.
    pub numbers: Option<(u32, u32)>,
}

/// `None` when a transfer may go ahead.
pub fn refusal(found: &Found) -> Option<Refusal> {
    let open_bank = |first, second| {
        Some(Refusal {
            message: MESSAGE_OPEN_BANK,
            numbers: Some((first, second)),
        })
    };
    match found {
        Found::Closed(code) => open_bank(STEP_OPEN, *code),
        // As Bank shows a size: never one of the console's result codes.
        Found::Size(size) => open_bank(STEP_OPEN, (*size as u32) & 0x6fff_ffff),
        Found::Slots(None) => open_bank(STEP_LOAD, TRANSPORT_DAMAGED),
        Found::Slots(Some(slots)) if transport::may_deliver(slots) => None,
        // Pokémon are in the way: in Bank's box, or in a waiting delivery.
        Found::Slots(Some(slots)) if slots.iter().flatten().any(|slot| slot.count != 0) => {
            Some(Refusal {
                message: MESSAGE_BOX_NOT_EMPTY,
                numbers: None,
            })
        }
        // Two valid empty slots: a Bank save was not finished.
        Found::Slots(Some(_)) => open_bank(STEP_RECOVERY, 0),
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
        refusal(&Found::Slots(Some(slots))).map(|refusal| refusal.message)
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
        for found in [
            Found::Slots(None),
            Found::Closed(0xc880_4478),
            Found::Size(14_112),
        ] {
            assert_eq!(refusal(&found).unwrap().message, MESSAGE_OPEN_BANK);
        }
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
    fn only_the_message_with_several_causes_gets_numbers_and_they_are_banks() {
        let numbers = |found| refusal(&found).unwrap().numbers;
        // Opening the files: the console's own code, or the size found.
        assert_eq!(numbers(Found::Closed(0xc880_4478)), Some((3, 0xc880_4478)));
        assert_eq!(numbers(Found::Closed(NO_HANDLE)), Some((3, 0x7100_0003)));
        assert_eq!(numbers(Found::Size(14_112)), Some((3, 14_112)));
        // A size never reads as a result code of the console or of Bank.
        assert_eq!(numbers(Found::Size(u64::MAX)), Some((3, 0x6fff_ffff)));
        // Loading: the transport file is damaged.
        assert_eq!(numbers(Found::Slots(None)), Some((9, 0x7a00_0201)));
        // An interrupted save has to be finished.
        assert_eq!(
            numbers(Found::Slots(Some([slot(0), slot(0)]))),
            Some((0x10, 0))
        );
        // Pokémon in the way is one cause, and its message says it.
        for slots in [[slot(2), None], [slot(0), slot(5)], [slot(1), slot(1)]] {
            assert_eq!(numbers(Found::Slots(Some(slots))), None);
        }
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
