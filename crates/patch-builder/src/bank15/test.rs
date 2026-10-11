//! The console test build of the offline patch (docs/building.md): the
//! offline patch with the original's main menu shown, on which a rehearsal
//! of an interrupted Save and Quit is chosen. Never part of a release.

use super::{offline, word, Profile, MESSAGE_ENTRY, VERSION_ENTRY};
use crate::{elf::PayloadElf, CheckedEdit};

const MENU_ENTRY: &str = "bank_test_menu_chosen";

/// The main menu's listener for "an entry was chosen": the one slot of its
/// table (the table at task 4's `+0x34`) and the function it holds, which
/// steps back to the task and handles the choice (002a6c8c).
pub const MENU_CHOSEN_SLOT: u32 = 0x003617dc;
pub const MENU_CHOSEN: u32 = 0x002a6c84;

pub const PROFILE: Profile = Profile {
    name: "test",
    entries: &[
        "bank_offline_next",
        "bank_offline_load",
        "bank_offline_save",
        "bank_offline_rewards",
        "bank_offline_timestamp",
        "bank_offline_dex_save_request",
        "bank_offline_dex_records_update",
        "bank_offline_dex_records_finish",
        MESSAGE_ENTRY,
        VERSION_ENTRY,
        MENU_ENTRY,
    ],
    // The offline patch's regions and the listener slot.
    regions: offline::PROFILE.regions + 1,
    menu_edits: &[MENU_CHOSEN_SLOT],
    edits,
};

fn edits(code: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    let mut edits = offline::edits(code, payload)?;
    edits.push(menu_edit(payload.export(MENU_ENTRY)?));
    Ok(edits)
}

fn menu_edit(target: u32) -> CheckedEdit {
    word(MENU_CHOSEN_SLOT, MENU_CHOSEN, target.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::super::{CODE_BASE, RETIRED_MENU_EDITS};
    use super::*;

    #[test]
    fn the_test_build_is_the_offline_patch_and_one_slot_of_the_main_menu() {
        assert_eq!(PROFILE.regions, offline::PROFILE.regions + 1);
        assert_eq!(
            PROFILE.entries[..offline::PROFILE.entries.len()],
            *offline::PROFILE.entries
        );
        assert_eq!(PROFILE.entries.len(), offline::PROFILE.entries.len() + 1);
        let edit = menu_edit(0x3fb800);
        assert_eq!(edit.offset, (MENU_CHOSEN_SLOT - CODE_BASE) as usize);
        assert_eq!(edit.expected, MENU_CHOSEN.to_le_bytes());
        assert_eq!(edit.replacement, 0x3fb800u32.to_le_bytes());
        // It is one of the main menu's known locations, with its original
        // word, and no edit of the offline patch is there.
        assert!(RETIRED_MENU_EDITS.contains(&(MENU_CHOSEN_SLOT, MENU_CHOSEN)));
        assert_eq!(PROFILE.menu_edits, [MENU_CHOSEN_SLOT]);
        assert!(offline::sample_edits()
            .iter()
            .all(|other| other.offset != edit.offset));
        assert!(offline::PROFILE.menu_edits.is_empty());
    }
}
