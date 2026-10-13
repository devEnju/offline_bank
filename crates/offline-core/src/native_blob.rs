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
pub const POKEMON_SIZE: usize = 0xE8;
pub const BOXES_OFFSET: usize = 0x17C;
pub const EXTRA_SLOT_FORMAT_OFFSET: usize = 0xAD5FC;
/// Whole Poké Miles, a u32 at object `+178`. Native accessors 001d588c and
/// 001d59fc clamp it to 65,535.
pub const MILES_OFFSET: usize = 0x170;
/// The reward date, six bytes at object `+168` (native 001d5880): the date
/// and time up to which Miles are counted.
pub const REWARD_DATE_OFFSET: usize = 0x160;
/// The day of the last redemption, four bytes at object `+17C`. The original
/// keeps the server's pass count there (002ba524, 002ba53c) and reaches it
/// only while the session reports a count, which never happens offline.
pub const REDEEMED_OFFSET: usize = 0x174;
/// The remainder below one Mile, two bytes of padding between the transport
/// box's format tags and the trainer records that the original never touches.
pub const FRACTION_OFFSET: usize = 0xAD61A;
const FORMAT_OFFSET: usize = 0x15C;
const BOX_COUNT_OFFSET: usize = 0x15E;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobError {
    WrongSize { expected: usize, actual: usize },
    UnsupportedFormat(u16),
    UnexpectedBoxCount(u16),
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

    /// The stored reward date; `None` while the body holds no date.
    pub fn reward_date(self) -> Option<crate::rewards::Moment> {
        let at = REWARD_DATE_OFFSET;
        crate::rewards::Moment::from_native(
            self.data[at..at + crate::rewards::Moment::NATIVE_SIZE]
                .try_into()
                .unwrap(),
        )
    }

    /// What the patch keeps for the Miles beside the balance and the date.
    pub fn rewards(self) -> crate::rewards::Record {
        crate::rewards::Record::from_fields(
            self.data[REDEEMED_OFFSET..REDEEMED_OFFSET + 4]
                .try_into()
                .unwrap(),
            self.data[FRACTION_OFFSET..FRACTION_OFFSET + 2]
                .try_into()
                .unwrap(),
        )
    }

    /// Copies every serialized byte, including fields whose meanings are unknown.
    /// This copies a blob buffer, never the surrounding native object or pointers.
    /// Does not invoke the application's load/completion callbacks.
    pub fn copy_to(self, destination: &mut [u8]) -> Result<(), BlobError> {
        exact_size(destination.len())?;
        destination.copy_from_slice(self.data);
        Ok(())
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
        const { assert!(REWARD_DATE_OFFSET + crate::rewards::Moment::NATIVE_SIZE <= MILES_OFFSET) };
        const { assert!(MILES_OFFSET + 4 == REDEEMED_OFFSET) };
        const { assert!(FRACTION_OFFSET == EXTRA_SLOT_FORMAT_OFFSET + 30) };
        assert_eq!(
            NativeBlobView::parse_layout(&data).unwrap().reward_date(),
            None
        );
        let at = REWARD_DATE_OFFSET;
        data[at..at + 6].copy_from_slice(&[0xea, 0x07, 10, 6, 18, 10]);
        let date = NativeBlobView::parse_layout(&data).unwrap().reward_date();
        assert_eq!(date.unwrap().to_native(), [0xea, 0x07, 10, 6, 18, 10]);
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
