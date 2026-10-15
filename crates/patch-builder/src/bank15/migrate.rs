//! The migration patch: converts the Bank of the previous release and does
//! nothing else. Task 3 runs the conversion without its native game scan.

use super::{
    archive_preservation_candidate, language_edits, message_hook, router_call, startup_call,
    version_call, word, Profile, MESSAGE_ENTRY, SCAN_TASK, VERSION_ENTRY,
};
use crate::{elf::PayloadElf, CheckedEdit};

pub const PROFILE: Profile = Profile {
    name: "migrate",
    entries: &[
        "bank_migrate_next",
        "bank_migrate_scan",
        MESSAGE_ENTRY,
        VERSION_ENTRY,
    ],
    // Nine edited words plus the linked bootstrap bytes.
    regions: 10,
    menu_edits: &[],
    edits,
};

fn edits(_: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    hook_edits(
        payload.bootstrap_entry,
        payload.export("bank_migrate_next")?,
        payload.export("bank_migrate_scan")?,
        payload.export(MESSAGE_ENTRY)?,
        payload.export(VERSION_ENTRY)?,
    )
}

fn hook_edits(
    startup: u32,
    next: u32,
    open: u32,
    message: u32,
    version: u32,
) -> crate::Result<Vec<CheckedEdit>> {
    let mut edits = vec![
        archive_preservation_candidate(),
        startup_call(startup)?,
        router_call(next)?,
        message_hook(message)?,
        version_call(version)?,
    ];
    // Task 3 keeps its native constructor, init and end; its update and
    // busy poll run the conversion.
    for (address, expected) in SCAN_TASK {
        edits.push(word(address, expected, open.to_le_bytes()));
    }
    // The manager's language edits. The first start's edits in task 3's
    // update would never run: the conversion replaces that update.
    edits.extend(language_edits());
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::super::{
        offline, CODE_BASE, LANGUAGE_EDITS, RETIRED_MENU_EDITS, SCAN_FIRST_START_EDITS,
    };
    use super::*;

    #[test]
    fn the_migration_replaces_the_scan_and_leaves_online_tasks_original() {
        let edits = hook_edits(0x313910, 0x3fb000, 0x3fb200, 0x3fb400, 0x3fb600).unwrap();
        assert_eq!(edits.len(), PROFILE.regions - 1);
        let at = |address: u32| {
            edits
                .iter()
                .find(|edit| edit.offset == (address - CODE_BASE) as usize)
        };
        for (address, expected) in SCAN_TASK {
            let edit = at(address).unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, 0x3fb200u32.to_le_bytes());
        }
        for (address, expected, replacement) in LANGUAGE_EDITS {
            let edit = at(address).unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, replacement.to_le_bytes());
        }
        // Task 3's update is replaced, so nothing inside it is edited.
        for (address, _, _) in SCAN_FIRST_START_EDITS {
            assert!(at(address).is_none(), "{address:#x}");
        }
        for address in [0x0029f338, 0x001040a4, 0x002a5a2c, 0x001d6650] {
            assert!(at(address).is_some());
        }
        // Loading, saving, rewards, the timestamp and the scene
        // set-up stay the original's, and so does the main menu.
        for address in [
            0x00361cfc,
            0x00361d0c,
            0x001d3bf4,
            offline::SCENE_UPDATE_SLOT,
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
        assert!(hook_edits(0x313910, 0x80000000, 0x3fb200, 0x3fb400, 0x3fb600).is_err());
    }

    #[test]
    fn the_migration_shares_startup_edits_and_has_its_own_utility_task() {
        let migration = hook_edits(0x313910, 0x3fb000, 0x3fb200, 0x3fb400, 0x3fb600).unwrap();
        let offline_edits = offline::sample_edits();
        assert!(migration.len() < offline_edits.len());
        for edit in &migration {
            if SCAN_TASK
                .iter()
                .any(|(address, _)| edit.offset == (address - CODE_BASE) as usize)
            {
                continue;
            }
            let same = offline_edits
                .iter()
                .find(|other| other.offset == edit.offset)
                .unwrap();
            assert_eq!(same.expected, edit.expected);
        }
        // Its own entry functions are its own; the one of `bank-common`
        // and the one for the version line have the same name in both.
        let shared = [MESSAGE_ENTRY, VERSION_ENTRY];
        assert!(PROFILE
            .entries
            .iter()
            .all(|entry| shared.contains(entry) || !offline::PROFILE.entries.contains(entry)));
        assert!(shared
            .iter()
            .all(|entry| offline::PROFILE.entries.contains(entry)));
    }
}
