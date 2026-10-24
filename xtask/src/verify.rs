//! Runs the independent check (`patch-verify`) on a package and writes
//! `verification.json` into it, `passed` or `failed`.

use crate::{packages::Package, root, write, Result};
use patch_verify::{failure_report, Json, Paths};
use std::path::PathBuf;

/// Where a check looks. By default the package's folder, its linked payload
/// in `build/intermediate/`, and the unpacked original under
/// `build/extracted/` whose code has the reviewed hash.
#[derive(Default)]
pub struct Places {
    pub package: Option<PathBuf>,
    pub elf: Option<PathBuf>,
    pub original: Option<PathBuf>,
}

/// Checks the package of `package`'s kind at `places`.
pub fn verify(package: &Package, places: Places) -> Result<Json> {
    let root = root();
    let folder = places.package.unwrap_or_else(|| package.folder(&root));
    let elf = places.elf.unwrap_or_else(|| package.elf(&root));
    let result = (|| {
        let expect = patch_verify::expectation(package.kind)?;
        let original = match places.original {
            Some(original) => original,
            None => patch_verify::find_original(&root.join("build/extracted"), expect.program)
                .map_err(|e| format!("Verification failed: {e}"))?,
        };
        let paths = Paths {
            package: &folder,
            elf: &elf,
            original: &original,
        };
        patch_verify::verify(&expect, &paths)
    })();
    let report = match &result {
        Ok(report) => report.clone(),
        Err(reason) => failure_report(
            &patch_verify::package_name(&folder).unwrap_or_else(|| "unknown".into()),
            reason,
        ),
    };
    if folder.is_dir() {
        write(
            &folder.join("verification.json"),
            report.to_pretty().as_bytes(),
        )?;
    }
    result
}
