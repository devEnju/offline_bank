//! Writes a checked package into a fresh directory: the paired `code.ips`
//! and `exheader.bin`, `report.md`, and `manifest.json`, the same for every
//! kind.
//!
//! The manifest is written last. A failed write leaves an incomplete directory
//! for inspection; the builder never replaces or deletes existing output.

use crate::{hex, prepare_patch, sha256, Error, PreparedPatch, Profile, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Artifacts {
    pub directory: PathBuf,
    pub ips_sha256: [u8; 32],
    pub exheader_sha256: [u8; 32],
    pub expanded_code_sha256: [u8; 32],
}

/// Checks everything (`prepare_patch`) before writing anything. The parent of
/// `output` must exist; `output` itself must not, not even as a link. No
/// console, save or SD card is touched.
pub fn build(
    profile: &Profile,
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
    output: &Path,
) -> Result<Artifacts> {
    let program = profile.program;
    let PreparedPatch {
        placement: prepared,
        payload: parsed,
    } = prepare_patch(profile, code, exheader, elf)?;
    let exports = parsed
        .entries
        .iter()
        .map(|(name, address)| format!("    \"{name}\": {address}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let layout = prepared.layout();
    let ips_hash = sha256(prepared.ips());
    let exheader_hash = sha256(prepared.exheader());
    let expanded_hash = prepared.expanded_code_sha256();
    let manifest = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 2,\n",
            "  \"program\": \"{}\",\n",
            "  \"patch\": \"{}\",\n",
            "  \"title_id\": \"{:016x}\",\n",
            "  \"tmd_version\": {},\n",
            "  \"remaster_version\": {},\n",
            "  \"source_code_sha256\": \"{}\",\n",
            "  \"source_exheader_sha256\": \"{}\",\n",
            "  \"source_elf_sha256\": \"{}\",\n",
            "  \"payload_image_sha256\": \"{}\",\n",
            "  \"expanded_code_sha256\": \"{}\",\n",
            "  \"expanded_code_bytes\": {},\n",
            "  \"artifacts\": {{\n",
            "    \"code.ips\": {{ \"sha256\": \"{}\", \"bytes\": {} }},\n",
            "    \"exheader.bin\": {{ \"sha256\": \"{}\", \"bytes\": {} }}\n",
            "  }},\n",
            "  \"placement\": {{\n",
            "    \"payload_address\": {},\n",
            "    \"payload_file_offset\": {},\n",
            "    \"payload_memory_bytes\": {},\n",
            "    \"payload_rx_bytes\": {},\n",
            "    \"payload_rw_address\": {},\n",
            "    \"payload_rw_bytes\": {},\n",
            "    \"bootstrap_address\": {},\n",
            "    \"bootstrap_bytes\": {}\n",
            "  }},\n",
            "  \"exports\": {{\n",
            "{}\n",
            "  }},\n",
            "  \"runtime_exports\": {},\n",
            "  \"native_edits\": {},\n",
            "  \"main_menu_edits\": {},\n",
            "  \"paired_files_required\": true,\n",
            "  \"hardware_tested_by_builder\": false\n",
            "}}\n"
        ),
        program.name,
        profile.name,
        program.title_id,
        program.tmd_version,
        program.remaster_version,
        hex(&program.identity.code_sha256),
        hex(&program.identity.exheader_sha256),
        hex(&parsed.elf_sha256),
        hex(&prepared.payload_sha256()),
        hex(&expanded_hash),
        layout.expanded_code_size,
        hex(&ips_hash),
        prepared.ips().len(),
        hex(&exheader_hash),
        prepared.exheader().len(),
        layout.payload_address,
        layout.payload_file_offset,
        layout.memory_size,
        layout.executable_size,
        layout.writable_address,
        layout.writable_size,
        parsed.bootstrap_entry,
        parsed.bootstrap_bytes().len(),
        exports,
        parsed.entries.len(),
        profile.regions(),
        profile.menu_edits.len(),
    );
    let report = format!(
        concat!(
            "# {}: {}\n\n",
            "This package passed the checks of its input files, of every edited ",
            "word of the original, of the linked payload, and of the paired ",
            "placement. How it behaves is shown only on a console.\n\n",
            "Install both files, never one without the other:\n\n",
            "```text\n",
            "SD:/luma/titles/{:016X}/code.ips\n",
            "SD:/luma/titles/{:016X}/exheader.bin\n",
            "```\n\n",
            "- Title {:016X}, TMD version {}, remaster {}.\n",
            "- ELF SHA-256: `{}`\n",
            "- code.ips SHA-256: `{}`\n",
            "- exheader.bin SHA-256: `{}`\n",
            "- Expanded code SHA-256: `{}`, {} bytes.\n",
            "- Start-up hook: {} bytes at `{:08X}`.\n",
            "- Payload at `{:08X}`: {} bytes of code, {} bytes in all.\n",
            "- {} entry functions; {} edited regions of the original.\n\n",
            "The manifest is written after both files and records their hashes. ",
            "The original files are not included.\n"
        ),
        program.label,
        profile.name,
        program.title_id,
        program.title_id,
        program.title_id,
        program.tmd_version,
        program.remaster_version,
        hex(&parsed.elf_sha256),
        hex(&ips_hash),
        hex(&exheader_hash),
        hex(&expanded_hash),
        layout.expanded_code_size,
        parsed.bootstrap_bytes().len(),
        parsed.bootstrap_entry,
        layout.payload_address,
        layout.executable_size,
        layout.memory_size,
        parsed.entries.len(),
        profile.regions(),
    );
    write_artifacts(
        output,
        &[
            ("code.ips", prepared.ips()),
            ("exheader.bin", prepared.exheader()),
            ("report.md", report.as_bytes()),
            ("manifest.json", manifest.as_bytes()),
        ],
    )?;
    Ok(Artifacts {
        directory: output.to_path_buf(),
        ips_sha256: ips_hash,
        exheader_sha256: exheader_hash,
        expanded_code_sha256: expanded_hash,
    })
}

/// Writes `files` into `output`, a new folder or an empty one: existing
/// output is never replaced. An empty folder is kept, so that a window open
/// on it stays open across builds.
fn write_artifacts(output: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    let empty = || fs::read_dir(output).is_ok_and(|mut entries| entries.next().is_none());
    if let Err(error) = fs::create_dir(output) {
        if error.kind() != std::io::ErrorKind::AlreadyExists || !empty() {
            return Err(Error(format!(
                "cannot create fresh output directory {}: {error}; existing output is never replaced",
                output.display()
            )));
        }
    }
    for (name, contents) in files {
        write_new_file(&output.join(name), contents).map_err(|error| {
            Error(format!(
                "{error}; incomplete output remains at {} and must not be used",
                output.display()
            ))
        })?;
    }
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| Error(format!("cannot create {}: {error}", path.display())))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| Error(format!("cannot write/sync {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "patch-builder-output-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            // Only this test-owned fresh directory is removed.
            let resolved = self.0.canonicalize().unwrap();
            let temporary_root = std::env::temp_dir().canonicalize().unwrap();
            assert_eq!(resolved.parent(), Some(temporary_root.as_path()));
            fs::remove_dir_all(resolved).unwrap();
        }
    }

    #[test]
    fn artifact_writer_refuses_existing_directory_and_files() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("code.ips");
        fs::write(&path, b"original").unwrap();
        assert!(write_artifacts(&directory, &[("code.ips", b"changed")]).is_err());
        assert!(write_new_file(&path, b"changed").is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
        assert!(!directory.join("manifest.json").exists());
    }

    #[test]
    fn the_writer_uses_an_empty_existing_directory() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        fs::create_dir(&directory).unwrap();
        write_artifacts(&directory, &[("code.ips", b"PATCH")]).unwrap();
        assert_eq!(fs::read(directory.join("code.ips")).unwrap(), b"PATCH");
        // A file there is not.
        let file = temp.0.join("file");
        fs::write(&file, b"original").unwrap();
        assert!(write_artifacts(&file, &[("code.ips", b"PATCH")]).is_err());
        assert_eq!(fs::read(file).unwrap(), b"original");
    }

    #[test]
    fn the_writer_stops_at_a_failed_file_and_keeps_what_it_wrote() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        // Duplicate name deliberately simulates a collision after one write.
        assert!(write_artifacts(
            &directory,
            &[
                ("code.ips", b"first"),
                ("code.ips", b"second"),
                ("manifest.json", b"completed"),
            ],
        )
        .is_err());
        assert_eq!(fs::read(directory.join("code.ips")).unwrap(), b"first");
        assert!(!directory.join("manifest.json").exists());
    }

    #[test]
    fn the_writer_writes_every_file_it_is_given_with_its_bytes() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        let files: [(&str, &[u8]); 4] = [
            ("code.ips", b"PATCH"),
            ("exheader.bin", b"paired-header"),
            ("report.md", b"report"),
            ("manifest.json", b"complete"),
        ];
        write_artifacts(&directory, &files).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 4);
        for (name, bytes) in files {
            assert_eq!(fs::read(directory.join(name)).unwrap(), bytes);
        }
    }

    #[test]
    fn rejected_inputs_create_no_output() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        for profile in crate::profile::PROFILES {
            assert!(build(profile, &[], &[], &[], &directory).is_err());
            assert!(!directory.exists());
        }
    }
}
