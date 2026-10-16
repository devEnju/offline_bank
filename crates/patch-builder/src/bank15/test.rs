//! The console test build of the offline patch (docs/building.md): the
//! offline patch with the original's main menu shown, on which a rehearsal
//! of an interrupted Save and Quit is chosen. Never part of a release.

use super::{offline, word, Profile, MESSAGE_ENTRY, VERSION_ENTRY};
use crate::{elf::PayloadElf, CheckedEdit};

const MENU_ENTRY: &str = "bank_test_menu_chosen";
const MENU_UPDATE_ENTRY: &str = "bank_test_menu_update";

/// Task 4, the main menu: the update slot of its table and the update it
/// holds. The entry gives the buttons the names of the rehearsals and runs
/// the original.
pub const MENU_UPDATE_SLOT: u32 = 0x003617bc;
pub const MENU_UPDATE: u32 = 0x002a6750;

/// The main menu's listener for "an entry was chosen": the one slot of its
/// table (the table at task 4's `+0x34`) and the function it holds, which
/// steps back to the task and handles the choice (002a6c8c).
pub const MENU_CHOSEN_SLOT: u32 = 0x003617dc;
pub const MENU_CHOSEN: u32 = 0x002a6c84;

pub const PROFILE: Profile = Profile {
    name: "test",
    program: &super::BANK,
    entries: &[
        "bank_offline_next",
        "bank_offline_load",
        "bank_offline_save",
        "bank_offline_rewards",
        "bank_offline_timestamp",
        "bank_offline_no_game",
        MESSAGE_ENTRY,
        VERSION_ENTRY,
        MENU_ENTRY,
        MENU_UPDATE_ENTRY,
    ],
    // The offline patch's regions and the two slots of the main menu.
    regions: offline::PROFILE.regions + 2,
    menu_edits: &[MENU_UPDATE_SLOT, MENU_CHOSEN_SLOT],
    edits,
};

fn edits(code: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    let mut edits = offline::edits(code, payload)?;
    edits.extend(menu_edits(
        payload.export(MENU_UPDATE_ENTRY)?,
        payload.export(MENU_ENTRY)?,
    ));
    Ok(edits)
}

fn menu_edits(update: u32, chosen: u32) -> [CheckedEdit; 2] {
    [
        word(MENU_UPDATE_SLOT, MENU_UPDATE, update.to_le_bytes()),
        word(MENU_CHOSEN_SLOT, MENU_CHOSEN, chosen.to_le_bytes()),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::{CODE_BASE, MENU_LOCATIONS};
    use super::*;

    #[test]
    fn the_test_build_is_the_offline_patch_and_two_slots_of_the_main_menu() {
        assert_eq!(PROFILE.regions, offline::PROFILE.regions + 2);
        assert_eq!(
            PROFILE.entries[..offline::PROFILE.entries.len()],
            *offline::PROFILE.entries
        );
        assert_eq!(PROFILE.entries.len(), offline::PROFILE.entries.len() + 2);
        let edits = menu_edits(0x3fb800, 0x3fba00);
        for (edit, slot, original, target) in [
            (&edits[0], MENU_UPDATE_SLOT, MENU_UPDATE, 0x3fb800u32),
            (&edits[1], MENU_CHOSEN_SLOT, MENU_CHOSEN, 0x3fba00),
        ] {
            assert_eq!(edit.offset, (slot - CODE_BASE) as usize);
            assert_eq!(edit.expected, original.to_le_bytes());
            assert_eq!(edit.replacement, target.to_le_bytes());
            // Each is one of the main menu's known locations, with its
            // original word, and no edit of the offline patch is there.
            assert!(MENU_LOCATIONS.contains(&(slot, original)));
            assert!(PROFILE.menu_edits.contains(&slot));
            assert!(offline::sample_edits()
                .iter()
                .all(|other| other.offset != edit.offset));
        }
        assert_eq!(PROFILE.menu_edits.len(), 2);
        assert!(offline::PROFILE.menu_edits.is_empty());
    }
}
