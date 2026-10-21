//! Pokémon Bank 1.5 (title 00040000000C9B00, TMD 6272, remaster 6), and the
//! edits of it that more than one patch for Bank makes.
//!
//! Each patch is a `Profile` of the program `BANK`, in a file of its own
//! beside this one: its tables of edited words (`profile::Edit`), each
//! checked against its original word. The edits several patches make are
//! defined here once; a profile lists them with its own.

pub(crate) use crate::profile::Profile;
use crate::{
    elf::{Contract, PayloadElf},
    fail,
    placement::InputIdentity,
    profile::{Edit, Program, Word::*},
    Result,
};
use bank_common::task::{
    TASK_COUNT, TASK_LOAD, TASK_OPEN, TASK_REWARDS, TASK_REWARD_GUARD, TASK_SAVE, TASK_SCAN,
};

pub mod download;
pub mod migrate;
pub mod offline;
pub mod test;

pub const TITLE_ID: u64 = 0x0004_0000_000c_9b00;
pub const TMD_VERSION: u16 = 6272;
pub const REMASTER_VERSION: u16 = 6;
pub const CODE_BASE: u32 = 0x0010_0000;
pub const CODE_LENGTH: usize = 0x2ac000;
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
    program_id: TITLE_ID,
    remaster_version: REMASTER_VERSION,
};

/// The original text ends here; its last page is zero up to `BOOTSTRAP_LIMIT`.
pub const BOOTSTRAP_ADDRESS: u32 = 0x0031_3910;
pub const BOOTSTRAP_LIMIT: u32 = 0x0031_4000;
/// The one call to application init in the original main function, and the
/// function it calls.
pub const STARTUP_CALL: u32 = 0x0010_40a4;
pub const ORIGINAL_APP_INIT: u32 = 0x0010_494c;
/// First page after the original zero-initialised data.
pub const PAYLOAD_ADDRESS: u32 = 0x003f_b000;

/// Pokémon Bank 1.5, which every patch for Bank edits.
pub const BANK: Program = Program {
    name: "bank15-title6272-remaster6",
    label: "Pokémon Bank 1.5",
    title_id: TITLE_ID,
    tmd_version: TMD_VERSION,
    remaster_version: REMASTER_VERSION,
    code_length: CODE_LENGTH,
    identity: INPUT_IDENTITY,
    contract: &CONTRACT,
    relies: &[&SDK_PRIORITY_CODE, &MENU_LOCATIONS, &NATIVE_OBJECTS],
    check,
};

/// The linked payload of every patch for Bank 1.5 (`elf::Contract`).
pub const CONTRACT: Contract = Contract {
    code_base: CODE_BASE,
    bootstrap: BOOTSTRAP_ADDRESS,
    bootstrap_end: BOOTSTRAP_LIMIT,
    payload: PAYLOAD_ADDRESS,
    original_app_init: ORIGINAL_APP_INIT,
    wrappers: crate::elf::KERNEL_WRAPPERS,
};

/// Start-up: run application init, then make the payload executable.
pub(crate) const STARTUP: Edit = (STARTUP_CALL, 0xeb00_0228, BranchLink(BOOTSTRAP_ADDRESS));

/// A corrupt extdata archive is kept (was: ldr r0, [r4, #0x2c], the start of
/// its delete and recreate): on to the existing error cleanup, which returns
/// the saved native error. The creation of a missing archive is unchanged.
/// See docs/internals.md for the control-flow evidence.
pub(crate) const KEEP_ARCHIVE: Edit = (0x0029_f338, 0xe594_002c, Branch(0x0029_f4fc));

/// The `BL` to the task router (002a5580), to a patch's own entry, which
/// calls the original first.
pub(crate) const fn router(entry: &'static str) -> Edit {
    (0x002a_5a2c, 0xebff_fed3, Call(entry))
}

/// The first instruction of the original's "show a message" function
/// (001d6650, `push {r4-r8, lr}`), to `bank-common`'s entry. It runs the
/// instruction, lets HOME and sleep through if the message waits for an
/// answer, and goes on at 001d6654. A branch, not a call: `lr` is the
/// caller's.
pub(crate) const MESSAGE: Edit = (0x001d_6650, 0xe92d_41f0, To("bank_message_stub"));

/// The start screen's call that writes its version line, "Ver. 1.5" (was:
/// bl 001e21c8, "put message 0x4E into its pane"), to the entry every patch
/// has under this name. It makes the original call and adds the patch's
/// name to the line.
pub(crate) const VERSION: Edit = (0x002b_49c4, 0xebfc_b5ff, Call("bank_version_shown"));

/// Task 9, "open Bank": the update and busy-poll slots of its vtable with
/// the functions they hold.
pub(crate) const OPEN_TASK: [(u32, u32); 2] =
    [(0x0036_1cfc, 0x002a_e568), (0x0036_1d0c, 0x002b_4a20)];

/// Task 3, the game scan: its update and busy poll.
pub(crate) const SCAN_TASK: [(u32, u32); 2] =
    [(0x0036_1ac4, 0x002a_be08), (0x0036_1ad4, 0x002b_4a20)];

/// The first start without prompts (docs/internals.md, "First start"). Bank
/// asks for a language while its own save holds none, and shows its
/// "Precaution for Use" notice while it has no valid save. These two edits
/// are in the manager's set-up, which every start runs.
pub(crate) const LANGUAGE_EDITS: [Edit; 2] = [
    // Manager set-up (002a47f4): a saved language is not applied (was:
    // bl 0025c08c), so the console's language stays...
    (0x002a_4898, 0xebfe_ddfb, Raw(NOP)),
    // ...and the "language known" flag is set also without one (was:
    // mov r5, r7, zero), so the language screen, task 1, is never entered.
    (0x002a_4940, 0xe1a0_5007, Raw(0xe3a0_5001)),
];
/// The first start's edits in task 3's update (002abe08). Only a patch that
/// runs the original's scan reaches them; the migration replaces that update.
pub(crate) const SCAN_FIRST_START_EDITS: [Edit; 6] = [
    // Task 3 without a valid save (was: ldr r5, [r4, #0x40], the start of
    // showing the notice): on to 002ac8c8, where accepting it selects state
    // 0x14. The save is created and written as after "accept". Accepting
    // first clears the notice's listener and puts the loading panel back up
    // (002ac8a8..002ac8c4); without the notice the scan's panel is still up
    // with its message, and the listener is as on a start with a valid save.
    (0x002a_c5b0, 0xe594_5040, Branch(0x002a_c8c8)),
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
    (0x002a_c61c, 0xe590_60f0, Raw(0xe590_6074)),
    (0x002a_c628, 0xe1b0_5000, Raw(0xe1d0_53b0)),
    (0x002a_c62c, NOP, Raw(0xe355_0000)),
    (0x002a_c640, 0xe1a0_2000, Raw(0xe3a0_2000)),
    (0x002a_c648, 0xe6ff_1075, Raw(0xe3a0_1000)),
];

/// `nop`.
pub(crate) const NOP: u32 = 0xe320_f000;

/// SDK CreateThread's priority conversion (002320d4), including its
/// encoded-priority literals. The worker passes SDK 0x11, which this code
/// turns into kernel 0x31.
pub(crate) const SDK_PRIORITY_CODE: [(u32, u32); 9] = [
    (0x0023_2144, 0xe355_0020),
    (0x0023_214c, 0x9285_0020),
    (0x0023_2154, 0x9a00_0008),
    (0x0023_2160, 0xe350_0027),
    (0x0023_2164, 0x9280_0018),
    (0x0023_2174, 0xe350_0040),
    (0x0023_2178, 0x83e0_0000),
    (0x0023_21b8, 0xaef6_2b00),
    (0x0023_21bc, 0x9372_5b00),
];

/// Factory bound, case targets and constructor vtable literals of the
/// tasks the checked task views accept. Tasks 0x10 and 0x1C share one
/// literal; the reward claim (0xD) is built by its constructor 002ABB10.
/// The update slots are edits, which name their original words.
pub(crate) const NATIVE_OBJECTS: [(u32, u32); 17] = [
    (0x002a_5a80, 0xe351_0000 | TASK_COUNT),
    (0x002a_5aa4, 0x002a_5bdc),
    (0x002a_5ab4, 0x002a_5da4),
    (0x002a_5abc, 0x002a_5e50),
    (0x002a_5ac8, 0x002a_5ebc),
    (0x002a_5acc, 0x002a_5f08),
    (0x002a_5ad8, 0x002a_6130),
    (0x002a_5b08, 0x002a_63a8),
    (0x002a_6534, TASK_SCAN),
    (0x002a_6548, TASK_SAVE),
    (0x002a_6550, TASK_OPEN),
    (0x002a_6554, TASK_REWARD_GUARD),
    (0x002a_6570, TASK_LOAD),
    (0x002a_5f1c, 0x1b00_16fb),
    (0x002a_bbf0, TASK_REWARDS),
    (0x002a_bb40, 0xe580_6000),
    (0x002a_5c1c, 0xe580_3000),
];

/// The main menu's locations that only the console test build may edit:
/// (virtual address, original word). Every other profile leaves them with
/// their original words; a profile edits only the ones it names as its own
/// (`Profile::menu_edits`).
pub const MENU_LOCATIONS: [(u32, u32); 5] = [
    (0x001d_6554, 0xeb00_00ef),
    (0x002b_33a4, 0xebfc_853e),
    (0x002b_33d4, 0xebfc_8532),
    (0x0036_17bc, 0x002a_6750),
    (0x0036_17dc, 0x002a_6c84),
];

/// What every patch for Bank relies on beyond its words: a profile edits
/// exactly the main-menu locations it names.
fn check(_: &[u8], profile: &Profile, _: &PayloadElf) -> Result<()> {
    menu_edits_named(profile)
}
fn menu_edits_named(profile: &Profile) -> Result<()> {
    for (address, _) in MENU_LOCATIONS {
        let edited = profile.all_edits().any(|(at, _, _)| *at == address);
        if edited != profile.menu_edits.contains(&address) {
            return fail(format!(
                "main-menu location {address:#010x} is not as this profile has it"
            ));
        }
    }
    if profile
        .menu_edits
        .iter()
        .any(|address| MENU_LOCATIONS.iter().all(|(known, _)| known != address))
    {
        return fail("profile names an unknown main-menu location");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{sample::branch_target, PROFILES};

    #[test]
    fn the_shared_edits_replace_the_reviewed_instructions() {
        // The archive edit goes to the existing error cleanup.
        assert_eq!(KEEP_ARCHIVE.2, Branch(0x0029_f4fc));
        // The router and the version line are calls, as the ones they
        // replace; the message function's first instruction is
        // push {r4-r8, lr}, which its entry runs.
        let (address, original, _) = router("bank_offline_next");
        assert_eq!(branch_target(address, original, true), Some(0x002a_5580));
        assert_eq!(branch_target(VERSION.0, VERSION.1, true), Some(0x001e_21c8));
        assert_eq!(MESSAGE.1, 0xe92d_41f0);
        // The six of task 3 lie inside its update, from 002abe08 up to the
        // scan's end (002acab8); the manager's two lie outside it.
        for (address, _, _) in SCAN_FIRST_START_EDITS {
            assert!((0x002a_be08..0x002a_cab8).contains(&address));
        }
        for (address, _, _) in LANGUAGE_EDITS {
            assert!(!(0x002a_be08..0x002a_cab8).contains(&address));
        }
    }

    #[test]
    fn only_the_profiles_that_name_a_menu_location_edit_it() {
        for profile in PROFILES
            .iter()
            .filter(|profile| profile.program.name == BANK.name)
        {
            assert!(menu_edits_named(profile).is_ok(), "{}", profile.name);
        }
        let named = Profile {
            menu_edits: &[MENU_LOCATIONS[0].0],
            ..offline::PROFILE
        };
        assert!(menu_edits_named(&named).is_err());
        let unknown = Profile {
            menu_edits: &[0x0036_17c0],
            ..offline::PROFILE
        };
        assert!(menu_edits_named(&unknown).is_err());
    }
}
