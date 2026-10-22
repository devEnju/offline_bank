//! The project's commands (docs/building.md), run as
//! `cargo +stable xtask <command>` through the alias in `.cargo/config.toml`.

mod build;
mod extract;
mod packages;
mod process;
mod test_verify;
mod verify;

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "usage: cargo +stable xtask <command>
  build [--kind <kind>] [--bank-cia <path>] [--transporter-cia <path>] [--ctrtool <path>]
                       build and verify packages: bank and transporter, or the one of <kind>
  verify <kind> [--package <folder>] [--elf <path>] [--original <folder>]
                       check a package independently and write verification.json into it
  test-verify          check that the verifier refuses changed copies of the built packages
  package-id <kind>    the name of the package of <kind>
  extract <cia> [--ctrtool <path>]
                       unpack a CIA into build/extracted/
kinds: bank, transporter, test, migration, download";

/// The project folder.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lies in the project folder")
        .to_path_buf()
}

/// A path the user gives: as it is if absolute, else in the project folder,
/// with this system's separators (ctrtool refuses mixed ones).
pub fn project_path(value: &str) -> PathBuf {
    root().join(Path::new(value).components().collect::<PathBuf>())
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

pub fn write(path: &Path, contents: &[u8]) -> Result<()> {
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// A text file for the user, with Windows line ends.
pub fn text_file(lines: &[impl AsRef<str>]) -> String {
    lines
        .iter()
        .map(|line| format!("{}\r\n", line.as_ref()))
        .collect()
}

/// The arguments after the command: options by name, then what is left in
/// order.
struct Args(Vec<String>);

impl Args {
    /// The value of `--name <value>`, taken out of the arguments.
    fn value(&mut self, name: &str) -> Result<Option<String>> {
        let Some(at) = self.0.iter().position(|a| a == name) else {
            return Ok(None);
        };
        if at + 1 == self.0.len() {
            return Err(format!("{name} needs a value"));
        }
        let value = self.0.remove(at + 1);
        self.0.remove(at);
        Ok(Some(value))
    }

    fn path(&mut self, name: &str) -> Result<Option<PathBuf>> {
        Ok(self.value(name)?.map(|value| project_path(&value)))
    }

    /// The remaining arguments, which must be `N` values.
    fn rest<const N: usize>(self) -> Result<[String; N]> {
        if let Some(unknown) = self.0.iter().find(|a| a.starts_with("--")) {
            return Err(format!("unknown option {unknown}\n{USAGE}"));
        }
        self.0.try_into().map_err(|_| USAGE.to_owned())
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut args = Args(args.collect());
    match command.as_str() {
        "build" => {
            let mut options = build::Options::new(false);
            let kind = args.value("--kind")?;
            if let Some(cia) = args.value("--bank-cia")? {
                options.bank_cia = cia;
            }
            if let Some(cia) = args.value("--transporter-cia")? {
                options.transporter_cia = cia;
            }
            if let Some(ctrtool) = args.value("--ctrtool")? {
                options.ctrtool = ctrtool;
            }
            let [] = args.rest()?;
            let packages = match kind {
                Some(kind) => vec![packages::package(&kind)?],
                None => vec![
                    packages::package("bank")?,
                    packages::package("transporter")?,
                ],
            };
            build::build(&packages, &options)?;
        }
        "verify" => {
            let places = verify::Places {
                package: args.path("--package")?,
                elf: args.path("--elf")?,
                original: args.path("--original")?,
            };
            let [kind] = args.rest()?;
            let report = verify::verify(packages::package(&kind)?, places)?;
            print!("{}", report.to_pretty());
        }
        "test-verify" => {
            let [] = args.rest()?;
            test_verify::test_verify()?;
        }
        "package-id" => {
            let [kind] = args.rest()?;
            println!("{}", packages::package(&kind)?.id(&root())?);
        }
        "extract" => {
            let ctrtool = args
                .value("--ctrtool")?
                .unwrap_or_else(extract::default_ctrtool);
            let [cia] = args.rest()?;
            let extracted = extract::extract(
                &extract::project_file(&cia)?,
                &extract::project_file(&ctrtool)?,
            )?;
            print!("{}", extracted.record.to_pretty());
        }
        "help" | "--help" | "-h" => println!("{USAGE}"),
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
