//! Reviewed Bank image identity and checked development hook profile.
//! Hardware behavior and save recovery still require console validation.

use crate::{placement::InputIdentity, CheckedEdit, BANK_TITLE_ID};

pub const TMD_VERSION: u16 = 6272;
pub const REMASTER_VERSION: u16 = 6;
pub const CODE_LENGTH: usize = 0x2ac000;
pub const CODE_BASE: u32 = 0x00100000;
pub const PAYLOAD_ADDRESS: u32 = 0x003fb000;
pub const PAYLOAD_FILE_OFFSET: usize = 0x002fb000;

pub const CODE_SHA256: [u8; 32] = [
    0x2d, 0xce, 0x47, 0x96, 0xf5, 0x48, 0x07, 0xcf, 0x8a, 0x67, 0xf1, 0xce, 0x62, 0x97, 0xbf, 0x47,
    0x2d, 0x96, 0x9b, 0x30, 0xed, 0x7a, 0x7e, 0x8e, 0x25, 0xc2, 0xa6, 0xc2, 0xbd, 0xc4, 0x0a, 0xbf,
];
/// Hash covers the complete extracted 0x800-byte exheader.
pub const EXHEADER_SHA256: [u8; 32] = [
    0x39, 0xd9, 0x35, 0x84, 0xb0, 0x79, 0x01, 0xdf, 0xa7, 0xea, 0x2f, 0xc8, 0xce, 0x12, 0x36, 0xcd,
    0xd9, 0x24, 0x76, 0x58, 0x4e, 0xe2, 0xdb, 0xb6, 0x0c, 0x11, 0xa4, 0xae, 0x94, 0x02, 0xab, 0xf7,
];
pub const INPUT_IDENTITY: InputIdentity = InputIdentity {
    code_sha256: CODE_SHA256,
    exheader_sha256: EXHEADER_SHA256,
    program_id: BANK_TITLE_ID,
    remaster_version: REMASTER_VERSION,
};

/// Skip the selected corrupt-extdata delete/recreate branch, returning its saved
/// native error through existing cleanup. Does not change the missing-archive
/// creation path. See docs/internals.md for the control-flow evidence.
/// The caller must still validate the reviewed whole-image fingerprint.
pub fn archive_preservation_candidate() -> CheckedEdit {
    CheckedEdit {
        offset: 0x0019f338,
        expected: vec![0x2c, 0x00, 0x94, 0xe5], // LDR r0,[r4,#0x2c]
        replacement: vec![0x6f, 0x00, 0x00, 0xea], // B 0x0029f4fc
    }
}

/// The first start without prompts (docs/internals.md, "First start"):
/// (virtual address, original word, replacement word). Bank asks for a
/// language while its own save holds none, and shows its "Precaution for
/// Use" notice while it has no valid save.
pub const FIRST_START_EDITS: [(u32, u32, u32); 8] = [
    // Manager set-up (002a47f4): a saved language is not applied (was:
    // bl 0025c08c), so the console's language stays...
    (0x002a4898, 0xebfeddfb, 0xe320f000),
    // ...and the "language known" flag is set also without one (was:
    // mov r5, r7, zero), so the language screen, task 1, is never entered.
    (0x002a4940, 0xe1a05007, 0xe3a05001),
    // Task 3 without a valid save (was: ldr r5, [r4, #0x40], the start of
    // showing the notice): b 002ac8a8, where accepting it continues. The
    // save is created and written as after "accept".
    (0x002ac5b0, 0xe5945040, FIRST_START_ACCEPT_BRANCH),
    // Task 3 stores a language in a new save only if one was picked
    // (002ac6ec, beq 002ac710). That is left as it is: the pick is written
    // by 0025d1d0 alone, whose one caller is the language screen's listener
    // (002d0b88), and that screen is never entered. A save made here holds
    // language 0, for which the original asks.
    // Task 3 with a valid save (002ac618) stored a language picked on the
    // language screen and wrote the save. It now does that for a save that
    // holds a language, with language 0 and kanji 0, so the original asks
    // again: r6 is the save (was: ldr r6, [r0, #0xf0], the picked
    // language), r5 its language (was: movs r5, r0; nop), and the store
    // gets zeros (was: mov r2, r0 and uxth r1, r5). The branch between
    // them and the write after them are the original's.
    (0x002ac61c, 0xe59060f0, 0xe5906074),
    (0x002ac628, 0xe1b05000, 0xe1d053b0),
    (0x002ac62c, 0xe320f000, 0xe3550000),
    (0x002ac640, 0xe1a02000, 0xe3a02000),
    (0x002ac648, 0xe6ff1075, 0xe3a01000),
];
/// `b 002ac8a8` at 002ac5b0.
pub const FIRST_START_ACCEPT_BRANCH: u32 = 0xea0000bc;

/// Native edit regions in the current profile: twenty-four edited words plus
/// the linked bootstrap bytes.
pub const NATIVE_EDIT_REGIONS: usize = 25;

/// Task 0xb (scene set-up between game selection and Bank loading): the
/// update slot of its vtable, the update it holds (a server check), and the
/// native function that replaces it. That function is the task's own "end"
/// callback and only answers "finished": `mov r0, #1; bx lr`.
pub const SCENE_UPDATE_SLOT: u32 = 0x00361e84;
pub const SCENE_UPDATE: u32 = 0x002af034;
pub const NATIVE_FINISHED: u32 = 0x002af124;
pub const NATIVE_FINISHED_BODY: [u32; 2] = [0xe3a00001, 0xe12fff1e];

/// Main-menu locations edited by earlier builds. They must keep their original
/// words: (virtual address, original little-endian word).
pub const RETIRED_MENU_EDITS: [(u32, u32); 5] = [
    (0x001d6554, 0xeb0000ef),
    (0x002b33a4, 0xebfc853e),
    (0x002b33d4, 0xebfc8532),
    (0x003617bc, 0x002a6750),
    (0x003617dc, 0x002a6c84),
];

/// Builds the complete current development hook set in memory, paired with its
/// expanded exheader. This verifies bytes and layout, not runtime readiness.
/// No install files are written. See docs/internals.md.
pub fn prepare_development_patch(
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
) -> crate::Result<crate::placement::PreparedPlacement> {
    let payload = crate::elf::inspect_payload_elf(elf)?;
    let targets = HookTargets {
        startup: payload.bootstrap_entry,
        next: payload.entry,
        load: payload.load_entry,
        save: payload.save_entry,
        rewards: payload.rewards_entry,
        timestamp: payload.timestamp_entry,
        dex_save_request: payload.dex_save_request_entry,
        dex_records_update: payload.dex_records_update_entry,
        dex_records_finish: payload.dex_records_finish_entry,
    };
    let mut edits = native_hook_edits(targets)?;
    edits.push(payload.bootstrap_edit());
    for (index, word) in NATIVE_FINISHED_BODY.into_iter().enumerate() {
        let offset = (NATIVE_FINISHED - CODE_BASE) as usize + index * 4;
        if crate::le32(code, offset)? != word {
            return crate::fail("native finished stub 002af124 is not the reviewed code");
        }
    }
    if edits.len() != NATIVE_EDIT_REGIONS {
        return crate::fail("development profile edit count changed");
    }
    for (address, original) in RETIRED_MENU_EDITS {
        let offset = (address - CODE_BASE) as usize;
        if edits
            .iter()
            .any(|edit| edit.offset < offset + 4 && offset < edit.offset + edit.replacement.len())
            || crate::le32(code, offset)? != original
        {
            return crate::fail(format!(
                "retired main-menu location {address:#010x} is not original"
            ));
        }
    }
    crate::placement::prepare_expanded_data(
        code,
        exheader,
        &INPUT_IDENTITY,
        &payload.linked_payload(),
        &edits,
    )
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
    fn word(address: u32, expected: u32, replacement: [u8; 4]) -> CheckedEdit {
        CheckedEdit {
            offset: (address - CODE_BASE) as usize,
            expected: expected.to_le_bytes().to_vec(),
            replacement: replacement.to_vec(),
        }
    }
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
        word(
            0x001040a4,
            0xeb000228,
            crate::encode_arm_branch(0x001040a4, targets.startup, true)?,
        ),
        word(
            0x002a5a2c,
            0xebfffed3,
            crate::encode_arm_branch(0x002a5a2c, targets.next, true)?,
        ),
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
        (0x00361cfc, 0x002ae568, targets.load),
        (0x00361d0c, 0x002b4a20, targets.load),
        (0x00361ed4, 0x002af460, targets.load),
        (0x00361ee4, 0x002b4a20, targets.load),
        (0x00362034, 0x002b1cf8, targets.save),
        (0x00362044, 0x002b4a20, targets.save),
    ] {
        edits.push(word(address, expected, target.to_le_bytes()));
    }
    for (address, expected, replacement) in FIRST_START_EDITS {
        edits.push(word(address, expected, replacement.to_le_bytes()));
    }
    Ok(edits)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_arm_branch;

    #[test]
    fn preservation_instruction_targets_existing_error_cleanup() {
        let edit = archive_preservation_candidate();
        assert_eq!(
            edit.replacement,
            encode_arm_branch(CODE_BASE + edit.offset as u32, 0x0029f4fc, false).unwrap()
        );
    }

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
        assert_eq!(edits.len(), NATIVE_EDIT_REGIONS - 1);
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
        assert_eq!(
            FIRST_START_ACCEPT_BRANCH.to_le_bytes(),
            encode_arm_branch(0x002ac5b0, 0x002ac8a8, false).unwrap()
        );
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
