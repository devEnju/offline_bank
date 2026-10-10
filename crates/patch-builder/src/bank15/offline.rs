//! The offline patch: Bank without its server.

use super::{
    archive_preservation_candidate, first_start_edits, router_call, startup_call, word, Profile,
    CODE_BASE, OPEN_TASK,
};
use crate::{elf::PayloadElf, CheckedEdit};

pub const PROFILE: Profile = Profile {
    name: "offline",
    entries: &[
        "bank_offline_next",
        "bank_offline_load",
        "bank_offline_save",
        "bank_offline_rewards",
        "bank_offline_timestamp",
        "bank_offline_dex_save_request",
        "bank_offline_dex_records_update",
        "bank_offline_dex_records_finish",
    ],
    // Twenty-four edited words plus the linked bootstrap bytes.
    regions: 25,
    edits,
};

/// Task 0xb (scene set-up between game selection and Bank loading): the
/// update slot of its vtable, the update it holds (a server check), and the
/// native function that replaces it. That function is the task's own "end"
/// callback and only answers "finished": `mov r0, #1; bx lr`.
pub const SCENE_UPDATE_SLOT: u32 = 0x00361e84;
pub const SCENE_UPDATE: u32 = 0x002af034;
pub const NATIVE_FINISHED: u32 = 0x002af124;
pub const NATIVE_FINISHED_BODY: [u32; 2] = [0xe3a00001, 0xe12fff1e];

fn edits(code: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>> {
    for (index, word) in NATIVE_FINISHED_BODY.into_iter().enumerate() {
        let offset = (NATIVE_FINISHED - CODE_BASE) as usize + index * 4;
        if crate::le32(code, offset)? != word {
            return crate::fail("native finished stub 002af124 is not the reviewed code");
        }
    }
    native_hook_edits(HookTargets {
        startup: payload.bootstrap_entry,
        next: payload.export("bank_offline_next")?,
        load: payload.export("bank_offline_load")?,
        save: payload.export("bank_offline_save")?,
        rewards: payload.export("bank_offline_rewards")?,
        timestamp: payload.export("bank_offline_timestamp")?,
        dex_save_request: payload.export("bank_offline_dex_save_request")?,
        dex_records_update: payload.export("bank_offline_dex_records_update")?,
        dex_records_finish: payload.export("bank_offline_dex_records_finish")?,
    })
}

#[derive(Clone, Copy)]
struct HookTargets {
    startup: u32,
    next: u32,
    load: u32,
    save: u32,
    rewards: u32,
    timestamp: u32,
    dex_save_request: u32,
    dex_records_update: u32,
    dex_records_finish: u32,
}
fn native_hook_edits(targets: HookTargets) -> crate::Result<Vec<CheckedEdit>> {
    // All addresses and original words independently checked against the whole
    // fingerprint above. Callsite hooks use ARM BL; the timestamp entry uses
    // ARM B to retain its caller's LR/SP before the original prologue executes.
    let mut edits = vec![
        archive_preservation_candidate(),
        word(
            0x001d3bf4,
            0xe92d4ff3,
            crate::encode_arm_branch(0x001d3bf4, targets.timestamp, false)?,
        ),
        startup_call(targets.startup)?,
        router_call(targets.next)?,
    ];
    // Original read-only vtables: taskC/D rewards, task9 open, task0x10 load, task7 normal /
    // forced-busy updates. Native constructors/init/end callbacks remain intact.
    // The main menu (task 4) is never created, so none of its code is edited.
    // Task 0xb keeps its native start-up routine, which orders the character
    // scene and the text window; only its server check is replaced.
    for (address, expected, target) in [
        (SCENE_UPDATE_SLOT, SCENE_UPDATE, NATIVE_FINISHED),
        (0x0033d30c, 0x002a7578, targets.dex_save_request),
        (0x0033d34c, 0x002a71a8, targets.dex_records_finish),
        (0x003600cc, 0x0026d8ec, targets.dex_records_update),
        (0x00361a08, 0x002a9750, targets.rewards),
        (0x00361be4, 0x002ad1bc, targets.rewards),
        (OPEN_TASK[0].0, OPEN_TASK[0].1, targets.load),
        (OPEN_TASK[1].0, OPEN_TASK[1].1, targets.load),
        (0x00361ed4, 0x002af460, targets.load),
        (0x00361ee4, 0x002b4a20, targets.load),
        (0x00362034, 0x002b1cf8, targets.save),
        (0x00362044, 0x002b4a20, targets.save),
    ] {
        edits.push(word(address, expected, target.to_le_bytes()));
    }
    edits.extend(first_start_edits());
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::super::{FIRST_START_EDITS, RETIRED_MENU_EDITS};
    use super::*;
    use crate::encode_arm_branch;

    #[test]
    fn development_hooks_encode_reviewed_native_calls_and_vtables() {
        let targets = HookTargets {
            startup: 0x313910,
            next: 0x3fb000,
            load: 0x3fb200,
            save: 0x3fb400,
            rewards: 0x3fba00,
            timestamp: 0x3fbc00,
            dex_save_request: 0x3fc400,
            dex_records_update: 0x3fc600,
            dex_records_finish: 0x3fc800,
        };
        let edits = native_hook_edits(targets).unwrap();
        assert_eq!(edits.len(), PROFILE.regions - 1);
        // No remaining edit touches the main menu's code or tables.
        for (address, _) in RETIRED_MENU_EDITS {
            assert!(edits
                .iter()
                .all(|edit| edit.offset != (address - CODE_BASE) as usize));
        }
        for (source, original, destination) in [
            (0x1040a4, 0x10494c, targets.startup),
            (0x2a5a2c, 0x2a5580, targets.next),
        ] {
            let edit = edits
                .iter()
                .find(|e| e.offset == (source - CODE_BASE) as usize)
                .unwrap();
            assert_eq!(
                edit.expected,
                encode_arm_branch(source, original, true).unwrap()
            );
            assert_eq!(
                edit.replacement,
                encode_arm_branch(source, destination, true).unwrap()
            );
        }
        for (address, target) in [
            (0x361e84, 0x2af124),
            (0x33d30c, targets.dex_save_request),
            (0x33d34c, targets.dex_records_finish),
            (0x3600cc, targets.dex_records_update),
            (0x361d0c, targets.load),
            (0x361ee4, targets.load),
            (0x361a08, targets.rewards),
            (0x361be4, targets.rewards),
            (0x361cfc, targets.load),
            (0x361ed4, targets.load),
            (0x362034, targets.save),
            (0x362044, targets.save),
        ] {
            let edit = edits
                .iter()
                .find(|e| e.offset == (address - CODE_BASE) as usize)
                .unwrap();
            assert_eq!(edit.replacement, target.to_le_bytes());
        }
        // The first start: each edit is present with its original word, and
        // the notice is skipped to where accepting it continues.
        for (address, expected, replacement) in FIRST_START_EDITS {
            let edit = edits
                .iter()
                .find(|e| e.offset == (address - CODE_BASE) as usize)
                .unwrap();
            assert_eq!(edit.expected, expected.to_le_bytes());
            assert_eq!(edit.replacement, replacement.to_le_bytes());
        }
        let timestamp = edits.iter().find(|e| e.offset == 0x000d3bf4).unwrap();
        assert_eq!(timestamp.expected, [0xf3, 0x4f, 0x2d, 0xe9]);
        assert_eq!(
            timestamp.replacement,
            encode_arm_branch(0x001d3bf4, targets.timestamp, false).unwrap()
        );
        // An entry tail branch must not replace LR as BL would.
        assert_eq!(timestamp.replacement[3], 0xea);
        assert!(native_hook_edits(HookTargets {
            timestamp: 0x80000000,
            ..targets
        })
        .is_err());
        let mut offsets: Vec<_> = edits.iter().map(|edit| edit.offset).collect();
        offsets.sort_unstable();
        offsets.dedup();
        assert_eq!(offsets.len(), edits.len());
        assert!(native_hook_edits(HookTargets {
            next: 0x80000000,
            ..targets
        })
        .is_err());
    }
}
