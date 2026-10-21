//! The console test build of the offline patch (docs/building.md): the
//! offline patch with the original's main menu shown, on which a rehearsal
//! of an interrupted Save and Quit is chosen. Never part of a release.

use super::{offline, Profile, LANGUAGE_EDITS, SCAN_FIRST_START_EDITS};
use crate::profile::{Edit, Word::*};

/// Task 4, the main menu: the update slot of its table and the update it
/// holds. The entry gives the buttons the names of the rehearsals and runs
/// the original.
pub const MENU_UPDATE_SLOT: u32 = 0x0036_17bc;
/// The main menu's listener for "an entry was chosen": the one slot of its
/// table (the table at task 4's `+0x34`) and the function it holds, which
/// steps back to the task and handles the choice (002a6c8c).
pub const MENU_CHOSEN_SLOT: u32 = 0x0036_17dc;

pub const PROFILE: Profile = Profile {
    name: "test",
    program: &super::BANK,
    edits: &[
        &offline::SHARED,
        &offline::OWN,
        &LANGUAGE_EDITS,
        &SCAN_FIRST_START_EDITS,
        &MENU,
    ],
    relies: offline::PROFILE.relies,
    menu_edits: &[MENU_UPDATE_SLOT, MENU_CHOSEN_SLOT],
};

const MENU: [Edit; 2] = [
    (
        MENU_UPDATE_SLOT,
        0x002a_6750,
        Entry("bank_test_menu_update"),
    ),
    (
        MENU_CHOSEN_SLOT,
        0x002a_6c84,
        Entry("bank_test_menu_chosen"),
    ),
];

#[cfg(test)]
mod tests {
    use super::super::MENU_LOCATIONS;
    use super::*;

    #[test]
    fn the_test_build_is_the_offline_patch_and_two_slots_of_the_main_menu() {
        assert_eq!(PROFILE.edits[..4], *offline::PROFILE.edits);
        assert_eq!(PROFILE.regions(), offline::PROFILE.regions() + 2);
        let entries = PROFILE.entries();
        assert_eq!(entries[..9], offline::PROFILE.entries()[..]);
        assert_eq!(
            entries[9..],
            ["bank_test_menu_update", "bank_test_menu_chosen"]
        );
        // Each slot is one of the main menu's known locations, with its
        // original word.
        for (slot, original, _) in MENU {
            assert!(MENU_LOCATIONS.contains(&(slot, original)));
            assert!(PROFILE.menu_edits.contains(&slot));
        }
    }
}
