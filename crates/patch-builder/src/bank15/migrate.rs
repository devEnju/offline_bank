//! The migration patch: converts a Bank an earlier release stored and does
//! nothing else. Task 3 runs the conversion without its native game scan.

use super::{router, Profile, KEEP_ARCHIVE, LANGUAGE_EDITS, MESSAGE, SCAN_TASK, STARTUP, VERSION};
use crate::profile::{Edit, Word::*};

pub const PROFILE: Profile = Profile {
    name: "migration",
    program: &super::BANK,
    // The manager's language edits; the first start's edits in task 3's
    // update would never run: the conversion replaces that update.
    edits: &[&SHARED, &OWN, &LANGUAGE_EDITS],
    relies: &[],
    menu_edits: &[],
};

const SHARED: [Edit; 5] = [
    STARTUP,
    KEEP_ARCHIVE,
    router("bank_migrate_next"),
    MESSAGE,
    VERSION,
];

/// Task 3 keeps its native constructor, init and end; its update and busy
/// poll run the conversion.
const OWN: [Edit; 2] = [
    (SCAN_TASK[0].0, SCAN_TASK[0].1, Entry("bank_migrate_scan")),
    (SCAN_TASK[1].0, SCAN_TASK[1].1, Entry("bank_migrate_scan")),
];

#[cfg(test)]
mod tests {
    use super::super::{offline, MENU_LOCATIONS, SCAN_FIRST_START_EDITS};
    use super::*;
    use crate::profile::sample::{at, edits};

    #[test]
    fn the_migration_replaces_the_scan_and_leaves_everything_online_original() {
        assert_eq!(PROFILE.regions(), 10);
        // Task 3's update is replaced, so nothing inside it is edited.
        // Loading, saving, rewards, the timestamp, the scene set-up and the
        // main menu stay the original's.
        for address in SCAN_FIRST_START_EDITS
            .map(|(address, _, _)| address)
            .into_iter()
            .chain(MENU_LOCATIONS.map(|(address, _)| address))
            .chain([
                0x0036_1cfc,
                0x0036_1d0c,
                0x001d_3bf4,
                offline::SCENE_UPDATE_SLOT,
                0x0036_1a08,
                0x0036_1be4,
                0x0036_1ed4,
                0x0036_1ee4,
                0x0036_2034,
                0x0036_2044,
            ])
        {
            assert!(at(&PROFILE, address).is_none(), "{address:#010x}");
        }
    }

    #[test]
    fn where_it_edits_what_the_offline_patch_edits_the_original_word_is_the_same() {
        let offline = edits(&offline::PROFILE);
        for (address, original, _) in edits(&PROFILE) {
            if let Some(same) = offline.iter().find(|edit| edit.0 == address) {
                assert_eq!(same.1, original);
            } else {
                assert_eq!(address, SCAN_TASK[1].0);
            }
        }
    }
}
