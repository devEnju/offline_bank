//! The download patch: has the original load the Bank from its server and
//! writes it as the offline patch's files. Everything online stays the
//! original's; the patch takes the router call, task 9 and the reward step,
//! and has the scan go on without its question when it finds no game.

use super::{
    archive_preservation_candidate, router_call, startup_call, version_call, word, Profile,
    OPEN_TASK, VERSION_ENTRY,
};
use crate::{elf::PayloadElf, CheckedEdit};

pub const PROFILE: Profile = Profile {
    name: "download",
    entries: &[
        "bank_download_next",
        "bank_download_open",
        "bank_download_open_busy",
        "bank_download_save",
        VERSION_ENTRY,
    ],
    // Eight edited words plus the linked bootstrap bytes.
    regions: 9,
    menu_edits: &[],
    edits,
};

/// The game scan's state 0x11, where it has found no usable game (all
/// eight availability bytes zero): `moveq r0, #0x18` at 002ac560 selects
/// the state that asks about Pokémon HOME, and this `beq 002ac4c4` stores
/// it. The branch goes to 002aca50 instead, the original's own code for the
/// first button of that question after its touch of the dialog: state
/// 0x12 and the byte `task + 0x44`, with which the scan ends with outcome
/// 0x17 and the session takes the load that needs no game.
pub const NO_GAME_BRANCH: (u32, u32, u32) = (0x002ac564, 0x0affffd6, 0x0a000139);

/// Task 0xc, the reward step the patch enters after the load: the update
/// slot of its vtable and the update it holds.
pub const REWARD_UPDATE_SLOT: u32 = 0x00361be4;
pub const REWARD_UPDATE: u32 = 0x002ad1bc;

fn edits(_: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    hook_edits(Targets {
        startup: payload.bootstrap_entry,
        next: payload.export("bank_download_next")?,
        open: payload.export("bank_download_open")?,
        open_busy: payload.export("bank_download_open_busy")?,
        save: payload.export("bank_download_save")?,
        version: payload.export(VERSION_ENTRY)?,
    })
}

#[derive(Clone, Copy)]
struct Targets {
    startup: u32,
    next: u32,
    open: u32,
    open_busy: u32,
    save: u32,
    version: u32,
}

fn hook_edits(targets: Targets) -> crate::Result<Vec<CheckedEdit>> {
    Ok(vec![
        archive_preservation_candidate(),
        startup_call(targets.startup)?,
        router_call(targets.next)?,
        version_call(targets.version)?,
        // Task 9 keeps its native constructor, init and end. Its update and
        // its busy poll each go to an entry of their own, which calls the
        // function the slot held while the session is online.
        word(OPEN_TASK[0].0, OPEN_TASK[0].1, targets.open.to_le_bytes()),
        word(
            OPEN_TASK[1].0,
            OPEN_TASK[1].1,
            targets.open_busy.to_le_bytes(),
        ),
        word(
            NO_GAME_BRANCH.0,
            NO_GAME_BRANCH.1,
            NO_GAME_BRANCH.2.to_le_bytes(),
        ),
        // The reward step asks the server nothing: its update writes the
        // files and ends as cancelled.
        word(
            REWARD_UPDATE_SLOT,
            REWARD_UPDATE,
            targets.save.to_le_bytes(),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::super::{offline, CODE_BASE, FIRST_START_EDITS, RETIRED_MENU_EDITS};
    use super::*;

    const TARGETS: Targets = Targets {
        startup: 0x313910,
        next: 0x3fb000,
        open: 0x3fb200,
        open_busy: 0x3fb400,
        save: 0x3fb600,
        version: 0x3fb800,
    };

    #[test]
    fn the_download_edits_the_router_task_nine_the_reward_step_and_the_scan_only() {
        let edits = hook_edits(TARGETS).unwrap();
        assert_eq!(edits.len(), PROFILE.regions - 1);
        let at = |address: u32| {
            edits
                .iter()
                .find(|edit| edit.offset == (address - CODE_BASE) as usize)
        };
        for ((address, expected), target) in OPEN_TASK.into_iter().zip([0x3fb200u32, 0x3fb400]) {
            let edit = at(address).unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, target.to_le_bytes());
        }
        let reward = at(REWARD_UPDATE_SLOT).unwrap();
        assert_eq!(reward.expected, REWARD_UPDATE.to_le_bytes());
        assert_eq!(reward.replacement, 0x3fb600u32.to_le_bytes());
        // The scan's branch is a conditional branch before and after, from
        // 002ac564 to 002ac4c4 and to 002aca50.
        let scan = at(NO_GAME_BRANCH.0).unwrap();
        assert_eq!(scan.expected, NO_GAME_BRANCH.1.to_le_bytes());
        assert_eq!(scan.replacement, NO_GAME_BRANCH.2.to_le_bytes());
        let target = |word: u32| {
            assert_eq!(word >> 24, 0x0a);
            let offset = ((word << 8) as i32) >> 6;
            (NO_GAME_BRANCH.0 + 8).wrapping_add(offset as u32)
        };
        assert_eq!(target(NO_GAME_BRANCH.1), 0x002ac4c4);
        assert_eq!(target(NO_GAME_BRANCH.2), 0x002aca50);
        // The original stays as it is where the offline patch replaces the
        // server: the first start's prompts, the load, the save, the reward
        // claim, the clock, the show-message function
        // (the original refuses HOME for its whole session itself), and the
        // main menu.
        for (address, _, _) in FIRST_START_EDITS {
            assert!(at(address).is_none());
        }
        for address in [
            0x001d6650, 0x001d3bf4, 0x00361ed4, 0x00361ee4, 0x00362034, 0x00362044, 0x00361a08,
            0x00361e84,
        ] {
            assert!(at(address).is_none(), "{address:#x}");
        }
        for (address, _) in RETIRED_MENU_EDITS {
            assert!(at(address).is_none());
        }
        assert!(PROFILE.menu_edits.is_empty());
        assert!(hook_edits(Targets {
            next: 0x80000000,
            ..TARGETS
        })
        .is_err());
    }

    #[test]
    fn where_it_edits_what_the_offline_patch_edits_the_original_word_is_the_same() {
        let offline_edits = offline::sample_edits();
        let mut own = 0;
        for edit in hook_edits(TARGETS).unwrap() {
            match offline_edits
                .iter()
                .find(|other| other.offset == edit.offset)
            {
                Some(same) => assert_eq!(same.expected, edit.expected),
                None => own += 1,
            }
        }
        // One place is the download's alone: the scan's no-game branch.
        assert_eq!(own, 1);
        // Its own entry functions are its own; the one for the version line
        // has the same name in every patch.
        assert!(PROFILE
            .entries
            .iter()
            .all(|entry| *entry == VERSION_ENTRY || !offline::PROFILE.entries.contains(entry)));
    }
}
