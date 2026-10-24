//! The offline patch: Transporter without its server. Every edit of the
//! original replaces whole instructions whose original words are listed here
//! and checked against the input. The offline flow follows zaksabeast's
//! Transporter-Offline-Patch; the differences are listed in
//! docs/internals.md.

use super::{BOOTSTRAP_ADDRESS, STARTUP_CALL};
use crate::profile::{Edit, Profile, Word::*};

/// The one patch for Transporter.
pub const PROFILE: Profile = Profile {
    name: "transporter",
    program: &super::TRANSPORTER,
    edits: &[EDITS],
    relies: &[],
    menu_edits: &[],
};

/// Every edit but the start-up hook's bytes. Sites that go to the patch
/// name their entry function; the router comes first, the entry of the ELF.
const EDITS: &[Edit] = &[
    // --- Start-up: run application init, then make the payload executable. ---
    (STARTUP_CALL, 0xeb00_0228, BranchLink(BOOTSTRAP_ADDRESS)),
    // --- Offline flow: the server tasks are removed. ---
    // The one call of the router, get_next_state (was: bl 00242BA0). The
    // entry asks the original and changes its answer in four places: after
    // SHOW_GAMES comes GET_POKEMON, where the original connected; after
    // GET_POKEMON comes CHECK_IF_USER_CAN_TRANSFER; and instead of the cancel
    // task and of the failure task comes the disconnect task. It also refuses HOME and
    // sleep for the tasks that read or write, as the connect task did for
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
    // not a call: lr is the caller's. Every message a task that reads or
    // writes ends with comes through here: "Could not find a game", the
    // three about the chosen game's save or card, and the closing message
    // of a transfer.
    (0x0019_b50c, 0xe92d_41f0, To("transporter_message_stub")),
    // The same function, the load of the message into the string it hands
    // to its dialog (was: bl 001DBEB8): the entry makes the load and keeps
    // only the second page of the failure message 0x0B, whose first is
    // about a server.
    (0x0019_b578, 0xeb01_024e, Call("transporter_message_text")),
    // Game search, its update in its vtable (was: 00246F04). In its first
    // state the entry asks whether Bank can take a delivery, and either runs
    // the original update or ends the search through its own states.
    (0x002e_6780, 0x0024_6f04, Entry("transporter_search")),
    // Bank task, sub-state 0 (was: the start of creating the server
    // request): on to where the original continues after a "yes", which
    // stores its record in the chosen game and saves it. Sub-states 1 to 3,
    // the request and its reply, are never entered.
    (0x0024_8cdc, 0xe59f_0304, Branch(0x0024_8e44)),
    // Bank task, the Game Card gone before the note is saved (was: mov r1,
    // #0x22, "No compatible Pokémon Game Card detected"): nothing was
    // written, and the failure message's advice to check the card follows.
    // A note save that failed (was: beq 00248F60, the end without a
    // message) takes the same path.
    (0x0024_8ebc, 0xe3a0_1022, Raw(0xe3a0_100b)),
    (0x0024_8f88, 0x0aff_fff4, Raw(0x0aff_ffba)),
    // --- Loading screens: 0.5 s before the work, 1.5 s for a success. ---
    // Three updates in their vtables (was: the original updates): reading
    // the chosen game, the question and the transfer. While this frame waits
    // for the loading panel the entry returns 0, as the update does on a
    // frame with nothing to do; else it runs the original update. Reading
    // and the delivery wait for the panel's first 0.5 s; the question's
    // first state, which ends the panel of the reading, and the transfer's
    // closing message wait for its 1.5 s.
    (0x002e_66c4, 0x0024_55d0, Entry("transporter_read")),
    // The Bank task's update in its vtable (was: the original update): a
    // note save that ends on the error screen keeps it there.
    (0x002e_697c, 0x0024_8c68, Entry("transporter_bank")),
    (0x002e_67d0, 0x0024_7304, Entry("transporter_question")),
    (0x002e_6a9c, 0x0024_a0c4, Entry("transporter_transfer")),
    // Reading the chosen game, the 1.5 s before each of its errors: a
    // damaged save (state 0xB), no save (0xC), no Game Card (0xD) (was:
    // bl 0019B758 with 1500 ms). An error ends the panel as soon as it is
    // found: the answer is "passed".
    (0x0024_6208, 0xebfd_5552, Raw(0xe3a0_0001)),
    (0x0024_6278, 0xebfd_5536, Raw(0xe3a0_0001)),
    (0x0024_62e8, 0xebfd_551a, Raw(0xe3a0_0001)),
    // Reading Gen 5 and Gen 1/2: skip the remote validation.
    (0x0024_5728, 0xe59f_0c74, Branch(0x0024_5800)),
    (0x0024_60b8, 0xe59f_02e4, Branch(0x0024_61d0)),
    // Task 0xA, the server's question whether a transfer is open: its
    // request counts as made. The task itself is never entered.
    (0x0024_88a0, 0xebfd_4ca3, Raw(0xe3a0_0001)),
    // Reading Gen 5, per-slot result code (was: ldr r0, [r1, r7]). The stub
    // gives empty slots the skip code the server used to send.
    (0x0024_58dc, 0xe791_0007, Call("transporter_slot_stub")),
    // Question task, sub-state 2: skip the notice that nicknames and OT names
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
    // chosen game is read, before the Bank task saves its note, and before
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
    // Start screen's constructor (0024B9E4), before and after the call that
    // loads its archives (0024BA70; was: mov r7, #3 and mov r3, #0x27): the
    // archive with the logo and the start prompt is read in the chosen
    // language, and the reading language is set back.
    (
        0x0024_ba44,
        0xe3a0_7003,
        Call("transporter_start_screen_begin_stub"),
    ),
    (
        0x0024_ba74,
        0xe3a0_3027,
        Call("transporter_start_screen_end_stub"),
    ),
    // The same constructor's call that writes the version line, "Ver. 1.5"
    // (was: bl 001D33CC, "put message 0x29 into its pane"): the entry makes
    // the call and adds the patch's name to the line.
    (0x0024_bae8, 0xebfe_1e37, Call("transporter_version_shown")),
    // Building a layout (0013AE2C), the layout binary on its way to
    // Layout::Build (was: mov r1, r0): the check does the same and lets the
    // payload give the language screen's layout its Back button first.
    (
        0x0013_aee4,
        0xe1a0_1000,
        Call("transporter_layout_check_stub"),
    ),
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

#[cfg(test)]
mod tests {
    use super::super::ORIGINAL_APP_INIT;
    use super::*;
    use crate::profile::sample::{at, branch_target};

    #[test]
    fn fifty_six_words_and_thirty_two_entry_functions() {
        assert_eq!(PROFILE.regions(), 57);
        assert_eq!(PROFILE.entries().len(), 32);
        // Every edit lies in the original text or, for the five updates in
        // their vtables, its read-only data, which ends where its data
        // begins (002EC000).
        assert!(EDITS
            .iter()
            .all(|(address, _, _)| *address < BOOTSTRAP_ADDRESS
                || (0x002e_6618..0x002e_c000).contains(address)));
    }

    #[test]
    fn the_replaced_instructions_are_the_reviewed_ones() {
        // The original calls the entries stand in for.
        for (address, original, target) in [
            (STARTUP_CALL, 0xeb00_0228, ORIGINAL_APP_INIT),
            (0x0024_2ef8, 0xebff_ff28, 0x0024_2ba0),
            (0x0024_3528, 0xebff_5d37, 0x0021_aa0c),
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
            assert!(EDITS
                .iter()
                .any(|edit| (edit.0, edit.1) == (address, original)));
            assert_eq!(branch_target(address, original, true), Some(target));
        }
        // B on the language list: the original branch.
        assert_eq!(
            branch_target(0x0022_b3b8, 0xea00_0054, false),
            Some(0x0022_b510)
        );
        // The message function's first instruction is push {r4-r8, lr},
        // which its stub runs before it goes on behind it.
        assert!(EDITS
            .iter()
            .any(|(address, original, _)| (*address, *original) == (0x0019_b50c, 0xe92d_41f0)));
        // The router is called through the entry, which calls it in turn;
        // nothing inside it is edited.
        assert!(EDITS
            .iter()
            .all(|(address, _, _)| !(0x0024_2ba0..0x0024_2e08).contains(address)));
        // The Bank task goes from its first instruction to where the
        // original continues after a "yes"; its reply parsing is not edited.
        assert_eq!(at(&PROFILE, 0x0024_8cdc), Some(0xea00_0058));
        assert!(at(&PROFILE, 0x0024_8d58).is_none());
    }
}
