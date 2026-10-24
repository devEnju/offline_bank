//! What the Transporter patch must be. Transporter has one package; its
//! edits are written out below with their original words, in the order of
//! the builder's profile.

use crate::{
    check,
    elf::Elf,
    word, Edit, Expectation, Program, Result,
    Target::{At, Sym},
    Word::{Bl, Ptr, Raw, B},
};

pub static PROGRAM: Program = Program {
    name: "Transporter",
    code_length: 0x22a000,
    title_id: "00040000000c9c00",
    tmd_version: 5200,
    remaster: 5,
    code_hash: "001c20ada74016507c969bb44a0a50f8edf803ec06a3fc46834263ba8df0fd2f",
    exheader_hash: "4b6081a5d0242d43713fd0be3cec21ae73f075441a759af3ed03c5270d1baceb",
    bootstrap: 0x0028D1AC,
    bootstrap_end: 0x0028E000,
    payload: 0x00364000,
    app_init: 0x00104644,
    startup_call: 0x00103D9C,
};

const EDITS: [Edit; 56] = [
    (0x00103D9C, 0xEB000228, Bl(Sym("patch_bootstrap_startup"))),
    (0x00242EF8, 0xEBFFFF28, Bl(Sym("transporter_next"))),
    (
        0x002445EC,
        0xE5940038,
        Bl(Sym("transporter_list_ready_stub")),
    ),
    (
        0x00244EAC,
        0xEB000090,
        Bl(Sym("transporter_list_indicator_stub")),
    ),
    (
        0x00244D40,
        0xE595005C,
        B(Sym("transporter_list_frame_stub")),
    ),
    (0x0019B50C, 0xE92D41F0, B(Sym("transporter_message_stub"))),
    (0x0019B578, 0xEB01024E, Bl(Sym("transporter_message_text"))),
    (0x002E6780, 0x00246F04, Ptr(Sym("transporter_search"))),
    (0x00248CDC, 0xE59F0304, B(At(0x00248E44))),
    (0x00248EBC, 0xE3A01022, Raw(0xE3A0100B)),
    (0x00248F88, 0x0AFFFFF4, Raw(0x0AFFFFBA)),
    (0x002E66C4, 0x002455D0, Ptr(Sym("transporter_read"))),
    (0x002E697C, 0x00248C68, Ptr(Sym("transporter_bank"))),
    (0x002E67D0, 0x00247304, Ptr(Sym("transporter_question"))),
    (0x002E6A9C, 0x0024A0C4, Ptr(Sym("transporter_transfer"))),
    (0x00246208, 0xEBFD5552, Raw(0xE3A00001)),
    (0x00246278, 0xEBFD5536, Raw(0xE3A00001)),
    (0x002462E8, 0xEBFD551A, Raw(0xE3A00001)),
    (0x00245728, 0xE59F0C74, B(At(0x00245800))),
    (0x002460B8, 0xE59F02E4, B(At(0x002461D0))),
    (0x002488A0, 0xEBFD4CA3, Raw(0xE3A00001)),
    (0x002458DC, 0xE7910007, Bl(Sym("transporter_slot_stub"))),
    (0x00247478, 0xE5940008, B(At(0x002474E0))),
    (0x00243528, 0xEBFF5D37, Bl(Sym("transporter_cart_id_stub"))),
    (0x002439A8, 0xEBFF5B8C, Bl(Sym("transporter_cart_read"))),
    (0x002437F4, 0xEBFF5CD5, Bl(Sym("transporter_cart_write"))),
    (0x0024397C, 0xEBFF5C73, Bl(Sym("transporter_cart_write"))),
    (
        0x00244F10,
        0xE5846010,
        Bl(Sym("transporter_list_next_stub")),
    ),
    (0x00244908, 0xE3A00000, Bl(Sym("transporter_select_stub"))),
    (0x00244F00, 0xEBFE654D, Bl(Sym("transporter_card_removed"))),
    (0x00245658, 0xEBFE6377, Bl(Sym("transporter_card_removed"))),
    (0x00248E68, 0xEBFE5573, Bl(Sym("transporter_card_removed"))),
    (0x0024A298, 0xEBFE5067, Bl(Sym("transporter_card_removed"))),
    (0x0024A474, 0xEBFE4FF0, Bl(Sym("transporter_card_removed"))),
    (0x0025C5A4, 0xE5940008, Raw(0xE1A00004)),
    (
        0x0025C5A8,
        0xEBFE07A3,
        Bl(Sym("transporter_entry_card_removed")),
    ),
    (
        0x0025C5FC,
        0xEBFF3A49,
        Bl(Sym("transporter_language_chosen")),
    ),
    (0x0025C618, 0xEBFF3E93, Raw(0xE320F000)),
    (
        0x0022BEE0,
        0xEBFFFF49,
        Bl(Sym("transporter_language_order_stub")),
    ),
    (
        0x0022B5B0,
        0xEBFDDA8A,
        Bl(Sym("transporter_language_buttons")),
    ),
    (0x0022B6E0, 0xE3500005, Raw(0xE3500007)),
    (0x0022B6E4, 0x03A01001, Raw(0x03A01003)),
    (
        0x0022B3B8,
        0xEA000054,
        B(Sym("transporter_language_back_stub")),
    ),
    (
        0x0024BA44,
        0xE3A07003,
        Bl(Sym("transporter_start_screen_begin_stub")),
    ),
    (
        0x0024BA74,
        0xE3A03027,
        Bl(Sym("transporter_start_screen_end_stub")),
    ),
    (0x0024BAE8, 0xEBFE1E37, Bl(Sym("transporter_version_shown"))),
    (
        0x0013AEE4,
        0xE1A01000,
        Bl(Sym("transporter_layout_check_stub")),
    ),
    (0x001E41C0, 0xEBFCA8B7, Bl(Sym("transporter_vc_mount_stub"))),
    (0x0010621C, 0xEB0377DE, Raw(0xE320F000)),
    (0x002412D4, 0xEB003FF3, Bl(Sym("transporter_vc_scan_stub"))),
    (0x0024A150, 0xE59F04F4, B(Sym("transporter_deliver_stub"))),
    (0x0024A3C0, 0xE3A0000A, Raw(0xE3A0000E)),
    (0x0024A3D0, 0xE3A0000C, B(At(0x0024A2A8))),
    (0x0024A4E8, 0x0AFFFFA7, Raw(0x0AFFFF6E)),
    (
        0x0024F6B0,
        0xEBFD35A1,
        Bl(Sym("transporter_shiny_build_stub")),
    ),
    (
        0x0024F8E0,
        0xE5D4101B,
        Bl(Sym("transporter_shiny_gender_stub")),
    ),
];

/// SDK CreateThread (001E3F58) adds 0x20 to the worker's SDK argument
/// before SVC 08. Checked native objects: the factory's bound and case
/// targets, constructor vtable literals, the update slots that are no edits,
/// and the Bank object's construction at +0xCC.
const RELIES: [(u32, u32); 30] = [
    (0x001E3FC8, 0xE3550020),
    (0x001E3FD0, 0x92850020),
    (0x001E3FD8, 0x9A000008),
    (0x001E3FE4, 0xE3500027),
    (0x001E3FE8, 0x92800018),
    (0x001E3FF8, 0xE3500040),
    (0x001E3FFC, 0x83E00000),
    (0x001E403C, 0xAEF62B00),
    (0x001E4040, 0x93725B00),
    (0x0019CEE8, 0xE3510013),
    (0x0019CF04, 0x0019CF8C),
    (0x0019CF18, 0x0019D1A4),
    (0x0019CF1C, 0x0019D1FC),
    (0x0019CF20, 0x0019D23C),
    (0x0019CF24, 0x0019D2C8),
    (0x0019CF2C, 0x0019D360),
    (0x0019CF30, 0x0019D3B4),
    (0x0019D620, 0x002E6680),
    (0x0019D638, 0x002E6A90),
    (0x0019D63C, 0x002E6774),
    (0x0019D640, 0x002E6618),
    (0x0019D644, 0x002E66B8),
    (0x0019D64C, 0x002E6970),
    (0x0019D650, 0x002E67C4),
    (0x002E668C, 0x0024538C),
    (0x002E6624, 0x002445A4),
    (0x0010DA38, 0x002E6DD0),
    (0x0010D8FC, 0xE5840000),
    (0x0010624C, 0xE58400CC),
    (0x00106388, 0x000BB530),
];

/// What must stay the original's: the router, which the router entry calls
/// (00242BA0..00242E08), and the two tables of the Virtual Console titles
/// whose languages the hooks copied (39 rows of three words at 002AFE0C, one
/// word each at 002C2B1C). The factory, the Bank constructor and the
/// vtables' first three words stay native too, as do the update slots that
/// are no edits.
const PROTECTED: [(u32, u32); 15] = [
    (0x00242BA0, 0x268),
    (0x002AFE0C, 39 * 12),
    (0x002C2B1C, 39 * 4),
    (0x0019CEE4, 0x780),
    (0x0010D8EC, 0x150),
    (0x0010624C, 4),
    (0x00106388, 4),
    (0x002E6680, 0x10),
    (0x002E6A90, 0xC),
    (0x002E6774, 0xC),
    (0x002E6618, 0x10),
    (0x002E66B8, 0xC),
    (0x002E6970, 0xC),
    (0x002E67C4, 0xC),
    (0x002E6DD0, 0x18),
];

pub fn expectation() -> Expectation {
    Expectation {
        kind: "transporter",
        program: &PROGRAM,
        edits: EDITS.to_vec(),
        relies: RELIES.to_vec(),
        protected: PROTECTED.to_vec(),
        router: "transporter_next",
        entries: 32,
        menu_edits: 0,
        extra,
        checks: &[
            "the layout check lies beside the start-up hook and begins with the instruction it stands in for",
            "the payload names the transport-box files and the SD save folder",
        ],
    }
}

fn extra(_expect: &Expectation, _original: &[u8], patched: &[u8], elf: &Elf) -> Result<()> {
    // The layout check, called from 0013AEE4 before the payload can run,
    // lies beside the start-up hook and begins with the instruction it
    // stands in for (mov r1, r0).
    let layout = elf
        .symbols
        .get("transporter_layout_check_stub")
        .map_or(0, |s| s.value);
    let boot = elf.boot();
    check(
        layout >= boot.address
            && layout as u64 + 4 <= boot.address as u64 + boot.file_bytes as u64
            && word(patched, layout).ok() == Some(0xE1A01000),
        "Layout check lies beside the start-up hook and begins with mov r1, r0",
    )?;
    let rx = elf.rx();
    let start = (rx.address - crate::BASE) as usize;
    let text = patched
        .get(start..start + rx.file_bytes as usize)
        .unwrap_or_default();
    let contains = |needle: &[u8]| text.windows(needle.len()).any(|w| w == needle);
    check(
        contains(b"/mover.bin") && contains(b"/mover.alt.bin"),
        "Payload names /mover.bin and /mover.alt.bin",
    )?;
    check(
        contains(b"/roms/nds/saves/POKEMON_"),
        "Payload names the SD save folder",
    )
}
