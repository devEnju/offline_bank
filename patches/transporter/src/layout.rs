//! Where the original keeps the transport box, and what the hooks answer.
//!
//! `task + 8` is the session manager, `manager + 0xCC` the Bank data object,
//! `object + 0xBB524` its transport accessor, and `accessor + 4` the body
//! pointer the original slot functions (`0019A6F4`, `0019A7E0`, `0019A858`,
//! `0024D624`) index from: record `i` at `+0xAAF1C + i * 0xE8`, tag `i` at
//! `+0xAD604 + i`. Both are the Bank body offsets plus 8.

use offline_core::{
    sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS},
    transport::Refusal,
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

/// The message that stops a session before it begins, for what Bank's
/// journal says (`transport::check`). Opening Bank shows or solves each
/// reason, so the message carries no numbers of its own.
pub fn refusal_message(refusal: Refusal) -> u32 {
    match refusal {
        // Pokémon are in the way: in Bank's box, or in a waiting delivery.
        Refusal::Occupied => MESSAGE_BOX_NOT_EMPTY,
        // No Bank yet, a Bank save that was not finished, or a file that
        // does not read: Bank before its first start, one not yet
        // converted, or one whose next save replaces the file.
        Refusal::NotReady => MESSAGE_OPEN_BANK,
    }
}

/// Answers of the delivery entry, as the stub of the patched site at
/// `0024A150` tests them (link.rs).
pub const DELIVER_FAILED: u32 = 0;
pub const DELIVER_DONE: u32 = 1;
pub const DELIVER_PENDING: u32 = 2;

/// Bank's extdata. The files Transporter reads and writes in it, the two
/// journal records and the two copies of the transport box, are named in
/// `offline_core::files`, as Bank names them.
pub const BANK_EXTDATA: u32 = 0x0000_0C9B;

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
        // The "remains in the Transport Box" message only when one does.
        assert_eq!(refusal_message(Refusal::Occupied), MESSAGE_BOX_NOT_EMPTY);
        assert_eq!(refusal_message(Refusal::NotReady), MESSAGE_OPEN_BANK);
        // The worker reports "go on" as 0, which no message may be.
        assert_eq!((MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY), (3, 5));
    }
}
