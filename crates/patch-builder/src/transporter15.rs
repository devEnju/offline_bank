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

use crate::placement::{prepare_expanded_data, InputIdentity, LinkedPayload, PreparedPlacement};
use crate::{encode_arm_branch, fail, hex, le16, le32, sha256, CheckedEdit, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

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
/// The check on the way of every layout (0013AEE4), in the same already
/// executable place as the start-up hook, twelve words after its start:
/// layouts are built before the payload can run.
pub const LAYOUT_CHECK: u32 = BOOTSTRAP_ADDRESS + 0x30;
/// First page after the original zero-initialised data (ends at 003638A4).
pub const PAYLOAD_ADDRESS: u32 = 0x0036_4000;
/// Entry words at the start of the payload, in this order.
pub const ENTRY_CHECK: u32 = PAYLOAD_ADDRESS;
pub const ENTRY_DELIVER: u32 = PAYLOAD_ADDRESS + 4;
pub const ENTRY_SESSION: u32 = PAYLOAD_ADDRESS + 8;
pub const ENTRY_SLOT: u32 = PAYLOAD_ADDRESS + 12;
pub const ENTRY_CART_ID: u32 = PAYLOAD_ADDRESS + 16;
pub const ENTRY_CART_READ: u32 = PAYLOAD_ADDRESS + 20;
pub const ENTRY_CART_WRITE: u32 = PAYLOAD_ADDRESS + 24;
pub const ENTRY_LIST_NEXT: u32 = PAYLOAD_ADDRESS + 28;
pub const ENTRY_SELECT: u32 = PAYLOAD_ADDRESS + 32;
pub const ENTRY_VC_SCAN: u32 = PAYLOAD_ADDRESS + 36;
pub const ENTRY_LANGUAGE_CHOSEN: u32 = PAYLOAD_ADDRESS + 40;
pub const ENTRY_LANGUAGE_ORDER: u32 = PAYLOAD_ADDRESS + 44;
pub const ENTRY_LANGUAGE_BUTTONS: u32 = PAYLOAD_ADDRESS + 48;
pub const ENTRY_LANGUAGE_BACK: u32 = PAYLOAD_ADDRESS + 52;
pub const ENTRY_TITLE_BEGIN: u32 = PAYLOAD_ADDRESS + 56;
pub const ENTRY_TITLE_END: u32 = PAYLOAD_ADDRESS + 60;
pub const ENTRY_COUNT: u32 = 16;

enum Word {
    Raw(u32),
    Branch(u32),
    BranchLink(u32),
    /// A branch with the given condition nibble (0 equal, 1 not equal).
    BranchIf(u32, u8),
}
use Word::*;
const EQUAL: u8 = 0;

/// (address, original word, replacement).
const EDITS: &[(u32, u32, Word)] = &[
    // --- Start-up: run application init, then make the payload executable. ---
    (STARTUP_CALL, 0xeb00_0228, BranchLink(BOOTSTRAP_ADDRESS)),
    // --- Offline flow: the server steps are removed. ---
    // get_next_state, SHOW_GAMES (was: next is CONNECT_ONLINE). The stub
    // refuses HOME and sleep as the connect step did, then answers
    // GET_POKEMON.
    (0x0024_2d10, 0x03a0_0003, BranchIf(ENTRY_SESSION, EQUAL)),
    // get_next_state, GET_POKEMON: next is CHECK_IF_USER_CAN_TRANSFER.
    (0x0024_2d28, 0xe352_0002, Raw(0xe3a0_000b)),
    (0x0024_2d2c, 0x0a00_002f, Branch(0x0024_2c4c)),
    // Bank check, sub-state 0: no request object; go to the answer.
    (0x0024_8cdc, 0xe59f_0304, Branch(0x0024_8d3c)),
    // Bank check, sub-state 3: no server reply to parse.
    (0x0024_8d58, 0xe1a0_0004, Branch(0x0024_8e44)),
    // Reading Gen 5 and Gen 1/2: skip the remote validation.
    (0x0024_5728, 0xe59f_0c74, Branch(0x0024_5800)),
    (0x0024_60b8, 0xe59f_02e4, Branch(0x0024_61d0)),
    (0x0024_88a0, 0xebfd_4ca3, Raw(0xe3a0_0001)),
    (0x0024_83cc, 0xebfd_4dd8, Raw(0xe3a0_0001)),
    // Reading Gen 5, per-slot result code (was: ldr r0, [r1, r7]). The stub
    // gives empty slots the skip code the server used to send.
    (0x0024_58dc, 0xe791_0007, BranchLink(ENTRY_SLOT)),
    // Question step, sub-state 2: skip the notice that nicknames and OT names
    // with prohibited words will be erased (message 0x10). Only the server
    // made that decision, so offline it never happens. Continue at the
    // original "go to sub-state 4".
    (0x0024_7478, 0xe594_0008, Branch(0x0024_74e0)),
    // --- Black/White saves on the SD card when no Gen 5 cartridge is in. ---
    // Cartridge task (00243510): its three cartridge calls. With a Gen 5
    // cartridge the entries call the original functions unchanged.
    (0x0024_3528, 0xebff_5d37, BranchLink(ENTRY_CART_ID)),
    (0x0024_39a8, 0xebff_5b8c, BranchLink(ENTRY_CART_READ)),
    (0x0024_37f4, 0xebff_5cd5, BranchLink(ENTRY_CART_WRITE)),
    (0x0024_397c, 0xebff_5c73, BranchLink(ENTRY_CART_WRITE)),
    // Game list (00244D2C): "state = 3, go on to the Virtual Console titles"
    // (was: str r6, [r4, #0x10]). The entry reads the next SD save first.
    (0x0024_4f10, 0xe584_6010, BranchLink(ENTRY_LIST_NEXT)),
    // Game list (002445A4), a DS entry was confirmed (was: mov r0, #0). The
    // entry notes which one and returns 0.
    (0x0024_4908, 0xe3a0_0000, BranchLink(ENTRY_SELECT)),
    // --- The language screen chooses which language's games are listed. ---
    // The screen ending (0025C5BC): no switch of the screens' language (was:
    // bl 0022AF28); the entry notes the choice. The record of the language
    // in the manager (was: bl 0022C06C) keeps its value.
    (0x0025_c5fc, 0xebff_3a49, BranchLink(ENTRY_LANGUAGE_CHOSEN)),
    (0x0025_c618, 0xebff_3e93, Raw(0xe320_f000)),
    // Constructor (0022BD28), ordering and placing the list (was:
    // bl 0022BC0C): the six languages that are not listed already, centred,
    // and the Back button below them.
    (0x0022_bee0, 0xebff_ff49, BranchLink(ENTRY_LANGUAGE_ORDER)),
    // Showing the list (0022B530): cursor range and enabled buttons (was:
    // bl 001A1FE0, which the entry calls first) for the shown entries only.
    (0x0022_b5b0, 0xebfd_da8a, BranchLink(ENTRY_LANGUAGE_BUTTONS)),
    // After an entry was picked (was: cmp r0, #5 / moveq r1, #1: list index
    // 5, Japanese, goes to the kana/kanji part): index 7, the Back button,
    // goes to part 3, the end of the screen; every language goes to the
    // confirm part.
    (0x0022_b6e0, 0xe350_0005, Raw(0xe350_0007)),
    (0x0022_b6e4, 0x03a0_1001, Raw(0x03a0_1003)),
    // Key handler (0022B330), B on a part without a Back button (was:
    // b 0022B510): on the list the entry presses the Back button.
    (0x0022_b3b8, 0xea00_0054, Branch(ENTRY_LANGUAGE_BACK)),
    // Title screen's constructor (0024B9E4), before and after the call that
    // loads its archives (0024BA70; was: mov r7, #3 and mov r3, #0x27): the
    // archive with the logo and the start prompt is read in the chosen
    // language, and the reading language is set back.
    (0x0024_ba44, 0xe3a0_7003, BranchLink(ENTRY_TITLE_BEGIN)),
    (0x0024_ba74, 0xe3a0_3027, BranchLink(ENTRY_TITLE_END)),
    // Building a layout (0013AE2C), the layout binary on its way to
    // Layout::Build (was: mov r1, r0): the check does the same and lets the
    // payload give the language screen's layout its Back button first.
    (0x0013_aee4, 0xe1a0_1000, BranchLink(LAYOUT_CHECK)),
    // Scanner of the Virtual Console titles (0024120C), asking whether a
    // row's title is installed (was: bl 002512A8): the entry answers no for
    // a row in another language than the listed one. The scanner files the
    // trainer names under the number of each title it finds, so the titles
    // are left out here and not when the list is built. The list holds 40
    // entries as the original built it; one language gives at most 12.
    (0x0024_12d4, 0xeb00_3ff3, BranchLink(ENTRY_VC_SCAN)),
    // --- Bank check: r0 = task; the entry answers the next sub-state. ---
    (0x0024_8d3c, 0xe594_003c, Raw(0xe1a0_0004)),
    (0x0024_8d40, 0xe594_1028, BranchLink(ENTRY_CHECK)),
    (0x0024_8d44, 0xebff_da5a, Branch(0x0024_8ec8)),
    // --- Transfer, sub-state 0: deliver first. ---
    // 1: delivered, continue with the original removal code (0024A274).
    // 2: still working, return and come back next frame (0024A5DC).
    // else: the original failure message (sub-state 0x11); the game is
    // left alone.
    (0x0024_a150, 0xe59f_04f4, Raw(0xe1a0_0004)),
    (0x0024_a154, 0xe594_100c, BranchLink(ENTRY_DELIVER)),
    (0x0024_a158, 0xe590_5000, Raw(0xe350_0001)),
    (0x0024_a15c, 0xe3a0_0050, BranchIf(0x0024_a274, EQUAL)),
    (0x0024_a160, 0xebfe_707e, Raw(0xe350_0002)),
    (0x0024_a164, 0xe350_0000, BranchIf(0x0024_a5dc, EQUAL)),
    (0x0024_a168, 0xe320_f000, Raw(0xe3a0_0011)),
    (0x0024_a16c, 0x1bfd_46ec, Branch(0x0024_a4c0)),
    // Transfer, sub-state 0xA: no commit request; continue with the
    // original final save and success message (sub-state 0xE).
    (0x0024_a3d8, 0xe594_003c, Raw(0xe3a0_000e)),
    (0x0024_a3dc, 0xe594_1028, Branch(0x0024_a4c0)),
];

fn word_edit(address: u32, original: u32, word: &Word) -> Result<CheckedEdit> {
    let replacement = match *word {
        Raw(value) => value.to_le_bytes(),
        Branch(target) => encode_arm_branch(address, target, false)?,
        BranchLink(target) => encode_arm_branch(address, target, true)?,
        BranchIf(target, condition) => {
            let mut bytes = encode_arm_branch(address, target, false)?;
            // Condition field: always (E) becomes the given condition.
            bytes[3] = (bytes[3] & 0x0f) | (condition << 4);
            bytes
        }
    };
    Ok(CheckedEdit {
        offset: (address - CODE_BASE) as usize,
        expected: original.to_le_bytes().to_vec(),
        replacement: replacement.to_vec(),
    })
}

/// The linked `transporter-payload` ELF, split into what goes where.
#[derive(Debug, Clone)]
pub struct HookImage {
    /// Start-up hook, for the zero bytes after the original text.
    pub bootstrap: Vec<u8>,
    /// Payload bytes from `PAYLOAD_ADDRESS`: code pages, then initialised data.
    pub image: Vec<u8>,
    pub executable_size: u32,
    pub memory_size: u32,
    pub entries: [u32; ENTRY_COUNT as usize],
}
impl HookImage {
    pub fn linked_payload(&self) -> LinkedPayload<'_> {
        LinkedPayload {
            image: &self.image,
            linked_address: PAYLOAD_ADDRESS,
            entry_offset: 0,
            executable_size: self.executable_size,
            memory_size: self.memory_size,
        }
    }
}

/// Target of an unconditional `b` (0xEA) or `bl` (0xEB).
fn branch_target(address: u32, word: u32, link: bool) -> Option<u32> {
    let top = if link { 0xeb00_0000 } else { 0xea00_0000 };
    if word & 0xff00_0000 != top {
        return None;
    }
    let offset = ((word & 0x00ff_ffff) << 8) as i32 >> 6;
    Some(address.wrapping_add(8).wrapping_add(offset as u32))
}

struct Load {
    address: u32,
    bytes: Vec<u8>,
    memory: u32,
    flags: u32,
}

/// Accepts exactly three segments: the start-up hook (read-execute, at
/// `BOOTSTRAP_ADDRESS`), the payload's code pages (read-execute, at
/// `PAYLOAD_ADDRESS`, whole pages), and its data (read-write, directly after,
/// ending on a page boundary). The hook must call the original application
/// init first, and the payload must start with its entry branches.
pub fn inspect_image(elf: &[u8]) -> Result<HookImage> {
    if elf.len() < 52 || &elf[..7] != b"\x7fELF\x01\x01\x01" {
        return fail("hook image is not a 32-bit little-endian ELF");
    }
    if le16(elf, 16)? != 2 || le16(elf, 18)? != 40 {
        return fail("hook image is not an ARM executable");
    }
    let (table, entry_size, count) = (
        le32(elf, 28)? as usize,
        le16(elf, 42)? as usize,
        le16(elf, 44)? as usize,
    );
    if entry_size != 32 {
        return fail("unexpected ELF program header size");
    }
    let mut loads = Vec::new();
    for index in 0..count {
        let at = table + index * 32;
        if le32(elf, at)? != 1 {
            continue;
        }
        let (offset, address, file, memory, flags) = (
            le32(elf, at + 4)? as usize,
            le32(elf, at + 8)?,
            le32(elf, at + 16)? as usize,
            le32(elf, at + 20)?,
            le32(elf, at + 24)?,
        );
        if memory == 0 {
            continue;
        }
        if file > memory as usize {
            return fail("hook image segment has more file bytes than memory");
        }
        loads.push(Load {
            address,
            bytes: crate::bytes(elf, offset, file)?.to_vec(),
            memory,
            flags,
        });
    }
    loads.sort_by_key(|load| load.address);
    let [bootstrap, code, data] = loads.as_slice() else {
        return fail("hook image must have exactly three loaded segments");
    };

    if bootstrap.address != BOOTSTRAP_ADDRESS
        || bootstrap.flags != 5
        || bootstrap.bytes.len() != bootstrap.memory as usize
        || bootstrap.bytes.len() % 4 != 0
        || bootstrap.bytes.len() < 8
        || bootstrap.bytes.len() > (BOOTSTRAP_LIMIT - BOOTSTRAP_ADDRESS) as usize
    {
        return fail("start-up hook is not the reviewed read-execute placement");
    }
    // push {r4, lr}; bl <original application init>
    if le32(&bootstrap.bytes, 0)? != 0xe92d_4010
        || branch_target(BOOTSTRAP_ADDRESS + 4, le32(&bootstrap.bytes, 4)?, true)
            != Some(ORIGINAL_APP_INIT)
    {
        return fail("start-up hook does not begin by calling the original application init");
    }
    // The layout check begins with the instruction it stands in for.
    let check = (LAYOUT_CHECK - BOOTSTRAP_ADDRESS) as usize;
    if bootstrap.bytes.len() < check + 4 || le32(&bootstrap.bytes, check)? != 0xe1a0_1000 {
        return fail("layout check is not at its reviewed place in the start-up hook");
    }

    if code.address != PAYLOAD_ADDRESS
        || code.flags != 5
        || code.bytes.len() != code.memory as usize
        || code.memory == 0
        || code.memory % 0x1000 != 0
    {
        return fail("payload code is not whole read-execute pages at the reviewed address");
    }
    let data_address = PAYLOAD_ADDRESS + code.memory;
    if data.address != data_address || data.flags != 6 || (data.address + data.memory) % 0x1000 != 0
    {
        return fail("payload data does not follow its code and end on a page boundary");
    }

    let first_code = PAYLOAD_ADDRESS + ENTRY_COUNT * 4;
    let end = PAYLOAD_ADDRESS + code.memory;
    let mut entries = [0; ENTRY_COUNT as usize];
    for (index, entry) in entries.iter_mut().enumerate() {
        let address = PAYLOAD_ADDRESS + index as u32 * 4;
        let target = branch_target(address, le32(&code.bytes, index * 4)?, false)
            .filter(|target| (first_code..end).contains(target));
        let Some(target) = target else {
            return fail("payload does not start with its branches into itself");
        };
        *entry = target;
    }
    for (index, entry) in entries.iter().enumerate() {
        if entries[..index].contains(entry) {
            return fail("payload entries are not distinct");
        }
    }
    let mut image = code.bytes.clone();
    image.extend_from_slice(&data.bytes);
    Ok(HookImage {
        bootstrap: bootstrap.bytes.clone(),
        image,
        executable_size: code.memory,
        memory_size: code.memory + data.memory,
        entries,
    })
}

/// All edits inside the original image: the listed words and the start-up
/// hook. The payload itself is placed by `prepare_expanded_data`.
pub fn edits(image: &HookImage) -> Result<Vec<CheckedEdit>> {
    let mut out = Vec::new();
    for (address, original, word) in EDITS {
        out.push(word_edit(*address, *original, word)?);
    }
    out.push(CheckedEdit {
        offset: (BOOTSTRAP_ADDRESS - CODE_BASE) as usize,
        expected: vec![0; image.bootstrap.len()],
        replacement: image.bootstrap.clone(),
    });
    Ok(out)
}

/// Checks the whole input pair and every original word, then builds the
/// paired IPS and exheader.
pub fn prepare(code: &[u8], exheader: &[u8], elf: &[u8]) -> Result<PreparedPlacement> {
    if code.len() != CODE_LENGTH || sha256(code) != CODE_SHA256 {
        return fail("code.bin is not the reviewed Poké Transporter 1.5 executable");
    }
    let image = inspect_image(elf)?;
    let edits = edits(&image)?;
    let prepared = prepare_expanded_data(
        code,
        exheader,
        &INPUT_IDENTITY,
        &image.linked_payload(),
        &edits,
    )?;
    if prepared.layout().payload_address != PAYLOAD_ADDRESS {
        return fail("payload placement differs from the reviewed address");
    }
    Ok(prepared)
}

#[derive(Debug, Clone)]
pub struct Artifacts {
    pub directory: PathBuf,
    pub ips_sha256: [u8; 32],
    pub exheader_sha256: [u8; 32],
    pub expanded_code_sha256: [u8; 32],
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| crate::Error(format!("{}: {e}", path.display())))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| crate::Error(format!("{}: {e}", path.display())))
}

/// Writes `code.ips`, `exheader.bin`, `manifest.json`, and `report.md` into a
/// new directory. Nothing existing is replaced, and no console or SD card is
/// touched.
pub fn build(code: &[u8], exheader: &[u8], elf: &[u8], output: &Path) -> Result<Artifacts> {
    let prepared = prepare(code, exheader, elf)?;
    let image = inspect_image(elf)?;
    let layout = prepared.layout();
    fs::create_dir(output).map_err(|e| crate::Error(format!("{}: {e}", output.display())))?;
    let ips_sha256 = sha256(prepared.ips());
    let exheader_sha256 = sha256(prepared.exheader());
    let sites: Vec<String> = EDITS
        .iter()
        .map(|(address, _, _)| format!("\"{address:08x}\""))
        .collect();
    let manifest = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 3,\n",
            "  \"profile\": \"transporter15-title5200-remaster5\",\n",
            "  \"status\": \"development-console-validation-required\",\n",
            "  \"title_id\": \"00040000000c9c00\",\n",
            "  \"tmd_version\": {},\n",
            "  \"source_code_sha256\": \"{}\",\n",
            "  \"source_exheader_sha256\": \"{}\",\n",
            "  \"source_elf_sha256\": \"{}\",\n",
            "  \"expanded_code_sha256\": \"{}\",\n",
            "  \"expanded_code_bytes\": {},\n",
            "  \"bootstrap_address\": \"{:08x}\",\n",
            "  \"bootstrap_bytes\": {},\n",
            "  \"payload_address\": \"{:08x}\",\n",
            "  \"payload_executable_bytes\": {},\n",
            "  \"payload_memory_bytes\": {},\n",
            "  \"artifacts\": {{\n",
            "    \"code.ips\": {{ \"sha256\": \"{}\", \"bytes\": {} }},\n",
            "    \"exheader.bin\": {{ \"sha256\": \"{}\", \"bytes\": {} }}\n",
            "  }},\n",
            "  \"word_edits\": [{}]\n",
            "}}\n"
        ),
        TMD_VERSION,
        hex(&CODE_SHA256),
        hex(&EXHEADER_SHA256),
        hex(&sha256(elf)),
        hex(&prepared.expanded_code_sha256()),
        layout.expanded_code_size,
        BOOTSTRAP_ADDRESS,
        image.bootstrap.len(),
        layout.payload_address,
        layout.executable_size,
        layout.memory_size,
        hex(&ips_sha256),
        prepared.ips().len(),
        hex(&exheader_sha256),
        prepared.exheader().len(),
        sites.join(", "),
    );
    let report = format!(
        concat!(
            "# Poké Transporter 1.5 offline patch\n\n",
            "Install both files from this folder, never one without the other:\n\n",
            "```text\n",
            "SD:/luma/titles/00040000000C9C00/code.ips\n",
            "SD:/luma/titles/00040000000C9C00/exheader.bin\n",
            "```\n\n",
            "- Edited instructions: {}\n",
            "- Start-up hook: {} bytes at `{:08X}`\n",
            "- Payload: {} bytes of code and {} bytes of data at `{:08X}`\n",
            "- code.ips SHA-256: `{}`\n",
            "- exheader.bin SHA-256: `{}`\n\n",
            "Back up the source game save and Bank's extdata before testing.\n"
        ),
        EDITS.len(),
        image.bootstrap.len(),
        BOOTSTRAP_ADDRESS,
        layout.executable_size,
        layout.writable_size,
        layout.payload_address,
        hex(&ips_sha256),
        hex(&exheader_sha256),
    );
    write_new(&output.join("code.ips"), prepared.ips())?;
    write_new(&output.join("exheader.bin"), prepared.exheader())?;
    write_new(&output.join("report.md"), report.as_bytes())?;
    write_new(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(Artifacts {
        directory: output.to_path_buf(),
        ips_sha256,
        exheader_sha256,
        expanded_code_sha256: prepared.expanded_code_sha256(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (address, flags, file bytes, memory size)
    type Segment = (u32, u32, Vec<u8>, u32);

    /// A minimal ELF with the given segments.
    fn elf(segments: &[Segment]) -> Vec<u8> {
        let count = segments.len();
        let mut out = vec![0u8; 52 + 32 * count];
        out[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        out[16..18].copy_from_slice(&2u16.to_le_bytes());
        out[18..20].copy_from_slice(&40u16.to_le_bytes());
        out[28..32].copy_from_slice(&52u32.to_le_bytes());
        out[42..44].copy_from_slice(&32u16.to_le_bytes());
        out[44..46].copy_from_slice(&(count as u16).to_le_bytes());
        for (index, (address, flags, bytes, memory)) in segments.iter().enumerate() {
            let offset = out.len() as u32;
            let header = [
                1,
                offset,
                *address,
                *address,
                bytes.len() as u32,
                *memory,
                *flags,
                4,
            ];
            for (word, value) in header.iter().enumerate() {
                let at = 52 + index * 32 + word * 4;
                out[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(bytes);
        }
        out
    }
    fn bootstrap() -> Vec<u8> {
        let mut bytes = 0xe92d_4010u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(
            &encode_arm_branch(BOOTSTRAP_ADDRESS + 4, ORIGINAL_APP_INIT, true).unwrap(),
        );
        bytes.resize((LAYOUT_CHECK - BOOTSTRAP_ADDRESS) as usize, 0xAA);
        bytes.extend_from_slice(&0xe1a0_1000u32.to_le_bytes());
        bytes.resize(0x40, 0xAA);
        bytes
    }
    fn code() -> Vec<u8> {
        let mut bytes = Vec::new();
        let first = PAYLOAD_ADDRESS + ENTRY_COUNT * 4;
        for index in 0..ENTRY_COUNT {
            let from = PAYLOAD_ADDRESS + index * 4;
            bytes.extend_from_slice(&encode_arm_branch(from, first + index * 4, false).unwrap());
        }
        bytes.resize(0x2000, 0xBB);
        bytes
    }
    fn good() -> Vec<Segment> {
        vec![
            (BOOTSTRAP_ADDRESS, 5, bootstrap(), 0x40),
            (PAYLOAD_ADDRESS, 5, code(), 0x2000),
            (PAYLOAD_ADDRESS + 0x2000, 6, vec![1, 2, 3, 4], 0x5000),
        ]
    }

    #[test]
    fn accepts_the_reviewed_three_segment_shape() {
        let image = inspect_image(&elf(&good())).unwrap();
        let first = PAYLOAD_ADDRESS + ENTRY_COUNT * 4;
        let expected: Vec<u32> = (0..ENTRY_COUNT).map(|index| first + index * 4).collect();
        assert_eq!(image.entries.to_vec(), expected);
        assert_eq!(image.bootstrap, bootstrap());
        assert_eq!((image.executable_size, image.memory_size), (0x2000, 0x7000));
        assert_eq!(image.image.len(), 0x2004);
        assert_eq!(&image.image[0x2000..], &[1, 2, 3, 4]);
        let payload = image.linked_payload();
        assert_eq!(
            (payload.linked_address, payload.entry_offset),
            (PAYLOAD_ADDRESS, 0)
        );
    }

    #[test]
    fn rejects_every_deviation_from_that_shape() {
        let reject = |change: &dyn Fn(&mut Vec<Segment>)| {
            let mut segments = good();
            change(&mut segments);
            assert!(inspect_image(&elf(&segments)).is_err());
        };
        // Segment count.
        reject(&|s| {
            s.pop();
        });
        reject(&|s| s.push((0x0037_0000, 6, vec![0; 4], 0x1000)));
        // Start-up hook: address, permissions, size, first two instructions.
        reject(&|s| s[0].0 += 4);
        reject(&|s| s[0].1 = 6);
        reject(&|s| {
            s[0].2 = vec![0xAA; 0xE58];
            s[0].3 = 0xE58;
        });
        reject(&|s| s[0].2[0] ^= 1);
        reject(&|s| {
            let other = encode_arm_branch(BOOTSTRAP_ADDRESS + 4, 0x0010_4648, true).unwrap();
            s[0].2[4..8].copy_from_slice(&other);
        });
        reject(&|s| s[0].2[(LAYOUT_CHECK - BOOTSTRAP_ADDRESS) as usize] ^= 1);
        // Payload code: address, permissions, partial page, entry words.
        reject(&|s| s[1].0 += 0x1000);
        reject(&|s| s[1].1 = 7);
        reject(&|s| {
            s[1].2.truncate(0x1ffc);
            s[1].3 = 0x1ffc;
        });
        reject(&|s| s[1].2[..4].copy_from_slice(&0xe3a0_0000u32.to_le_bytes()));
        reject(&|s| {
            let outside = encode_arm_branch(ENTRY_DELIVER, 0x0024_a274, false).unwrap();
            s[1].2[4..8].copy_from_slice(&outside);
        });
        reject(&|s| {
            let first = PAYLOAD_ADDRESS + ENTRY_COUNT * 4;
            let same = encode_arm_branch(ENTRY_DELIVER, first, false).unwrap();
            s[1].2[4..8].copy_from_slice(&same);
        });
        // Payload data: gap after the code, permissions, end off a page.
        reject(&|s| s[2].0 += 0x1000);
        reject(&|s| s[2].1 = 5);
        reject(&|s| s[2].3 = 0x4ffc);
    }

    #[test]
    fn edits_are_whole_words_disjoint_and_inside_the_original_text() {
        let image = inspect_image(&elf(&good())).unwrap();
        let mut list = edits(&image).unwrap();
        assert_eq!(list.len(), EDITS.len() + 1);
        list.sort_by_key(|edit| edit.offset);
        for pair in list.windows(2) {
            assert!(pair[0].offset + pair[0].replacement.len() <= pair[1].offset);
        }
        let text_end = (BOOTSTRAP_ADDRESS - CODE_BASE) as usize;
        for edit in &list[..EDITS.len()] {
            assert_eq!(edit.replacement.len(), 4);
            assert!(edit.offset % 4 == 0 && edit.offset < text_end);
            assert_ne!(edit.expected, edit.replacement);
        }
        assert_eq!(list[EDITS.len()].offset, text_end);
        assert!(list[EDITS.len()].expected.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn hook_calls_and_conditional_branches_are_encoded_as_intended() {
        let word = |address: u32, word: &Word| {
            u32::from_le_bytes(
                word_edit(address, 0, word)
                    .unwrap()
                    .replacement
                    .try_into()
                    .unwrap(),
            )
        };
        for (address, entry) in [
            (0x0024_8d40, ENTRY_CHECK),
            (0x0024_a154, ENTRY_DELIVER),
            (0x0024_58dc, ENTRY_SLOT),
            (0x0024_3528, ENTRY_CART_ID),
            (0x0024_39a8, ENTRY_CART_READ),
            (0x0024_37f4, ENTRY_CART_WRITE),
            (0x0024_397c, ENTRY_CART_WRITE),
            (0x0024_4f10, ENTRY_LIST_NEXT),
            (0x0024_4908, ENTRY_SELECT),
            (0x0025_c5fc, ENTRY_LANGUAGE_CHOSEN),
            (0x0022_bee0, ENTRY_LANGUAGE_ORDER),
            (0x0022_b5b0, ENTRY_LANGUAGE_BUTTONS),
            (0x0024_12d4, ENTRY_VC_SCAN),
            (0x0013_aee4, LAYOUT_CHECK),
            (0x0024_ba44, ENTRY_TITLE_BEGIN),
            (0x0024_ba74, ENTRY_TITLE_END),
            (STARTUP_CALL, BOOTSTRAP_ADDRESS),
        ] {
            let call = word(address, &BranchLink(entry));
            assert_eq!(branch_target(address, call, true), Some(entry));
        }
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
                word(0x0022_b3b8, &Branch(ENTRY_LANGUAGE_BACK)),
                false
            ),
            Some(ENTRY_LANGUAGE_BACK)
        );
        // The original start-up call that the hook replaces and then makes.
        assert_eq!(
            branch_target(STARTUP_CALL, 0xeb00_0228, true),
            Some(ORIGINAL_APP_INIT)
        );
        let session = word(0x0024_2d10, &BranchIf(ENTRY_SESSION, EQUAL));
        assert_eq!(session >> 24, 0x0a);
        assert_eq!(
            branch_target(0x0024_2d10, (session & 0x00ff_ffff) | 0xea00_0000, false),
            Some(ENTRY_SESSION)
        );
        // beq keeps the offset of the unconditional form.
        assert_eq!(word(0x0024_a15c, &Branch(0x0024_a274)), 0xea00_0044);
        assert_eq!(
            word(0x0024_a15c, &BranchIf(0x0024_a274, EQUAL)),
            0x0a00_0044
        );
        // The reference patch's own branches, for comparison with its source.
        assert_eq!(word(0x0024_8cdc, &Branch(0x0024_8d3c)), 0xea00_0016);
        assert_eq!(word(0x0024_8d58, &Branch(0x0024_8e44)), 0xea00_0039);
    }

    #[test]
    fn wrong_input_is_refused_before_anything_is_built() {
        assert!(prepare(&[0; 16], &[0; 0x800], &[]).is_err());
        assert!(prepare(&vec![0; CODE_LENGTH], &[0; 0x800], &[]).is_err());
    }
}
