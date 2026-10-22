//! What each package for Bank must be: the offline patch (bank), its console
//! test build (test), the migration (migration) and the download
//! (download). Each has its own list of edits, written out below with their
//! original words. The test build is the offline patch with two more entry
//! functions and two more edits, in the main menu, which only it shows.

use crate::{
    check,
    elf::Elf,
    word, Edit, Expectation, Program, Result,
    Target::{At, Sym},
    Word::{Bl, Ptr, Raw, B},
};

pub static PROGRAM: Program = Program {
    name: "Bank",
    code_length: 0x2ac000,
    title_id: "00040000000c9b00",
    tmd_version: 6272,
    remaster: 6,
    code_hash: "2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf",
    exheader_hash: "39d93584b07901dfa7ea2fc8ce1236cdd92476584ee2dbb60c11a4ae9402abf7",
    bootstrap: 0x00313910,
    bootstrap_end: 0x00314000,
    payload: 0x003fb000,
    app_init: 0x0010494c,
    startup_call: 0x001040a4,
};

/// The main menu's locations, which only the test build may edit.
const MENU_LOCATIONS: [(u32, u32); 5] = [
    (0x001d6554, 0xeb0000ef),
    (0x002b33a4, 0xebfc853e),
    (0x002b33d4, 0xebfc8532),
    (0x003617bc, 0x002a6750),
    (0x003617dc, 0x002a6c84),
];

pub fn expectation(kind: &'static str) -> Expectation {
    let offline = matches!(kind, "bank" | "test");
    let router = match kind {
        "migration" => "bank_migrate_next",
        "download" => "bank_download_next",
        _ => "bank_offline_next",
    };

    // What every patch edits: the start-up hook is called, the archive is
    // kept, the router is the patch's own, and the start screen's call that
    // writes its version line goes to the entry that adds the patch's name
    // to it.
    let mut edits: Vec<Edit> = vec![
        (0x001040a4, 0xeb000228, Bl(Sym("patch_bootstrap_startup"))),
        (0x0029f338, 0xe594002c, B(At(0x0029f4fc))),
        (0x002a5a2c, 0xebfffed3, Bl(Sym(router))),
        (0x002b49c4, 0xebfcb5ff, Bl(Sym("bank_version_shown"))),
    ];
    // Every patch but the download, which runs the original online (there
    // the first start keeps its prompts, and the original refuses HOME and
    // sleep for its whole session itself): the "show a message" function
    // lets HOME and sleep through for a message that waits, and the
    // manager's set-up does not ask for a language (nop; mov r5, #1).
    if kind != "download" {
        edits.extend([
            (0x001d6650, 0xe92d41f0, B(Sym("bank_message_stub"))),
            (0x002a4898, 0xebfeddfb, Raw(0xe320f000)),
            (0x002a4940, 0xe1a05007, Raw(0xe3a05001)),
        ]);
    }
    match kind {
        // Task 3's update and busy poll run the conversion.
        "migration" => edits.extend([
            (0x00361ac4, 0x002abe08, Ptr(Sym("bank_migrate_scan"))),
            (0x00361ad4, 0x002b4a20, Ptr(Sym("bank_migrate_scan"))),
        ]),
        // Task 9 and task 3 each have an entry for their update and their
        // busy poll; the reward step's update writes the files and ends as
        // cancelled.
        "download" => edits.extend([
            (0x00361cfc, 0x002ae568, Ptr(Sym("bank_download_open"))),
            (0x00361d0c, 0x002b4a20, Ptr(Sym("bank_download_open_busy"))),
            (0x00361ac4, 0x002abe08, Ptr(Sym("bank_download_scan"))),
            (0x00361ad4, 0x002b4a20, Ptr(Sym("bank_download_scan_busy"))),
            (0x00361be4, 0x002ad1bc, Ptr(Sym("bank_download_save"))),
        ]),
        // The offline patch: the first start's notice is skipped to where
        // accepting it continues, and a saved language is cleared through
        // the original's store and write in task 3 (ldr r6, [r0, #0x74];
        // ldrh r5, [r0, #0x30]; cmp r5, #0; mov r2, #0; mov r1, #0). The
        // scan's no-game notice without its HOME question, its first state
        // without the loading panel (bl 001d5b44, bl 0025da24 -> nop), the
        // clock, and the updates of opening, loading, saving, rewards and
        // the scene set-up's server check (the original's "finished" stub).
        _ => edits.extend([
            (0x002ac5b0, 0xe5945040, B(At(0x002ac8c8))),
            (0x002ac61c, 0xe59060f0, Raw(0xe5906074)),
            (0x002ac628, 0xe1b05000, Raw(0xe1d053b0)),
            (0x002ac62c, 0xe320f000, Raw(0xe3550000)),
            (0x002ac640, 0xe1a02000, Raw(0xe3a02000)),
            (0x002ac648, 0xe6ff1075, Raw(0xe3a01000)),
            (0x002ac7a4, 0xebfca7a9, Bl(Sym("bank_offline_no_game_stub"))),
            (0x002ac7a8, 0xe3a00021, Raw(0xe3a00019)),
            (0x001d3bf4, 0xe92d4ff3, B(Sym("bank_offline_timestamp"))),
            (0x002ac00c, 0xebfca6cc, Raw(0xe320f000)),
            (0x002ac018, 0xebfec681, Raw(0xe320f000)),
            (0x00361ac4, 0x002abe08, Ptr(Sym("bank_offline_scan"))),
            (0x00361a08, 0x002a9750, Ptr(Sym("bank_offline_rewards"))),
            (0x00361be4, 0x002ad1bc, Ptr(Sym("bank_offline_rewards"))),
            (0x00361cfc, 0x002ae568, Ptr(Sym("bank_offline_load"))),
            (0x00361d0c, 0x002b4a20, Ptr(Sym("bank_offline_load"))),
            (0x00361ed4, 0x002af460, Ptr(Sym("bank_offline_load"))),
            (0x00361ee4, 0x002b4a20, Ptr(Sym("bank_offline_load"))),
            (0x00362034, 0x002b1cf8, Ptr(Sym("bank_offline_save"))),
            (0x00362044, 0x002b4a20, Ptr(Sym("bank_offline_save"))),
            (0x00361e84, 0x002af034, Raw(0x002af124)),
        ]),
    }
    // Only the test build: the main menu's update (its table slot) and its
    // listener "an entry was chosen" are the build's own.
    if kind == "test" {
        edits.extend([
            (0x003617bc, 0x002a6750, Ptr(Sym("bank_test_menu_update"))),
            (0x003617dc, 0x002a6c84, Ptr(Sym("bank_test_menu_chosen"))),
        ]);
    }

    // What every patch relies on: SDK CreateThread adds 0x20 to the worker's
    // SDK argument before SVC 08, and the main menu's locations, which only
    // the test build may edit, hold their original words. The offline patch
    // also relies on the original's "finished" stub (002af124), on claim
    // state 0x1b reaching the Bank field at 174 only behind its test of the
    // session's count (cmn r0, #1; beq 002aac54), where the patch keeps the
    // day of the last redemption, and on the scan's no-game route and
    // acknowledgment.
    let mut relies = vec![
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
    relies.extend(MENU_LOCATIONS);
    // Checked native objects: the factory's bound and the cases of the tasks
    // the hooks check, their vtable literals, and the reward claim's
    // constructor.
    relies.extend([
        (0x002a5a80, 0xe351001e),
        (0x002a5aa4, 0x002a5bdc),
        (0x002a5ab4, 0x002a5da4),
        (0x002a5abc, 0x002a5e50),
        (0x002a5ac8, 0x002a5ebc),
        (0x002a5acc, 0x002a5f08),
        (0x002a5ad8, 0x002a6130),
        (0x002a5b08, 0x002a63a8),
        (0x002a6534, 0x00361ab8),
        (0x002a6548, 0x00362028),
        (0x002a6550, 0x00361cf0),
        (0x002a6554, 0x00361bd8),
        (0x002a6570, 0x00361ec8),
        (0x002a5f1c, 0x1b0016fb),
        (0x002abbf0, 0x003619fc),
        (0x002abb40, 0xe5806000),
        (0x002a5c1c, 0xe5803000),
    ]);
    if offline {
        relies.extend([
            (0x002af124, 0xe3a00001),
            (0x002af128, 0xe12fff1e),
            (0x002aac04, 0xe3700001),
            (0x002aac08, 0x0a000011),
            (0x002abe0c, 0xe1a04000),
            (0x002ac560, 0x03a00018),
            (0x002ac7b4, 0xe5940040),
            (0x002ac7c0, 0xe3500004),
            (0x002ac7c8, 0x0a0000b0),
            (0x002ac924, 0xe3a00003),
            (0x002aca90, 0xe3a00020),
        ]);
    }

    // What must stay the original's: the main menu's locations but the test
    // build's own two, and for the offline patch the scan's no-game route
    // and acknowledgment and the code of game selection and its progress
    // check.
    let mut protected: Vec<(u32, u32)> = MENU_LOCATIONS
        .iter()
        .filter(|(address, _)| !(kind == "test" && matches!(address, 0x003617bc | 0x003617dc)))
        .map(|&(address, _)| (address, 4))
        .collect();
    // The factory, the reward claim's constructor and the vtables' first
    // three words stay native too; their update slots are edits.
    protected.extend([
        (0x002a5a7c, 0xb14),
        (0x002abb10, 0xe4),
        (0x00361ab8, 0xc),
        (0x00361cf0, 0xc),
        (0x00361ec8, 0xc),
        (0x00362028, 0xc),
        (0x003619fc, 0xc),
        (0x00361bd8, 0xc),
    ]);
    if offline {
        protected.extend([
            (0x002abe0c, 4),
            (0x002ac560, 4),
            (0x002ac7b4, 4),
            (0x002ac7c0, 4),
            (0x002ac7c8, 4),
            (0x002ac924, 4),
            (0x002aca90, 4),
            (0x002adc50, 0x388),
            (0x002d0fc8, 0xbc),
            (0x002b0098, 0x1d8),
            (0x002ae310, 0x230),
        ]);
    }

    Expectation {
        kind,
        program: &PROGRAM,
        edits,
        relies,
        protected,
        router,
        entries: match kind {
            "bank" => 9,
            "migration" => 4,
            "test" => 11,
            _ => 7,
        },
        menu_edits: if kind == "test" { 2 } else { 0 },
        extra,
        checks: &[
            "no entry function of another patch is linked; the test build's menu entries only in the test build",
            "the main-menu locations hold their original words, and only the test build edits two of them",
        ],
    }
}

fn extra(expect: &Expectation, _original: &[u8], patched: &[u8], elf: &Elf) -> Result<()> {
    let kind = expect.kind;
    // No entry function of another patch is linked into this one, nor the
    // test build's menu entries into a package that is released.
    let foreign: &[&str] = match kind {
        "bank" | "test" => &["bank_migrate_", "bank_download_"],
        "migration" => &["bank_offline_", "bank_download_"],
        _ => &["bank_offline_", "bank_migrate_"],
    };
    for prefix in foreign.iter().chain(&["bank_offline_menu"]) {
        check(
            !elf.symbols.keys().any(|name| name.starts_with(prefix)),
            format!("Entry function of another patch is linked: {prefix}*"),
        )?;
    }
    for name in ["bank_test_menu_chosen", "bank_test_menu_update"] {
        check(
            (kind == "test") == elf.symbols.contains_key(name),
            format!("Menu entry of the test build {name}"),
        )?;
    }
    if matches!(kind, "bank" | "test") {
        // The no-game stub hands the task in r4 on and keeps lr and sp.
        let stub = elf
            .symbols
            .get("bank_offline_no_game_stub")
            .ok_or("Missing ELF symbol bank_offline_no_game_stub")?;
        check(
            stub.size == 8
                && word(patched, stub.value).ok() == Some(0xe1a00004)
                && word(patched, stub.value + 4).ok().map(|w| w & 0xff000000) == Some(0xea000000),
            "No-game stub must pass r4 and tail-branch without changing LR/SP",
        )?;
    }
    Ok(())
}
