//! The package kinds, once: the program each patches, the crate and cargo
//! features of its payload, and its folder. What a package must contain is not here: the
//! verifier keeps its own lists, independent of the builder.

use crate::Result;
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
    /// Cargo features beside `linked-image`.
    pub features: &'static [&'static str],
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
        features: &[],
        folder: "build/binary/bank",
    },
    Package {
        kind: "transporter",
        program: Program::Transporter,
        krate: "transporter-payload",
        features: &[],
        folder: "build/binary/transporter",
    },
    Package {
        kind: "test",
        program: Program::Bank,
        krate: "bank-payload",
        features: &["test-build"],
        folder: "build/test",
    },
    Package {
        kind: "migration",
        program: Program::Bank,
        krate: "bank-migrate-payload",
        features: &[],
        folder: "build/migration",
    },
    Package {
        kind: "download",
        program: Program::Bank,
        krate: "bank-download-payload",
        features: &[],
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_and_the_verifier_name_a_package_alike() {
        let (ips, exheader) = (b"PATCH\0\0\x01EOF".as_slice(), [7u8; 0x800]);
        assert_eq!(
            patch_builder::package_name(ips, &exheader),
            patch_verify::name_of(ips, &exheader)
        );
    }

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
}
