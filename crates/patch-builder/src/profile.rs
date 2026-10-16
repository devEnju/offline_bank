//! What a patch is made from: the original program it edits, and its
//! profile, the entry functions and edits of one kind of package. Every
//! package, of Bank and of Transporter, is prepared the same way
//! (`prepare_patch`).

use crate::{
    bank15,
    elf::{inspect_payload_elf, Contract, PayloadElf},
    fail,
    placement::{prepare_expanded_data, InputIdentity, PreparedPlacement},
    sha256, transporter15, CheckedEdit, Error, Result,
};

/// One original program, identified by its exact files.
pub struct Program {
    /// Its name in the manifest.
    pub name: &'static str,
    /// What the report calls it.
    pub label: &'static str,
    pub title_id: u64,
    pub tmd_version: u16,
    pub remaster_version: u16,
    pub code_length: usize,
    pub identity: InputIdentity,
    /// What its linked payloads must look like.
    pub contract: &'static Contract,
    /// What is checked beyond the edited words: code of the original a
    /// patch relies on, and places only some profiles may edit.
    pub check: fn(
        code: &[u8],
        profile: &Profile,
        payload: &PayloadElf,
        edits: &[CheckedEdit],
    ) -> Result<()>,
}

/// One kind of package of a program.
pub struct Profile {
    /// The package kind: its name on the command line, in the manifest and
    /// in the scripts.
    pub name: &'static str,
    pub program: &'static Program,
    /// The entry functions its payload must export, in the order of the
    /// manifest. The first is the entry of the ELF.
    pub entries: &'static [&'static str],
    /// Edited regions of the original, the start-up hook's bytes included.
    pub regions: usize,
    /// The main-menu locations of Bank (`bank15::MENU_LOCATIONS`) it edits.
    /// None for every package that is released.
    pub menu_edits: &'static [u32],
    /// Every edit but the start-up hook's bytes. `code` is the original.
    pub(crate) edits: fn(code: &[u8], payload: &PayloadElf) -> Result<Vec<CheckedEdit>>,
}

/// The five kinds of package: the two patches of a release, and the three
/// other packages for Bank.
pub const PROFILES: [&Profile; 5] = [
    &bank15::offline::PROFILE,
    &transporter15::PROFILE,
    &bank15::test::PROFILE,
    &bank15::migrate::PROFILE,
    &bank15::download::PROFILE,
];

pub fn profile(name: &str) -> Result<&'static Profile> {
    PROFILES
        .into_iter()
        .find(|profile| profile.name == name)
        .ok_or_else(|| Error(format!("unknown package kind: {name}")))
}

/// Checks the input pair, the linked payload and every edit, then builds
/// the paired IPS and exheader in memory. Nothing is written.
pub fn prepare_patch(
    profile: &Profile,
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
) -> Result<PreparedPlacement> {
    let program = profile.program;
    if code.len() != program.code_length || sha256(code) != program.identity.code_sha256 {
        return fail(format!("code.bin is not the reviewed {}", program.label));
    }
    let payload = inspect_payload_elf(program.contract, elf, profile.entries)?;
    let mut edits = (profile.edits)(code, &payload)?;
    edits.push(payload.bootstrap_edit());
    if edits.len() != profile.regions {
        return fail("profile edit count changed");
    }
    (program.check)(code, profile, &payload, &edits)?;
    let prepared = prepare_expanded_data(
        code,
        exheader,
        &program.identity,
        &payload.linked_payload(),
        &edits,
    )?;
    if prepared.layout().payload_address != program.contract.payload {
        return fail("payload placement differs from the reviewed address");
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_found_by_its_name_once() {
        for (index, known) in PROFILES.into_iter().enumerate() {
            assert_eq!(profile(known.name).unwrap().entries, known.entries);
            assert!(PROFILES[..index]
                .iter()
                .all(|earlier| earlier.name != known.name));
        }
        assert_eq!(
            PROFILES.map(|profile| profile.name),
            ["bank", "transporter", "test", "migration", "download"]
        );
        assert!(profile("offline").is_err());
    }

    #[test]
    fn wrong_input_is_refused_before_anything_is_built() {
        for known in PROFILES {
            assert!(prepare_patch(known, &[0; 16], &[0; 0x800], &[]).is_err());
            let length = known.program.code_length;
            assert!(prepare_patch(known, &vec![0; length], &[0; 0x800], &[]).is_err());
        }
    }
}
