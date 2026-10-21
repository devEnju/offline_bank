//! The original's task objects, as far as the hooks need them: where a task
//! keeps its manager and its UI, where the manager keeps the Bank data
//! object with the transport box, and the HOME and sleep mask
//! (`refuse_home`). See docs/internals.md, "Bank data object".
//!
//! `task + 8` is the session manager, `manager + 0xCC` the Bank data object,
//! `object + 0xBB524` its transport accessor, and `accessor + 4` the body
//! pointer the original slot functions (`0019A6F4`, `0019A7E0`, `0019A858`,
//! `0024D624`) index from: record `i` at `+0xAAF1C + i * 0xE8`, tag `i` at
//! `+0xAD604 + i`. Both are the Bank body offsets plus 8.

use offline_core::sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS};

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

/// Refuses HOME and sleep, or lets them through again
/// (`patch_common::home`).
#[cfg(target_arch = "arm")]
pub fn refuse_home(refuse: bool) {
    patch_common::home::refuse::<crate::program::Transporter15>(refuse);
}

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
}
