//! The package kinds, once: the program each patches, the crate and cargo
//! features of its payload, the builder's profile files its name is hashed
//! from, and its folder. What a package must contain is not here: the
//! verifier keeps its own lists, independent of the builder.

use crate::{read, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Bank,
    Transporter,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Program::Bank => "bank",
            Program::Transporter => "transporter",
        }
    }

    /// The update CIA the user puts into `input/`.
    pub fn default_cia(self) -> &'static str {
        match self {
            Program::Bank => "input/Bank-v1.5-update.cia",
            Program::Transporter => "input/Transporter-v1.5-update.cia",
        }
    }
}

pub struct Package {
    /// Its name on the command line, in the manifest and in the builder.
    pub kind: &'static str,
    pub program: Program,
    /// The payload crate; its link binary is `<crate>-link`.
    pub krate: &'static str,
    /// Its linker script.
    pub linker: &'static str,
    /// Cargo features beside `linked-image`.
    pub features: &'static [&'static str],
    /// The builder's profile files (under `crates/patch-builder/src/`) with
    /// the edits of the original, which the package name covers.
    pub profiles: &'static [&'static str],
    /// Where the package lies; the next build of its kind replaces it.
    pub folder: &'static str,
}

/// The two patches of a release first, then the other packages for Bank:
/// the console test build, the migration from earlier releases, and the
/// download from the official server.
pub const PACKAGES: [Package; 5] = [
    Package {
        kind: "bank",
        program: Program::Bank,
        krate: "bank-payload",
        linker: "patches/bank/link/bank15.ld",
        features: &[],
        profiles: &["bank15.rs", "bank15/offline.rs"],
        folder: "build/binary/bank",
    },
    Package {
        kind: "transporter",
        program: Program::Transporter,
        krate: "transporter-payload",
        linker: "patches/transporter/link/transporter15.ld",
        features: &[],
        profiles: &["transporter15.rs", "transporter15/offline.rs"],
        folder: "build/binary/transporter",
    },
    Package {
        kind: "test",
        program: Program::Bank,
        krate: "bank-payload",
        linker: "patches/bank/link/bank15.ld",
        features: &["test-build"],
        profiles: &["bank15.rs", "bank15/offline.rs", "bank15/test.rs"],
        folder: "build/test",
    },
    Package {
        kind: "migration",
        program: Program::Bank,
        krate: "bank-migrate-payload",
        linker: "patches/bank-migrate/link/bank15-migrate.ld",
        features: &[],
        profiles: &["bank15.rs", "bank15/migrate.rs"],
        folder: "build/migration",
    },
    Package {
        kind: "download",
        program: Program::Bank,
        krate: "bank-download-payload",
        linker: "patches/bank-download/link/bank15-download.ld",
        features: &[],
        profiles: &["bank15.rs", "bank15/download.rs"],
        folder: "build/download",
    },
];

pub fn package(kind: &str) -> Result<&'static Package> {
    PACKAGES.iter().find(|p| p.kind == kind).ok_or_else(|| {
        let kinds: Vec<_> = PACKAGES.iter().map(|p| p.kind).collect();
        format!("unknown package kind {kind:?}; kinds: {}", kinds.join(", "))
    })
}

impl Package {
    /// The linked payload, in `build/intermediate/<kind>/`.
    pub fn elf(&self, root: &Path) -> PathBuf {
        root.join("build")
            .join("intermediate")
            .join(self.kind)
            .join(format!("{}.elf", self.krate))
    }

    pub fn folder(&self, root: &Path) -> PathBuf {
        root.join(Path::new(self.folder).components().collect::<PathBuf>())
    }

    /// The package's name: the first 16 hex digits of the SHA-256 over
    /// everything the patch is made from, which is the linked payload and
    /// the builder's profile with the edits of the original (some edits are
    /// plain words in the profile and leave the payload as it is). The same
    /// sources give the same name; line endings of the profile do not count.
    pub fn id(&self, root: &Path) -> Result<String> {
        let mut source = read(&self.elf(root))?;
        for file in self.profiles {
            let profile = read(&root.join("crates/patch-builder/src").join(file))?;
            source.extend(profile.into_iter().filter(|&b| b != b'\r'));
        }
        Ok(patch_builder::hex(&patch_builder::sha256(&source))[..16].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_one_of_the_builder() {
        for each in &PACKAGES {
            let profile = patch_builder::profile(each.kind).unwrap();
            assert!(profile.program.name.starts_with(each.program.name()));
            assert_eq!(package(each.kind).unwrap().krate, each.krate);
        }
        assert_eq!(PACKAGES.len(), patch_builder::PROFILES.len());
        assert!(package("other").is_err());
    }

    #[test]
    fn every_profile_file_exists() {
        let root = crate::root();
        for package in &PACKAGES {
            for file in package.profiles {
                assert!(
                    root.join("crates/patch-builder/src").join(file).is_file(),
                    "{file}"
                );
            }
        }
    }
}
