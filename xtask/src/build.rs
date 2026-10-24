//! Builds packages and checks each with the verifier (docs/building.md).
//! Every package is made with the same steps: unpack its CIA, lint and link
//! its payload, build the package, name it (PACKAGE.txt), verify it. A
//! folder holds one package, the latest: an existing one is replaced, and
//! the folder stays, so that a window open on it stays open.

use crate::{
    extract::{self, Extracted},
    packages::{Package, Program},
    process, read, root, text_file,
    verify::{self, Places},
    write, Result,
};
use patch_verify::Json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub struct Options {
    /// A release: a package for Bank says on its error and result screens
    /// the version from Cargo.toml. Without it every package is a draft and
    /// says "draft" and the commit it is built from, as "draft-a782e1b".
    /// Transporter shows none.
    pub release: bool,
    pub bank_cia: String,
    pub transporter_cia: String,
    pub ctrtool: String,
}

impl Options {
    pub fn new(release: bool) -> Self {
        Options {
            release,
            bank_cia: Program::Bank.default_cia().into(),
            transporter_cia: Program::Transporter.default_cia().into(),
            ctrtool: extract::default_ctrtool(),
        }
    }
}

struct Builder<'a> {
    options: &'a Options,
    toolchain: String,
    /// What a draft says: the commit it is built from.
    draft: Option<String>,
    /// Cargo's target folder.
    target: PathBuf,
    /// The original of each program, unpacked once.
    inputs: HashMap<&'static str, Extracted>,
}

/// The ARM build's arguments beside the command, the package and the
/// target.
const ARM: [&str; 6] = [
    "--release",
    "--target",
    "targets/armv6k-3ds.json",
    "-Zjson-target-spec",
    "-Zbuild-std=core,compiler_builtins",
    "-Zbuild-std-features=compiler-builtins-mem",
];

impl Builder<'_> {
    fn input(&mut self, program: Program) -> Result<&Extracted> {
        if !self.inputs.contains_key(program.name()) {
            let cia = match program {
                Program::Bank => &self.options.bank_cia,
                Program::Transporter => &self.options.transporter_cia,
            };
            let cia = extract::project_file(cia)?;
            let info = patch_builder::inspect_cia(&read(&cia)?).map_err(|e| e.to_string())?;
            println!(
                "{}: title {:016x}, version {}",
                cia.display(),
                info.title_id,
                info.title_version
            );
            let extracted = extract::extract(&cia, &extract::project_file(&self.options.ctrtool)?)?;
            self.inputs.insert(program.name(), extracted);
        }
        Ok(&self.inputs[program.name()])
    }

    /// Lints and links the payload of `package` into
    /// `build/intermediate/<kind>/`.
    fn payload(&self, package: &Package) -> Result<PathBuf> {
        let root = root();
        let elf = package.elf(&root);
        let output = elf.parent().expect("in a folder").to_path_buf();
        std::fs::create_dir_all(&output)
            .map_err(|e| format!("cannot create {}: {e}", output.display()))?;
        let features = std::iter::once("linked-image")
            .chain(package.features.iter().copied())
            .collect::<Vec<_>>()
            .join(",");
        let link = format!("{}-link", package.krate);
        // The payload reads whether it is a release, and of a draft the
        // commit, from these variables when it is compiled
        // (numbers::VERSION), for the error screen of either program.
        let cargo = |command: &str| {
            let mut cargo = process::cargo(&self.toolchain);
            cargo
                .args([command, "--locked", "--offline", "-p", package.krate])
                .args(["--features", &features])
                .args(ARM)
                .env_remove("OFFLINE_BANK_RELEASE")
                .env_remove("OFFLINE_BANK_COMMIT");
            if self.options.release {
                cargo.env("OFFLINE_BANK_RELEASE", "1");
            }
            if let Some(commit) = &self.draft {
                cargo.env("OFFLINE_BANK_COMMIT", commit);
            }
            cargo
        };
        // The ARM code is linted as it is built: the host lint does not
        // compile it.
        process::run(
            "Payload lint",
            cargo("clippy")
                .args(["--quiet", "--lib", "--bin", &link])
                .args(["--", "-D", "warnings"]),
        )?;
        let linker = linker_script(package, &output)?;
        let map = output
            .join(format!("{}.map", package.krate))
            .to_string_lossy()
            .replace('\\', "/");
        process::run(
            "Payload link",
            cargo("rustc").args(["--bin", &link, "--"]).args([
                "-C",
                &format!("link-arg=-T{linker}"),
                "-C",
                &format!("link-arg=-Map={map}"),
                "-C",
                "link-arg=--build-id=none",
                "-C",
                "link-arg=--fatal-warnings",
                "-C",
                "link-arg=-zmax-page-size=4096",
                "--remap-path-prefix",
                &format!("{}=/offline_bank", root.display()),
            ]),
        )?;
        let linked = self.target.join("armv6k-3ds/release").join(&link);
        write(&elf, &read(&linked)?)?;
        Ok(elf)
    }

    fn package(&mut self, package: &Package) -> Result<()> {
        let root = root();
        self.input(package.program)?;
        let elf = self.payload(package)?;
        let original = &self.inputs[package.program.name()];
        let output = package.folder(&root);
        if output.is_dir() {
            for entry in std::fs::read_dir(&output)
                .map_err(|e| format!("cannot list {}: {e}", output.display()))?
            {
                let path = entry.map_err(|e| e.to_string())?.path();
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                removed.map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
            }
        } else {
            std::fs::create_dir_all(&output)
                .map_err(|e| format!("cannot create {}: {e}", output.display()))?;
        }
        let profile = patch_builder::profile(package.kind).map_err(|e| e.to_string())?;
        let built = patch_builder::build(
            profile,
            &read(&original.code)?,
            &read(&original.exheader)?,
            &read(&elf)?,
            &output,
        )
        .map_err(|e| format!("Package build: {e}"))?;
        verify::verify(package, Places::default())
            .map_err(|e| format!("Verification of the {} package failed: {e}", package.kind))?;
        if package.kind == "test" {
            write(
                &output.join("TEST-BUILD.txt"),
                text_file(&TEST_NOTE).as_bytes(),
            )?;
        }
        println!(
            "Package {} {}: {} (verified)",
            package.kind,
            built.name,
            output.display()
        );
        Ok(())
    }
}

/// Writes the linker script of `package` (`patch_builder::linker_script`)
/// into `folder` and returns its path. Its name holds the hash of its text,
/// so that a changed layout changes rustc's arguments and Cargo links again.
fn linker_script(package: &Package, folder: &Path) -> Result<String> {
    let profile = patch_builder::profile(package.kind).map_err(|e| e.to_string())?;
    let script = patch_builder::linker_script(profile);
    let name = format!(
        "link-{}.ld",
        &patch_builder::hex(&patch_builder::sha256(script.as_bytes()))[..16]
    );
    // Earlier layouts go.
    for entry in
        std::fs::read_dir(folder).map_err(|e| format!("cannot list {}: {e}", folder.display()))?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        if file.starts_with("link-") && file.ends_with(".ld") && file != name {
            std::fs::remove_file(&path)
                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        }
    }
    let path = folder.join(name);
    write(&path, script.as_bytes())?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

/// Cargo's target folder, wherever it is configured.
fn target_directory() -> Result<PathBuf> {
    let metadata = process::output(
        "cargo metadata",
        process::cargo("stable").args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--offline",
        ]),
    )?;
    Json::parse(&metadata)?
        .get("target_directory")
        .and_then(Json::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata names no target directory".into())
}

pub fn build(packages: &[&Package], options: &Options) -> Result<()> {
    // A draft names the commit it is built from. Built from changes that
    // are not committed, it names one its sources are not: commit first.
    let draft = if options.release {
        None
    } else {
        let commit = process::git(&["rev-parse", "--short=7", "HEAD"])?;
        if !process::git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
            eprintln!("warning: Uncommitted changes: this draft says draft-{commit}, which its sources are not. Commit first.");
        }
        Some(commit)
    };
    let mut builder = Builder {
        options,
        toolchain: process::pinned_toolchain()?,
        draft,
        target: target_directory()?,
        inputs: HashMap::new(),
    };
    for package in packages {
        builder.package(package)?;
    }
    Ok(())
}

/// The test build is never released: its folder says what it does.
const TEST_NOTE: [&str; 20] = [
    "Pokemon Bank offline patch, TEST BUILD. Not for normal use. Its start screen says \"Ver. 1.5-test\".",
    "",
    "After START it shows the main menu of the original. The entry chosen there opens the Bank and says",
    "what the next Save and Quit of that session does. The buttons are in the console's language:",
    "",
    "  Use Pokemon Bank           saves normally. Also settles a save that another entry stopped.",
    "  Use Pokemon Bank (before)  stops after the Bank has marked its save as in progress and before",
    "                             the game is written. Shows error 00007E57 00000001.",
    "  Use Pokemon Bank (after)   stops after the game is written and before the Bank clears its mark.",
    "                             Shows error 00007E57 00000002.",
    "  Use Pokemon Bank (record)  first writes the journal record 150 times, then saves normally.",
    "                             For cutting the power during those writes.",
    "  Use Pokemon Bank (boxes)   first writes the file of the boxes that is not in use 30 times, then",
    "                             saves normally.",
    "                             For cutting the power during those writes.",
    "  Back                       the original: asks, then returns to the start screen.",
    "",
    "Back on the game selection leads to this menu again.",
    "The reward dialogs are never shown, and nothing is claimed.",
    "Back up Bank's extdata and the game saves before using it, and put the normal package back afterwards.",
];
