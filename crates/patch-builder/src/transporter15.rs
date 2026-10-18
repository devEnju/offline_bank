//! Poké Transporter 1.5 (title 00040000000C9C00, TMD 5200, remaster 5).
//!
//! The patch is a `code.ips` with a paired `exheader.bin`, built with the
//! same placement as the Bank patch: the payload goes on the first page after
//! the original zero-initialised data, and a start-up hook in the zero bytes
//! after the original text makes its code pages executable. Every edit of the
//! original replaces whole instructions whose original words are listed here
//! and checked against the input. The offline flow follows zaksabeast's
//! Transporter-Offline-Patch; the differences are listed in
//! docs/internals.md.

use crate::elf::{Contract, PayloadElf};
use crate::placement::InputIdentity;
use crate::profile::{Profile, Program};
use crate::{encode_arm_branch, fail, le32, CheckedEdit, Result};
use transporter_payload::language::{LIST_CAPACITY, VC_LANGUAGES, VC_TITLE_LANGUAGES};
use transporter_payload::sdsave::GAMES;

pub const TITLE_ID: u64 = 0x0004_0000_000c_9c00;
pub const TMD_VERSION: u16 = 5200;
pub const REMASTER_VERSION: u16 = 5;
pub const CODE_BASE: u32 = 0x0010_0000;
pub const CODE_LENGTH: usize = 0x22a000;
pub const CODE_SHA256: [u8; 32] = [
    0x00, 0x1c, 0x20, 0xad, 0xa7, 0x40, 0x16, 0x50, 0x7c, 0x96, 0x9b, 0xb4, 0x4a, 0x0a, 0x50, 0xf8,
    0xed, 0xf8, 0x03, 0xec, 0x06, 0xa3, 0xfc, 0x46, 0x83, 0x42, 0x63, 0xba, 0x8d, 0xf0, 0xfd, 0x2f,
];
pub const EXHEADER_SHA256: [u8; 32] = [
    0x4b, 0x60, 0x81, 0xa5, 0xd0, 0x24, 0x2d, 0x43, 0x71, 0x3f, 0xd0, 0xbe, 0x3c, 0xec, 0x21, 0xae,
    0x73, 0xf0, 0x75, 0x44, 0x1a, 0x75, 0x9a, 0xf3, 0xed, 0x03, 0xc5, 0x27, 0x0d, 0x1b, 0xac, 0xeb,
];
pub const INPUT_IDENTITY: InputIdentity = InputIdentity {
    code_sha256: CODE_SHA256,
    exheader_sha256: EXHEADER_SHA256,
    program_id: TITLE_ID,
    remaster_version: REMASTER_VERSION,
};

/// The original text ends here; its last page is zero up to `BOOTSTRAP_LIMIT`.
pub const BOOTSTRAP_ADDRESS: u32 = 0x0028_d1ac;
pub const BOOTSTRAP_LIMIT: u32 = 0x0028_e000;
/// The one call to application init in the original main function, and the
/// function it calls. The same engine code as Bank's 001040A4 / 0010494C.
pub const STARTUP_CALL: u32 = 0x0010_3d9c;
pub const ORIGINAL_APP_INIT: u32 = 0x0010_4644;
/// First page after the original zero-initialised data (ends at 003638A4).
pub const PAYLOAD_ADDRESS: u32 = 0x0036_4000;

/// Poké Transporter 1.5.
pub const TRANSPORTER: Program = Program {
    name: "transporter15-title5200-remaster5",
    label: "Poké Transporter 1.5",
    title_id: TITLE_ID,
    tmd_version: TMD_VERSION,
    remaster_version: REMASTER_VERSION,
    code_length: CODE_LENGTH,
    identity: INPUT_IDENTITY,
    contract: &CONTRACT,
    check,
};

/// The one patch for Transporter.
pub const PROFILE: Profile = Profile {
    name: "transporter",
    program: &TRANSPORTER,
    entries: ENTRIES,
    regions: EDITS.len() + 1,
    menu_edits: &[],
    edits,
};

/// The linked payload (`elf::Contract`).
pub const CONTRACT: Contract = Contract {
    code_base: CODE_BASE,
    bootstrap: BOOTSTRAP_ADDRESS,
    bootstrap_end: BOOTSTRAP_LIMIT,
    payload: PAYLOAD_ADDRESS,
    original_app_init: ORIGINAL_APP_INIT,
    wrappers: crate::elf::KERNEL_WRAPPERS,
};

/// The entry functions the patched sites branch to, by name; the first is
/// the entry of the ELF. Each is a function of the payload, or, for the
/// layout check, of the start-up hook.
pub const ENTRIES: &[&str] = &[
    "transporter_next",
    "transporter_check_stub",
    "transporter_deliver_stub",
    "transporter_slot",
    "transporter_cart_id_stub",
    "transporter_cart_read",
    "transporter_cart_write",
    "transporter_list_next_stub",
    "transporter_select_stub",
    "transporter_vc_scan_stub",
    "transporter_language_chosen",
    "transporter_language_order_stub",
    "transporter_language_buttons",
    "transporter_language_back_stub",
    "transporter_title_begin_stub",
    "transporter_title_end_stub",
    "transporter_list_ready_stub",
    "transporter_list_indicator_stub",
    "transporter_list_frame_stub",
    "transporter_message_stub",
    "transporter_version",
    "transporter_shiny_build_stub",
    "transporter_shiny_gender_stub",
    "transporter_layout_check",
    "transporter_vc_mount_stub",
    "transporter_card_removed",
    "transporter_entry_card_removed",
    "transporter_message_text",
];
/// The layout check stands in for this instruction at 0013AEE4 and begins
/// with it: `mov r1, r0`.
const LAYOUT_CHECK_FIRST: u32 = 0xe1a0_1000;

enum Word {
    Raw(u32),
    Branch(u32),
    BranchLink(u32),
    /// `b` to an entry function, by name.
    To(&'static str),
    /// `bl` to an entry function, by name.
    Call(&'static str),
}
use Word::*;

/// (address, original word, replacement).
const EDITS: &[(u32, u32, Word)] = &[
    // --- Start-up: run application init, then make the payload executable. ---
    (STARTUP_CALL, 0xeb00_0228, BranchLink(BOOTSTRAP_ADDRESS)),
    // --- Offline flow: the server steps are removed. ---
    // The one call of the router, get_next_state (was: bl 00242BA0). The
    // entry asks the original and changes its answer in three places: after
    // SHOW_GAMES comes GET_POKEMON, where the original connected; after
    // GET_POKEMON comes CHECK_IF_USER_CAN_TRANSFER; and instead of the cancel
    // step comes the disconnect step. It also refuses HOME and
    // sleep for the steps that read or write, as the connect step did for
    // the whole session.
    (0x0024_2ef8, 0xebff_ff28, Call("transporter_next")),
    // Game list, the first instruction of its update (was: ldr r0, [r4,
    // #0x38]): the list is set up and on screen. The stub lets HOME and
    // sleep through again and runs the instruction.
    (
        0x0024_45ec,
        0xe594_0038,
        Call("transporter_list_ready_stub"),
    ),
    // Game list set-up, state 0: the call that starts its loading indicator
    // (was: bl 002450F4). While the search's loading panel is up the stub
    // starts nothing and only notes when the list began; otherwise the call.
    (
        0x0024_4eac,
        0xeb00_0090,
        Call("transporter_list_indicator_stub"),
    ),
    // Game list set-up, every frame: the check that restarts its
    // indicator's sound (was: ldr r0, [r5, #0x5c]). Skipped while the
    // search's panel, whose sound plays, is up.
    (0x0024_4d40, 0xe595_005c, To("transporter_list_frame_stub")),
    // The original's "show a message" function, its first instruction (was:
    // push {r4-r8, lr}). The stub runs it, lets HOME and sleep through if
    // the message waits for an answer, and goes on at 0019B510. A branch,
    // not a call: lr is the caller's. Every message a step that reads or
    // writes ends with comes through here: "Could not find a game", the
    // three about the chosen game's save or card, and the closing message
    // of a transfer.
    (0x0019_b50c, 0xe92d_41f0, To("transporter_message_stub")),
    // The same function, the load of the message into the string it hands
    // to its dialog (was: bl 001DBEB8): the entry makes the load and keeps
    // only the second page of the failure message 0x0B, whose first is
    // about a server.
    (0x0019_b578, 0xeb01_024e, Call("transporter_message_text")),
    // Game search, first step (was: ldr r0, [r0], the start of the cartridge
    // scan). The stub asks the check entry with the task (r4) whether Bank
    // can take a delivery, and either goes on with the search or ends it
    // through the search's own steps.
    (0x0024_6f48, 0xe590_0000, To("transporter_check_stub")),
    // Bank step, sub-state 0 (was: the start of creating the server
    // request): on to where the original continues after a "yes", which
    // stores its record in the chosen game and saves it. Sub-states 1 to 3,
    // the request and its reply, are never entered.
    (0x0024_8cdc, 0xe59f_0304, Branch(0x0024_8e44)),
    // Bank step, the Game Card gone before the note is saved (was: mov r1,
    // #0x22, "No compatible Pokémon Game Card detected"): nothing was
    // written, and the failure message's advice to check the card follows.
    // A note save that failed (was: beq 00248F60, the end without a
    // message) takes the same path.
    (0x0024_8ebc, 0xe3a0_1022, Raw(0xe3a0_100b)),
    (0x0024_8f88, 0x0aff_fff4, Raw(0x0aff_ffba)),
    // Reading Gen 5 and Gen 1/2: skip the remote validation.
    (0x0024_5728, 0xe59f_0c74, Branch(0x0024_5800)),
    (0x0024_60b8, 0xe59f_02e4, Branch(0x0024_61d0)),
    // Step 0xA, the server's question whether a transfer is open: its
    // request counts as made. The step itself is never entered.
    (0x0024_88a0, 0xebfd_4ca3, Raw(0xe3a0_0001)),
    // Reading Gen 5, per-slot result code (was: ldr r0, [r1, r7]). The stub
    // gives empty slots the skip code the server used to send.
    (0x0024_58dc, 0xe791_0007, Call("transporter_slot")),
    // Question step, sub-state 2: skip the notice that nicknames and OT names
    // with prohibited words will be erased (message 0x10). Only the server
    // made that decision, so offline it never happens. Continue at the
    // original "go to sub-state 4".
    (0x0024_7478, 0xe594_0008, Branch(0x0024_74e0)),
    // --- Black/White saves on the SD card when no Gen 5 cartridge is in. ---
    // Cartridge task (00243510): its three cartridge calls. With a Gen 5
    // cartridge the entries call the original functions unchanged.
    (0x0024_3528, 0xebff_5d37, Call("transporter_cart_id_stub")),
    (0x0024_39a8, 0xebff_5b8c, Call("transporter_cart_read")),
    (0x0024_37f4, 0xebff_5cd5, Call("transporter_cart_write")),
    (0x0024_397c, 0xebff_5c73, Call("transporter_cart_write")),
    // Game list (00244D2C): "state = 3, go on to the Virtual Console titles"
    // (was: str r6, [r4, #0x10]). The entry reads the next item first: the
    // next SD save, or the cartridge unless its card is gone.
    (0x0024_4f10, 0xe584_6010, Call("transporter_list_next_stub")),
    // Game list (002445A4), a DS entry was confirmed (was: mov r0, #0). The
    // entry notes which one and returns 0.
    (0x0024_4908, 0xe3a0_0000, Call("transporter_select_stub")),
    // The original's question whether the Game Card was taken out (was:
    // bl 001DE43C): before the first item of the list is read, before the
    // chosen game is read, before the Bank step saves its note, and before
    // each save of the transfer. The entry answers it for the cartridge
    // slot only; a save on the SD card never needs the card.
    (0x0024_4f00, 0xebfe_654d, Call("transporter_card_removed")),
    (0x0024_5658, 0xebfe_6377, Call("transporter_card_removed")),
    (0x0024_8e68, 0xebfe_5573, Call("transporter_card_removed")),
    (0x0024_a298, 0xebfe_5067, Call("transporter_card_removed")),
    (0x0024_a474, 0xebfe_4ff0, Call("transporter_card_removed")),
    // The same question in the game list's listener (0025C520), a DS entry
    // confirmed (was: ldr r0, [r4, #8] and bl 001DE43C): the entry gets the
    // task and answers for the entry at the cursor.
    (0x0025_c5a4, 0xe594_0008, Raw(0xe1a0_0004)),
    (
        0x0025_c5a8,
        0xebfe_07a3,
        Call("transporter_entry_card_removed"),
    ),
    // --- The language screen chooses which language's games are listed. ---
    // The screen ending (0025C5BC): no switch of the screens' language (was:
    // bl 0022AF28); the entry notes the choice. The record of the language
    // in the manager (was: bl 0022C06C) keeps its value.
    (
        0x0025_c5fc,
        0xebff_3a49,
        Call("transporter_language_chosen"),
    ),
    (0x0025_c618, 0xebff_3e93, Raw(0xe320_f000)),
    // Constructor (0022BD28), ordering and placing the list (was:
    // bl 0022BC0C): the six languages that are not listed already, centred,
    // and the Back button below them.
    (
        0x0022_bee0,
        0xebff_ff49,
        Call("transporter_language_order_stub"),
    ),
    // Showing the list (0022B530): cursor range and enabled buttons (was:
    // bl 001A1FE0, which the entry calls first) for the shown entries only.
    (
        0x0022_b5b0,
        0xebfd_da8a,
        Call("transporter_language_buttons"),
    ),
    // After an entry was picked (was: cmp r0, #5 / moveq r1, #1: list index
    // 5, Japanese, goes to the kana/kanji part): index 7, the Back button,
    // goes to part 3, the end of the screen; every language goes to the
    // confirm part.
    (0x0022_b6e0, 0xe350_0005, Raw(0xe350_0007)),
    (0x0022_b6e4, 0x03a0_1001, Raw(0x03a0_1003)),
    // Key handler (0022B330), B on a part without a Back button (was:
    // b 0022B510): on the list the entry presses the Back button.
    (
        0x0022_b3b8,
        0xea00_0054,
        To("transporter_language_back_stub"),
    ),
    // Title screen's constructor (0024B9E4), before and after the call that
    // loads its archives (0024BA70; was: mov r7, #3 and mov r3, #0x27): the
    // archive with the logo and the start prompt is read in the chosen
    // language, and the reading language is set back.
    (
        0x0024_ba44,
        0xe3a0_7003,
        Call("transporter_title_begin_stub"),
    ),
    (0x0024_ba74, 0xe3a0_3027, Call("transporter_title_end_stub")),
    // The same constructor's call that writes the version line, "Ver. 1.5"
    // (was: bl 001D33CC, "put message 0x29 into its pane"): the entry makes
    // the call and adds the patch's name to the line.
    (0x0024_bae8, 0xebfe_1e37, Call("transporter_version")),
    // Building a layout (0013AE2C), the layout binary on its way to
    // Layout::Build (was: mov r1, r0): the check does the same and lets the
    // payload give the language screen's layout its Back button first.
    (0x0013_aee4, 0xe1a0_1000, Call("transporter_layout_check")),
    // The scan of the Virtual Console saves (001E419C): the call that mounts
    // a title's save (was: bl 0010E4A4). The stub skips the titles in another
    // language than the listed one.
    (0x001e_41c0, 0xebfc_a8b7, Call("transporter_vc_mount_stub")),
    // Application init's call of that scan (was: bl 001E419C), before the
    // payload can run: skipped. Its only result, the table at 003296E4, is
    // read only by the search and the list's scanner, each right after a
    // scan of its own; the call's return value is not used.
    (0x0010_621c, 0xeb03_77de, Raw(0xe320_f000)),
    // Scanner of the Virtual Console titles (0024120C), asking whether a
    // row's title is installed (was: bl 002512A8): the entry answers no for
    // a row in another language than the listed one. The scanner files the
    // trainer names under the number of each title it finds, so the titles
    // are left out here and not when the list is built. The list holds 40
    // entries as the original built it; one language gives at most 11.
    (0x0024_12d4, 0xeb00_3ff3, Call("transporter_vc_scan_stub")),
    // --- Transfer, sub-state 0 (was: the start of creating the upload
    // request): deliver first. The stub calls the deliver entry with the
    // task (r4) and goes on inside the original by its answer:
    // 1: delivered, the original removal code (0024A274).
    // 2: still working, return and come back next frame (0024A5DC).
    // else: the original failure message (sub-state 0x11, stored at
    // 0024A4C0); the game is left alone. That is also the answer while the
    // chosen cartridge's card is gone, before anything is written.
    (0x0024_a150, 0xe59f_04f4, To("transporter_deliver_stub")),
    // Transfer, after the source game is saved (was: mov r0, #0xa, the one
    // place that selects the commit request, sub-state 0xA): no commit
    // request; the original final save and success message (sub-state 0xE)
    // come next.
    (0x0024_a3c0, 0xe3a0_000a, Raw(0xe3a0_000e)),
    // Transfer, a save of the game that failed after the delivery: the save
    // without the Pokémon (was: mov r0, #0xC, the server's commit, whose
    // request was never created) and the final save (was: beq 0024A38C, the
    // end without a message). Both go to the original's message that no
    // Game Card is detected (0024A2A8), as for a card taken out.
    (0x0024_a3d0, 0xe3a0_000c, Branch(0x0024_a2a8)),
    (0x0024_a4e8, 0x0aff_ffa7, Raw(0x0aff_ff6e)),
    // --- Gen 1 conversion (0024F428): the Easter egg. ---
    // The constructor call (was: bl 0019CD3C): the entry may ask for a shiny
    // in the construction parameters, then makes the call.
    (
        0x0024_f6b0,
        0xebfd_35a1,
        Call("transporter_shiny_build_stub"),
    ),
    // The gender from the Attack DV (was: ldrb r1, [r4, #0x1b]): the entry
    // keeps the constructor's gender, or runs the instruction.
    (
        0x0024_f8e0,
        0xe5d4_101b,
        Call("transporter_shiny_gender_stub"),
    ),
];

/// The edit at `address`, whose original word is `original`. `entry` gives
/// the address of an entry function by its name.
fn word_edit(
    address: u32,
    original: u32,
    word: &Word,
    entry: impl Fn(&str) -> Result<u32>,
) -> Result<CheckedEdit> {
    let replacement = match *word {
        Raw(value) => value.to_le_bytes(),
        Branch(target) => encode_arm_branch(address, target, false)?,
        BranchLink(target) => encode_arm_branch(address, target, true)?,
        To(name) => encode_arm_branch(address, entry(name)?, false)?,
        Call(name) => encode_arm_branch(address, entry(name)?, true)?,
    };
    Ok(CheckedEdit {
        offset: (address - CODE_BASE) as usize,
        expected: original.to_le_bytes().to_vec(),
        replacement: replacement.to_vec(),
    })
}

// SDK CreateThread's priority conversion (`001E3F58`, the same code as
// Bank's), including its encoded-priority literals. The worker passes SDK
// 0x11, which this code turns into kernel 0x31.
const SDK_PRIORITY_CODE: [(u32, u32); 9] = [
    (0x001e_3fc8, 0xe355_0020),
    (0x001e_3fd0, 0x9285_0020),
    (0x001e_3fd8, 0x9a00_0008),
    (0x001e_3fe4, 0xe350_0027),
    (0x001e_3fe8, 0x9280_0018),
    (0x001e_3ff8, 0xe350_0040),
    (0x001e_3ffc, 0x83e0_0000),
    (0x001e_403c, 0xaef6_2b00),
    (0x001e_4040, 0x9372_5b00),
];

/// What Transporter relies on beyond its edited words: the SDK's conversion
/// of the worker's thread priority, the game list, and the layout check,
/// which must begin with the instruction it stands in for.
fn check(code: &[u8], _: &Profile, payload: &PayloadElf, _: &[CheckedEdit]) -> Result<()> {
    for (address, expected) in SDK_PRIORITY_CODE {
        if le32(code, (address - CODE_BASE) as usize)? != expected {
            return fail("SDK thread-priority conversion is not the reviewed code");
        }
    }
    check_game_list(code)?;
    let check = payload.export("transporter_layout_check")?;
    let at = check
        .checked_sub(BOOTSTRAP_ADDRESS)
        .ok_or_else(|| crate::Error("layout check is not in the start-up hook".into()))?;
    if le32(payload.bootstrap_bytes(), at as usize)? != LAYOUT_CHECK_FIRST {
        return fail("layout check does not begin with the instruction it stands in for");
    }
    Ok(())
}

/// Every edit of the listed words. The start-up hook's bytes and the payload
/// are added by `profile::prepare_patch`.
fn edits(_: &[u8], payload: &PayloadElf) -> Result<Vec<CheckedEdit>> {
    EDITS
        .iter()
        .map(|(address, original, word)| {
            word_edit(*address, *original, word, |name| payload.export(name))
        })
        .collect()
}

/// The original's table of the Virtual Console titles: 39 rows of
/// `{ version, language id, title index }`, three words each.
pub const VC_TABLE: u32 = 0x002a_fe0c;
const VC_ROW: usize = 12;
const VC_ROW_LANGUAGE: usize = 4;
/// The language of each Virtual Console title by its index, one word each
/// (`002C2B1C`), as the scan of their saves numbers them.
pub const VC_TITLE_TABLE: u32 = 0x002c_2b1c;

/// The hooks list the games of one language and leave the original's game
/// list unbounded, as it is. That holds only while the payload's copy of the
/// table's languages is the original's and one language cannot fill the list.
fn check_game_list(code: &[u8]) -> Result<()> {
    let table = (VC_TABLE - CODE_BASE) as usize;
    for (row, &language) in VC_LANGUAGES.iter().enumerate() {
        if le32(code, table + row * VC_ROW + VC_ROW_LANGUAGE)? != u32::from(language) {
            return fail(format!(
                "Virtual Console table row {row} is not in the language the hooks expect"
            ));
        }
    }
    let titles = (VC_TITLE_TABLE - CODE_BASE) as usize;
    for (index, &language) in VC_TITLE_LANGUAGES.iter().enumerate() {
        if le32(code, titles + index * 4)? != u32::from(language) {
            return fail(format!(
                "Virtual Console title {index} is not in the language the hooks expect"
            ));
        }
    }
    let most = (0..=u8::MAX)
        .map(|language| VC_LANGUAGES.iter().filter(|&&row| row == language).count())
        .max()
        .unwrap_or(0);
    if GAMES.len() + most > LIST_CAPACITY {
        return fail("the games of one language do not fit the original game list");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Target of an unconditional `b` (0xEA) or `bl` (0xEB).
    fn branch_target(address: u32, word: u32, link: bool) -> Option<u32> {
        let top = if link { 0xeb00_0000 } else { 0xea00_0000 };
        if word & 0xff00_0000 != top {
            return None;
        }
        let offset = ((word & 0x00ff_ffff) << 8) as i32 >> 6;
        Some(address.wrapping_add(8).wrapping_add(offset as u32))
    }
    /// A stand-in address for each entry function, in the payload.
    fn entry(name: &str) -> Result<u32> {
        ENTRIES
            .iter()
            .position(|known| *known == name)
            .map(|index| PAYLOAD_ADDRESS + 0x100 + index as u32 * 0x10)
            .ok_or_else(|| crate::Error(format!("missing ELF export {name}")))
    }
    fn site_edits() -> Vec<CheckedEdit> {
        EDITS
            .iter()
            .map(|(address, original, word)| word_edit(*address, *original, word, entry).unwrap())
            .collect()
    }

    #[test]
    fn every_entry_is_used_by_a_site_and_every_site_names_an_entry() {
        let named: Vec<&str> = EDITS
            .iter()
            .filter_map(|(_, _, word)| match word {
                To(name) | Call(name) => Some(*name),
                _ => None,
            })
            .collect();
        for name in ENTRIES {
            assert!(named.contains(name), "{name} is not used");
        }
        for name in &named {
            assert!(entry(name).is_ok(), "{name} is not an entry");
        }
        // The router comes first: it is the entry of the ELF.
        assert_eq!(ENTRIES[0], "transporter_next");
    }

    #[test]
    fn edits_are_whole_words_disjoint_and_inside_the_original_text() {
        let mut list = site_edits();
        list.sort_by_key(|edit| edit.offset);
        for pair in list.windows(2) {
            assert!(pair[0].offset + pair[0].replacement.len() <= pair[1].offset);
        }
        let text_end = (BOOTSTRAP_ADDRESS - CODE_BASE) as usize;
        for edit in &list {
            assert_eq!(edit.replacement.len(), 4);
            assert!(edit.offset % 4 == 0 && edit.offset < text_end);
            assert_ne!(edit.expected, edit.replacement);
        }
    }

    #[test]
    fn hook_calls_and_conditional_branches_are_encoded_as_intended() {
        let word = |address: u32, word: &Word| {
            u32::from_le_bytes(
                word_edit(address, 0, word, entry)
                    .unwrap()
                    .replacement
                    .try_into()
                    .unwrap(),
            )
        };
        let at = |name| entry(name).unwrap();
        for (address, name) in [
            (0x0024_58dc, "transporter_slot"),
            (0x0024_3528, "transporter_cart_id_stub"),
            (0x0024_39a8, "transporter_cart_read"),
            (0x0024_37f4, "transporter_cart_write"),
            (0x0024_397c, "transporter_cart_write"),
            (0x0024_4f10, "transporter_list_next_stub"),
            (0x0024_4908, "transporter_select_stub"),
            (0x0025_c5fc, "transporter_language_chosen"),
            (0x0022_bee0, "transporter_language_order_stub"),
            (0x0022_b5b0, "transporter_language_buttons"),
            (0x0024_12d4, "transporter_vc_scan_stub"),
            (0x0013_aee4, "transporter_layout_check"),
            (0x001e_41c0, "transporter_vc_mount_stub"),
            (0x0024_ba44, "transporter_title_begin_stub"),
            (0x0024_ba74, "transporter_title_end_stub"),
            (0x0024_f6b0, "transporter_shiny_build_stub"),
            (0x0024_f8e0, "transporter_shiny_gender_stub"),
        ] {
            let call = word(address, &Call(name));
            assert_eq!(branch_target(address, call, true), Some(at(name)));
        }
        let call = word(STARTUP_CALL, &BranchLink(BOOTSTRAP_ADDRESS));
        assert_eq!(
            branch_target(STARTUP_CALL, call, true),
            Some(BOOTSTRAP_ADDRESS)
        );
        // The original cartridge calls the entries stand in for.
        for (address, original, target) in [
            (0x0024_3528u32, 0xebff_5d37u32, 0x0021_aa0cu32),
            (0x0024_39a8, 0xebff_5b8c, 0x0021_a7e0),
            (0x0024_37f4, 0xebff_5cd5, 0x0021_ab50),
            (0x0024_397c, 0xebff_5c73, 0x0021_ab50),
            // The language screen's calls.
            (0x0025_c5fc, 0xebff_3a49, 0x0022_af28),
            (0x0025_c618, 0xebff_3e93, 0x0022_c06c),
            (0x0022_bee0, 0xebff_ff49, 0x0022_bc0c),
            (0x0022_b5b0, 0xebfd_da8a, 0x001a_1fe0),
            // The scanner's "is this title installed?".
            (0x0024_12d4, 0xeb00_3ff3, 0x0025_12a8),
            // The Gen 1 converter's call of the Pokémon constructor.
            (0x0024_f6b0, 0xebfd_35a1, 0x0019_cd3c),
        ] {
            assert_eq!(branch_target(address, original, true), Some(target));
        }
        // B on the language list: the original branch and its replacement.
        assert_eq!(
            branch_target(0x0022_b3b8, 0xea00_0054, false),
            Some(0x0022_b510)
        );
        assert_eq!(
            branch_target(
                0x0022_b3b8,
                word(0x0022_b3b8, &To("transporter_language_back_stub")),
                false
            ),
            Some(at("transporter_language_back_stub"))
        );
        // Sites that leave the original for good: plain branches to stubs
        // that go on inside the original themselves.
        for (address, name) in [
            (0x0024_6f48, "transporter_check_stub"),
            (0x0024_a150, "transporter_deliver_stub"),
            (0x0019_b50c, "transporter_message_stub"),
        ] {
            let jump = word(address, &To(name));
            assert_eq!(branch_target(address, jump, false), Some(at(name)));
        }
        // The message function's first instruction is push {r4-r8, lr},
        // which its stub runs before it goes on behind it.
        assert!(EDITS
            .iter()
            .any(|(address, original, _)| (*address, *original) == (0x0019_b50c, 0xe92d_41f0)));
        // The original start-up call that the hook replaces and then makes.
        assert_eq!(
            branch_target(STARTUP_CALL, 0xeb00_0228, true),
            Some(ORIGINAL_APP_INIT)
        );
        // The router is called through the entry, which calls it in turn;
        // nothing inside it is edited.
        assert_eq!(
            branch_target(0x0024_2ef8, 0xebff_ff28, true),
            Some(0x0024_2ba0)
        );
        assert_eq!(
            branch_target(
                0x0024_2ef8,
                word(0x0024_2ef8, &Call("transporter_next")),
                true
            ),
            Some(at("transporter_next"))
        );
        assert!(EDITS
            .iter()
            .all(|(address, _, _)| !(0x0024_2ba0..0x0024_2e08).contains(address)));
        assert_eq!(word(0x0024_a15c, &Branch(0x0024_a274)), 0xea00_0044);
        // The Bank step goes from its first instruction to where the
        // original continues after a "yes"; its reply parsing is not edited.
        assert_eq!(word(0x0024_8cdc, &Branch(0x0024_8e44)), 0xea00_0058);
        assert!(EDITS.iter().all(|(address, _, _)| *address != 0x0024_8d58));
    }

    #[test]
    fn the_virtual_console_table_must_be_the_one_the_hooks_copied() {
        let table = (VC_TABLE - CODE_BASE) as usize;
        let titles = (VC_TITLE_TABLE - CODE_BASE) as usize;
        let mut code = vec![0u8; titles + VC_TITLE_LANGUAGES.len() * 4];
        for (row, &language) in VC_LANGUAGES.iter().enumerate() {
            let at = table + row * VC_ROW + VC_ROW_LANGUAGE;
            code[at..at + 4].copy_from_slice(&u32::from(language).to_le_bytes());
        }
        for (index, &language) in VC_TITLE_LANGUAGES.iter().enumerate() {
            let at = titles + index * 4;
            code[at..at + 4].copy_from_slice(&u32::from(language).to_le_bytes());
        }
        assert!(check_game_list(&code).is_ok());
        for row in [0, 12, VC_LANGUAGES.len() - 1] {
            let mut other = code.clone();
            other[table + row * VC_ROW + VC_ROW_LANGUAGE] ^= 8;
            assert!(check_game_list(&other).is_err());
        }
        for index in [0, 19, VC_TITLE_LANGUAGES.len() - 1] {
            let mut other = code.clone();
            other[titles + index * 4] ^= 8;
            assert!(check_game_list(&other).is_err());
        }
        // A table cut short is refused, not read past its end.
        assert!(check_game_list(&code[..code.len() - 4]).is_err());
    }
}
