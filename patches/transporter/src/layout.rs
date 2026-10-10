//! Where the original keeps the transport box, and what the hooks answer.
//!
//! `task + 8` is the session manager, `manager + 0xCC` the Bank data object,
//! `object + 0xBB524` its transport accessor, and `accessor + 4` the body
//! pointer the original slot functions (`0019A6F4`, `0019A7E0`, `0019A858`,
//! `0024D624`) index from: record `i` at `+0xAAF1C + i * 0xE8`, tag `i` at
//! `+0xAD604 + i`. Both are the Bank body offsets plus 8.

use offline_core::sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS};

pub const TASK_MANAGER: usize = 0x8;
/// Index into the original message table used by sub-state 7 of the check.
pub const TASK_MESSAGE_INDEX: usize = 0x40;
pub const MANAGER_BANK_OBJECT: usize = 0xCC;
pub const BANK_OBJECT_ACCESSOR: usize = 0xBB524;
pub const ACCESSOR_BODY: usize = 0x4;
/// The accessor's body pointer is 8 bytes before the body Bank stores.
pub const BODY_BIAS: usize = 8;
pub const RECORDS_OFFSET: usize = TRANSPORT_RECORDS.start + BODY_BIAS;
pub const TAGS_OFFSET: usize = TRANSPORT_TAGS.start + BODY_BIAS;
pub const RECORDS_LEN: usize = TRANSPORT_SLOTS * RECORD_SIZE;

/// Sub-states of the original "can Bank take them?" task (`00248C68`).
/// 0 re-enters the patched site on the next frame; 3 continues to the
/// question; 7 shows the original "transport box is not empty" message and
/// ends without a transfer.
pub const CHECK_PENDING: u32 = 0;
pub const CHECK_ALLOWED: u32 = 3;
pub const CHECK_REFUSED: u32 = 7;

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
    fn the_paths_are_the_ones_bank_uses_and_fit_the_name_limit() {
        assert_eq!(MOVER_PATHS, [&b"/mover.bin\0"[..], b"/mover.alt.bin\0"]);
        for path in MOVER_PATHS {
            assert_eq!(path.last(), Some(&0));
            assert!(path.len() - 2 <= 16);
        }
    }
}
