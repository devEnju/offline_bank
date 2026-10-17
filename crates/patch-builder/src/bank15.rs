//! Reviewed Bank image identity, placement, and the edits of the original
//! that the patches for Bank are made from.
//!
//! Each patch is a `Profile` of the program `BANK`: the entry functions its payload exports and
//! the edits it makes. The edits that more than one patch makes are defined
//! here once; a profile only picks from them and adds its own.
//! Hardware behavior and save recovery still require console validation.

pub(crate) use crate::profile::Profile;
use crate::{
    elf::PayloadElf, placement::InputIdentity, profile::Program, CheckedEdit, BANK_TITLE_ID,
};

pub mod download;
pub mod migrate;
pub mod offline;
pub mod test;

pub const TMD_VERSION: u16 = 6272;
pub const REMASTER_VERSION: u16 = 6;
pub const CODE_LENGTH: usize = 0x2ac000;
pub const CODE_BASE: u32 = 0x00100000;
pub const PAYLOAD_ADDRESS: u32 = 0x003fb000;

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

/// The linked payload of every patch for Bank 1.5 (`elf::Contract`).
pub const CONTRACT: crate::elf::Contract = crate::elf::Contract {
    prefix: "bank",
    code_base: CODE_BASE,
    bootstrap: 0x0031_3910,
    bootstrap_end: 0x0031_4000,
    payload: PAYLOAD_ADDRESS,
    original_app_init: 0x0010_494c,
    wrappers: &[
        (
            "patch_svc_close_handle",
            &[0xe92d4010, 0xef000023, 0xe8bd8010],
        ),
        (
            "patch_svc_wait_thread",
            &[0xe92d4010, 0xef000024, 0xe8bd8010],
        ),
        (
            "patch_svc_get_resource_limit",
            &[
                0xe92d4001, 0xef000038, 0xe59d2000, 0xe5821000, 0xe28dd004, 0xe49df004,
            ],
        ),
        (
            "patch_svc_get_resource_limit_values",
            &[0xe92d4010, 0xef000039, 0xe8bd8010],
        ),
        (
            "patch_svc_get_resource_current_values",
            &[0xe92d4010, 0xef00003a, 0xe8bd8010],
        ),
    ],
};

/// Pokémon Bank 1.5, which every patch for Bank edits.
pub const BANK: Program = Program {
    name: "bank15-title6272-remaster6",
    label: "Pokémon Bank 1.5",
    title_id: BANK_TITLE_ID,
    tmd_version: TMD_VERSION,
    remaster_version: REMASTER_VERSION,
    code_length: CODE_LENGTH,
    identity: INPUT_IDENTITY,
    contract: &CONTRACT,
    check,
};

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

/// The entry function of `bank-common` that every patch exports beside its
/// own: where the original's "show a message" function goes first.
pub(crate) const MESSAGE_ENTRY: &str = "bank_message_shown";

/// The first instruction of the original's "show a message" function
/// (001d6650, `push {r4-r8, lr}`), to that entry. It runs the instruction,
/// lets HOME and sleep through if the message waits for an answer, and goes
/// on at 001d6654. A branch, not a call: `lr` is the caller's.
pub(crate) fn message_hook(target: u32) -> crate::Result<CheckedEdit> {
    Ok(word(
        0x001d6650,
        0xe92d41f0,
        crate::encode_arm_branch(0x001d6650, target, false)?,
    ))
}

/// The entry function every patch has for the start screen's version line.
pub(crate) const VERSION_ENTRY: &str = "bank_version_shown";

/// The start screen's call that writes its version line, "Ver. 1.5" (was:
/// bl 001e21c8, "put message 0x4E into its pane"), to that entry. It makes
/// the original call and adds the patch's name to the line.
pub(crate) fn version_call(target: u32) -> crate::Result<CheckedEdit> {
    Ok(word(
        0x002b49c4,
        0xebfcb5ff,
        crate::encode_arm_branch(0x002b49c4, target, true)?,
    ))
}

/// Task 9, "open Bank": the update and busy-poll slots of its vtable with
/// the functions they hold.
pub(crate) const OPEN_TASK: [(u32, u32); 2] = [(0x00361cfc, 0x002ae568), (0x00361d0c, 0x002b4a20)];

/// Task 3's update and busy poll. Utility patches intercept it before games.
pub(crate) const SCAN_TASK: [(u32, u32); 2] = [(0x00361ac4, 0x002abe08), (0x00361ad4, 0x002b4a20)];

// SDK CreateThread's priority conversion, including its encoded-priority
// literals. The worker passes SDK 0x11, which this code turns into kernel 0x31.
const SDK_PRIORITY_CODE: [(u32, u32); 9] = [
    (0x00232144, 0xe3550020),
    (0x0023214c, 0x92850020),
    (0x00232154, 0x9a000008),
    (0x00232160, 0xe3500027),
    (0x00232164, 0x92800018),
    (0x00232174, 0xe3500040),
    (0x00232178, 0x83e00000),
    (0x002321b8, 0xaef62b00),
    (0x002321bc, 0x93725b00),
];

/// The first start without prompts (docs/internals.md, "First start"):
/// (virtual address, original word, replacement word). Bank asks for a
/// language while its own save holds none, and shows its "Precaution for
/// Use" notice while it has no valid save. Two edits are in the manager's
/// set-up, which every start runs; the others are in task 3's update.
pub const LANGUAGE_EDITS: [(u32, u32, u32); 2] = [
    // Manager set-up (002a47f4): a saved language is not applied (was:
    // bl 0025c08c), so the console's language stays...
    (0x002a4898, 0xebfeddfb, 0xe320f000),
    // ...and the "language known" flag is set also without one (was:
    // mov r5, r7, zero), so the language screen, task 1, is never entered.
    (0x002a4940, 0xe1a05007, 0xe3a05001),
];
/// The first start's edits in task 3's update (002abe08). Only a patch that
/// runs the original's scan reaches them; the migration replaces that update.
pub const SCAN_FIRST_START_EDITS: [(u32, u32, u32); 6] = [
    // Task 3 without a valid save (was: ldr r5, [r4, #0x40], the start of
    // showing the notice): b 002ac8c8, where accepting it selects state
    // 0x14. The save is created and written as after "accept". Accepting
    // first clears the notice's listener and puts the loading panel back up
    // (002ac8a8..002ac8c4); without the notice the scan's panel is still up
    // with its message, and the listener is as on a start with a valid save.
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
/// `b 002ac8c8` at 002ac5b0.
pub const FIRST_START_ACCEPT_BRANCH: u32 = 0xea0000c4;

/// All eight first-start edits, for a patch that runs the original's scan.
pub(crate) fn first_start_edits() -> impl Iterator<Item = CheckedEdit> {
    checked(LANGUAGE_EDITS.into_iter().chain(SCAN_FIRST_START_EDITS))
}

/// Only the manager's two, for a patch that replaces task 3's update.
pub(crate) fn language_edits() -> impl Iterator<Item = CheckedEdit> {
    checked(LANGUAGE_EDITS.into_iter())
}

fn checked(edits: impl Iterator<Item = (u32, u32, u32)>) -> impl Iterator<Item = CheckedEdit> {
    edits.map(|(address, expected, replacement)| word(address, expected, replacement.to_le_bytes()))
}

/// The main menu's locations that only the console test build may edit.
/// Every other profile leaves them with their original words; a profile edits
/// only the ones it names as its own (`Profile::menu_edits`): (virtual
/// address, original little-endian word).
pub const MENU_LOCATIONS: [(u32, u32); 5] = [
    (0x001d6554, 0xeb0000ef),
    (0x002b33a4, 0xebfc853e),
    (0x002b33d4, 0xebfc8532),
    (0x003617bc, 0x002a6750),
    (0x003617dc, 0x002a6c84),
];

/// What every patch for Bank relies on beyond its edited words: the SDK's
/// conversion of the worker's thread priority, and the main menu's
/// locations, which only a profile that names one may edit.
fn check(
    code: &[u8],
    profile: &Profile,
    _: &PayloadElf,
    edits: &[CheckedEdit],
) -> crate::Result<()> {
    for (address, expected) in SDK_PRIORITY_CODE {
        if crate::le32(code, (address - CODE_BASE) as usize)? != expected {
            return crate::fail("SDK thread-priority conversion is not the reviewed code");
        }
    }
    for (address, original) in MENU_LOCATIONS {
        let offset = (address - CODE_BASE) as usize;
        let touching = edits
            .iter()
            .filter(|edit| {
                edit.offset < offset + 4 && offset < edit.offset + edit.replacement.len()
            })
            .count();
        let own = profile.menu_edits.contains(&address);
        if touching != usize::from(own) || crate::le32(code, offset)? != original {
            return crate::fail(format!(
                "main-menu location {address:#010x} is not as this profile has it"
            ));
        }
    }
    if profile
        .menu_edits
        .iter()
        .any(|address| MENU_LOCATIONS.iter().all(|(known, _)| known != address))
    {
        return crate::fail("profile names an unknown main-menu location");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_arm_branch;

    #[test]
    fn reviewed_sdk_conversion_rejects_a_kernel_priority_used_as_an_sdk_argument() {
        // Transcription of the checked ARM block at 00232144..00232178.
        let convert = |sdk: u32| {
            if sdk <= 0x20 {
                return sdk + 0x20;
            }
            let first = 0xaef6_2b00u32.wrapping_add(sdk);
            if first <= 0x27 {
                return first + 0x18;
            }
            let last = 0x9372_5b00u32.wrapping_add(sdk);
            if last > 0x40 {
                u32::MAX
            } else {
                last
            }
        };
        assert_eq!(convert(0x11), 0x31);
        assert_eq!(convert(0x1f), 0x3f);
        assert_eq!(convert(0x31), u32::MAX);
    }

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
        // The message function's first instruction, push {r4-r8, lr}, to
        // the entry: a branch, so that the caller's lr stays.
        let message = message_hook(0x3fb600).unwrap();
        assert_eq!(message.offset, 0x000d6650);
        assert_eq!(message.expected, [0xf0, 0x41, 0x2d, 0xe9]);
        assert_eq!(
            message.replacement,
            encode_arm_branch(0x001d6650, 0x3fb600, false).unwrap()
        );
        assert_eq!(message.replacement[3], 0xea);
        assert!(message_hook(0x80000000).is_err());
        assert_eq!(
            FIRST_START_ACCEPT_BRANCH.to_le_bytes(),
            encode_arm_branch(0x002ac5b0, 0x002ac8c8, false).unwrap()
        );
        assert_eq!(first_start_edits().count(), 8);
        // The six of task 3 lie inside its update, from 002abe08 up to the
        // scan's end (002acab8); the manager's two lie outside it.
        for (address, _, _) in SCAN_FIRST_START_EDITS {
            assert!((0x002abe08..0x002acab8).contains(&address));
        }
        for (address, _, _) in LANGUAGE_EDITS {
            assert!(!(0x002abe08..0x002acab8).contains(&address));
        }
    }
}
