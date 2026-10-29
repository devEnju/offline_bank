//! The download patch: has the original load the Bank from its server and
//! writes it as the offline patch's files. Everything online stays the
//! original's; task 3 checks local data before native preparation, asks
//! neither for a language nor for the first start's notice, and never
//! detects games. Task 9 and the reward step load and store the Bank.

use super::{
    router, Profile, FIRST_START_NOTICE, KEEP_ARCHIVE, LANGUAGE_EDITS, OPEN_TASK, SCAN_TASK,
    STARTUP, VERSION,
};
use crate::profile::{Edit, Word::*};

pub const PROFILE: Profile = Profile {
    name: "download",
    program: &super::BANK,
    edits: &[&EDITS, &LANGUAGE_EDITS],
    relies: &[],
    menu_edits: &[],
};

/// Task 0xc, the reward step the patch enters after the load: the update
/// slot of its vtable and the update it holds.
pub const REWARD_UPDATE_SLOT: u32 = 0x0036_1be4;
pub const REWARD_UPDATE: u32 = 0x002a_d1bc;

/// The original's "show a message" is not edited: the original refuses
/// HOME for its whole online session itself. Of the first start's edits in
/// task 3 only the notice's: a language the save holds stays in it.
const EDITS: [Edit; 10] = [
    STARTUP,
    KEEP_ARCHIVE,
    router("bank_download_next"),
    VERSION,
    FIRST_START_NOTICE,
    // Task 9 keeps its native constructor, init and end. Its update and its
    // busy poll each go to an entry of their own, which calls the function
    // the slot held while the session is online.
    (OPEN_TASK[0].0, OPEN_TASK[0].1, Entry("bank_download_open")),
    (
        OPEN_TASK[1].0,
        OPEN_TASK[1].1,
        Entry("bank_download_open_busy"),
    ),
    (SCAN_TASK[0].0, SCAN_TASK[0].1, Entry("bank_download_scan")),
    (
        SCAN_TASK[1].0,
        SCAN_TASK[1].1,
        Entry("bank_download_scan_busy"),
    ),
    // The reward step asks the server nothing: its update writes the files
    // and ends as cancelled.
    (
        REWARD_UPDATE_SLOT,
        REWARD_UPDATE,
        Entry("bank_download_save"),
    ),
];

#[cfg(test)]
mod tests {
    use super::super::{offline, MENU_LOCATIONS, MESSAGE, SCAN_FIRST_START_EDITS};
    use super::*;
    use crate::profile::sample::{at, edits};

    #[test]
    fn the_download_edits_the_router_task_nine_the_reward_step_the_scan_and_the_first_start_only() {
        assert_eq!(PROFILE.regions(), 13);
        // The first start asks for no language and shows no notice.
        for (address, _, _) in LANGUAGE_EDITS.into_iter().chain([FIRST_START_NOTICE]) {
            assert!(at(&PROFILE, address).is_some(), "{address:#010x}");
        }
        // The original stays as it is where the offline patch replaces the
        // server: the saved language, the load, the save, the reward claim,
        // the clock, the show-message function, and the main menu.
        for address in SCAN_FIRST_START_EDITS[1..]
            .iter()
            .map(|(address, _, _)| *address)
            .chain(MENU_LOCATIONS.map(|(address, _)| address))
            .chain([
                MESSAGE.0,
                0x001d_3bf4,
                0x0036_1ed4,
                0x0036_1ee4,
                0x0036_2034,
                0x0036_2044,
                0x0036_1a08,
                offline::SCENE_UPDATE_SLOT,
                0x002a_c564,
            ])
        {
            assert!(at(&PROFILE, address).is_none(), "{address:#010x}");
        }
    }

    #[test]
    fn where_it_edits_what_the_offline_patch_edits_the_original_word_is_the_same() {
        let offline = edits(&offline::PROFILE);
        let mut own = 0;
        for (address, original, _) in edits(&PROFILE) {
            match offline.iter().find(|edit| edit.0 == address) {
                Some(same) => assert_eq!(same.1, original),
                None => own += 1,
            }
        }
        // Its scan task's busy poll; the scan's update, which it replaces
        // before game detection, the offline patch replaces too.
        assert_eq!(own, 1);
    }
}
