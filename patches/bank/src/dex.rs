//! What the patch needs of the original's Pokédex and adventure records.
//!
//! The original imports a game's Pokédex and records in its records screen
//! (task update 0026d8ec), and that screen runs unchanged. This module reloads
//! the original's caches after a body restore, reads the selected game's
//! trainer through native 0026de9c, and digests the regions for the save and
//! restore checks.
//!
//! One thing is the patch's own: at Save and Quit a game whose version
//! slot already holds the same trainer brings its records and Pokédex along
//! (`refresh`), with the two calls the records screen makes for that case.
//! A game the Bank does not know, or one of another trainer, is taken over
//! in the records screen only, as in the original. No function in this
//! module writes a game file or the offline Bank store.

use offline_core::{
    crc32,
    native_blob::BLOB_SIZE,
    sections::{POKEDEX, RECORDS},
};

pub const RECORDS_OFFSET: usize = RECORDS.start;
/// One trainer and adventure record per game version.
pub const RECORD_SIZE: usize = 0x44;
pub const DEX_OFFSET: usize = POKEDEX.start;
pub const VALIDITY_OFFSET: usize = DEX_OFFSET + 0x7254;
const FAMILY_OFFSETS: [usize; 9] = [
    8, 0x6A8, 0xD48, 0x1F18, 0x30E8, 0x4060, 0x4FE0, 0x5F58, 0x6F18,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DexError {
    NativeObject,
    UnsupportedFormat,
    ChangedSelection,
}

/// Diagnostic values for the actual native regions, without storing Pokémon or
/// trainer data in logs. Each family digest includes its records and Dex bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub validity_mask: u8,
    pub family_crc32: [u32; 8],
}

pub fn evidence(body: &[u8]) -> Result<Evidence, DexError> {
    if body.len() != BLOB_SIZE {
        return Err(DexError::NativeObject);
    }
    let mut families = [0; 8];
    for (index, value) in families.iter_mut().enumerate() {
        let records = RECORDS_OFFSET + index * RECORD_SIZE;
        let dex = DEX_OFFSET + FAMILY_OFFSETS[index];
        let end = DEX_OFFSET + FAMILY_OFFSETS[index + 1];
        // Keep both CRCs in one unambiguous fixed-size input.
        let mut parts = [0; 8];
        parts[..4].copy_from_slice(&crc32(&body[records..records + RECORD_SIZE]).to_le_bytes());
        parts[4..].copy_from_slice(&crc32(&body[dex..end]).to_le_bytes());
        *value = crc32(&parts);
    }
    Ok(Evidence {
        validity_mask: body[VALIDITY_OFFSET],
        family_crc32: families,
    })
}

/// What a game's version slot in the Bank holds, for the selected game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// No game of this version was ever taken over.
    Empty,
    /// The selected game's own trainer.
    Same,
    /// Another trainer's game of this version.
    Other,
}

/// Whether Save and Quit takes the selected game's records and Pokédex
/// over by itself: only into a slot that is this trainer's already. An
/// empty slot and another trainer's are the records screen's to decide.
pub fn refreshed_by_a_save(slot: Slot) -> bool {
    slot == Slot::Same
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use bank_common::native_bank::NativeBank;
    use core::mem::transmute;
    use patch_common::native::pointer as native_pointer;
    use patch_common::native::read_word as word;

    /// The application object's pointer, whose manager is at `+0x1c`.
    const APPLICATION: *const u32 = 0x003a_b90c as *const u32;
    /// The records screen's collection of the selected game's data:
    /// `0026de9c(selection)`.
    const COLLECT: usize = 0x0026_de9c;
    /// The records screen's two questions about a version slot,
    /// `001e1e58(records, version)` "is it there" and `002cb7b8(records,
    /// version, name, id, secret)` "is it this trainer", and its two imports,
    /// `001e1ba8(records, version, words)` and `001e1b94(records, version,
    /// dex)`.
    const SLOT_EXISTS: usize = 0x001e_1e58;
    const SLOT_MATCHES: usize = 0x002c_b7b8;
    const COPY_RECORDS: usize = 0x001e_1ba8;
    const COPY_DEX: usize = 0x001e_1b94;
    /// After a restore: `002b9a3c(dex)` reloads the eight version caches,
    /// `002b9b90(dex)` has the totals built anew.
    const RELOAD: usize = 0x002b_9a3c;
    const INVALIDATE: usize = 0x002b_9b90;

    fn pointer(value: u32) -> Result<*mut u8, DexError> {
        native_pointer(value).ok_or(DexError::NativeObject)
    }
    unsafe fn helpers(bank: *mut u8) -> Result<(*mut u8, *mut u8), DexError> {
        pointer(bank as u32)?;
        {
            let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
            let body = view.snapshot().map_err(|_| DexError::NativeObject)?;
            if body.as_bytes()[DEX_OFFSET + 4..DEX_OFFSET + 8] != [0; 4] {
                return Err(DexError::UnsupportedFormat);
            }
        }
        let records = pointer(unsafe { word(bank, 0xBB52C) })?;
        let dex = pointer(unsafe { word(records, 8) })?;
        if unsafe { word(records, 4) } != bank as u32 || unsafe { word(dex, 4) } != bank as u32 {
            return Err(DexError::NativeObject);
        }
        for offset in (8..=0x24).step_by(4) {
            pointer(unsafe { word(dex, offset) })?;
        }
        Ok((records, dex))
    }
    unsafe fn current() -> Result<(*mut u8, *mut u8), DexError> {
        let application = pointer(unsafe { APPLICATION.read() })?;
        let manager = pointer(unsafe { word(application, 0x1C) })?;
        let bank = pointer(unsafe { word(manager, 0xCC) })?;
        Ok((manager, bank))
    }

    /// Only the fields populated by native 0026de9c are used. That function
    /// neither requires a constructed UI nor accesses its uninitialized fields.
    #[repr(C, align(4))]
    struct Selection {
        words: [u32; 0x94 / 4],
    }
    impl Selection {
        unsafe fn capture(manager: *mut u8, bank: *mut u8) -> Result<Self, DexError> {
            let (actual_manager, actual_bank) = unsafe { current() }?;
            if (manager, bank) != (actual_manager, actual_bank) {
                return Err(DexError::ChangedSelection);
            }
            unsafe { helpers(bank) }?;
            unsafe { crate::native_game::selected(manager) }.map_err(|_| DexError::NativeObject)?;
            let mut selected = Self {
                words: [0; 0x94 / 4],
            };
            // Unmapped trainer game versions must not default to family X.
            unsafe { selected.raw().add(0x4C).write(0xFF) };
            let collect: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(COLLECT) };
            unsafe { collect(selected.raw()) };
            if unsafe { selected.raw().add(0x4C).read() } >= 8
                || unsafe { word(selected.raw(), 0x48) } != unsafe { word(bank, 0xBB52C) }
            {
                return Err(DexError::NativeObject);
            }
            pointer(unsafe { word(selected.raw(), 0x40) })?;
            Ok(selected)
        }
        fn raw(&mut self) -> *mut u8 {
            self.words.as_mut_ptr().cast()
        }
        /// The version of the selected game, 0 to 7.
        fn family(&mut self) -> u32 {
            u32::from(unsafe { self.raw().add(0x4C).read() })
        }
        /// What the Bank's slot of that version holds: the original's own
        /// two questions, "is it there" (001e1e58) and "is it this
        /// trainer" (002cb7b8), as its records screen asks them.
        unsafe fn slot(&mut self) -> Slot {
            let records = unsafe { word(self.raw(), 0x48) } as *mut u8;
            let exists: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
                unsafe { transmute(SLOT_EXISTS) };
            let matches: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8, u32, u32) -> u32 =
                unsafe { transmute(SLOT_MATCHES) };
            let family = self.family();
            if unsafe { exists(records, family) } == 0 {
                return Slot::Empty;
            }
            let same = unsafe {
                matches(
                    records,
                    family,
                    self.raw().add(0x4E),
                    word(self.raw(), 0x68),
                    word(self.raw(), 0x6C),
                )
            };
            if same != 0 {
                Slot::Same
            } else {
                Slot::Other
            }
        }
        /// The records screen's import for the same trainer: the nine
        /// record words (001e1ba8) and the Pokédex (001e1b94).
        unsafe fn import(&mut self) {
            let records = unsafe { word(self.raw(), 0x48) } as *mut u8;
            let family = self.family();
            let copy_records: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) =
                unsafe { transmute(COPY_RECORDS) };
            let copy_dex: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) =
                unsafe { transmute(COPY_DEX) };
            unsafe {
                copy_records(records, family, self.raw().add(0x70));
                copy_dex(records, family, word(self.raw(), 0x40) as *mut u8);
            }
        }
    }

    fn digest(bank: *mut u8) -> Result<Evidence, DexError> {
        let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
        evidence(
            view.snapshot()
                .map_err(|_| DexError::NativeObject)?
                .as_bytes(),
        )
    }

    /// At Save and Quit, before the Bank is staged: takes the selected
    /// game's records and Pokédex into its version slot if that slot is
    /// this trainer's already (`refreshed_by_a_save`). `Ok(false)` when
    /// nothing was taken, also when the game could not be asked: then the
    /// save goes on with the Pokédex as it is. An error only if the import
    /// changed anything but that one version, which must stop the save.
    ///
    /// # Safety
    /// Main UI thread only, after the native Save-and-Quit confirmation.
    /// manager and bank must be the selected session's exclusively
    /// accessible native objects, with no outstanding game/storage worker
    /// access.
    pub unsafe fn refresh(manager: *mut u8, bank: *mut u8) -> Result<bool, DexError> {
        let Ok(mut selected) = (unsafe { Selection::capture(manager, bank) }) else {
            return Ok(false);
        };
        if !refreshed_by_a_save(unsafe { selected.slot() }) {
            return Ok(false);
        }
        let family = selected.family() as usize;
        let before = digest(bank)?;
        unsafe { selected.import() };
        let after = digest(bank)?;
        for index in 0..8 {
            if index != family && before.family_crc32[index] != after.family_crc32[index] {
                return Err(DexError::NativeObject);
            }
        }
        Ok(true)
    }

    /// Reloads all eight native family objects and invalidates aggregate caches.
    /// Call after a validated persisted-body restore, before returning to native
    /// views.
    ///
    /// # Safety
    /// Main UI thread only; bank must be the live, exclusively accessible native
    /// object for the verified binary. No storage worker may access it.
    pub unsafe fn refresh_after_restore(bank: *mut u8) -> Result<(), DexError> {
        let (_, dex) = unsafe { helpers(bank) }?;
        let reload: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(RELOAD) };
        let invalidate: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(INVALIDATE) };
        unsafe {
            reload(dex);
            invalidate(dex);
        }
        Ok(())
    }

    /// The trainer of the selected game as the original reads it for its
    /// records (0026de9c): the name with its end mark, and the game's 32-bit
    /// trainer ID. Nothing of the Bank is changed.
    ///
    /// # Safety
    /// Main UI thread only, with a game selected. manager and bank must be
    /// the session's exclusively accessible native objects, with no
    /// outstanding game/storage worker access.
    pub unsafe fn trainer(manager: *mut u8, bank: *mut u8) -> Result<([u16; 13], u32), DexError> {
        let mut selected = unsafe { Selection::capture(manager, bank) }?;
        let mut name = [0; 13];
        for (index, unit) in name.iter_mut().enumerate() {
            *unit = unsafe { selected.raw().add(0x4E + index * 2).cast::<u16>().read() };
        }
        Ok((name, unsafe { word(selected.raw(), 0x68) }))
    }
}
#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;

    fn fixture() -> std::vec::Vec<u8> {
        let mut body = vec![0; BLOB_SIZE];
        body[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        body[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        body
    }

    #[test]
    fn a_save_refreshes_only_a_slot_that_is_this_trainers_already() {
        assert!(refreshed_by_a_save(Slot::Same));
        // A game the Bank does not know, and another trainer's: the records
        // screen decides, as in the original.
        assert!(!refreshed_by_a_save(Slot::Empty));
        assert!(!refreshed_by_a_save(Slot::Other));
    }

    #[test]
    fn evidence_tells_one_changed_version_and_a_restored_body() {
        let committed = fixture();
        let before = evidence(&committed).unwrap();
        let mut live = committed.clone();
        for (family, &dex_offset) in FAMILY_OFFSETS.iter().take(8).enumerate() {
            // Both the Pokédex and adventure records must detect a change
            // in this family alone; restoring must recover the evidence.
            for offset in [
                DEX_OFFSET + dex_offset,
                RECORDS_OFFSET + family * RECORD_SIZE,
            ] {
                live[offset] = 7;
                let changed = evidence(&live).unwrap();
                assert_eq!(changed.validity_mask, before.validity_mask);
                for index in 0..8 {
                    assert_eq!(
                        changed.family_crc32[index] != before.family_crc32[index],
                        index == family
                    );
                }
                // Leaving without saving reloads the committed body.
                live.copy_from_slice(&committed);
                assert_eq!(evidence(&live).unwrap(), before);
            }
        }
        assert_eq!(evidence(&live[1..]), Err(DexError::NativeObject));
    }

    #[test]
    fn evidence_keeps_validity_separate_from_family_digests() {
        let mut body = fixture();
        let before = evidence(&body).unwrap();
        for family in 0..8 {
            body[VALIDITY_OFFSET] = 1 << family;
            let changed = evidence(&body).unwrap();
            assert_eq!(changed.validity_mask, 1 << family);
            assert_eq!(changed.family_crc32, before.family_crc32);
        }
    }
}
