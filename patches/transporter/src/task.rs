//! The original's task objects, as far as the hooks need them: their
//! vtables, where a task keeps its manager and its UI, where the manager
//! keeps the Bank data object with the transport box, and the HOME and sleep
//! mask (`refuse_home`). See docs/internals.md, "Tasks and routing" and
//! "Bank data object".
//!
//! `task + 8` is the session manager, `manager + 0xCC` the Bank data object,
//! `object + 0xBB524` its transport accessor, and `accessor + 4` the body
//! pointer the original slot functions (`0019A6F4`, `0019A7E0`, `0019A858`,
//! `0024D624`) index from: record `i` at `+0xAAF1C + i * 0xE8`, tag `i` at
//! `+0xAD604 + i`. Both are the Bank body offsets plus 8.

use core::ptr::NonNull;
use offline_core::sections::{RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_SLOTS, TRANSPORT_TAGS};
use patch_common::native::{follow, object, NotNative};

/// Factory 0019CEE4 accepts IDs below 0x13 (0019CEE8).
pub const TASK_COUNT: u32 = 0x13;
/// Vtables installed by the factory; the update is slot +0xC.
pub const TASK_LANGUAGE: u32 = 0x002e_6680;
pub const TASK_TRANSFER: u32 = 0x002e_6a90;
pub const TASK_SEARCH: u32 = 0x002e_6774;
pub const TASK_LIST: u32 = 0x002e_6618;
pub const TASK_READ: u32 = 0x002e_66b8;
pub const TASK_BANK_STEP: u32 = 0x002e_6970;
pub const TASK_QUESTION: u32 = 0x002e_67c4;
/// Constructor 0010D8EC installs this at 0010D8FC; startup stores the
/// constructed object at manager + 0xCC (0010624C).
pub const BANK_VTABLE: u32 = 0x002e_6dd0;

#[derive(Clone, Copy)]
pub struct Task(NonNull<u8>);
impl Task {
    /// The task a hook is given, if its vtable is one of `expected`
    /// (`patch_common::native::object`).
    /// # Safety
    /// `raw` is the live task object the patched site passes, on the main
    /// thread.
    pub unsafe fn from_raw(raw: *mut u8, expected: &[u32]) -> Result<Self, NotNative> {
        unsafe { object(raw, expected) }.map(Self)
    }
    pub fn raw(self) -> *mut u8 {
        self.0.as_ptr()
    }
    pub fn pointer(self, offset: usize) -> Result<*mut u8, NotNative> {
        unsafe { follow(self.raw(), offset) }.ok_or(NotNative)
    }
    /// The manager's Bank data object, if its vtable is the one its
    /// constructor installs, before anything of it is read.
    pub fn bank_pointer(self) -> Result<*mut u8, NotNative> {
        let manager = self.pointer(TASK_MANAGER)?;
        let bank = unsafe { follow(manager, MANAGER_BANK_OBJECT) }.ok_or(NotNative)?;
        unsafe { object(bank, &[BANK_VTABLE]) }.map(NonNull::as_ptr)
    }
}

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
