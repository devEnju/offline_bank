//! Read-only views of the serialized Bank body for the reviewed 1.5 image.
//!
//! Evidence: `docs/internals.md`, code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! This module does not synthesize an empty Bank or edit Pokemon. The native
//! constructor depends on additional application/configuration state. Checking
//! this layout does not verify account identity, every field, or save legality.

pub const BLOB_SIZE: usize = 0xBB518;
pub const FORMAT_VERSION: u16 = 2;
pub const BOX_COUNT: usize = 100;
pub const SLOTS_PER_BOX: usize = 30;
pub const POKEMON_SIZE: usize = 0xE8;
pub const BOX_STRIDE: usize = 0x1B56;
pub const BOXES_OFFSET: usize = 0x17C;
pub const EXTRA_SLOTS_OFFSET: usize = 0xAAF14;
pub const SLOT_FORMAT_OFFSET: usize = 0xACA44;
pub const EXTRA_SLOT_FORMAT_OFFSET: usize = 0xAD5FC;
pub const SLOT_METADATA_BYTE_OFFSET: usize = 0xB4AA0;
pub const SLOT_METADATA_WORDS_OFFSET: usize = 0xB5658;
pub const BOX_NAME_SIZE: usize = 0x24;
/// Whole Poké Miles, a u32 at object `+178`. Native accessors 001d588c and
/// 001d59fc clamp it to 65,535.
pub const MILES_OFFSET: usize = 0x170;
const FORMAT_OFFSET: usize = 0x15C;
const BOX_COUNT_OFFSET: usize = 0x15E;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobError {
    WrongSize { expected: usize, actual: usize },
    UnsupportedFormat(u16),
    UnexpectedBoxCount(u16),
    BoxOutOfRange,
    SlotOutOfRange,
}

/// Byte view of one native slot and the metadata moved with it by native code.
/// Tag meanings and the eight-byte field remain intentionally uninterpreted.
#[derive(Debug, Clone, Copy)]
pub struct SlotView<'a> {
    pub encrypted_pokemon: &'a [u8; POKEMON_SIZE],
    pub format_tag_raw: u8,
    pub metadata_byte_raw: u8,
    pub metadata_words_raw: &'a [u8; 8],
}

#[derive(Debug, Clone, Copy)]
pub struct NativeBlobView<'a> {
    data: &'a [u8],
}

impl<'a> NativeBlobView<'a> {
    /// Checks size, native format marker, and the supported 100-box layout.
    /// Does not claim to validate the complete native Bank format.
    pub fn parse_layout(data: &'a [u8]) -> Result<Self, BlobError> {
        exact_size(data.len())?;
        let version = word(data, FORMAT_OFFSET);
        if version != FORMAT_VERSION {
            return Err(BlobError::UnsupportedFormat(version));
        }
        let count = word(data, BOX_COUNT_OFFSET);
        if count != BOX_COUNT as u16 {
            return Err(BlobError::UnexpectedBoxCount(count));
        }
        Ok(Self { data })
    }

    pub fn as_bytes(self) -> &'a [u8] {
        self.data
    }

    /// The stored whole-Miles balance, clamped like the native getter.
    pub fn miles(self) -> u32 {
        let at = MILES_OFFSET;
        u32::from_le_bytes(self.data[at..at + 4].try_into().unwrap()).min(0xffff)
    }

    /// Copies every serialized byte, including fields whose meanings are unknown.
    /// This copies a blob buffer, never the surrounding native object or pointers.
    /// Does not invoke the application's load/completion callbacks.
    pub fn copy_to(self, destination: &mut [u8]) -> Result<(), BlobError> {
        exact_size(destination.len())?;
        destination.copy_from_slice(self.data);
        Ok(())
    }

    pub fn slot(self, box_index: usize, slot_index: usize) -> Result<SlotView<'a>, BlobError> {
        check_box(box_index)?;
        check_slot(slot_index)?;
        let flat_index = box_index * SLOTS_PER_BOX + slot_index;
        let at = BOXES_OFFSET + box_index * BOX_STRIDE + slot_index * POKEMON_SIZE;
        let metadata_at = SLOT_METADATA_WORDS_OFFSET + flat_index * 8;
        Ok(SlotView {
            encrypted_pokemon: self.data[at..at + POKEMON_SIZE].try_into().unwrap(),
            format_tag_raw: self.data[SLOT_FORMAT_OFFSET + flat_index],
            metadata_byte_raw: self.data[SLOT_METADATA_BYTE_OFFSET + flat_index],
            metadata_words_raw: self.data[metadata_at..metadata_at + 8].try_into().unwrap(),
        })
    }

    /// Returns the native 36-byte box-name field without text conversion.
    pub fn box_name_bytes(self, box_index: usize) -> Result<&'a [u8; BOX_NAME_SIZE], BlobError> {
        check_box(box_index)?;
        let at = BOXES_OFFSET + box_index * BOX_STRIDE + SLOTS_PER_BOX * POKEMON_SIZE;
        Ok(self.data[at..at + BOX_NAME_SIZE].try_into().unwrap())
    }

    pub fn box_index_field(self, box_index: usize) -> Result<u16, BlobError> {
        check_box(box_index)?;
        let at =
            BOXES_OFFSET + box_index * BOX_STRIDE + SLOTS_PER_BOX * POKEMON_SIZE + BOX_NAME_SIZE;
        Ok(word(self.data, at))
    }

    /// The native initializer has 30 additional slots. Their workflow is not
    /// implemented here; expose bytes only so callers can preserve/inspect them.
    pub fn extra_slot(self, slot_index: usize) -> Result<(&'a [u8; POKEMON_SIZE], u8), BlobError> {
        check_slot(slot_index)?;
        let at = EXTRA_SLOTS_OFFSET + slot_index * POKEMON_SIZE;
        Ok((
            self.data[at..at + POKEMON_SIZE].try_into().unwrap(),
            self.data[EXTRA_SLOT_FORMAT_OFFSET + slot_index],
        ))
    }
}

/// Native stored-record checksum, observed in 00220E90/00220F00 and 0013D284.
/// XOR-decrypts bytes 8..232 with the EC at 0, then sums 112 little-endian u16
/// words modulo 65536. Block ordering does not affect this sum. This does not
/// establish Pokemon legality, species, format-tag consistency, or other fields.
/// The returned value is compared with the stored u16 at bytes 6..8.
pub fn stored_pokemon_checksum(record: &[u8; POKEMON_SIZE]) -> u16 {
    let mut seed = u32::from_le_bytes(record[..4].try_into().unwrap());
    let mut sum = 0_u16;
    for bytes in record[8..].chunks_exact(2) {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        let plain = u16::from_le_bytes([bytes[0], bytes[1]]) ^ (seed >> 16) as u16;
        sum = sum.wrapping_add(plain);
    }
    sum
}

pub fn stored_pokemon_checksum_matches(record: &[u8; POKEMON_SIZE]) -> bool {
    stored_pokemon_checksum(record) == word(record, 6)
}

fn exact_size(actual: usize) -> Result<(), BlobError> {
    if actual == BLOB_SIZE {
        Ok(())
    } else {
        Err(BlobError::WrongSize {
            expected: BLOB_SIZE,
            actual,
        })
    }
}
fn check_box(index: usize) -> Result<(), BlobError> {
    if index < BOX_COUNT {
        Ok(())
    } else {
        Err(BlobError::BoxOutOfRange)
    }
}
fn check_slot(index: usize) -> Result<(), BlobError> {
    if index < SLOTS_PER_BOX {
        Ok(())
    } else {
        Err(BlobError::SlotOutOfRange)
    }
}
fn word(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    // Structural fixtures only. These are not valid empty Banks or real Pokemon.
    fn fixture() -> Vec<u8> {
        let mut data = vec![0xA5; BLOB_SIZE];
        data[FORMAT_OFFSET..FORMAT_OFFSET + 2].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        data[BOX_COUNT_OFFSET..BOX_COUNT_OFFSET + 2]
            .copy_from_slice(&(BOX_COUNT as u16).to_le_bytes());
        data
    }

    #[test]
    fn rejects_wrong_size_format_and_count_without_panics() {
        for size in [0, FORMAT_OFFSET, 0xACA48, BLOB_SIZE - 1, BLOB_SIZE + 1] {
            assert!(matches!(
                NativeBlobView::parse_layout(&vec![0; size]),
                Err(BlobError::WrongSize { .. })
            ));
        }
        let mut data = fixture();
        data[FORMAT_OFFSET] = 1;
        assert!(matches!(
            NativeBlobView::parse_layout(&data),
            Err(BlobError::UnsupportedFormat(1))
        ));
        data[FORMAT_OFFSET] = 2;
        data[BOX_COUNT_OFFSET] = 99;
        assert!(matches!(
            NativeBlobView::parse_layout(&data),
            Err(BlobError::UnexpectedBoxCount(99))
        ));
    }

    #[test]
    fn final_regular_slot_returns_all_associated_metadata() {
        let mut data = fixture();
        let at = BOXES_OFFSET + 99 * BOX_STRIDE + 29 * POKEMON_SIZE;
        data[at..at + POKEMON_SIZE].fill(0x73);
        data[SLOT_FORMAT_OFFSET + 2999] = 2;
        data[SLOT_METADATA_BYTE_OFFSET + 2999] = 9;
        data[SLOT_METADATA_WORDS_OFFSET + 2999 * 8..SLOT_METADATA_WORDS_OFFSET + 3000 * 8]
            .copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let slot = NativeBlobView::parse_layout(&data)
            .unwrap()
            .slot(99, 29)
            .unwrap();
        assert_eq!(slot.encrypted_pokemon, &[0x73; POKEMON_SIZE]);
        assert_eq!(slot.format_tag_raw, 2);
        assert_eq!(slot.metadata_byte_raw, 9);
        assert_eq!(slot.metadata_words_raw, &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn invalid_indices_cannot_wrap_into_other_fields() {
        let data = fixture();
        let blob = NativeBlobView::parse_layout(&data).unwrap();
        for index in [100, usize::MAX] {
            assert!(matches!(blob.slot(index, 0), Err(BlobError::BoxOutOfRange)));
            assert!(matches!(
                blob.box_name_bytes(index),
                Err(BlobError::BoxOutOfRange)
            ));
            assert!(matches!(
                blob.box_index_field(index),
                Err(BlobError::BoxOutOfRange)
            ));
        }
        for index in [30, usize::MAX] {
            assert!(matches!(
                blob.slot(0, index),
                Err(BlobError::SlotOutOfRange)
            ));
            assert!(matches!(
                blob.extra_slot(index),
                Err(BlobError::SlotOutOfRange)
            ));
        }
    }

    #[test]
    fn box_boundary_and_additional_slots_are_distinct() {
        assert_eq!(BOXES_OFFSET + BOX_COUNT * BOX_STRIDE, EXTRA_SLOTS_OFFSET);
        assert_eq!(
            EXTRA_SLOTS_OFFSET + SLOTS_PER_BOX * POKEMON_SIZE,
            SLOT_FORMAT_OFFSET
        );
        let mut data = fixture();
        let name = BOXES_OFFSET + 99 * BOX_STRIDE + SLOTS_PER_BOX * POKEMON_SIZE;
        data[name..name + BOX_NAME_SIZE].fill(0x22);
        data[name + BOX_NAME_SIZE..name + BOX_NAME_SIZE + 2].copy_from_slice(&99_u16.to_le_bytes());
        data[EXTRA_SLOTS_OFFSET..EXTRA_SLOTS_OFFSET + POKEMON_SIZE].fill(0x55);
        data[EXTRA_SLOT_FORMAT_OFFSET] = 7;
        let blob = NativeBlobView::parse_layout(&data).unwrap();
        assert_eq!(blob.box_name_bytes(99).unwrap(), &[0x22; BOX_NAME_SIZE]);
        assert_eq!(blob.box_index_field(99).unwrap(), 99);
        assert_eq!(blob.extra_slot(0).unwrap(), (&[0x55; POKEMON_SIZE], 7));
    }

    #[test]
    fn whole_blob_copy_preserves_unknown_bytes_and_checks_destination_first() {
        let source = fixture();
        let blob = NativeBlobView::parse_layout(&source).unwrap();
        let mut destination = vec![0; BLOB_SIZE];
        blob.copy_to(&mut destination).unwrap();
        assert_eq!(destination, source);
        let mut short = vec![0xEE; BLOB_SIZE - 1];
        assert!(blob.copy_to(&mut short).is_err());
        assert!(short.iter().all(|&byte| byte == 0xEE));
    }

    #[test]
    fn checksum_rejects_corrupted_payload_and_seed() {
        let mut record = [0_u8; POKEMON_SIZE];
        record[..4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        // Plaintext words 0..111 sum to 6216, modulo 65536.
        record[6..8].copy_from_slice(&6216_u16.to_le_bytes());
        let mut seed = 0x1234_5678_u64;
        for (index, chunk) in record[8..].chunks_exact_mut(2).enumerate() {
            seed = (seed * 0x41C6_4E6D + 0x6073) & 0xFFFF_FFFF;
            chunk.copy_from_slice(&((index as u16) ^ (seed >> 16) as u16).to_le_bytes());
        }
        assert_eq!(stored_pokemon_checksum(&record), 6216);
        assert!(stored_pokemon_checksum_matches(&record));
        record[113] ^= 0x10;
        assert!(!stored_pokemon_checksum_matches(&record));
        record[113] ^= 0x10;
        record[0] ^= 0x80;
        assert!(!stored_pokemon_checksum_matches(&record));
    }

    #[test]
    fn miles_balance_uses_the_native_field_and_limit() {
        let mut data = fixture();
        data[MILES_OFFSET..MILES_OFFSET + 4].copy_from_slice(&1234_u32.to_le_bytes());
        assert_eq!(NativeBlobView::parse_layout(&data).unwrap().miles(), 1234);
        data[MILES_OFFSET..MILES_OFFSET + 4].copy_from_slice(&0x1_0000_u32.to_le_bytes());
        assert_eq!(NativeBlobView::parse_layout(&data).unwrap().miles(), 0xffff);
        const { assert!(MILES_OFFSET + 4 <= BOXES_OFFSET) };
    }

    #[test]
    fn checksum_wraps_at_u16_boundary() {
        let mut record = [0_u8; POKEMON_SIZE];
        let expected = (112_u32 * 0xFFFF) as u16;
        record[6..8].copy_from_slice(&expected.to_le_bytes());
        let mut seed = 0_u64;
        for chunk in record[8..].chunks_exact_mut(2) {
            seed = (seed * 0x41C6_4E6D + 0x6073) & 0xFFFF_FFFF;
            chunk.copy_from_slice(&(0xFFFF ^ (seed >> 16) as u16).to_le_bytes());
        }
        assert_eq!(stored_pokemon_checksum(&record), expected);
    }
}
