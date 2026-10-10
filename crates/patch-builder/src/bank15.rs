//! Reviewed Bank image identity, placement, and the edits of the original
//! that the patches for Bank are made from.
//!
//! Each patch is a `Profile`: the entry functions its payload exports and
//! the edits it makes. The edits that more than one patch makes are defined
//! here once; a profile only picks from them and adds its own.
//! Hardware behavior and save recovery still require console validation.

use crate::{elf::PayloadElf, placement::InputIdentity, CheckedEdit, BANK_TITLE_ID};

pub mod migrate;
pub mod offline;

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

/// One patch for Bank 1.5.
pub struct Profile {
    /// Its name on the command line and in the manifest.
    pub name: &'static str,
    /// The entry functions its payload must export, in the order of the
    /// manifest. The first is the entry of the ELF.
    pub entries: &'static [&'static str],
    /// Edited regions of the original, the start-up hook's bytes included.
    pub regions: usize,
    /// Every edit but the start-up hook's bytes. `code` is the original.
    edits: fn(code: &[u8], payload: &PayloadElf) -> crate::Result<Vec<CheckedEdit>>,
}

pub const PROFILES: [&Profile; 2] = [&offline::PROFILE, &migrate::PROFILE];

pub fn profile(name: &str) -> crate::Result<&'static Profile> {
    PROFILES
        .into_iter()
        .find(|profile| profile.name == name)
        .ok_or_else(|| crate::Error(format!("unknown Bank patch profile: {name}")))
}

/// One edited word: (virtual address, original word, replacement bytes).
pub(crate) fn word(address: u32, expected: u32, replacement: [u8; 4]) -> CheckedEdit {
    CheckedEdit {
        offset: (address - CODE_BASE) as usize,
        expected: expected.to_le_bytes().to_vec(),
        replacement: replacement.to_vec(),
    }
}

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

/// The one `BL` to application init (0010494c), to the start-up hook that
/// makes the payload executable.
pub(crate) fn startup_call(target: u32) -> crate::Result<CheckedEdit> {
    Ok(word(
        0x001040a4,
        0xeb000228,
        crate::encode_arm_branch(0x001040a4, target, true)?,
    ))
}

/// The `BL` to the task router (002a5580), to a patch's own, which calls the
/// original first.
pub(crate) fn router_call(target: u32) -> crate::Result<CheckedEdit> {
    Ok(word(
        0x002a5a2c,
        0xebfffed3,
        crate::encode_arm_branch(0x002a5a2c, target, true)?,
    ))
}

/// Task 9, "open Bank": the update and busy-poll slots of its vtable with
/// the functions they hold.
pub(crate) const OPEN_TASK: [(u32, u32); 2] = [(0x00361cfc, 0x002ae568), (0x00361d0c, 0x002b4a20)];

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

pub(crate) fn first_start_edits() -> impl Iterator<Item = CheckedEdit> {
    FIRST_START_EDITS
        .into_iter()
        .map(|(address, expected, replacement)| word(address, expected, replacement.to_le_bytes()))
}

/// Main-menu locations edited by earlier builds. They must keep their original
/// words: (virtual address, original little-endian word).
pub const RETIRED_MENU_EDITS: [(u32, u32); 5] = [
    (0x001d6554, 0xeb0000ef),
    (0x002b33a4, 0xebfc853e),
    (0x002b33d4, 0xebfc8532),
    (0x003617bc, 0x002a6750),
    (0x003617dc, 0x002a6c84),
];

/// Builds the complete hook set of `profile` in memory, paired with its
/// expanded exheader. This verifies bytes and layout, not runtime readiness.
/// No install files are written. See docs/internals.md.
pub fn prepare_development_patch(
    profile: &Profile,
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
) -> crate::Result<crate::placement::PreparedPlacement> {
    let payload = crate::elf::inspect_payload_elf(elf, profile.entries)?;
    let mut edits = (profile.edits)(code, &payload)?;
    edits.push(payload.bootstrap_edit());
    if edits.len() != profile.regions {
        return crate::fail("profile edit count changed");
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
    fn the_shared_calls_replace_the_reviewed_branches() {
        for (edit, source, original, target) in [
            (
                startup_call(0x313910).unwrap(),
                0x1040a4,
                0x10494c,
                0x313910,
            ),
            (router_call(0x3fb000).unwrap(), 0x2a5a2c, 0x2a5580, 0x3fb000),
        ] {
            assert_eq!(edit.offset, (source - CODE_BASE) as usize);
            assert_eq!(
                edit.expected,
                encode_arm_branch(source, original, true).unwrap()
            );
            assert_eq!(
                edit.replacement,
                encode_arm_branch(source, target, true).unwrap()
            );
        }
        assert!(startup_call(0x80000000).is_err());
        assert!(router_call(0x80000000).is_err());
        assert_eq!(
            FIRST_START_ACCEPT_BRANCH.to_le_bytes(),
            encode_arm_branch(0x002ac5b0, 0x002ac8a8, false).unwrap()
        );
        assert_eq!(first_start_edits().count(), 8);
    }

    #[test]
    fn every_profile_has_a_name_of_its_own_and_is_found_by_it() {
        for (index, known) in PROFILES.into_iter().enumerate() {
            assert_eq!(profile(known.name).unwrap().entries, known.entries);
            assert!(!known.entries.is_empty());
            assert!(PROFILES[..index]
                .iter()
                .all(|earlier| earlier.name != known.name));
        }
        assert!(profile("unknown").is_err());
    }
}
