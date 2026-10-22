//! The independent check of a built package. It shares no code with
//! `patch-builder` and depends on no payload crate: it parses the ELF, the
//! IPS and the manifest itself, applies code.ips to the original over added
//! memory that is not zero, rebuilds the expected image from its own
//! hard-coded list of edits and the ELF's loaded segments, and compares
//! every byte. `bank` and `transporter` each bring what only they know: the
//! program, the list of edits with their original words, the entry
//! functions, and the checks of their own.

mod bank;
pub mod elf;
mod ips;
pub mod json;
mod transporter;

use elf::{u32_at, Elf};
pub use json::Json;
use offline_core::Sha256;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

fn fail<T>(message: &str) -> Result<T> {
    Err(message.to_owned())
}

fn check(condition: bool, message: impl AsRef<str>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.as_ref().to_owned())
    }
}

/// Where every program's code lies.
const BASE: u32 = 0x0010_0000;

/// One original program, as the check knows it.
pub struct Program {
    pub name: &'static str,
    pub code_length: usize,
    /// Hex, as the manifest writes it.
    pub title_id: &'static str,
    pub tmd_version: u64,
    pub remaster: u64,
    pub code_hash: &'static str,
    pub exheader_hash: &'static str,
    /// The zero bytes after the original text that the start-up hook takes.
    pub bootstrap: u32,
    pub bootstrap_end: u32,
    /// The first page after the original zero-initialised data.
    pub payload: u32,
    pub app_init: u32,
    /// The one call of application init, which the start-up hook replaces.
    pub startup_call: u32,
}

/// Where an edited branch or pointer goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A function of the ELF, by name.
    Sym(&'static str),
    /// An address of the original.
    At(u32),
}

/// What an edited word becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Raw(u32),
    B(Target),
    Bl(Target),
    Ptr(Target),
}

/// (address, original word, replacement).
pub type Edit = (u32, u32, Word);

/// What one package must be.
pub struct Expectation {
    pub kind: &'static str,
    pub program: &'static Program,
    pub edits: Vec<Edit>,
    /// (address, word) of the original the patch relies on.
    pub relies: Vec<(u32, u32)>,
    /// (address, bytes) of the original that must stay as they are: no
    /// listed edit lies in them, and the patched image holds them unchanged.
    pub protected: Vec<(u32, u32)>,
    /// The ELF's entry.
    pub router: &'static str,
    /// How many entry functions the edits name.
    pub entries: usize,
    /// How many edits the manifest counts as main-menu edits.
    pub menu_edits: u64,
    /// Checks of its own, with the original, the patched image and the ELF.
    pub extra: fn(&Expectation, &[u8], &[u8], &Elf) -> Result<()>,
    /// What those checks are, for the report.
    pub checks: &'static [&'static str],
}

/// What the package of `kind` must be.
pub fn expectation(kind: &str) -> Result<Expectation> {
    if kind == "transporter" {
        return Ok(transporter::expectation());
    }
    ["bank", "test", "migration", "download"]
        .into_iter()
        .find(|&each| each == kind)
        .map(bank::expectation)
        .ok_or_else(|| format!("no verifier for package kind {kind:?}"))
}

/// The programs the check knows.
pub fn programs() -> [&'static Program; 2] {
    [&bank::PROGRAM, &transporter::PROGRAM]
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes).expect("in memory");
    hex(&hash.finalize())
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// The unpacked original of `program`: the folder under `extracted` whose
/// code.bin has the reviewed hash, whatever the folder is called.
pub fn find_original(extracted: &Path, program: &Program) -> Result<PathBuf> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(extracted)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    folders.sort();
    folders
        .into_iter()
        .find(|folder| {
            std::fs::read(folder.join("exefs/code.bin"))
                .is_ok_and(|code| sha256(&code) == program.code_hash)
        })
        .ok_or_else(|| {
            format!(
                "No extracted {} input with the reviewed code hash",
                program.name
            )
        })
}

/// What a check reads.
pub struct Paths<'a> {
    /// The package folder: code.ips, exheader.bin, manifest.json, PACKAGE.txt.
    pub package: &'a Path,
    pub elf: &'a Path,
    /// The unpacked original (`find_original`).
    pub original: &'a Path,
}

/// The package's name, as PACKAGE.txt holds it.
pub fn package_name(package: &Path) -> Option<String> {
    let text = std::fs::read_to_string(package.join("PACKAGE.txt")).ok()?;
    Some(text.trim().trim_start_matches('\u{feff}').to_owned())
}

/// Checks the package and returns its report. `name` is the name the
/// package must have: the one of the current payload and profile.
pub fn verify(expect: &Expectation, paths: &Paths, name: &str) -> Result<Json> {
    run(expect, paths, name).map_err(|message| format!("Verification failed: {message}"))
}

/// The report of a check that failed.
pub fn failure_report(package: &str, reason: &str) -> Json {
    Json::object([
        ("status", Json::string("failed")),
        ("package", Json::string(package)),
        ("reason", Json::string(reason)),
    ])
}

/// A u32 of `bytes` at a virtual address.
fn word(bytes: &[u8], address: u32) -> Result<u32> {
    u32_at(bytes, address.wrapping_sub(BASE) as usize)
}

fn slice(bytes: &[u8], start: u64, length: u64) -> Result<&[u8]> {
    let start = usize::try_from(start).map_err(|_| "range out of bounds")?;
    let length = usize::try_from(length).map_err(|_| "range out of bounds")?;
    start
        .checked_add(length)
        .and_then(|end| bytes.get(start..end))
        .ok_or_else(|| "range out of bounds".to_owned())
}

fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b == 0)
}

/// The target of a B or BL at `address`.
fn branch_target(address: u32, word: u32) -> i64 {
    let mut offset = (word & 0x00ff_ffff) as i64;
    if offset & 0x0080_0000 != 0 {
        offset -= 0x0100_0000;
    }
    address as i64 + 8 + offset * 4
}

/// What the manifest holds at `path`.
fn field<'a>(manifest: &'a Json, path: &[&str]) -> Option<&'a Json> {
    path.iter().try_fold(manifest, |json, key| json.get(key))
}
fn number(manifest: &Json, path: &[&str]) -> Option<u64> {
    field(manifest, path).and_then(Json::as_u64)
}
fn text<'a>(manifest: &'a Json, path: &[&str]) -> Option<&'a str> {
    field(manifest, path).and_then(Json::as_str)
}

const WRAPPERS: [&str; 5] = [
    "patch_svc_close_handle",
    "patch_svc_wait_thread",
    "patch_svc_get_resource_limit",
    "patch_svc_get_resource_limit_values",
    "patch_svc_get_resource_current_values",
];

const CHECKS: [&str; 11] = [
    "original inputs identified by hash",
    "every listed edit holds its original word before and its replacement after, branch and pointer targets by their ELF symbols",
    "all IPS records bounded and disjoint, none outside the listed edits and the start-up hook",
    "added memory fully written over a nonzero fill",
    "the expanded image equals the independently built one, byte for byte",
    "no listed edit lies in a protected place, and every protected place is unchanged",
    "the start-up hook replaces only zero bytes and calls the original application init first",
    "every entry function and system-call wrapper is a global ARM function of the start-up hook or the payload code",
    "former zero-initialised data and every zero-initialised ELF byte are zero",
    "only paired exheader fields 0x34/0x38/0x3c changed, and they cover the expanded image",
    "manifest hashes, exports, placement and status match",
];

/// The address of a global ARM function of the start-up hook or the
/// payload code.
fn function(elf: &Elf, name: &str, what: &str) -> Result<u32> {
    let Some(symbol) = elf.symbols.get(name) else {
        return fail(&format!("Missing ELF symbol {name}"));
    };
    let within = |load: &elf::Load| {
        symbol.value >= load.address
            && symbol.value as u64 + symbol.size as u64
                <= load.address as u64 + load.file_bytes as u64
    };
    check(
        symbol.info == 0x12
            && symbol.value % 4 == 0
            && symbol.size > 0
            && (within(elf.boot()) || within(elf.rx())),
        format!("{what} {name} is a global ARM function of the start-up hook or the payload code"),
    )?;
    Ok(symbol.value)
}

fn run(expect: &Expectation, paths: &Paths, expected_name: &str) -> Result<Json> {
    let program = expect.program;
    let package_name = package_name(paths.package).ok_or("cannot read PACKAGE.txt")?;
    let manifest = std::fs::read_to_string(paths.package.join("manifest.json"))
        .map_err(|e| format!("cannot read manifest.json: {e}"))?;
    let manifest = Json::parse(manifest.trim_start_matches('\u{feff}'))?;
    let original = read(&paths.original.join("exefs/code.bin"))?;
    let original_header = read(&paths.original.join("exheader.bin"))?;
    let elf_bytes = read(paths.elf)?;
    let ips = read(&paths.package.join("code.ips"))?;
    let header = read(&paths.package.join("exheader.bin"))?;

    // The inputs, the manifest and the package name.
    let code_hash = sha256(&original);
    let header_hash = sha256(&original_header);
    let elf_hash = sha256(&elf_bytes);
    check(
        code_hash == program.code_hash && original.len() == program.code_length,
        "Original code",
    )?;
    check(
        header_hash == program.exheader_hash && original_header.len() == 0x800,
        "Original exheader",
    )?;
    check(
        text(&manifest, &["source_code_sha256"]) == Some(&code_hash)
            && text(&manifest, &["source_exheader_sha256"]) == Some(&header_hash),
        "Manifest input hashes",
    )?;
    check(
        text(&manifest, &["source_elf_sha256"]) == Some(&elf_hash),
        "Manifest ELF hash",
    )?;
    check(
        package_name == expected_name,
        "Package is not the one of the current payload and profile",
    )?;
    check(
        text(&manifest, &["patch"]) == Some(expect.kind),
        "Manifest names another patch",
    )?;
    check(
        text(&manifest, &["title_id"]) == Some(program.title_id)
            && number(&manifest, &["tmd_version"]) == Some(program.tmd_version)
            && number(&manifest, &["remaster_version"]) == Some(program.remaster),
        "Manifest version",
    )?;
    for (name, bytes) in [("code.ips", &ips), ("exheader.bin", &header)] {
        check(
            text(&manifest, &["artifacts", name, "sha256"]) == Some(&sha256(bytes))
                && number(&manifest, &["artifacts", name, "bytes"]) == Some(bytes.len() as u64),
            format!("Artifact {name}"),
        )?;
    }
    check(
        field(&manifest, &["paired_files_required"]).and_then(Json::as_bool) == Some(true)
            && field(&manifest, &["hardware_tested_by_builder"]).and_then(Json::as_bool)
                == Some(false),
        "Manifest validation status",
    )?;
    for &(address, value) in &expect.relies {
        check(
            word(&original, address).ok() == Some(value),
            format!("Original word {address:08x}"),
        )?;
    }

    // The linked payload: where its segments lie, its entry functions, the
    // start-up hook and the kernel-call wrappers.
    let elf = elf::read(&elf_bytes)?;
    let (boot, rx, rw) = (*elf.boot(), *elf.rx(), *elf.rw());
    check(
        boot.address == program.bootstrap
            && boot.flags == 5
            && boot.memory_bytes == boot.file_bytes
            && boot.address as u64 + boot.memory_bytes as u64 <= program.bootstrap_end as u64,
        "Start-up hook LOAD",
    )?;
    check(
        rx.address == program.payload
            && rx.flags == 5
            && rx.memory_bytes == rx.file_bytes
            && rx.memory_bytes % 0x1000 == 0,
        "Payload code LOAD",
    )?;
    check(
        rw.address as u64 == rx.address as u64 + rx.memory_bytes as u64
            && rw.flags == 6
            && rw.memory_bytes % 0x1000 == 0,
        "Payload data LOAD",
    )?;
    let image_end = rw.address as u64 + rw.memory_bytes as u64;
    let expanded_size = image_end - BASE as u64;
    check(
        number(&manifest, &["expanded_code_bytes"]) == Some(expanded_size)
            && image_end < 0x0800_0000,
        "Expanded allocation",
    )?;
    let expanded_size = expanded_size as usize;
    for name in WRAPPERS {
        function(&elf, name, "System-call wrapper")?;
    }
    let mut names: Vec<&str> = expect
        .edits
        .iter()
        .filter_map(|(_, _, word)| match word {
            Word::B(Target::Sym(name))
            | Word::Bl(Target::Sym(name))
            | Word::Ptr(Target::Sym(name)) => Some(*name),
            _ => None,
        })
        .filter(|&name| name != "patch_bootstrap_startup")
        .collect();
    names.sort_unstable();
    names.dedup();
    check(
        names.len() == expect.entries
            && number(&manifest, &["runtime_exports"]) == Some(expect.entries as u64)
            && field(&manifest, &["exports"]).map(|e| e.members().len()) == Some(expect.entries),
        "Entry function count",
    )?;
    for name in &names {
        let address = function(&elf, name, "Entry")?;
        check(
            number(&manifest, &["exports", name]) == Some(address as u64),
            format!("Manifest export {name}"),
        )?;
    }
    check(
        elf.symbols.get("patch_bootstrap_startup").map(|s| s.value) == Some(boot.address)
            && elf.symbols.get(expect.router).map(|s| s.value) == Some(elf.entry),
        "ELF entry points",
    )?;
    for (bound, value) in [
        ("__patch_payload_start", rx.address as u64),
        ("__patch_payload_rx_end", rw.address as u64),
        ("__patch_payload_rx_size", rx.memory_bytes as u64),
        ("__patch_payload_end", image_end),
        ("__patch_original_app_init", program.app_init as u64),
    ] {
        check(
            elf.symbols.get(bound).map(|s| s.value as u64) == Some(value),
            format!("ELF bound {bound}"),
        )?;
    }
    // The start-up hook replaces the one call of application init and makes
    // it first: push {r4, lr}; bl <application init>.
    let original_call = word(&original, program.startup_call)?;
    check(
        original_call >> 24 == 0xeb
            && branch_target(program.startup_call, original_call) == program.app_init as i64,
        "Original start-up call",
    )?;
    check(
        u32_at(&elf_bytes, boot.offset as usize)? == 0xe92d_4010,
        "Start-up hook begins with push {r4, lr}",
    )?;
    let second = u32_at(&elf_bytes, boot.offset as usize + 4)?;
    check(
        second >> 24 == 0xeb && branch_target(boot.address + 4, second) == program.app_init as i64,
        "Start-up hook calls the original application init first",
    )?;

    // The image as it must be: the original with every listed edit, the
    // start-up hook in the zero bytes after the original text, the former
    // zero-initialised data zero, and the payload's segments.
    check(expanded_size >= original.len(), "Expanded allocation")?;
    let mut expected = vec![0; expanded_size];
    expected[..original.len()].copy_from_slice(&original);
    let mut allowed = vec![false; original.len()];
    for &(address, value, replacement) in &expect.edits {
        let offset = address.wrapping_sub(BASE) as usize;
        check(
            word(&original, address).ok() == Some(value),
            format!("Original word at {address:08x}"),
        )?;
        let target = |target: Target| match target {
            Target::At(address) => Ok(address),
            Target::Sym(name) => function(&elf, name, "Target"),
        };
        let new = match replacement {
            Word::Raw(value) => value,
            Word::Ptr(to) => target(to)?,
            Word::B(to) | Word::Bl(to) => {
                let delta = target(to)? as i64 - address as i64 - 8;
                check(
                    delta % 4 == 0 && (-0x0200_0000..0x0200_0000).contains(&delta),
                    "ARM branch reach",
                )?;
                let opcode = if matches!(replacement, Word::B(_)) {
                    0xea00_0000
                } else {
                    0xeb00_0000
                };
                opcode | ((delta >> 2) as u32 & 0x00ff_ffff)
            }
        };
        check(
            !allowed[offset..offset + 4].contains(&true),
            format!("Edit listed twice at {address:08x}"),
        )?;
        expected[offset..offset + 4].copy_from_slice(&new.to_le_bytes());
        allowed[offset..offset + 4].fill(true);
    }
    let boot_offset = (boot.address - BASE) as u64;
    check(
        zero(slice(
            &original,
            boot_offset,
            (program.bootstrap_end - boot.address) as u64,
        )?),
        "Bytes after the original text are zero",
    )?;
    allowed[boot_offset as usize..boot_offset as usize + boot.file_bytes as usize].fill(true);
    for load in &elf.loads {
        let bytes = slice(&elf_bytes, load.offset as u64, load.file_bytes as u64)?;
        let at = (load.address - BASE) as usize;
        expected
            .get_mut(at..at + bytes.len())
            .ok_or("ELF LOAD outside the expanded image")?
            .copy_from_slice(bytes);
    }
    let expansion = ips::expand(&ips, &original, expanded_size, &allowed)?;
    let patched = expansion.patched;
    let patched_hash = sha256(&patched);
    check(
        patched == expected && text(&manifest, &["expanded_code_sha256"]) == Some(&patched_hash),
        "Expanded image differs from independent expected image",
    )?;
    for load in &elf.loads {
        let zeros = load.memory_bytes - load.file_bytes;
        if zeros > 0 {
            check(
                zero(slice(
                    &patched,
                    (load.address - BASE) as u64 + load.file_bytes as u64,
                    zeros as u64,
                )?),
                "ELF zero-initialised bytes",
            )?;
        }
    }
    for &(start, length) in &expect.protected {
        for &(address, _, _) in &expect.edits {
            check(
                !(start..start + length).contains(&address),
                format!("Edit listed in a protected place {address:08x}"),
            )?;
        }
        let offset = (start - BASE) as u64;
        check(
            slice(&patched, offset, length as u64)? == slice(&original, offset, length as u64)?,
            format!("Protected original changed at {start:08x}"),
        )?;
    }
    check(
        number(&manifest, &["native_edits"]) == Some(expect.edits.len() as u64 + 1)
            && number(&manifest, &["main_menu_edits"]) == Some(expect.menu_edits),
        "Native region count",
    )?;

    // The manifest's placement and payload hash.
    let payload_image = rw.address as u64 + rw.file_bytes as u64 - rx.address as u64;
    check(
        text(&manifest, &["payload_image_sha256"])
            == Some(&sha256(slice(
                &patched,
                (rx.address - BASE) as u64,
                payload_image,
            )?)),
        "Payload image hash",
    )?;
    let placement = |key: &str| number(&manifest, &["placement", key]);
    check(
        placement("payload_address") == Some(rx.address as u64)
            && placement("payload_file_offset") == Some((rx.address - BASE) as u64)
            && placement("payload_memory_bytes") == Some(image_end - rx.address as u64)
            && placement("payload_rx_bytes") == Some(rx.memory_bytes as u64)
            && placement("payload_rw_address") == Some(rw.address as u64)
            && placement("payload_rw_bytes") == Some(rw.memory_bytes as u64)
            && placement("bootstrap_address") == Some(boot.address as u64)
            && placement("bootstrap_bytes") == Some(boot.file_bytes as u64),
        "Manifest placement",
    )?;

    // The paired exheader: only the data page count, its size and the BSS
    // size, so that the data segment covers the expanded image. The payload
    // follows the original zero-initialised data, which the IPS writes as
    // zero.
    let data_address = u32_at(&original_header, 0x30)? as u64;
    let data_bytes = image_end - data_address;
    let mut expected_header = original_header.clone();
    expected_header[0x34..0x38].copy_from_slice(&((data_bytes / 0x1000) as u32).to_le_bytes());
    expected_header[0x38..0x3c].copy_from_slice(&(data_bytes as u32).to_le_bytes());
    expected_header[0x3c..0x40].fill(0);
    check(header == expected_header, "Unexpected paired-exheader edit")?;
    let pages = u32_at(&header, 0x14)? as u64
        + u32_at(&header, 0x24)? as u64
        + u32_at(&header, 0x34)? as u64;
    check(
        pages * 0x1000 == expanded_size as u64,
        "Paired allocation length",
    )?;
    let bss_start = data_address + u32_at(&original_header, 0x38)? as u64;
    let bss_end = bss_start + u32_at(&original_header, 0x3c)? as u64;
    check(
        (bss_end + 0xfff) & !0xfff == rx.address as u64,
        "Payload follows the original zero-initialised data",
    )?;
    check(
        zero(slice(
            &patched,
            bss_start - BASE as u64,
            rx.address as u64 - bss_start,
        )?),
        "Former zero-initialised data is zero",
    )?;

    (expect.extra)(expect, &original, &patched, &elf)?;

    let loads = elf
        .loads
        .iter()
        .map(|load| {
            Json::object([
                ("offset", Json::number(load.offset.into())),
                ("address", Json::number(load.address.into())),
                ("file_bytes", Json::number(load.file_bytes.into())),
                ("memory_bytes", Json::number(load.memory_bytes.into())),
                ("flags", Json::number(load.flags.into())),
                ("alignment", Json::number(load.alignment.into())),
            ])
        })
        .collect();
    let checks = CHECKS
        .iter()
        .chain(expect.checks)
        .map(|&c| Json::string(c))
        .collect();
    Ok(Json::object([
        ("status", Json::string("passed")),
        ("package", Json::string(package_name)),
        ("patch", Json::string(expect.kind)),
        ("source_code_sha256", Json::string(code_hash)),
        ("source_exheader_sha256", Json::string(header_hash)),
        ("source_elf_sha256", Json::string(elf_hash)),
        ("ips_sha256", Json::string(sha256(&ips))),
        ("paired_exheader_sha256", Json::string(sha256(&header))),
        ("ips_records", Json::number(expansion.records as u64)),
        (
            "native_regions",
            Json::number(expect.edits.len() as u64 + 1),
        ),
        ("runtime_exports", Json::number(names.len() as u64)),
        ("system_call_wrappers", Json::number(WRAPPERS.len() as u64)),
        ("expanded_code_bytes", Json::number(expanded_size as u64)),
        ("expanded_code_sha256", Json::string(patched_hash)),
        (
            "initial_extension_fill",
            Json::string(format!("{:02x}", ips::FILL)),
        ),
        ("elf_loads", Json::Array(loads)),
        ("checks", Json::Array(checks)),
        (
            "limitations",
            Json::Array(vec![Json::string(
                "static artifact verification; hardware behavior not established",
            )]),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_are_read_both_ways() {
        assert_eq!(branch_target(0x0010_40a4, 0xeb00_0228), 0x0010_494c);
        assert_eq!(branch_target(0x0010_0010, 0xebff_fffe), 0x0010_0010);
    }

    #[test]
    fn every_list_is_consistent() {
        for kind in ["bank", "transporter", "test", "migration", "download"] {
            let expect = expectation(kind).unwrap();
            let mut addresses: Vec<u32> = expect.edits.iter().map(|e| e.0).collect();
            addresses.sort_unstable();
            addresses.dedup();
            assert_eq!(
                addresses.len(),
                expect.edits.len(),
                "{kind}: an edit listed twice"
            );
            for &(start, length) in &expect.protected {
                assert!(
                    !addresses
                        .iter()
                        .any(|a| (start..start + length).contains(a)),
                    "{kind}: an edit in a protected place"
                );
            }
            assert_eq!(expect.kind, kind);
        }
        assert!(expectation("other").is_err());
    }
}
