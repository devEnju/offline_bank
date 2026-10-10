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

/// What makes one Pokémon that Pokémon wherever its record is stored and
/// whatever else about it changes: the encryption constant, the personality
/// value, and the original trainer's ID and secret ID.
pub type Identity = [u32; 3];

/// The identity of a stored record; `None` for an empty slot. The three
/// fields besides the encryption constant lie in the block that starts with
/// the species (words 2, 3 and 8..10 of it).
pub fn stored_identity(record: &[u8; RECORD_SIZE]) -> Option<Identity> {
    let key = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    let shuffle = (key >> 13) & 31;
    let shuffle = if shuffle >= 24 { shuffle - 24 } else { shuffle };
    let first = usize::from(BLOCK_POSITION[shuffle as usize * 4]) * 28;
    let mut plain = [0u16; 10];
    let mut seed = key;
    for word in 0..first + plain.len() {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        if word >= first {
            let at = 8 + word * 2;
            plain[word - first] =
                u16::from_le_bytes([record[at], record[at + 1]]) ^ (seed >> 16) as u16;
        }
    }
    if plain[0] == 0 {
        return None;
    }
    Some([
        key,
        u32::from(plain[8]) | u32::from(plain[9]) << 16,
        u32::from(plain[2]) | u32::from(plain[3]) << 16,
    ])
}

/// The 100 boxes in a body or a Bank file payload: 30 records each, then
/// the box's name and index.
pub const BOXES: usize = 100;
pub const BOX_SLOTS: usize = 30;
const BOXES_START: usize = 0x17C;
const BOX_STRIDE: usize = 0x1B56;
/// Most Pokémon one Bank snapshot holds: its boxes and the transport box.
pub const HELD: usize = BOXES * BOX_SLOTS + TRANSPORT_SLOTS;

/// Appends the identity of every occupied record among whole stored records
/// to `out[count..]` and returns the new count. Records beyond the room in
/// `out` are not counted.
pub fn identities(records: &[u8], out: &mut [Identity], mut count: usize) -> usize {
    for record in records.chunks_exact(RECORD_SIZE) {
        let identity = record.try_into().ok().and_then(stored_identity);
        if let (Some(identity), Some(place)) = (identity, out.get_mut(count)) {
            *place = identity;
            count += 1;
        }
    }
    count
}

/// `identities` for the 100 boxes of a Bank file payload.
pub fn box_identities(
    payload: &[u8],
    out: &mut [Identity],
    mut count: usize,
) -> Result<usize, SectionError> {
    sized(payload.len(), BANK_SIZE)?;
    for index in 0..BOXES {
        let at = BOXES_START + index * BOX_STRIDE;
        count = identities(&payload[at..at + BOX_SLOTS * RECORD_SIZE], out, count);
    }
    Ok(count)
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

    /// Like `record`, with the fields of an identity and others beside them.
    fn pokemon(key: u32, species: u16, identity: (u32, u16, u16), other: u16) -> [u8; RECORD_SIZE] {
        let mut plain = [0u16; 112];
        plain[0] = species;
        plain[1] = other; // held item
        plain[2] = identity.1;
        plain[3] = identity.2;
        plain[4] = other; // experience
        plain[8] = identity.0 as u16;
        plain[9] = (identity.0 >> 16) as u16;
        plain[30] = other;
        plain[90] = other;
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
    fn identity_is_found_for_every_block_order_and_ignores_everything_else() {
        for shuffle in 0..32u32 {
            let key = 0x4321_0000 | (shuffle << 13) | 0x0aa;
            let identity = (0x89ab_cdef, 0x1234, 0x5678);
            let expected = Some([key, 0x89ab_cdef, 0x5678_1234]);
            assert_eq!(
                stored_identity(&pokemon(key, 25, identity, 0)),
                expected,
                "shuffle {shuffle}"
            );
            // Evolved, levelled, given an item: the same Pokémon.
            assert_eq!(
                stored_identity(&pokemon(key, 26, identity, 0x7e57)),
                expected,
                "shuffle {shuffle}"
            );
            // An empty slot is nobody, whatever else it holds.
            assert_eq!(stored_identity(&pokemon(key, 0, identity, 9)), None);
            // Each field counts.
            for other in [(0x89ab_cdee, 0x1234, 0x5678), (0x89ab_cdef, 0x1235, 0x5678)] {
                assert_ne!(stored_identity(&pokemon(key, 25, other, 0)), expected);
            }
            assert_ne!(
                stored_identity(&pokemon(key, 25, (0x89ab_cdef, 0x1234, 0x5679), 0)),
                expected
            );
        }
        // The species agrees with `stored_species` on the same record.
        let both = pokemon(0x0bad_f00d, 151, (1, 2, 3), 4);
        assert_eq!(stored_species(&both), 151);
        assert!(stored_identity(&both).is_some());
    }

    #[test]
    fn the_boxes_end_where_the_transport_box_begins_and_all_slots_are_read() {
        assert_eq!(BOXES_START + BOXES * BOX_STRIDE, TRANSPORT_RECORDS.start);
        assert_eq!(BOX_STRIDE, BOX_SLOTS * RECORD_SIZE + 0x24 + 2);
        assert_eq!(HELD, 3030);

        let mut work = vec![0u8; BLOB_SIZE];
        work[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        work[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        // Every slot holds an encrypted empty record, as in a real Bank,
        // and the box names hold bytes that are not records.
        for index in 0..BOXES {
            let at = BOXES_START + index * BOX_STRIDE;
            for slot in 0..BOX_SLOTS {
                let key = (index * 64 + slot) as u32 | 0x5000_0000;
                work[at + slot * RECORD_SIZE..at + (slot + 1) * RECORD_SIZE]
                    .copy_from_slice(&pokemon(key, 0, (1, 2, 3), 0));
            }
            work[at + BOX_SLOTS * RECORD_SIZE..at + BOX_STRIDE].fill(0xEE);
        }
        compact(&mut work).unwrap();
        let mut found = vec![[0; 3]; HELD];
        assert_eq!(box_identities(&work[..BANK_SIZE], &mut found, 0), Ok(0));

        // The first and the last slot of the first and the last box.
        let mut expected = Vec::new();
        for (index, slot) in [(0, 0), (0, 29), (57, 13), (99, 0), (99, 29)] {
            let key = (index * 64 + slot) as u32 | 0x7000_0000;
            let at = BOXES_START + index * BOX_STRIDE + slot * RECORD_SIZE;
            work[at..at + RECORD_SIZE].copy_from_slice(&pokemon(key, 133, (key ^ 5, 77, 88), 0));
            expected.push([key, key ^ 5, 88 << 16 | 77]);
        }
        assert_eq!(box_identities(&work[..BANK_SIZE], &mut found, 0), Ok(5));
        assert_eq!(&found[..5], expected.as_slice());
        // Appending keeps what is there; a full list takes no more.
        assert_eq!(box_identities(&work[..BANK_SIZE], &mut found, 5), Ok(10));
        assert_eq!(&found[..5], expected.as_slice());
        assert_eq!(
            box_identities(&work[..BANK_SIZE], &mut found[..7], 5),
            Ok(7)
        );
        assert!(box_identities(&work[..BANK_SIZE - 1], &mut found, 0).is_err());
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
