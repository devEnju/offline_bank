//! Where each stored file's bytes live inside the native Bank body.
//!
//! The original code loads and saves one `0xBB518`-byte block. The patch keeps
//! three regions of it in separate files and stores each exactly as the
//! original bytes, in the original order:
//!
//! | Region | Body range | File |
//! | --- | --- | --- |
//! | Transport box records | `AAF14..ACA44` (30 x `E8`) | transport |
//! | Transport box format tags | `AD5FC..AD61A` (30 bytes) | transport |
//! | Trainer records and Pokédex | `AD61C..B4A9C` | Pokédex |
//! | Everything else | the four remaining ranges | Bank |
//!
//! Loading is a read into place; saving is the reverse. Nothing is converted.

use crate::{checksum::Crc32, native_blob::BLOB_SIZE, sidecar::Kind};
use core::ops::Range;

pub const TRANSPORT_SLOTS: usize = 30;
pub const RECORD_SIZE: usize = 0xE8;
pub const TRANSPORT_RECORDS: Range<usize> = 0xAAF14..0xACA44;
pub const TRANSPORT_TAGS: Range<usize> = 0xAD5FC..0xAD61A;
pub const DEX: Range<usize> = 0xAD61C..0xB4A9C;
/// The native whole-Miles field. The Bank file stores it as zero; the balance
/// lives in the rewards file with the record it is calculated from.
pub const MILES: Range<usize> = 0x170..0x174;

pub const TRANSPORT_SIZE: usize = TRANSPORT_SLOTS * RECORD_SIZE + TRANSPORT_SLOTS;
pub const DEX_SIZE: usize = DEX.end - DEX.start;
pub const BANK_SIZE: usize = BLOB_SIZE - TRANSPORT_SIZE - DEX_SIZE;

/// The Pokédex side file: one `DEX` region per slot.
pub const DEX_FILE: Kind = Kind {
    magic: *b"BKOFDEX1",
    capacity: DEX_SIZE as u32,
};

/// Body ranges kept in the Bank file, in order.
const BANK_RANGES: [Range<usize>; 4] = [
    0..TRANSPORT_RECORDS.start,
    TRANSPORT_RECORDS.end..TRANSPORT_TAGS.start,
    TRANSPORT_TAGS.end..DEX.start,
    DEX.end..BLOB_SIZE,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionError {
    WrongSize { expected: usize, actual: usize },
    UnsupportedFormat(u16),
    UnexpectedBoxCount(u16),
}

fn sized(actual: usize, expected: usize) -> Result<(), SectionError> {
    if actual == expected {
        Ok(())
    } else {
        Err(SectionError::WrongSize { expected, actual })
    }
}

/// CRC of the Bank file payload, computed on the full body without moving it.
pub fn bank_crc(body: &[u8]) -> Result<u32, SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    let mut crc = Crc32::new();
    for range in BANK_RANGES {
        crc.update(&body[range]);
    }
    Ok(crc.finish())
}

/// Moves the Bank ranges together in place: afterwards `body[..BANK_SIZE]` is
/// the Bank file payload. The side regions are overwritten, so copy them out
/// first.
pub fn compact(body: &mut [u8]) -> Result<(), SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    let mut at = 0;
    for range in BANK_RANGES {
        let length = range.len();
        body.copy_within(range, at);
        at += length;
    }
    Ok(())
}

/// Inverse of `compact`: spreads a payload in `body[..BANK_SIZE]` back to its
/// native offsets. The side regions are zeroed and must be filled by the caller.
pub fn expand(body: &mut [u8]) -> Result<(), SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    let mut at = BANK_SIZE;
    for range in BANK_RANGES.into_iter().rev() {
        at -= range.len();
        body.copy_within(at..at + range.len(), range.start);
    }
    body[TRANSPORT_RECORDS].fill(0);
    body[TRANSPORT_TAGS].fill(0);
    body[DEX].fill(0);
    Ok(())
}

/// Checks a Bank file payload: exact size, native format marker, 100 boxes.
/// These fields keep their native offsets because the first range starts at 0.
pub fn validate_bank(payload: &[u8]) -> Result<(), SectionError> {
    sized(payload.len(), BANK_SIZE)?;
    let word = |at: usize| u16::from_le_bytes([payload[at], payload[at + 1]]);
    if word(0x15C) != 2 {
        return Err(SectionError::UnsupportedFormat(word(0x15C)));
    }
    if word(0x15E) != 100 {
        return Err(SectionError::UnexpectedBoxCount(word(0x15E)));
    }
    Ok(())
}

// Position of each plaintext block inside a stored record, per shuffle value.
// This is the standard Gen 6/7 stored layout; Bank's records use it unchanged.
const BLOCK_POSITION: [u8; 96] = [
    0, 1, 2, 3, 0, 1, 3, 2, 0, 2, 1, 3, 0, 3, 1, 2, 0, 2, 3, 1, 0, 3, 2, 1, 1, 0, 2, 3, 1, 0, 3, 2,
    2, 0, 1, 3, 3, 0, 1, 2, 2, 0, 3, 1, 3, 0, 2, 1, 1, 2, 0, 3, 1, 3, 0, 2, 2, 1, 0, 3, 3, 1, 0, 2,
    2, 3, 0, 1, 3, 2, 0, 1, 1, 2, 3, 0, 1, 3, 2, 0, 2, 1, 3, 0, 3, 1, 2, 0, 2, 3, 1, 0, 3, 2, 1, 0,
];

/// Species of a stored record; zero means the slot is empty. Decrypts only the
/// one word needed, with the same generator as `stored_pokemon_checksum`.
pub fn stored_species(record: &[u8; RECORD_SIZE]) -> u16 {
    let key = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    // `% 24` on a value below 32, without a division routine.
    let shuffle = (key >> 13) & 31;
    let shuffle = if shuffle >= 24 { shuffle - 24 } else { shuffle };
    let block = usize::from(BLOCK_POSITION[shuffle as usize * 4]);
    let word = block * 28;
    let mut seed = key;
    for _ in 0..=word {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
    }
    let at = 8 + word * 2;
    u16::from_le_bytes([record[at], record[at + 1]]) ^ (seed >> 16) as u16
}

/// Occupied slots among whole stored records.
pub fn occupied(records: &[u8]) -> u32 {
    let mut count = 0;
    for record in records.chunks_exact(RECORD_SIZE) {
        if let Ok(record) = record.try_into() {
            count += u32::from(stored_species(record) != 0);
        }
    }
    count
}

/// Occupied transport-box slots in a full body.
pub fn transport_count(body: &[u8]) -> Result<u32, SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    Ok(occupied(&body[TRANSPORT_RECORDS]))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn body() -> Vec<u8> {
        // Every byte differs from its neighbors and from other regions.
        let mut bytes: Vec<u8> = (0..BLOB_SIZE)
            .map(|index| (index as u32).wrapping_mul(2_654_435_761).to_le_bytes()[3])
            .collect();
        bytes[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        bytes
    }

    #[test]
    fn regions_cover_the_body_exactly_once() {
        assert_eq!(TRANSPORT_RECORDS.len(), 30 * 0xE8);
        assert_eq!(TRANSPORT_TAGS.len(), 30);
        assert_eq!(
            (DEX_SIZE, TRANSPORT_SIZE, BANK_SIZE),
            (29_824, 6_990, 730_442)
        );
        let mut covered = vec![0u8; BLOB_SIZE];
        for range in BANK_RANGES
            .into_iter()
            .chain([TRANSPORT_RECORDS, TRANSPORT_TAGS, DEX])
        {
            for mark in &mut covered[range] {
                *mark += 1;
            }
        }
        assert!(covered.iter().all(|count| *count == 1));
        assert!(MILES.end <= BANK_RANGES[0].end);
    }

    #[test]
    fn split_and_join_return_every_byte() {
        let original = body();
        let (dex, records, tags) = (
            original[DEX].to_vec(),
            original[TRANSPORT_RECORDS].to_vec(),
            original[TRANSPORT_TAGS].to_vec(),
        );
        let mut work = original.clone();
        let crc = bank_crc(&work).unwrap();
        compact(&mut work).unwrap();
        assert_eq!(crate::crc32(&work[..BANK_SIZE]), crc);
        validate_bank(&work[..BANK_SIZE]).unwrap();
        // Stale bytes after the payload must not survive the round trip.
        work[BANK_SIZE..].fill(0xEE);
        expand(&mut work).unwrap();
        assert!(work[DEX].iter().all(|byte| *byte == 0));
        work[DEX].copy_from_slice(&dex);
        work[TRANSPORT_RECORDS].copy_from_slice(&records);
        work[TRANSPORT_TAGS].copy_from_slice(&tags);
        assert!(work == original);
    }

    #[test]
    fn bank_payload_is_checked_and_sizes_are_exact() {
        let mut work = body();
        compact(&mut work).unwrap();
        assert!(validate_bank(&work[..BANK_SIZE - 1]).is_err());
        assert!(validate_bank(&work).is_err());
        work[0x15C] = 9;
        assert_eq!(
            validate_bank(&work[..BANK_SIZE]),
            Err(SectionError::UnsupportedFormat(9))
        );
        for size in [0, BANK_SIZE, BLOB_SIZE - 1, BLOB_SIZE + 1] {
            let mut short = vec![0; size];
            assert!(compact(&mut short).is_err());
            assert!(expand(&mut short).is_err());
            assert!(bank_crc(&short).is_err());
            assert!(transport_count(&short).is_err());
        }
    }

    /// Builds a stored record with `species`, shuffled and encrypted by `key`.
    fn record(key: u32, species: u16) -> [u8; RECORD_SIZE] {
        let mut plain = [0u16; 112];
        plain[0] = species;
        plain[30] = 0x1111; // other blocks hold unrelated nonzero data
        plain[60] = 0x2222;
        plain[90] = 0x3333;
        let shuffle = (((key >> 13) & 31) % 24) as usize;
        let mut stored = [0u16; 112];
        for block in 0..4 {
            let position = usize::from(BLOCK_POSITION[shuffle * 4 + block]);
            stored[position * 28..position * 28 + 28]
                .copy_from_slice(&plain[block * 28..block * 28 + 28]);
        }
        let mut out = [0u8; RECORD_SIZE];
        out[..4].copy_from_slice(&key.to_le_bytes());
        let mut seed = key;
        for (index, word) in stored.iter().enumerate() {
            seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
            out[8 + index * 2..10 + index * 2]
                .copy_from_slice(&(word ^ (seed >> 16) as u16).to_le_bytes());
        }
        out
    }

    #[test]
    fn species_is_found_for_every_block_order() {
        for shuffle in 0..32u32 {
            let key = 0x1234_0000 | (shuffle << 13) | 0x155;
            assert_eq!(stored_species(&record(key, 0)), 0, "shuffle {shuffle}");
            assert_eq!(stored_species(&record(key, 151)), 151, "shuffle {shuffle}");
            assert_eq!(stored_species(&record(key, 807)), 807, "shuffle {shuffle}");
        }
    }

    #[test]
    fn transport_count_counts_only_occupied_slots() {
        let mut work = vec![0u8; BLOB_SIZE];
        for slot in 0..TRANSPORT_SLOTS {
            let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
            work[at..at + RECORD_SIZE].copy_from_slice(&record(0x9000 + slot as u32, 0));
        }
        assert_eq!(transport_count(&work), Ok(0));
        for slot in [0, 7, 29] {
            let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
            work[at..at + RECORD_SIZE].copy_from_slice(&record(0x77_0000 + slot as u32, 25));
        }
        assert_eq!(transport_count(&work), Ok(3));
    }
}
