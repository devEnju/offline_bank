//! Tests the verifier on the built packages: each passes, and a copy with
//! one thing changed is refused for that thing. Where a change touches
//! bytes, the copy's manifest gets the new hashes, so that the refusal comes
//! from the independent comparison and not from a hash. The changes are made
//! to Bank's and Transporter's packages, which must be built; of the
//! others, each one built is checked unchanged. Works on copies in a
//! temporary folder, which it deletes.

use crate::{
    packages::{package, Package, PACKAGES},
    read, root,
    verify::{verify, Places},
    write, Result,
};
use patch_verify::{sha256, Json};
use std::path::{Path, PathBuf};

/// The IPS records: offset, length and where the data begins in the file.
/// The builder writes no RLE record, so every record carries its data.
fn ips_records(ips: &[u8]) -> Result<Vec<(usize, usize, usize)>> {
    let mut records = Vec::new();
    let mut at = 5;
    while ips.get(at..at + 3).ok_or("IPS ends early")? != b"EOF" {
        let byte = |i: usize| ips[at + i] as usize;
        let (offset, length) = (
            byte(0) << 16 | byte(1) << 8 | byte(2),
            byte(3) << 8 | byte(4),
        );
        if length == 0 {
            return Err("test-verify expects no RLE record".into());
        }
        records.push((offset, length, at + 5));
        at += 5 + length;
    }
    Ok(records)
}

fn read_manifest(dir: &Path) -> Result<Json> {
    let text = String::from_utf8(read(&dir.join("manifest.json"))?)
        .map_err(|_| "manifest.json is not UTF-8")?;
    Json::parse(&text)
}

fn write_manifest(dir: &Path, manifest: &Json) -> Result<()> {
    write(&dir.join("manifest.json"), manifest.to_pretty().as_bytes())
}

fn set(manifest: &mut Json, path: &[&str], value: Json) -> Result<()> {
    let mut json = manifest;
    for key in path {
        json = json
            .get_mut(key)
            .ok_or_else(|| format!("manifest has no {key}"))?;
    }
    *json = value;
    Ok(())
}

fn number(manifest: &Json, path: &[&str]) -> Result<u64> {
    path.iter()
        .try_fold(manifest, |json, key| json.get(key))
        .and_then(Json::as_u64)
        .ok_or_else(|| format!("manifest has no number {}", path.join(".")))
}

/// The unpacked original of the program of `package`.
fn original(package: &Package) -> Result<PathBuf> {
    let program = patch_verify::expectation(package.kind)?.program;
    patch_verify::find_original(&root().join("build/extracted"), program)
}

/// Gives the copy's manifest the hashes of its changed files: the ELF, both
/// package files, and the image the IPS makes of the original.
fn update_hashes(dir: &Path, package: &Package) -> Result<()> {
    let mut manifest = read_manifest(dir)?;
    let ips = read(&dir.join("code.ips"))?;
    let hash = |file: &str| Ok::<_, String>(Json::string(sha256(&read(&dir.join(file))?)));
    set(&mut manifest, &["source_elf_sha256"], hash("payload.elf")?)?;
    set(
        &mut manifest,
        &["artifacts", "code.ips", "sha256"],
        hash("code.ips")?,
    )?;
    set(
        &mut manifest,
        &["artifacts", "exheader.bin", "sha256"],
        hash("exheader.bin")?,
    )?;
    let code = read(&original(package)?.join("exefs/code.bin"))?;
    let mut image = vec![0; number(&manifest, &["expanded_code_bytes"])? as usize];
    image[..code.len()].copy_from_slice(&code);
    for (offset, length, data) in ips_records(&ips)? {
        image[offset..offset + length].copy_from_slice(&ips[data..data + length]);
    }
    set(
        &mut manifest,
        &["expanded_code_sha256"],
        Json::string(sha256(&image)),
    )?;
    write_manifest(dir, &manifest)
}

/// Flips one bit of `file` in the copy at `at`.
fn flip(dir: &Path, file: &str, at: usize) -> Result<()> {
    let path = dir.join(file);
    let mut bytes = read(&path)?;
    *bytes.get_mut(at).ok_or("flip outside the file")? ^= 0x01;
    write(&path, &bytes)
}

/// Where in code.ips the byte of the image at `address` lies.
fn ips_byte(dir: &Path, address: u64) -> Result<usize> {
    let offset = address as usize - 0x0010_0000;
    ips_records(&read(&dir.join("code.ips"))?)?
        .into_iter()
        .find(|&(start, length, _)| (start..start + length).contains(&offset))
        .map(|(start, _, data)| data + offset - start)
        .ok_or_else(|| format!("no IPS record holds {address:08X}"))
}

type Change = Box<dyn Fn(&Path) -> Result<()>>;

struct Run {
    work: PathBuf,
    failures: Vec<String>,
}

impl Run {
    /// A copy of the package of `package` and its ELF, as `payload.elf`.
    fn copy(&self, package: &Package, name: &str) -> Result<PathBuf> {
        let dir = self.work.join(name);
        std::fs::create_dir(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let folder = package.folder(&root());
        for file in ["code.ips", "exheader.bin", "manifest.json", "PACKAGE.txt"] {
            write(&dir.join(file), &read(&folder.join(file))?)?;
        }
        write(&dir.join("payload.elf"), &read(&package.elf(&root()))?)?;
        Ok(dir)
    }

    /// Checks a copy of `package`, changed by `change`, as a package of
    /// `checked_as`; `expect` is the refusal looked for, or none.
    fn case(
        &mut self,
        package: &Package,
        name: &str,
        change: Option<Change>,
        expect: Option<&str>,
        checked_as: &Package,
    ) -> Result<()> {
        let folder = format!(
            "{}-{}",
            package.kind,
            name.replace(|c: char| !c.is_alphanumeric(), "-")
        );
        let dir = self.copy(package, &folder)?;
        if let Some(change) = change {
            change(&dir)?;
        }
        let places = Places {
            package: Some(dir.clone()),
            elf: Some(dir.join("payload.elf")),
            original: None,
        };
        let refusal = verify(checked_as, places).err();
        let result = match (expect, refusal) {
            (None, None) => "passed".to_owned(),
            (None, Some(message)) => format!("FAILED: refused ({message})"),
            (Some(_), None) => "FAILED: accepted".to_owned(),
            (Some(expect), Some(message)) if !message.contains(expect) => {
                format!("FAILED: refused for another reason ({message})")
            }
            (Some(_), Some(_)) => "refused".to_owned(),
        };
        println!("{:<12} {:<28} {result}", package.kind, name);
        if result.starts_with("FAILED") {
            self.failures.push(format!("{} {name}", package.kind));
        }
        Ok(())
    }
}

fn manifest_change(edit: impl Fn(&mut Json) -> Result<()> + 'static) -> Option<Change> {
    Some(Box::new(move |dir: &Path| {
        let mut manifest = read_manifest(dir)?;
        edit(&mut manifest)?;
        write_manifest(dir, &manifest)
    }))
}

fn cases(run: &mut Run) -> Result<()> {
    for package in &PACKAGES {
        if matches!(package.kind, "bank" | "transporter")
            || package.folder(&root()).join("manifest.json").is_file()
        {
            run.case(package, "unchanged", None, None, package)?;
        } else {
            println!("{:<12} {:<28} not built", package.kind, "unchanged");
        }
    }
    for kind in ["bank", "transporter"] {
        let package = package(kind)?;
        let bytes = |file: &'static str, at: fn(&Path) -> Result<usize>| -> Option<Change> {
            Some(Box::new(move |dir: &Path| {
                flip(dir, file, at(dir)?)?;
                update_hashes(dir, package)
            }))
        };
        let differs = Some("Expanded image differs from independent expected image");
        run.case(
            package,
            "ELF header",
            bytes("payload.elf", |_| Ok(4)),
            Some("ELF32 little-endian header"),
            package,
        )?;
        run.case(
            package,
            "start-up hook byte",
            bytes("code.ips", |dir| {
                ips_byte(
                    dir,
                    number(&read_manifest(dir)?, &["placement", "bootstrap_address"])? + 8,
                )
            }),
            differs,
            package,
        )?;
        run.case(
            package,
            "payload byte",
            bytes("code.ips", |dir| {
                ips_byte(
                    dir,
                    number(&read_manifest(dir)?, &["placement", "payload_address"])? + 0x40,
                )
            }),
            differs,
            package,
        )?;
        run.case(
            package,
            "exheader size field",
            bytes("exheader.bin", |_| Ok(0x3d)),
            Some("Unexpected paired-exheader edit"),
            package,
        )?;
        run.case(
            package,
            "manifest export",
            manifest_change(|manifest| {
                let first = manifest
                    .get("exports")
                    .and_then(|e| e.members().first())
                    .map(|(name, _)| name.clone())
                    .ok_or("manifest has no export")?;
                set(manifest, &["exports", &first], Json::number(0))
            }),
            Some("Manifest export "),
            package,
        )?;
        run.case(
            package,
            "manifest placement",
            manifest_change(|manifest| {
                let bytes = number(manifest, &["placement", "payload_rw_bytes"])?;
                set(
                    manifest,
                    &["placement", "payload_rw_bytes"],
                    Json::number(bytes + 0x1000),
                )
            }),
            Some("Manifest placement"),
            package,
        )?;
        run.case(
            package,
            "manifest kind",
            manifest_change(|manifest| set(manifest, &["patch"], Json::string("download"))),
            Some("Manifest names another patch"),
            package,
        )?;
        run.case(
            package,
            "package name",
            Some(Box::new(|dir: &Path| {
                write(&dir.join("PACKAGE.txt"), b"0000000000000000\n")
            })),
            Some("Package is not the one of the current payload and profile"),
            package,
        )?;
    }
    // The original is found by its hash, whatever its extraction's folder
    // is called, as for a CIA that holds the same program packed otherwise.
    for kind in ["bank", "transporter"] {
        let package = package(kind)?;
        let folder = original(package)?;
        let renamed = folder.with_file_name(format!(
            "{}-renamed",
            folder.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::rename(&folder, &renamed)
            .map_err(|e| format!("cannot rename {}: {e}", folder.display()))?;
        let result = run.case(package, "original in another folder", None, None, package);
        std::fs::rename(&renamed, &folder)
            .map_err(|e| format!("cannot rename {} back: {e}", renamed.display()))?;
        result?;
    }
    // The package of one kind is not that of another, when the other is
    // built.
    if !package("test")?.elf(&root()).is_file() {
        println!(
            "{:<12} {:<28} not built",
            "bank", "checked as the test build"
        );
        return Ok(());
    }
    run.case(
        package("bank")?,
        "checked as the test build",
        None,
        Some("Package is not the one of the current payload and profile"),
        package("test")?,
    )
}

pub fn test_verify() -> Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let work = std::env::temp_dir().join(format!(
        "offline_bank-verify-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir(&work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let mut run = Run {
        work: work.clone(),
        failures: Vec::new(),
    };
    let result = cases(&mut run);
    let _ = std::fs::remove_dir_all(&work);
    result?;
    if !run.failures.is_empty() {
        return Err(format!(
            "Verifier tests failed: {}",
            run.failures.join(", ")
        ));
    }
    println!("Verifier tests passed.");
    Ok(())
}
