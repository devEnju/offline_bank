//! An exclusive view of the live native Bank object for the verified revision.
//!
//! Native 002CB870 saves `object + 8`, length 0xBB518. Native 0023650C
//! restores the same range using memcpy. Copying that range in Rust preserves
//! the object's vtable, reserved header, and four runtime helper pointers.
//! The original constructor must have run before this adapter is used.

use core::{marker::PhantomData, ptr::NonNull};
use offline_core::native_blob::{BlobError, NativeBlobView, BLOB_SIZE, MILES_OFFSET};

pub const OBJECT_SIZE: usize = 0xBB530;
pub const BODY_OFFSET: usize = 8;
pub const VTABLE_ADDRESS: u32 = 0x0036_26FC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeBankError {
    NullObject,
    UnalignedObject,
    WrongVtable(u32),
    Blob(BlobError),
}

/// Holds the exclusive borrow promised by the native hook. No allocation,
/// destruction, initialization, game-save write, or storage commit occurs here.
pub struct NativeBank<'a> {
    pointer: NonNull<u8>,
    exclusive: PhantomData<&'a mut [u8; OBJECT_SIZE]>,
}

impl<'a> NativeBank<'a> {
    /// Borrows an already-constructed Bank object for one paused native state.
    ///
    /// # Safety
    /// `pointer` must identify OBJECT_SIZE initialized, writable bytes for the
    /// entire borrow. The native constructor must have completed, and no native
    /// thread, callback, Rust reference, or second adapter may access the object
    /// during this exclusive borrow. The allocation may not move or be freed.
    /// The matching executable must already have been verified by the builder.
    pub unsafe fn from_raw(pointer: *mut u8) -> Result<Self, NativeBankError> {
        let pointer = NonNull::new(pointer).ok_or(NativeBankError::NullObject)?;
        if pointer.as_ptr() as usize & 3 != 0 {
            return Err(NativeBankError::UnalignedObject);
        }
        // SAFETY: the caller guarantees a live initialized object; four-byte
        // alignment was checked above. Bank stores a 32-bit ARM vtable address.
        let vtable = unsafe { pointer.as_ptr().cast::<u32>().read() }.to_le();
        if vtable != VTABLE_ADDRESS {
            return Err(NativeBankError::WrongVtable(vtable));
        }
        let object = Self {
            pointer,
            exclusive: PhantomData,
        };
        object.snapshot()?;
        Ok(object)
    }

    /// Captures the existing native representation, including a fresh empty
    /// Bank produced by the original constructor. Does not synthesize records.
    pub fn snapshot(&self) -> Result<NativeBlobView<'_>, NativeBankError> {
        // SAFETY: from_raw's exclusive lifetime contract covers this subrange.
        let bytes = unsafe {
            core::slice::from_raw_parts(self.pointer.as_ptr().add(BODY_OFFSET), BLOB_SIZE)
        };
        NativeBlobView::parse_layout(bytes).map_err(NativeBankError::Blob)
    }

    /// Equivalent to the body-copy portion of download completion 002D11B0:
    /// retain flags detected by local game scanning before the persisted load.
    pub fn restore_preserving_detected_games(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), NativeBankError> {
        let previous = self.snapshot()?;
        // Native helper +4 points at the Bank object, not the serialized body.
        // 002BA554/574 read object+183/BB420; 001D5208/51F0 set these to 1.
        let sun_moon = previous.as_bytes()[0x17B] == 1;
        let ultra = previous.as_bytes()[0xBB418] == 1;
        self.restore(bytes)?;
        // SAFETY: restore validated the body; the exclusive object borrow lives.
        unsafe {
            if sun_moon {
                self.pointer.as_ptr().add(0x183).write(1);
            }
            if ultra {
                self.pointer.as_ptr().add(0xBB420).write(1);
            }
        }
        Ok(())
    }

    /// Preserves the positive transfer counters updated by native 002B2320
    /// immediately before serialization. Both fields are written if either
    /// destination's count increased; an unchanged session leaves them alone.
    pub fn record_transfer_counts(
        &mut self,
        previous_game: u32,
        current_game: u32,
        previous_bank: u32,
        current_bank: u32,
    ) {
        let game_delta = current_game.saturating_sub(previous_game);
        let bank_delta = current_bank.saturating_sub(previous_bank);
        if game_delta != 0 || bank_delta != 0 {
            // SAFETY: aligned u16 fields are inside the exclusively borrowed
            // native body. Conversion reproduces the native halfword stores.
            unsafe {
                self.pointer
                    .as_ptr()
                    .add(0xB4AA4)
                    .cast::<u16>()
                    .write((bank_delta as u16).to_le());
                self.pointer
                    .as_ptr()
                    .add(0xB4AA6)
                    .cast::<u16>()
                    .write((game_delta as u16).to_le());
            }
        }
    }
    /// Whole Poké Miles as native getter 001d588c reports them.
    pub fn miles(&self) -> Result<u32, NativeBankError> {
        Ok(self.snapshot()?.miles())
    }

    /// Equivalent to native setter 001d59fc, including its 65,535 limit.
    pub fn set_miles(&mut self, miles: u32) {
        // SAFETY: the aligned u32 lies inside the exclusively borrowed body.
        unsafe {
            self.pointer
                .as_ptr()
                .add(BODY_OFFSET + MILES_OFFSET)
                .cast::<u32>()
                .write(miles.min(0xffff).to_le());
        }
    }

    /// Restores exactly the native serializer's body. A rejected snapshot leaves
    /// the object untouched. Storage integrity/recovery must succeed first;
    /// layout validation alone is not a substitute for that transaction check.
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), NativeBankError> {
        let source = NativeBlobView::parse_layout(bytes).map_err(NativeBankError::Blob)?;
        // SAFETY: from_raw grants exclusive writable ownership of this range.
        // The source borrow cannot alias it through safe use of this adapter.
        let body = unsafe {
            core::slice::from_raw_parts_mut(self.pointer.as_ptr().add(BODY_OFFSET), BLOB_SIZE)
        };
        source.copy_to(body).map_err(NativeBankError::Blob)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    // This is a layout fixture, deliberately not a playable empty Bank.
    fn object_fixture() -> Vec<u32> {
        let mut object = vec![0xA5A5_A5A5; OBJECT_SIZE / 4];
        object[0] = VTABLE_ADDRESS.to_le();
        let blob = body_fixture(0x72);
        // SAFETY: the u32 allocation provides sufficient initialized bytes and
        // no references to it coexist during this temporary view.
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(object.as_mut_ptr().cast::<u8>(), OBJECT_SIZE)
        };
        bytes[BODY_OFFSET..BODY_OFFSET + BLOB_SIZE].copy_from_slice(&blob);
        object
    }

    fn body_fixture(fill: u8) -> Vec<u8> {
        let mut blob = vec![fill; BLOB_SIZE];
        blob[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        blob[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        blob
    }

    #[test]
    fn restores_body_and_preserves_native_header_and_helpers() {
        let mut object = object_fixture();
        let pointer = object.as_mut_ptr().cast::<u8>();
        let replacement = body_fixture(0x35);
        {
            // SAFETY: exclusive borrowing of the initialized aligned fixture.
            let mut bank = unsafe { NativeBank::from_raw(pointer) }.unwrap();
            assert_eq!(bank.snapshot().unwrap().as_bytes()[0], 0x72);
            bank.restore(&replacement).unwrap();
            assert_eq!(bank.snapshot().unwrap().as_bytes(), replacement);
        }
        assert_eq!(object[0], VTABLE_ADDRESS.to_le());
        assert_eq!(object[1], 0xA5A5_A5A5);
        assert!(object[(BODY_OFFSET + BLOB_SIZE) / 4..]
            .iter()
            .all(|&word| word == 0xA5A5_A5A5));
    }

    #[test]
    fn download_preserves_detected_versions_and_transfer_delta_semantics() {
        let mut object = object_fixture();
        // SAFETY: exclusive borrowing of the initialized aligned fixture.
        let mut bank = unsafe { NativeBank::from_raw(object.as_mut_ptr().cast()) }.unwrap();
        let mut local = body_fixture(0);
        local[0x17B] = 1;
        local[0xBB418] = 1;
        bank.restore(&local).unwrap();
        let persisted = body_fixture(0);
        bank.restore_preserving_detected_games(&persisted).unwrap();
        let loaded = bank.snapshot().unwrap();
        assert_eq!(loaded.as_bytes()[0x17B], 1);
        assert_eq!(loaded.as_bytes()[0xBB418], 1);
        bank.record_transfer_counts(30, 27, 10, 13);
        assert_eq!(
            &bank.snapshot().unwrap().as_bytes()[0xB4A9C..0xB4AA0],
            &[3, 0, 0, 0]
        );
        bank.record_transfer_counts(30, 30, 10, 10);
        assert_eq!(
            &bank.snapshot().unwrap().as_bytes()[0xB4A9C..0xB4AA0],
            &[3, 0, 0, 0]
        );
        bank.record_transfer_counts(30, 32, 10, 8);
        assert_eq!(
            &bank.snapshot().unwrap().as_bytes()[0xB4A9C..0xB4AA0],
            &[0, 0, 2, 0]
        );
    }
    #[test]
    fn miles_use_native_object_offset_and_limit_without_touching_neighbors() {
        let mut object = object_fixture();
        // SAFETY: exclusive borrowing of the initialized aligned fixture.
        let mut bank = unsafe { NativeBank::from_raw(object.as_mut_ptr().cast()) }.unwrap();
        bank.restore(&body_fixture(0)).unwrap();
        bank.set_miles(1234);
        assert_eq!(bank.miles(), Ok(1234));
        bank.set_miles(70_000);
        assert_eq!(bank.miles(), Ok(0xffff));
        let mut expected = body_fixture(0);
        expected[0x170..0x174].copy_from_slice(&0xffff_u32.to_le_bytes());
        assert_eq!(bank.snapshot().unwrap().as_bytes(), expected);
        // Native accessors address this field as object + 0x178.
        assert_eq!(object[0x178 / 4], 0xffff_u32.to_le());
    }

    #[test]
    fn invalid_snapshot_cannot_partly_replace_live_body() {
        let mut object = object_fixture();
        // SAFETY: exclusive borrowing of the initialized aligned fixture.
        let mut bank = unsafe { NativeBank::from_raw(object.as_mut_ptr().cast()) }.unwrap();
        let mut replacement = body_fixture(0x35);
        replacement[0x15C] = 99;
        assert!(bank.restore(&replacement).is_err());
        assert!(bank.restore(&replacement[..BLOB_SIZE - 1]).is_err());
        assert_eq!(bank.snapshot().unwrap().as_bytes(), body_fixture(0x72));
    }

    #[test]
    fn rejects_wrong_object_type_and_alignment_before_body_access() {
        let mut object = object_fixture();
        object[0] = 0;
        // SAFETY: memory remains valid; the wrong type is expected to be rejected.
        assert!(matches!(
            unsafe { NativeBank::from_raw(object.as_mut_ptr().cast()) },
            Err(NativeBankError::WrongVtable(0))
        ));
        // SAFETY: the adapter rejects this unaligned pointer before dereferencing.
        let unaligned = unsafe { object.as_mut_ptr().cast::<u8>().add(1) };
        assert!(matches!(
            unsafe { NativeBank::from_raw(unaligned) },
            Err(NativeBankError::UnalignedObject)
        ));
        // SAFETY: a null pointer is rejected before dereferencing.
        assert!(matches!(
            unsafe { NativeBank::from_raw(core::ptr::null_mut()) },
            Err(NativeBankError::NullObject)
        ));
    }
}
