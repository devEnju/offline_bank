//! Makes a release of both patches from the committed sources: tests, build,
//! independent verification, zips for the SD card, release notes, and an
//! annotated git tag on the commit it was built from (docs/building.md).
//! Beside them it ships the migration below v1.0.0 and the download while
//! the official service runs, each as long as its module is here. Nothing
//! leaves this machine unless `--publish` is given, which pushes the tag and,
//! with the gh tool, creates the GitHub release from the zips of an earlier
//! run. A release is only ever made whole: to test one, make it, and
//! withdraw it if it fails. Its version is the workspace's in Cargo.toml.

mod download;
mod guide;
mod migration;

use crate::{build, packages, process, read, root, text_file, write, zip, Result};
use guide::{row, Block, Table};
use patch_verify::{sha256, Json};
use std::path::{Path, PathBuf};

const REPOSITORY: &str = "https://github.com/devEnju/offline_bank";
const DISCLAIMER: [&str; 3] = [
    "Unofficial fan project. Not affiliated with or endorsed by Nintendo,",
    "The Pokemon Company, GAME FREAK or Creatures. Provided as is, without",
    "warranty; use at your own risk.",
];

/// What every guide refers to.
pub struct Context {
    pub version: String,
    pub numbers: Version,
    /// The day of the release, in UTC.
    pub today: (u32, u32, u32),
    pub repository: &'static str,
    /// The anchor of the notes' "Installing" heading.
    pub installing: String,
    /// The install step as the zips' README.txt says it.
    pub install: String,
}

/// A package that ships beside the two patches for a while, with its zip.
pub struct Extra {
    /// Its package kind.
    pub kind: &'static str,
    /// Its zip, without ".zip".
    pub name: &'static str,
    /// The heading of its README.txt, and its package's label there.
    pub heading: String,
    pub label: String,
    /// The notes' section with its guide, and what the notes' table of
    /// files calls its package.
    pub section: &'static str,
    pub package: &'static str,
    pub guide: Vec<Block>,
    /// Released versions its guide names, which must exist.
    pub tags: Vec<String>,
    /// The screens its guide shows, as `pictures/` has them on main.
    pub pictures: Vec<String>,
    /// Its rows of "Which part is for you", before the row of every other
    /// version or after it.
    pub routes: Vec<Vec<String>>,
    pub routes_after: bool,
    /// Its rows of the table of Bank's errors.
    pub errors: Vec<Vec<String>>,
    /// Why this release may not ship it, if so.
    pub refusal: Option<String>,
}

impl Context {
    pub fn release_link(&self, tag: &str) -> String {
        format!("[{tag} release]({}/releases/tag/{tag})", self.repository)
    }
}

/// Numbers shown as the console shows them: two, or only a first one.
pub fn numbers(numbers: &str) -> String {
    format!("`{numbers}`")
}

pub type Version = (u64, u64, u64);

/// "v1.2.3" as numbers.
fn version_of(tag: &str) -> Option<Version> {
    let mut parts = tag.strip_prefix('v')?.split('.').map(|p| {
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    });
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// The version under `[workspace.package]`, the one every crate but
/// offline-core takes, as "v1.2.3".
fn workspace_version() -> Result<String> {
    let text = String::from_utf8(read(&root().join("Cargo.toml"))?).map_err(|e| e.to_string())?;
    let mut section = "";
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
        } else if section == "workspace.package" {
            if let Some(value) = line.strip_prefix("version").map(str::trim_start) {
                if let Some(value) = value.strip_prefix('=') {
                    found = value
                        .trim()
                        .strip_prefix('"')
                        .and_then(|v| v.strip_suffix('"'));
                }
            }
        }
    }
    let version = format!(
        "v{}",
        found.ok_or("Cargo.toml has no version under [workspace.package].")?
    );
    if version_of(&version).is_none() {
        return Err(format!(
            "The version in Cargo.toml must look like 1.0.0: {version}"
        ));
    }
    Ok(version)
}

/// The released versions, oldest first.
fn tags() -> Result<Vec<String>> {
    let mut tags: Vec<String> = process::git(&["tag", "-l", "v*"])?
        .lines()
        .filter(|tag| version_of(tag).is_some())
        .map(str::to_owned)
        .collect();
    tags.sort_by_key(|tag| version_of(tag));
    Ok(tags)
}

fn tag_exists(tag: &str) -> bool {
    process::command("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{tag}"),
        ])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Days since 1970-01-01 as a date (H. Hinnant's civil_from_days).
fn civil(days: i64) -> (u32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as u32;
    (year, month, day)
}

/// Now, in UTC.
fn now() -> zip::Stamp {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    let time = seconds.rem_euclid(86_400) as u32;
    zip::Stamp {
        year,
        month,
        day,
        hour: time / 3600,
        minute: time / 60 % 60,
        second: time % 60,
    }
}

fn checked(what: &str, command: &mut std::process::Command) -> Result<()> {
    println!("== {what}");
    process::run(what, command)
}

/// A built package that passed its verification, with its files' hashes.
struct Verified {
    name: String,
    directory: PathBuf,
    hashes: [(&'static str, String); 2],
    /// The title its folder on the SD card is named for.
    title: String,
}

fn verified(kind: &str) -> Result<Verified> {
    let package = packages::package(kind)?;
    let directory = package.folder(&root());
    let name = patch_verify::package_name(&directory).unwrap_or_default();
    let json = |file: &str| -> Result<Json> {
        let text = String::from_utf8(read(&directory.join(file))?).map_err(|e| e.to_string())?;
        Json::parse(&text)
    };
    let verification = json("verification.json")?;
    let field = |key: &str| {
        verification
            .get(key)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if field("status") != "passed" || field("package") != name {
        return Err(format!("Package {kind}/{name} did not pass verification."));
    }
    let hashes = [
        ("code.ips", field("ips_sha256")),
        ("exheader.bin", field("paired_exheader_sha256")),
    ];
    for (file, hash) in &hashes {
        if sha256(&read(&directory.join(file))?) != *hash {
            return Err(format!(
                "Package {kind}/{name}: {file} changed after verification."
            ));
        }
    }
    let title = json("manifest.json")?
        .get("title_id")
        .and_then(Json::as_str)
        .ok_or("manifest.json names no title")?
        .to_uppercase();
    Ok(Verified {
        name,
        directory,
        hashes,
        title,
    })
}

/// A zip being put together, with its README.txt and the notes' rows of its
/// files.
struct Archive<'a> {
    name: &'a str,
    readme: Vec<String>,
    files: Vec<(String, Vec<u8>)>,
}

impl<'a> Archive<'a> {
    /// The facts to trace it: its heading, when and from what it was built.
    fn new(name: &'a str, heading: &str, date: &str, commit: &str) -> Self {
        let mut readme = vec![
            heading.to_owned(),
            format!("Built {date} from commit {commit}"),
            String::new(),
        ];
        readme.extend(DISCLAIMER.map(str::to_owned));
        readme.push(String::new());
        Archive {
            name,
            readme,
            files: Vec::new(),
        }
    }

    /// Adds a package in its luma folder and lists its files.
    fn add(&mut self, package: &Verified, label: &str, table: &mut Vec<String>) -> Result<()> {
        self.readme
            .push(format!("{label}, package {}", package.name));
        for (file, hash) in &package.hashes {
            let path = format!("luma/titles/{}/{file}", package.title);
            self.readme.push(format!("  {path}"));
            self.readme.push(format!("    SHA-256 {hash}"));
            table.push(format!("| `{}.zip` | `{path}` | `{hash}` |", self.name));
            self.files
                .push((path, read(&package.directory.join(file))?));
        }
        Ok(())
    }

    /// Writes the zip, its README.txt first.
    fn write(self, folder: &Path, stamp: zip::Stamp) -> Result<PathBuf> {
        let mut files = vec![(
            "README.txt".to_owned(),
            text_file(&self.readme).into_bytes(),
        )];
        files.extend(self.files);
        let path = folder.join(format!("{}.zip", self.name));
        write(&path, &zip::zip(&files, stamp))?;
        unzips_as_written(&path, &files)?;
        Ok(path)
    }
}

/// Every file of `files` in the zip at `path`, as an unzip tool of the
/// system reads it: Windows' own `tar` extracts the zip into a temporary
/// folder, and every file there must be one of `files` with its bytes, and
/// every one of `files` must be there. A zip that another tool reads
/// otherwise is never released.
fn unzips_as_written(path: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    // Windows' tar reads zips; another tar found first on the path may not.
    let tar = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("tar.exe"))
        .filter(|tar| tar.is_file())
        .unwrap_or_else(|| PathBuf::from("tar"));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let out =
        std::env::temp_dir().join(format!("offline_bank-unzip-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let result = (|| {
        process::run(
            &format!("Extracting {}", path.display()),
            process::command(&tar.to_string_lossy())
                .arg("-xf")
                .arg(path)
                .arg("-C")
                .arg(&out),
        )?;
        let mut found = Vec::new();
        let mut folders = vec![out.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?.path();
                if entry.is_dir() {
                    folders.push(entry);
                } else {
                    found.push(entry);
                }
            }
        }
        if found.len() != files.len() {
            return Err(format!(
                "{} holds {} files, not {}",
                path.display(),
                found.len(),
                files.len()
            ));
        }
        for (name, bytes) in files {
            let extracted = out.join(Path::new(name).components().collect::<PathBuf>());
            if read(&extracted)? != *bytes {
                return Err(format!("{name} in {} is not as written", path.display()));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&out);
    result
}

pub fn release(publish: bool) -> Result<()> {
    let version = workspace_version()?;
    let numbers = version_of(&version).expect("checked");
    let stamp = now();
    let name = format!("offline_bank-{version}");
    let context = Context {
        version: version.clone(),
        numbers,
        today: (stamp.year, stamp.month, stamp.day),
        repository: REPOSITORY,
        installing: guide::anchor(&format!("Installing {version}")),
        install: format!("Install {version}: extract {name}.zip from this release and copy its luma folder to the root of the SD card."),
    };
    // What ships beside the two patches. When one goes, its line goes.
    let extras = [migration::extra(&context), download::extra(&context)];
    // A release lies in a folder of its own, named for its version.
    let folder = root().join("build").join("release").join(&version);
    let zip_path = folder.join(format!("{name}.zip"));
    let notes_path = folder.join(format!("{name}.md"));
    if publish {
        return self::publish(&version, &folder, &zip_path, &notes_path);
    }
    if let Some(refusal) = extras.iter().find_map(|extra| extra.refusal.clone()) {
        return Err(refusal);
    }

    // 1. State of the repository.
    let commit = process::git(&["rev-parse", "HEAD"])?;
    for extra in &extras {
        for tag in &extra.tags {
            if !tag_exists(tag) {
                return Err(format!(
                    "There is no released version {tag} to migrate from."
                ));
            }
        }
        // The notes show the screens of the guides' steps as pictures/ has
        // them on main.
        for picture in &extra.pictures {
            if !root().join("pictures").join(picture).is_file() {
                return Err(format!("The notes need pictures/{picture}."));
            }
        }
    }
    let branch = process::git(&["branch", "--show-current"])?;
    if branch != "main" {
        return Err(format!("Releases are made from main; this is '{branch}'."));
    }
    // Files git does not track may lie in the folder; a tracked file must be
    // as committed.
    if !process::git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err("There are uncommitted changes. Commit them first.".into());
    }
    if tag_exists(&version) {
        return Err(format!(
            "Tag {version} already exists. Set the new version in Cargo.toml and Cargo.lock first."
        ));
    }
    let tags = tags()?;
    if let Some(newest) = tags.last() {
        if version_of(newest) >= Some(numbers) {
            return Err(format!("{version} is not newer than {newest}. Set the new version in Cargo.toml and Cargo.lock first."));
        }
    }
    let outputs = [zip_path.clone(), notes_path.clone()].into_iter().chain(
        extras
            .iter()
            .map(|extra| folder.join(format!("{}.zip", extra.name))),
    );
    for path in outputs {
        if path.exists() {
            return Err(format!(
                "Output already exists and will not be replaced: {}",
                path.display()
            ));
        }
    }

    // 2. Tests.
    let cargo = |args: &[&str]| {
        let mut cargo = process::cargo("stable");
        cargo.args(args);
        cargo
    };
    checked(
        "tests",
        &mut cargo(&["test", "--workspace", "--locked", "--offline", "--quiet"]),
    )?;
    checked(
        "test-build tests",
        &mut cargo(&[
            "test",
            "-p",
            "bank-common",
            "-p",
            "bank-payload",
            "--features",
            "test-build",
            "--release",
            "--locked",
            "--offline",
            "--quiet",
        ]),
    )?;
    checked(
        "lint",
        &mut cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--offline",
            "--quiet",
            "--",
            "-D",
            "warnings",
        ]),
    )?;
    checked("formatting", &mut cargo(&["fmt", "--all", "--", "--check"]))?;

    // 3. The patches, built from these sources and independently verified.
    // A package for Bank says the release's version on its error and result
    // screens; the migration puts the version of the Bank it converted in
    // front.
    let kinds = ["bank", "transporter"]
        .into_iter()
        .chain(extras.iter().map(|extra| extra.kind));
    let packages = kinds.map(packages::package).collect::<Result<Vec<_>>>()?;
    build::build(&packages, &build::Options::new(true))?;
    // The verifier still refuses what it must.
    crate::test_verify::test_verify()?;
    let bank = verified("bank")?;
    let transporter = verified("transporter")?;
    let extra_packages = extras
        .iter()
        .map(|extra| verified(extra.kind))
        .collect::<Result<Vec<_>>>()?;

    // 4. The zips, each laid out for the root of the SD card.
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let date = format!("{:04}-{:02}-{:02}", stamp.year, stamp.month, stamp.day);
    let installing = &context.installing;

    // Whose Bank goes which way: what they have, what they do, and what the
    // zip says instead. Without the migration, a Bank stored an earlier way
    // goes through the last release that shipped it.
    let migrated_by = if extras.iter().any(|extra| !extra.errors.is_empty()) {
        None
    } else {
        let last = tags
            .iter()
            .rev()
            .find(|tag| tag.starts_with("v0."))
            .ok_or("There is no v0 release.")?;
        Some(context.release_link(last))
    };
    let mut routes = vec![row![
        "Never used the offline patch",
        format!("[Install {version}]({installing})")
    ]];
    for extra in extras.iter().filter(|extra| !extra.routes_after) {
        routes.extend(extra.routes.iter().cloned());
    }
    if let Some(migrated_by) = &migrated_by {
        routes.push(row![
            "An offline Bank of an earlier release",
            format!(
                "Convert it first with the migration of the {migrated_by}, then install {version}"
            ),
        ]);
    }
    routes.push(row![
        "An offline Bank from every other version",
        format!("[Install {version}]({installing})")
    ]);
    for extra in extras.iter().filter(|extra| extra.routes_after) {
        routes.extend(extra.routes.iter().cloned());
    }

    // What Bank says when the Bank on the SD card is not continued.
    let errors = Table {
        head: row!["First number", "Meaning", "What to do"],
        rows: match &migrated_by {
            Some(migrated_by) => vec![row![
                "`00000003`",
                "The Bank is from an earlier release and not converted yet.",
                format!("Convert it first with the migration of the {migrated_by}."),
            ]],
            None => extras
                .iter()
                .flat_map(|extra| extra.errors.iter().cloned())
                .collect(),
        },
    };

    let mut files_table = Vec::new();
    let mut archive = Archive::new(
        &name,
        &format!("Pokemon Bank and Transporter Offline Patch {version}"),
        &date,
        &commit,
    );
    archive.readme.extend(
        [
            "Copy the luma folder to the root of the SD card (merge it with the",
            "existing one) and switch on \"Enable game patching\" in the Luma3DS",
            "configuration (hold SELECT while powering on).",
            "",
            "The two files of a title only work as a pair. Back up your game saves",
            "first, and the extra data of Pokemon Bank before every update. Start",
            "Bank once before using Transporter. Always update Bank and Transporter",
            "together.",
            "",
            "Which part is for you:",
        ]
        .map(str::to_owned),
    );
    for route in &routes {
        let what = match route.get(2) {
            Some(plain) => plain.clone(),
            None => guide::plain(&route[1]),
        };
        let mut chars = what.chars();
        let what: String = chars
            .next()
            .map(|c| c.to_lowercase().chain(chars).collect())
            .unwrap_or_default();
        archive.readme.extend(guide::wrap(
            &format!("{}: {what}.", guide::plain(&route[0])),
            "- ",
            "  ",
        ));
    }
    archive.readme.push(String::new());
    archive
        .readme
        .push("If Bank then shows an error with one of these as its first number:".into());
    archive.readme.extend(guide::text_table(&errors, "  "));
    archive.readme.push(String::new());
    archive.add(&bank, "Pokemon Bank 1.5", &mut files_table)?;
    archive.add(&transporter, "Poke Transporter 1.5", &mut files_table)?;
    let main_zip = archive.write(&folder, stamp)?;
    // Each other zip with its README.txt: the facts to trace it, and its
    // guide.
    let mut extra_zips = Vec::new();
    for (extra, package) in extras.iter().zip(&extra_packages) {
        let mut archive = Archive::new(extra.name, &extra.heading, &date, &commit);
        archive.readme.extend(guide::text(&extra.guide));
        archive.readme.push(String::new());
        archive.add(package, &extra.label, &mut files_table)?;
        extra_zips.push(archive.write(&folder, stamp)?);
    }

    // 5. Release notes: the same guides, and everything needed to trace
    // this version.
    let hash = |path: &Path| read(path).map(|bytes| sha256(&bytes));
    let mut notes = vec![
        format!("Pokémon Bank and Transporter Offline Patch {version}"),
        String::new(),
        "## Which part is for you".into(),
        String::new(),
    ];
    notes.extend(guide::markdown_table(
        &Table {
            head: row!["You have", "Do"],
            rows: routes,
        },
        "",
    ));
    for extra in &extras {
        notes.extend([
            String::new(),
            format!("## {}", extra.section),
            String::new(),
        ]);
        notes.extend(guide::markdown(&extra.guide));
    }
    notes.extend([
        String::new(),
        format!("## Installing {version}"),
        String::new(),
        format!("Extract `{name}.zip` and copy its `luma` folder to the root of the SD card. See the README of the repository for installation and the guides for each patch."),
        String::new(),
        "If Bank then shows an error with one of these as its first number:".into(),
        String::new(),
    ]);
    notes.extend(guide::markdown_table(&errors, ""));
    notes.extend(
        [
            "",
            "<details>",
            "<summary>Files and hashes</summary>",
            "",
            "| | |",
            "| --- | --- |",
        ]
        .map(str::to_owned),
    );
    notes.extend([
        format!("| Built | {date} |"),
        format!("| Commit | `{commit}` |"),
        format!("| Bank package | `{}` |", bank.name),
        format!("| Transporter package | `{}` |", transporter.name),
        format!("| `{name}.zip` | `{}` |", hash(&main_zip)?),
    ]);
    for ((extra, package), zip) in extras.iter().zip(&extra_packages).zip(&extra_zips) {
        notes.push(format!("| {} | `{}` |", extra.package, package.name));
        notes.push(format!("| `{}.zip` | `{}` |", extra.name, hash(zip)?));
    }
    notes.extend([
        String::new(),
        "| In | File | SHA-256 |".into(),
        "| --- | --- | --- |".into(),
    ]);
    notes.extend(files_table);
    notes.extend([String::new(), "</details>".into()]);
    let notes: String = notes.iter().map(|line| format!("{line}\n")).collect();
    write(&notes_path, notes.as_bytes())?;

    // 6. The tag, on the commit the zips were built from.
    if process::git(&["rev-parse", "HEAD"])? != commit
        || !process::git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty()
    {
        return Err(
            "The repository changed while the release was built; nothing was tagged.".into(),
        );
    }
    checked(
        &format!("tag {version}"),
        process::command("git")
            .args(["tag", "-a", &version, "-F"])
            .arg(&notes_path)
            .args(["--cleanup=verbatim", &commit]),
    )?;

    println!();
    for path in std::iter::once(&main_zip).chain(&extra_zips) {
        println!("Archive: {}", path.display());
    }
    println!("Notes:   {}", notes_path.display());
    println!("Tag:     {version} on {commit} (local; nothing was pushed)");
    println!();
    println!("Next: test the zips on a console. Then either");
    println!("  cargo +stable xtask release --publish");
    println!(
        "or push the tag (git push origin {version}) and upload the zips and the notes by hand."
    );
    println!(
        "To withdraw before publishing: git tag -d {version}, and delete build/release/{version}."
    );
    Ok(())
}

/// The second step, after the console test: pushes the tag and, with the gh
/// tool, creates the GitHub release from the zips the notes name.
fn publish(version: &str, folder: &Path, zip_path: &Path, notes_path: &Path) -> Result<()> {
    for path in [zip_path, notes_path] {
        if !path.is_file() {
            return Err(format!(
                "Run without --publish first; missing {}",
                path.display()
            ));
        }
    }
    let tagged = process::git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/tags/{version}^{{commit}}"),
    ])?;
    let notes = String::from_utf8(read(notes_path)?).map_err(|e| e.to_string())?;
    if !notes.contains(&tagged) {
        return Err(format!(
            "The notes do not name the commit of tag {version} ({tagged})."
        ));
    }
    // A migration or download zip the notes name belongs to this release as
    // well.
    let mut assets = vec![zip_path.to_path_buf()];
    for other in ["offline_bank-migration.zip", "offline_bank-download.zip"] {
        if notes.contains(other) {
            assets.push(folder.join(other));
        }
    }
    for asset in &assets {
        let name = asset.file_name().unwrap_or_default().to_string_lossy();
        if !asset.is_file() {
            return Err(format!("Missing {}", asset.display()));
        }
        if !notes.contains(&sha256(&read(asset)?)) {
            return Err(format!(
                "{name} changed since its release notes were written."
            ));
        }
    }
    checked(
        &format!("push tag {version}"),
        process::command("git").args(["push", "origin", &format!("refs/tags/{version}")]),
    )?;
    let gh = process::command("gh")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status();
    if gh.is_err() {
        println!("The gh tool is not installed. On GitHub, draft a new release for tag {version}, paste {} and attach:", notes_path.display());
        for asset in &assets {
            println!("  {}", asset.display());
        }
        return Ok(());
    }
    checked(
        &format!("create release {version}"),
        process::command("gh")
            .args(["release", "create", version])
            .args(&assets)
            .args(["--title", version, "--notes-file"])
            .arg(notes_path)
            .arg("--verify-tag"),
    )?;
    println!("Published {version}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(version_of("v0.10.2"), Some((0, 10, 2)));
        assert!(version_of("v0.10.0") > version_of("v0.9.9"));
        for bad in ["0.1.0", "v1.0", "v1.0.0.0", "v1.a.0", "v1..0", "v+1.0.0"] {
            assert_eq!(version_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn days_are_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_748), (2026, 10, 22));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }

    #[test]
    fn the_workspace_has_a_release_version() {
        assert!(version_of(&workspace_version().unwrap()).is_some());
    }

    #[test]
    fn the_systems_unzip_reads_a_zip_as_written_and_refuses_a_changed_one() {
        let stamp = zip::Stamp {
            year: 2026,
            month: 10,
            day: 22,
            hour: 12,
            minute: 0,
            second: 0,
        };
        let mut ips = b"PATCH".to_vec();
        ips.extend(vec![0; 70_000]);
        ips.extend((0..40_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8));
        ips.extend(b"EOF");
        let files = vec![
            ("README.txt".to_owned(), b"Read me\r\n".to_vec()),
            ("luma/titles/00040000000C9B00/code.ips".to_owned(), ips),
            (
                "luma/titles/00040000000C9B00/exheader.bin".to_owned(),
                vec![7; 0x800],
            ),
        ];
        let path =
            std::env::temp_dir().join(format!("offline_bank-zip-test-{}.zip", std::process::id()));
        let mut bytes = zip::zip(&files, stamp);
        write(&path, &bytes).unwrap();
        let written = unzips_as_written(&path, &files);
        // The README's stored bytes follow its local header and name.
        bytes[30 + "README.txt".len()] ^= 1;
        write(&path, &bytes).unwrap();
        let changed = unzips_as_written(&path, &files);
        let _ = std::fs::remove_file(&path);
        assert_eq!(written, Ok(()));
        assert!(changed.is_err());
    }
}
