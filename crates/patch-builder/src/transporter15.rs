//! Poké Transporter 1.5 (title 00040000000C9C00, TMD 5200, remaster 5), as
//! `bank15.rs` is Bank 1.5. Its patch is a `Profile` in a file of its own
//! beside this one (`offline.rs`), as Bank's are, with the same placement:
//! the payload goes on the first page after the original zero-initialised
//! data, and a start-up hook in the zero bytes after the original text makes
//! its code pages executable.

pub mod offline;

use crate::elf::{Contract, PayloadElf};
use crate::placement::InputIdentity;
use crate::profile::{Profile, Program};
use crate::{fail, le32, Result};
use transporter_payload::language::{LIST_CAPACITY, VC_LANGUAGES, VC_TITLE_LANGUAGES};
use transporter_payload::sdsave::GAMES;
use transporter_payload::task::{
    BANK_VTABLE, TASK_BANK, TASK_COUNT, TASK_LANGUAGE, TASK_LIST, TASK_QUESTION, TASK_READ,
    TASK_RESTORE, TASK_SEARCH, TASK_TRANSFER,
};

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
    relies: &[&SDK_PRIORITY_CODE, &NATIVE_OBJECTS],
    check,
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

/// The layout check stands in for this instruction at 0013AEE4 and begins
/// with it: `mov r1, r0`.
const LAYOUT_CHECK_FIRST: u32 = 0xe1a0_1000;

// SDK CreateThread's priority conversion (`001E3F58`, the same code as
// Bank's), including its encoded-priority literals. The worker passes SDK
// 0x11, which this code turns into kernel 0x31.
pub(crate) const SDK_PRIORITY_CODE: [(u32, u32); 9] = [
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

/// Factory bound, case targets and constructor vtable literals of the
/// tasks the checked task views accept, and the update slots that are no
/// edits; the others name their original words. Bank data comes from
/// constructor 0010D8EC: it installs BANK_VTABLE and startup stores its
/// result at +0xCC.
pub(crate) const NATIVE_OBJECTS: [(u32, u32); 23] = [
    (0x0019_cee8, 0xe351_0000 | TASK_COUNT),
    (0x0019_cf04, 0x0019_cf8c),
    (0x0019_cf18, 0x0019_d1a4),
    (0x0019_cf1c, 0x0019_d1fc),
    (0x0019_cf20, 0x0019_d23c),
    (0x0019_cf24, 0x0019_d2c8),
    (0x0019_cf2c, 0x0019_d360),
    (0x0019_cf30, 0x0019_d3b4),
    (0x0019_cf38, 0x0019_d45c),
    (0x0019_d620, TASK_LANGUAGE),
    (0x0019_d638, TASK_TRANSFER),
    (0x0019_d63c, TASK_SEARCH),
    (0x0019_d640, TASK_LIST),
    (0x0019_d644, TASK_READ),
    (0x0019_d64c, TASK_BANK),
    (0x0019_d650, TASK_QUESTION),
    (0x0019_d658, TASK_RESTORE),
    (TASK_LANGUAGE + 0xc, 0x0024_538c),
    (TASK_LIST + 0xc, 0x0024_45a4),
    (0x0010_da38, BANK_VTABLE),
    (0x0010_d8fc, 0xe584_0000),
    (0x0010_624c, 0xe584_00cc),
    (0x0010_6388, 0x000b_b530),
];

/// What Transporter relies on beyond its edited words: the game list, and
/// the layout check, which must begin with the instruction it stands in for.
fn check(code: &[u8], _: &Profile, payload: &PayloadElf) -> Result<()> {
    check_game_list(code)?;
    let check = payload.export("transporter_layout_check_stub")?;
    let at = check
        .checked_sub(BOOTSTRAP_ADDRESS)
        .ok_or_else(|| crate::Error("layout check is not in the start-up hook".into()))?;
    if le32(payload.bootstrap_bytes(), at as usize)? != LAYOUT_CHECK_FIRST {
        return fail("layout check does not begin with the instruction it stands in for");
    }
    Ok(())
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
