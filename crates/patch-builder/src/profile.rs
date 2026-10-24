//! What a patch is made from: the original program it edits, and its
//! profile, the edits of one kind of package. Every package, of Bank and of
//! Transporter, is prepared the same way (`prepare_patch`), from tables of
//! edited words (`Edit`).

use crate::{
    bank15,
    elf::{inspect_payload_elf, Contract, PayloadElf},
    encode_arm_branch, fail, le32,
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
    /// Words of the original every patch of it relies on beyond its edits,
    /// in groups: (virtual address, word).
    pub relies: &'static [&'static [(u32, u32)]],
    /// What else is checked beyond the edited words.
    pub check: fn(code: &[u8], profile: &Profile, payload: &PayloadElf) -> Result<()>,
}

/// What an edited word becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    /// This word.
    Raw(u32),
    /// `b` to an address of the original.
    Branch(u32),
    /// `bl` to an address of the original or of the start-up hook.
    BranchLink(u32),
    /// `b` to an entry function, by name.
    To(&'static str),
    /// `bl` to an entry function, by name.
    Call(&'static str),
    /// The address of an entry function, as a table of the original holds a
    /// function.
    Entry(&'static str),
}
impl Word {
    /// The entry function this word names, if any.
    pub fn entry(self) -> Option<&'static str> {
        match self {
            Word::To(name) | Word::Call(name) | Word::Entry(name) => Some(name),
            Word::Raw(_) | Word::Branch(_) | Word::BranchLink(_) => None,
        }
    }
}

/// One edited word: (virtual address, original word, what it becomes).
pub type Edit = (u32, u32, Word);

/// One kind of package of a program.
pub struct Profile {
    /// The package kind: its name on the command line, in the manifest and
    /// in xtask.
    pub name: &'static str,
    pub program: &'static Program,
    /// Every edit but the start-up hook's bytes, in groups that profiles
    /// share. The first entry function named is the router's, the entry of
    /// the ELF.
    pub edits: &'static [&'static [Edit]],
    /// Words of the original this profile relies on beyond its edits and
    /// those of the program: (virtual address, word).
    pub relies: &'static [&'static [(u32, u32)]],
    /// The main-menu locations of Bank (`bank15::MENU_LOCATIONS`) it edits.
    /// None for every package that is released.
    pub menu_edits: &'static [u32],
}
impl Profile {
    pub(crate) fn all_edits(&self) -> impl Iterator<Item = &'static Edit> {
        self.edits.iter().copied().flatten()
    }
    /// The entry functions its payload must export, each once, in the order
    /// the edits name them. The first is the entry of the ELF.
    pub fn entries(&self) -> Vec<&'static str> {
        let mut entries = Vec::new();
        for name in self.all_edits().filter_map(|(_, _, word)| word.entry()) {
            if !entries.contains(&name) {
                entries.push(name);
            }
        }
        entries
    }
    /// Edited regions of the original, the start-up hook's bytes included.
    pub fn regions(&self) -> usize {
        self.all_edits().count() + 1
    }
    /// Every edit but the start-up hook's bytes, with `entry` giving the
    /// address of an entry function by its name.
    fn checked_edits(&self, entry: impl Fn(&str) -> Result<u32>) -> Result<Vec<CheckedEdit>> {
        let base = self.program.contract.code_base;
        self.all_edits()
            .map(|&(address, original, word)| {
                let replacement = match word {
                    Word::Raw(value) => value.to_le_bytes(),
                    Word::Branch(target) => encode_arm_branch(address, target, false)?,
                    Word::BranchLink(target) => encode_arm_branch(address, target, true)?,
                    Word::To(name) => encode_arm_branch(address, entry(name)?, false)?,
                    Word::Call(name) => encode_arm_branch(address, entry(name)?, true)?,
                    Word::Entry(name) => entry(name)?.to_le_bytes(),
                };
                Ok(CheckedEdit {
                    offset: (address - base) as usize,
                    expected: original.to_le_bytes().to_vec(),
                    replacement: replacement.to_vec(),
                })
            })
            .collect()
    }
}

/// The five kinds of package: the two patches of a release, and the three
/// other packages for Bank.
pub const PROFILES: [&Profile; 5] = [
    &bank15::offline::PROFILE,
    &transporter15::offline::PROFILE,
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

/// A package in memory: the paired IPS and exheader, and the payload they
/// were made from.
pub struct PreparedPatch {
    pub placement: PreparedPlacement,
    pub payload: PayloadElf,
}

/// Checks the input pair, the linked payload and every edit, then builds
/// the paired IPS and exheader in memory. Nothing is written.
pub fn prepare_patch(
    profile: &Profile,
    code: &[u8],
    exheader: &[u8],
    elf: &[u8],
) -> Result<PreparedPatch> {
    let program = profile.program;
    if code.len() != program.code_length || sha256(code) != program.identity.code_sha256 {
        return fail(format!("code.bin is not the reviewed {}", program.label));
    }
    let base = program.contract.code_base;
    for &(address, word) in [program.relies, profile.relies]
        .into_iter()
        .flatten()
        .copied()
        .flatten()
    {
        if le32(code, (address - base) as usize)? != word {
            return fail(format!(
                "the original word at {address:#010x} is not the reviewed code"
            ));
        }
    }
    let payload = inspect_payload_elf(program.contract, elf, &profile.entries())?;
    (program.check)(code, profile, &payload)?;
    let mut edits = profile.checked_edits(|name| payload.export(name))?;
    edits.push(payload.bootstrap_edit());
    let placement = prepare_expanded_data(
        code,
        exheader,
        &program.identity,
        &payload.linked_payload(),
        &edits,
    )?;
    if placement.layout().payload_address != program.contract.payload {
        return fail("payload placement differs from the reviewed address");
    }
    Ok(PreparedPatch { placement, payload })
}

/// Test helpers: a profile's edits with stand-in addresses for its entry
/// functions, and the target of a branch.
#[cfg(test)]
pub(crate) mod sample {
    use super::*;

    /// The stand-in address of entry `name` of `profile`, in its payload.
    pub fn entry(profile: &Profile, name: &str) -> u32 {
        let index = profile.entries().iter().position(|known| *known == name);
        profile.program.contract.payload + 0x100 + index.expect(name) as u32 * 0x10
    }
    /// The profile's edits with those addresses: (virtual address, original
    /// word, new word).
    pub fn edits(profile: &Profile) -> Vec<(u32, u32, u32)> {
        let base = profile.program.contract.code_base;
        profile
            .checked_edits(|name| Ok(entry(profile, name)))
            .unwrap()
            .into_iter()
            .map(|edit| {
                let word = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
                (
                    edit.offset as u32 + base,
                    word(&edit.expected),
                    word(&edit.replacement),
                )
            })
            .collect()
    }
    /// The new word at `address`, if the profile edits it.
    pub fn at(profile: &Profile, address: u32) -> Option<u32> {
        edits(profile)
            .into_iter()
            .find(|(at, _, _)| *at == address)
            .map(|(_, _, word)| word)
    }
    /// Target of an unconditional `b` (`link` false) or `bl` (true).
    pub fn branch_target(address: u32, word: u32, link: bool) -> Option<u32> {
        let top = if link { 0xeb00_0000 } else { 0xea00_0000 };
        if word & 0xff00_0000 != top {
            return None;
        }
        let offset = ((word & 0x00ff_ffff) << 8) as i32 >> 6;
        Some(address.wrapping_add(8).wrapping_add(offset as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_found_by_its_name_once() {
        for (index, known) in PROFILES.into_iter().enumerate() {
            assert_eq!(profile(known.name).unwrap().name, known.name);
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

    #[test]
    fn every_edit_is_one_whole_word_of_the_original_text_and_changes_it() {
        for profile in PROFILES {
            let contract = profile.program.contract;
            let mut edits = sample::edits(profile);
            edits.sort_unstable_by_key(|(address, _, _)| *address);
            for pair in edits.windows(2) {
                assert!(
                    pair[0].0 + 4 <= pair[1].0,
                    "{} {:#010x}",
                    profile.name,
                    pair[1].0
                );
            }
            let end = contract.code_base + profile.program.code_length as u32;
            for (address, original, word) in edits {
                assert!(address % 4 == 0, "{address:#x}");
                assert!(address >= contract.code_base && address + 4 <= end);
                // The start-up hook's bytes are its own region.
                assert!(address + 4 <= contract.bootstrap || address >= contract.bootstrap_end);
                assert_ne!(original, word, "{} {address:#010x}", profile.name);
            }
            assert_eq!(profile.regions(), profile.all_edits().count() + 1);
        }
    }

    #[test]
    fn the_router_is_the_first_entry_and_every_entry_is_named_once() {
        for profile in PROFILES {
            let entries = profile.entries();
            assert!(entries[0].ends_with("_next"), "{}", profile.name);
            let mut sorted = entries.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), entries.len());
        }
    }

    #[test]
    fn the_sdk_conversion_rejects_a_kernel_priority_used_as_an_sdk_argument() {
        // Both programs have the same instructions, at other addresses.
        let words = |code: &[(u32, u32); 9]| code.map(|(_, word)| word);
        assert_eq!(
            words(&bank15::SDK_PRIORITY_CODE),
            words(&transporter15::SDK_PRIORITY_CODE)
        );
        // Transcription of the checked block (Bank 00232144..00232178).
        let convert = |sdk: u32| {
            if sdk <= 0x20 {
                return sdk + 0x20;
            }
            let first = 0xaef6_2b00u32.wrapping_add(sdk);
            if first <= 0x27 {
                return first + 0x18;
            }
            let last = 0x9372_5b00u32.wrapping_add(sdk);
            if last > 0x40 {
                u32::MAX
            } else {
                last
            }
        };
        assert_eq!(convert(0x11), 0x31);
        assert_eq!(convert(0x1f), 0x3f);
        assert_eq!(convert(0x31), u32::MAX);
    }

    #[test]
    fn the_start_up_call_is_the_first_edit_of_every_profile() {
        for profile in PROFILES {
            let contract = profile.program.contract;
            let (address, original, word) = profile.edits[0][0];
            assert_eq!(
                word,
                Word::BranchLink(contract.bootstrap),
                "{}",
                profile.name
            );
            assert_eq!(
                sample::branch_target(address, original, true),
                Some(contract.original_app_init)
            );
        }
    }
}
