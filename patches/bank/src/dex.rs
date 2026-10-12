//! What the patch needs of the original's Pokédex and adventure records.
//!
//! The original imports a game's Pokédex and records in its records screen
//! (task update 0026d8ec), and that screen runs unchanged. This module reloads
//! the original's caches after a body restore, reads the selected game's
//! trainer through native 0026de9c, and digests the regions for the save and
//! restore checks. No function in this module changes the Bank's Pokédex,
//! writes a game file or writes the offline Bank store.

use offline_core::{crc32, native_blob::BLOB_SIZE};

pub const RECORDS_OFFSET: usize = 0xAD61C;
pub const RECORD_SIZE: usize = 0x44;
pub const DEX_OFFSET: usize = 0xAD83C;
pub const DEX_SIZE: usize = 0x7260;
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
    pub dex_crc32: u32,
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
        dex_crc32: crc32(&body[DEX_OFFSET..DEX_OFFSET + DEX_SIZE]),
        family_crc32: families,
    })
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use bank_common::native_bank::NativeBank;
    use core::mem::transmute;

    unsafe fn word(base: *mut u8, offset: usize) -> u32 {
        unsafe { base.add(offset).cast::<u32>().read() }
    }
    fn pointer(value: u32) -> Result<*mut u8, DexError> {
        if value == 0 || value & 3 != 0 {
            Err(DexError::NativeObject)
        } else {
            Ok(value as *mut u8)
        }
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
        let application = pointer(unsafe { (0x003A_B90C as *const u32).read() })?;
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
            let get_game: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
                unsafe { transmute(0x0023_3A6Cusize) };
            let game = unsafe { get_game(manager) };
            pointer(game as u32)?;
            if !(1..=8).contains(&unsafe { game.add(8).read() }) {
                return Err(DexError::NativeObject);
            }
            let mut selected = Self {
                words: [0; 0x94 / 4],
            };
            // Unmapped trainer game versions must not default to family X.
            unsafe { selected.raw().add(0x4C).write(0xFF) };
            let collect: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x0026_DE9Cusize) };
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
        let reload: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002B_9A3Cusize) };
        let invalidate: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002B_9B90usize) };
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
    fn evidence_tells_one_changed_version_and_a_restored_body() {
        let committed = fixture();
        let before = evidence(&committed).unwrap();
        let mut live = committed.clone();
        live[DEX_OFFSET + FAMILY_OFFSETS[1]] = 7;
        live[VALIDITY_OFFSET] |= 2;
        let changed = evidence(&live).unwrap();
        assert_ne!(changed, before);
        for index in 0..8 {
            assert_eq!(
                changed.family_crc32[index] != before.family_crc32[index],
                index == 1
            );
        }
        // Leaving without saving reloads the committed body.
        live.copy_from_slice(&committed);
        assert_eq!(evidence(&live).unwrap(), before);
        assert_eq!(evidence(&live[1..]), Err(DexError::NativeObject));
    }
}
