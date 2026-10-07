//! Fresh-directory emission of a fully checked development artifact pair.
//!
//! The manifest is written last. A failed write leaves an incomplete directory
//! for inspection; the builder never replaces or deletes existing output.

use crate::{
    bank15, elf::inspect_payload_elf, hex, prepare_development_patch, sha256, Error, Result,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DevelopmentArtifacts {
    pub directory: PathBuf,
    pub elf_sha256: [u8; 32],
    pub ips_sha256: [u8; 32],
    pub exheader_sha256: [u8; 32],
    pub expanded_code_sha256: [u8; 32],
    pub expanded_code_size: usize,
}

/// Validates the full reviewed profile before writing anything. The parent of
/// output must exist; output itself must not exist, including as a symlink.
/// No console, save file, or SD-card deployment is accessed.
pub fn build_development_patch(
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
    output: &Path,
) -> Result<DevelopmentArtifacts> {
    let prepared = prepare_development_patch(code, exheader, elf)?;
    let parsed = inspect_payload_elf(elf)?;
    let layout = prepared.layout();
    let ips_hash = sha256(prepared.ips());
    let exheader_hash = sha256(prepared.exheader());
    let expanded_hash = prepared.expanded_code_sha256();
    let manifest = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"profile\": \"bank15-title6272-remaster6\",\n",
            "  \"status\": \"development-console-validation-required\",\n",
            "  \"title_id\": \"00040000000c9b00\",\n",
            "  \"tmd_version\": 6272,\n",
            "  \"remaster_version\": 6,\n",
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
            "    \"bank_offline_next\": {},\n",
            "    \"bank_offline_load\": {},\n",
            "    \"bank_offline_save\": {},\n",
            "    \"bank_offline_validate_game\": {},\n",
            "    \"bank_offline_rewards\": {},\n",
            "    \"bank_offline_timestamp\": {},\n",
            "    \"bank_offline_dex_save_request\": {},\n",
            "    \"bank_offline_dex_records_update\": {},\n",
            "    \"bank_offline_dex_records_finish\": {}\n",
            "  }},\n",
            "  \"runtime_exports\": 9,\n",
            "  \"native_edits\": {},\n",
            "  \"main_menu_edits\": 0,\n",
            "  \"paired_files_required\": true,\n",
            "  \"hardware_tested_by_builder\": false\n",
            "}}\n"
        ),
        hex(&bank15::CODE_SHA256),
        hex(&bank15::EXHEADER_SHA256),
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
        parsed.entry,
        parsed.load_entry,
        parsed.save_entry,
        parsed.validate_game_entry,
        parsed.rewards_entry,
        parsed.timestamp_entry,
        parsed.dex_save_request_entry,
        parsed.dex_records_update_entry,
        parsed.dex_records_finish_entry,
        bank15::NATIVE_EDIT_REGIONS,
    );
    let report = format!(
        concat!(
            "# Pokemon Bank development patch\n\n",
            "This build passed the exact input fingerprints, original hook bytes, ",
            "ARM ELF structure, and paired allocation checks. Runtime behavior and ",
            "power-loss recovery still require testing on a 3DS.\n\n",
            "- Title: 00040000000c9b00; TMD version 6272; remaster 6.\n",
            "- ELF SHA-256: {}.\n",
            "- IPS SHA-256: {}.\n",
            "- Exheader SHA-256: {}.\n",
            "- Expanded code SHA-256: {}; {} bytes.\n",
            "- Payload address: 0x{:08x}; RX 0x{:x} bytes; total 0x{:x} bytes.\n",
            "- Nine runtime exports; {} native edit regions; the five earlier ",
            "main-menu locations keep their original bytes.\n\n",
            "code.ips and exheader.bin must be kept together. The manifest was ",
            "written after both files and records their hashes. The build command ",
            "does not deploy the files or access game saves.\n\n",
            "The original CIA and game data are not included. See the project's ",
            "docs/bank.md for the original/replacement comparison, the ",
            "checklist, and the limits.\n"
        ),
        hex(&parsed.elf_sha256),
        hex(&ips_hash),
        hex(&exheader_hash),
        hex(&expanded_hash),
        layout.expanded_code_size,
        layout.payload_address,
        layout.executable_size,
        layout.memory_size,
        bank15::NATIVE_EDIT_REGIONS,
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
    Ok(DevelopmentArtifacts {
        directory: output.to_path_buf(),
        elf_sha256: parsed.elf_sha256,
        ips_sha256: ips_hash,
        exheader_sha256: exheader_hash,
        expanded_code_sha256: expanded_hash,
        expanded_code_size: layout.expanded_code_size,
    })
}

fn write_artifacts(output: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    fs::create_dir(output).map_err(|error| {
        Error(format!(
            "cannot create fresh output directory {}: {error}; existing output is never replaced",
            output.display()
        ))
    })?;
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
    fn manifest_is_last_and_failure_keeps_incomplete_output() {
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
    fn successful_output_contains_exact_pair_and_completion_marker() {
        let temp = Temp::new();
        let directory = temp.0.join("output");
        let files: [(&str, &[u8]); 4] = [
            ("code.ips", b"PATCH"),
            ("exheader.bin", b"paired-header"),
            ("report.md", b"development"),
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
        assert!(build_development_patch(&[], &[], &[], &directory).is_err());
        assert!(!directory.exists());
    }
}
