//! The original code.bin and exheader.bin of a program, unpacked from its
//! CIA with ctrtool. The CIA is never modified: every extraction has its own
//! folder, `build/extracted/<first 16 hex digits of the CIA's SHA-256>/`,
//! with `source.json` beside the files.

use crate::{process, read, write, Result};
use patch_verify::Json;
use std::path::{Path, PathBuf};

pub struct Extracted {
    pub code: PathBuf,
    pub exheader: PathBuf,
    pub record: Json,
}

/// ctrtool in `tools/`, as the user puts it there.
pub fn default_ctrtool() -> String {
    format!("tools/ctrtool{}", std::env::consts::EXE_SUFFIX)
}

/// A path the user gives: as it is if absolute, else in the project folder.
/// It must exist.
pub fn project_file(value: &str) -> Result<PathBuf> {
    let path = crate::project_path(value);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("missing file: {}", path.display()))
    }
}

fn hash(path: &Path) -> Result<String> {
    Ok(patch_builder::hex(&patch_builder::sha256(&read(path)?)))
}

pub fn extract(cia: &Path, ctrtool: &Path) -> Result<Extracted> {
    let cia_hash = hash(cia)?;
    let output = crate::project_path("build/extracted").join(&cia_hash[..16]);
    std::fs::create_dir_all(&output)
        .map_err(|e| format!("cannot create {}: {e}", output.display()))?;
    let contents = output.join("content");
    process::run(
        "CIA extraction",
        process::command(&ctrtool.to_string_lossy())
            .arg("-q")
            .arg(arg("--contents=", &contents))
            .arg(cia),
    )?;
    let main: Vec<PathBuf> = std::fs::read_dir(&output)
        .map_err(|e| format!("cannot list {}: {e}", output.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("content.0000."))
        })
        .collect();
    let [main] = main.as_slice() else {
        return Err("Expected exactly one main NCCH content at index 0.".into());
    };
    let exheader = output.join("exheader.bin");
    let exefs = output.join("exefs");
    process::run(
        "NCCH extraction",
        process::command(&ctrtool.to_string_lossy())
            .arg("-q")
            .arg(arg("--exheader=", &exheader))
            .arg(arg("--exefsdir=", &exefs))
            .arg(arg("--romfs=", &output.join("romfs.bin")))
            .arg(main),
    )?;
    let code = exefs.join("code.bin");
    let path = |p: &Path| Json::string(p.to_string_lossy());
    let record = Json::object([
        ("schema_version", Json::number(1)),
        ("source", path(cia)),
        ("cia_sha256", Json::string(cia_hash)),
        ("code", path(&code)),
        ("code_sha256", Json::string(hash(&code)?)),
        ("exheader", path(&exheader)),
        ("exheader_sha256", Json::string(hash(&exheader)?)),
        ("main_content", path(main)),
        (
            "note",
            Json::string("Extraction metadata only; not proof of patch compatibility."),
        ),
    ]);
    write(&output.join("source.json"), record.to_pretty().as_bytes())?;
    Ok(Extracted {
        code,
        exheader,
        record,
    })
}

fn arg(option: &str, path: &Path) -> String {
    format!("{option}{}", path.display())
}
