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
/// a message is on screen, so wait for it and end the search, which leads
/// back to the title screen; end the search at once.
pub const SEARCH_GO: u32 = 0;
pub const SEARCH_PENDING: u32 = 1;
pub const SEARCH_SHOWN: u32 = 2;
pub const SEARCH_END: u32 = 3;

/// The original's message 3: "Your communication with Pokémon Bank did not
/// complete correctly during your last session. Please open Pokémon Bank
/// and perform the cleanup process before using Poké Transporter." The
/// original shows it where the server asked for that (`0025C964`).
pub const MESSAGE_OPEN_BANK: u32 = 3;
/// The original's message 5: "At least one Pokémon remains in the Transport
/// Box from your previous session. Please empty the Transport Box by using
/// Pokémon Bank before using Poké Transporter."
pub const MESSAGE_BOX_NOT_EMPTY: u32 = 5;

/// The message that stops a session before it begins, for the two slots of
/// Bank's transport box; `None` when a transfer may go ahead. Slots that are
/// `None` could not be read at all: a file is missing, of another size or
/// unreadable, or Bank's extdata is not there.
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

    #[test]
    fn the_check_tells_pokemon_in_the_way_from_a_bank_that_is_not_ready() {
        let slot = |count| {
            Some(Slot {
                tag: None,
                aux: 1,
                count,
                len: 0,
                crc: 0,
            })
        };
        assert_eq!(refusal_message(Some(&[None, None])), None);
        assert_eq!(refusal_message(Some(&[slot(0), None])), None);
        assert_eq!(refusal_message(Some(&[None, slot(0)])), None);
        // The "remains in the Transport Box" message only when one does.
        for slots in [
            [slot(2), None],
            [slot(0), slot(5)],
            [slot(3), slot(0)],
            [slot(1), slot(1)],
        ] {
            assert_eq!(refusal_message(Some(&slots)), Some(MESSAGE_BOX_NOT_EMPTY));
        }
        // An unfinished Bank save, and files that are not there or do not
        // read: Bank before its first start, or one not yet converted.
        assert_eq!(
            refusal_message(Some(&[slot(0), slot(0)])),
            Some(MESSAGE_OPEN_BANK)
        );
        assert_eq!(refusal_message(None), Some(MESSAGE_OPEN_BANK));
        // A refusal is exactly where the delivery itself would refuse.
        for slots in [
            [None, None],
            [slot(0), None],
            [slot(2), None],
            [slot(0), slot(0)],
        ] {
            assert_eq!(
                refusal_message(Some(&slots)).is_none(),
                transport::may_deliver(&slots)
            );
        }
        // The worker reports "go on" as 0, which no message may be.
        assert_eq!((MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY), (3, 5));
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
