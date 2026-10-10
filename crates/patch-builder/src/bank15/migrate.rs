//! The migration patch: converts the Bank of v0.2.1 for v0.3.0 and does
//! nothing else. It makes the offline patch's edits on the way from the
//! start screen to "open Bank" and none behind it.

use super::{
    archive_preservation_candidate, first_start_edits, router_call, startup_call, word, Profile,
    OPEN_TASK,
};
use crate::{elf::PayloadElf, CheckedEdit};

pub const PROFILE: Profile = Profile {
    name: "migrate",
    entries: &["bank_migrate_next", "bank_migrate_open"],
    // Thirteen edited words plus the linked bootstrap bytes.
    regions: 14,
    edits,
};

fn edits(_: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    hook_edits(
        payload.bootstrap_entry,
        payload.export("bank_migrate_next")?,
        payload.export("bank_migrate_open")?,
    )
}

fn hook_edits(startup: u32, next: u32, open: u32) -> crate::Result<Vec<CheckedEdit>> {
    let mut edits = vec![
        archive_preservation_candidate(),
        startup_call(startup)?,
        router_call(next)?,
    ];
    // Task 9 keeps its native constructor, init and end; its update and
    // busy poll run the conversion.
    for (address, expected) in OPEN_TASK {
        edits.push(word(address, expected, open.to_le_bytes()));
    }
    edits.extend(first_start_edits());
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::super::{offline, CODE_BASE, FIRST_START_EDITS, RETIRED_MENU_EDITS};
    use super::*;

    #[test]
    fn the_migration_edits_the_way_to_opening_and_nothing_behind_it() {
        let edits = hook_edits(0x313910, 0x3fb000, 0x3fb200).unwrap();
        assert_eq!(edits.len(), PROFILE.regions - 1);
        let at = |address: u32| {
            edits
                .iter()
                .find(|edit| edit.offset == (address - CODE_BASE) as usize)
        };
        for (address, expected) in OPEN_TASK {
            let edit = at(address).unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, 0x3fb200u32.to_le_bytes());
        }
        for (address, expected, replacement) in FIRST_START_EDITS {
            let edit = at(address).unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, replacement.to_le_bytes());
        }
        for address in [0x0029f338, 0x001040a4, 0x002a5a2c] {
            assert!(at(address).is_some());
        }
        // Loading, saving, rewards, Pokédex, the timestamp and the scene
        // set-up stay the original's, and so does the main menu.
        for address in [
            0x001d3bf4,
            offline::SCENE_UPDATE_SLOT,
            0x0033d30c,
            0x0033d34c,
            0x003600cc,
            0x00361a08,
            0x00361be4,
            0x00361ed4,
            0x00361ee4,
            0x00362034,
            0x00362044,
        ] {
            assert!(at(address).is_none(), "{address:#x}");
        }
        for (address, _) in RETIRED_MENU_EDITS {
            assert!(at(address).is_none());
        }
        let mut offsets: Vec<_> = edits.iter().map(|edit| edit.offset).collect();
        offsets.sort_unstable();
        offsets.dedup();
        assert_eq!(offsets.len(), edits.len());
        assert!(hook_edits(0x313910, 0x80000000, 0x3fb200).is_err());
    }

    #[test]
    fn every_edit_of_the_migration_is_one_the_offline_patch_makes_at_the_same_place() {
        let migration = hook_edits(0x313910, 0x3fb000, 0x3fb200).unwrap();
        let offline_edits = offline::sample_edits();
        assert!(migration.len() < offline_edits.len());
        for edit in &migration {
            let same = offline_edits
                .iter()
                .find(|other| other.offset == edit.offset)
                .unwrap();
            assert_eq!(same.expected, edit.expected);
        }
        assert!(PROFILE
            .entries
            .iter()
            .all(|entry| !offline::PROFILE.entries.contains(entry)));
    }
}
