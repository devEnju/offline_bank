//! The offline patch: Bank without its server.

use super::{
    router, Profile, KEEP_ARCHIVE, LANGUAGE_EDITS, MESSAGE, NOP, OPEN_TASK, SCAN_FIRST_START_EDITS,
    SCAN_TASK, STARTUP, VERSION,
};
use crate::profile::{Edit, Word::*};

pub const PROFILE: Profile = Profile {
    name: "bank",
    program: &super::BANK,
    edits: &[&SHARED, &OWN, &LANGUAGE_EDITS, &SCAN_FIRST_START_EDITS],
    relies: &[
        &NO_GAME_NATIVE,
        &NATIVE_FINISHED_BODY,
        &COUNT_UNAVAILABLE_BODY,
    ],
    menu_edits: &[],
};

/// The edits every patch that runs the original's screens makes.
pub(super) const SHARED: [Edit; 5] = [
    STARTUP,
    KEEP_ARCHIVE,
    router("bank_offline_next"),
    MESSAGE,
    VERSION,
];

/// Task 0xb (scene set-up between game selection and Bank loading): the
/// update slot of its vtable and the update it holds (a server check). The
/// original's function that replaces it, `NATIVE_FINISHED`, is the task's
/// own "end" callback and only answers "finished": `mov r0, #1; bx lr`. The
/// task keeps its start-up routine, which orders the character scene and
/// the text window.
pub const SCENE_UPDATE_SLOT: u32 = 0x0036_1e84;
const NATIVE_FINISHED: u32 = 0x002a_f124;

/// The offline patch's own edits. Call sites go to their entry with a `bl`;
/// the timestamp entry and the message entry with a `b`, which keeps the
/// caller's `lr` and `sp` for the original's prologue. Vtable slots get the
/// entry's address; constructors and init callbacks stay the original's.
/// The main menu (task 4) is never created, so none of its code is edited.
pub(super) const OWN: [Edit; 15] = [
    // Scan state 0x18 keeps its native loading-panel shutdown and notice.
    // Only the message call is adapted (was: bl 001d6650); state 0x19 then
    // waits for A and exits through the native unsuccessful-scan route,
    // instead of asking about HOME (was: state 0x21).
    (0x002a_c7a4, 0xebfc_a7a9, Call("bank_offline_no_game")),
    (0x002a_c7a8, 0xe3a0_0021, Raw(0xe3a0_0019)),
    // The timestamp helper, whose online-client check fails offline (was:
    // push {r0, r1, r4-r11, lr}).
    (0x001d_3bf4, 0xe92d_4ff3, To("bank_offline_timestamp")),
    // Scan state 0 mounts Bank's extdata, then starts the loading panel
    // (bl 001d5b44) and its text 0x20 (bl 0025da24). The scan entry starts
    // both one frame earlier, so these two calls become `nop`: the panel is
    // up before the mount, and started once.
    (0x002a_c00c, 0xebfc_a6cc, Raw(NOP)),
    (0x002a_c018, 0xebfe_c681, Raw(NOP)),
    // The scene set-up's server check: answered "finished".
    (SCENE_UPDATE_SLOT, 0x002a_f034, Raw(NATIVE_FINISHED)),
    // Task 3's update: the scan's loading panel, then the original.
    (SCAN_TASK[0].0, SCAN_TASK[0].1, Entry("bank_offline_scan")),
    // Tasks 0xd and 0xc, the reward claim and its guard.
    (0x0036_1a08, 0x002a_9750, Entry("bank_offline_rewards")),
    (0x0036_1be4, 0x002a_d1bc, Entry("bank_offline_rewards")),
    // Task 9, opening the Bank, and task 0x10, loading it: update and busy
    // poll.
    (OPEN_TASK[0].0, OPEN_TASK[0].1, Entry("bank_offline_load")),
    (OPEN_TASK[1].0, OPEN_TASK[1].1, Entry("bank_offline_load")),
    (0x0036_1ed4, 0x002a_f460, Entry("bank_offline_load")),
    (0x0036_1ee4, 0x002b_4a20, Entry("bank_offline_load")),
    // Task 7, saving: update and busy poll.
    (0x0036_2034, 0x002b_1cf8, Entry("bank_offline_save")),
    (0x0036_2044, 0x002b_4a20, Entry("bank_offline_save")),
];

/// The native task in r4, the no-game branch, the acknowledgment loop and
/// the failure exit the no-game entry relies on.
const NO_GAME_NATIVE: [(u32, u32); 7] = [
    (0x002a_be0c, 0xe1a0_4000),
    (0x002a_c560, 0x03a0_0018),
    (0x002a_c7b4, 0xe594_0040),
    (0x002a_c7c0, 0xe350_0004),
    (0x002a_c7c8, 0x0a00_00b0),
    (0x002a_c924, 0xe3a0_0003),
    (0x002a_ca90, 0xe3a0_0020),
];
/// The original's "finished", `mov r0, #1; bx lr`.
const NATIVE_FINISHED_BODY: [(u32, u32); 2] = [
    (NATIVE_FINISHED, 0xe3a0_0001),
    (NATIVE_FINISHED + 4, 0xe12f_ff1e),
];
/// Claim state 0x1b reads and writes the Bank field at body offset 174 only
/// behind this test of the session's count: `cmn r0, #1; beq 002aac54`. The
/// reward hook presents the count as -1, so the original never reaches the
/// field, and the patch keeps the day of the last redemption in it.
const COUNT_UNAVAILABLE_BODY: [(u32, u32); 2] =
    [(0x002a_ac04, 0xe370_0001), (0x002a_ac08, 0x0a00_0011)];

#[cfg(test)]
mod tests {
    use super::super::MENU_LOCATIONS;
    use super::*;
    use crate::profile::sample::{at, branch_target, edits, entry};

    #[test]
    fn twenty_eight_words_and_nine_entry_functions() {
        assert_eq!(PROFILE.regions(), 29);
        assert_eq!(
            PROFILE.entries(),
            [
                "bank_offline_next",
                "bank_message_shown",
                "bank_version_shown",
                "bank_offline_no_game",
                "bank_offline_timestamp",
                "bank_offline_scan",
                "bank_offline_rewards",
                "bank_offline_load",
                "bank_offline_save",
            ]
        );
    }

    #[test]
    fn calls_stay_calls_and_entries_with_the_callers_lr_are_branches() {
        for (address, original, word) in edits(&PROFILE) {
            if original >> 24 == 0xeb && word >> 24 != 0xe3 {
                assert_eq!(word >> 24, 0xeb, "{address:#010x}");
            }
        }
        // The no-game call replaced the original's message call.
        assert_eq!(
            branch_target(0x002a_c7a4, 0xebfc_a7a9, true),
            Some(0x001d_6650)
        );
        // The timestamp and message entries keep the caller's lr: b, not bl.
        for (address, name) in [
            (0x001d_3bf4, "bank_offline_timestamp"),
            (0x001d_6650, "bank_message_shown"),
        ] {
            let word = at(&PROFILE, address).unwrap();
            assert_eq!(
                branch_target(address, word, false),
                Some(entry(&PROFILE, name))
            );
        }
        // The scan's first state no longer starts its panel and text.
        for (address, call) in [(0x002a_c00c, 0x001d_5b44), (0x002a_c018, 0x0025_da24)] {
            let (_, original, word) = edits(&PROFILE)
                .into_iter()
                .find(|e| e.0 == address)
                .unwrap();
            assert_eq!(branch_target(address, original, true), Some(call));
            assert_eq!(word, NOP);
        }
    }

    #[test]
    fn the_scan_keeps_its_native_no_game_route_and_game_selection() {
        // Detection, loading-panel shutdown, acknowledgment polling and
        // both native scan exits stay intact, as does game selection, and
        // no edit touches the main menu.
        for address in [
            0x002a_c560,
            0x002a_c564,
            0x002a_c568,
            0x002a_c4c4,
            0x002a_c758,
            0x002a_c7b4,
            0x002a_c7c8,
            0x002a_c904,
            0x002a_c924,
            0x002a_dc50,
            0x002d_0fc8,
            0x002b_0098,
        ]
        .into_iter()
        .chain(MENU_LOCATIONS.map(|(address, _)| address))
        {
            assert!(at(&PROFILE, address).is_none(), "{address:#010x}");
        }
    }
}
