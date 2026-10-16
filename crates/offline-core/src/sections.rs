//! Where each stored file's bytes live inside the native Bank body.
//!
//! The original code loads and saves one `0xBB518`-byte block. The patch keeps
//! it in three files, by what the data is, and stores each range exactly as
//! the original bytes, in the original order:
//!
//! | Body range | Content | File |
//! | --- | --- | --- |
//! | `0..17C` | Header: group names, format, reward date, Miles state, flags | main |
//! | `17C..AAF14` | 100 boxes | bank |
//! | `AAF14..ACA44` | Transport box records (30 x `E8`) | transport |
//! | `ACA44..AD5FC` | Format tag per box slot | bank |
//! | `AD5FC..AD61A` | Transport box format tags (30 bytes) | transport |
//! | `AD61A..B4AA0` | Padding, trainer records, Pokédex, transfer counters | main |
//! | `B4AA0..BB418` | One byte and one timestamp per box slot | bank |
//! | `BB418..BB518` | Tail | main |
//!
//! Loading is a read into place; saving is the reverse. Nothing is converted.

use crate::native_blob::{BlobError, NativeBlobView, BLOB_SIZE, BOX_COUNT};
use core::ops::Range;

pub const TRANSPORT_SLOTS: usize = 30;
pub const RECORD_SIZE: usize = 0xE8;
pub const TRANSPORT_RECORDS: Range<usize> = 0xAAF14..0xACA44;
pub const TRANSPORT_TAGS: Range<usize> = 0xAD5FC..0xAD61A;
/// The trainer and adventure records, one of `0x44` bytes per game version.
pub const RECORDS: Range<usize> = 0xAD61C..0xAD83C;
/// The Pokédex: eight version regions and what is derived from them.
pub const POKEDEX: Range<usize> = 0xAD83C..0xB4A9C;
/// The records and the Pokédex together, what a game brings into the Bank.
pub const DEX: Range<usize> = RECORDS.start..POKEDEX.end;

pub const TRANSPORT_SIZE: usize = TRANSPORT_SLOTS * RECORD_SIZE + TRANSPORT_SLOTS;
/// Body ranges kept in the main file, in order: everything Bank needs that
/// is not a Pokémon slot.
pub const MAIN_RANGES: [Range<usize>; 3] = [
    0..BOXES_START,
    TRANSPORT_TAGS.end..SLOT_DATA_START,
    TAIL_START..BLOB_SIZE,
];
/// Body ranges kept in the Bank file, in order: the boxes and what is kept
/// per box slot.
pub const BANK_RANGES: [Range<usize>; 3] = [
    BOXES_START..TRANSPORT_RECORDS.start,
    TRANSPORT_RECORDS.end..TRANSPORT_TAGS.start,
    SLOT_DATA_START..TAIL_START,
];
const SLOT_DATA_START: usize = 0xB4AA0;
const TAIL_START: usize = 0xBB418;
pub const MAIN_SIZE: usize =
    BOXES_START + (SLOT_DATA_START - TRANSPORT_TAGS.end) + (BLOB_SIZE - TAIL_START);
pub const BANK_SIZE: usize = BLOB_SIZE - TRANSPORT_SIZE - MAIN_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionError {
    WrongSize {
        expected: usize,
        actual: usize,
    },
    /// The header is not the format and box count of this layout.
    Header(BlobError),
}

fn sized(actual: usize, expected: usize) -> Result<(), SectionError> {
    if actual == expected {
        Ok(())
    } else {
        Err(SectionError::WrongSize { expected, actual })
    }
}

/// Moves the Bank ranges together in place: afterwards `body[..BANK_SIZE]` is
/// the Bank file payload. The other ranges are overwritten, so copy them out
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
/// native offsets. The other ranges are zeroed and must be filled by the caller.
pub fn expand(body: &mut [u8]) -> Result<(), SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    let mut at = BANK_SIZE;
    for range in BANK_RANGES.into_iter().rev() {
        at -= range.len();
        body.copy_within(at..at + range.len(), range.start);
    }
    body[TRANSPORT_RECORDS].fill(0);
    body[TRANSPORT_TAGS].fill(0);
    for range in MAIN_RANGES {
        body[range].fill(0);
    }
    Ok(())
}

/// Checks a Bank file payload: its exact size. What it holds are Pokémon
/// records and per-slot data, which carry no marker of their own.
pub fn validate_bank(payload: &[u8]) -> Result<(), SectionError> {
    sized(payload.len(), BANK_SIZE)
}

/// Checks the header of an assembled body as `NativeBlobView` does: native
/// format marker, 100 boxes. The header is the first range of the main file.
pub fn validate_header(body: &[u8]) -> Result<(), SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    NativeBlobView::parse_layout(body)
        .map(|_| ())
        .map_err(SectionError::Header)
}

/// The main ranges of `body` as separate slices, in order.
pub fn main_parts(body: &mut [u8]) -> Result<[&mut [u8]; 3], SectionError> {
    sized(body.len(), BLOB_SIZE)?;
    let [first, second, third] = MAIN_RANGES;
    let (head, rest) = body.split_at_mut(second.start);
    let (middle, tail) = rest.split_at_mut(third.start - second.start);
    Ok([&mut head[first], &mut middle[..second.len()], tail])
}

// Position of each plaintext block inside a stored record, per shuffle value.
// This is the standard Gen 6/7 stored layout; Bank's records use it unchanged.
const BLOCK_POSITION: [u8; 96] = [
    0, 1, 2, 3, 0, 1, 3, 2, 0, 2, 1, 3, 0, 3, 1, 2, 0, 2, 3, 1, 0, 3, 2, 1, 1, 0, 2, 3, 1, 0, 3, 2,
    2, 0, 1, 3, 3, 0, 1, 2, 2, 0, 3, 1, 3, 0, 2, 1, 1, 2, 0, 3, 1, 3, 0, 2, 2, 1, 0, 3, 3, 1, 0, 2,
    2, 3, 0, 1, 3, 2, 0, 1, 1, 2, 3, 0, 1, 3, 2, 0, 2, 1, 3, 0, 3, 1, 2, 0, 2, 3, 1, 0, 3, 2, 1, 0,
];

/// The first `N` words of block A of a stored record, decrypted: the block
/// that starts with the species. Decrypts only as far as needed, with the
/// generator the games encrypt records with.
fn block_a<const N: usize>(record: &[u8; RECORD_SIZE]) -> [u16; N] {
    let key = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    // `% 24` on a value below 32, without a division routine.
    let shuffle = (key >> 13) & 31;
    let shuffle = if shuffle >= 24 { shuffle - 24 } else { shuffle };
    let first = usize::from(BLOCK_POSITION[shuffle as usize * 4]) * 28;
    let mut plain = [0u16; N];
    let mut seed = key;
    for word in 0..first + N {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        if word >= first {
            let at = 8 + word * 2;
            plain[word - first] =
                u16::from_le_bytes([record[at], record[at + 1]]) ^ (seed >> 16) as u16;
        }
    }
    plain
}

/// Species of a stored record; zero means the slot is empty.
pub fn stored_species(record: &[u8; RECORD_SIZE]) -> u16 {
    block_a::<1>(record)[0]
}

/// What makes one Pokémon that Pokémon wherever its record is stored and
/// whatever else about it changes: the encryption constant, the personality
/// value, and the original trainer's ID and secret ID.
pub type Identity = [u32; 3];

/// The identity of a stored record; `None` for an empty slot. The three
/// fields besides the encryption constant lie in the block that starts with
/// the species (words 2, 3 and 8..10 of it).
pub fn stored_identity(record: &[u8; RECORD_SIZE]) -> Option<Identity> {
    let plain = block_a::<10>(record);
    if plain[0] == 0 {
        return None;
    }
    Some([
        u32::from_le_bytes([record[0], record[1], record[2], record[3]]),
        u32::from(plain[8]) | u32::from(plain[9]) << 16,
        u32::from(plain[2]) | u32::from(plain[3]) << 16,
    ])
}

/// The boxes (`native_blob::BOX_COUNT`): 30 records each, then the box's
/// name and index. They begin at `BOXES_START` in a body and at 0 in a Bank
/// file payload.
pub const BOX_SLOTS: usize = 30;
const BOXES_START: usize = 0x17C;
const BOX_STRIDE: usize = 0x1B56;
/// Most Pokémon one Bank snapshot holds: its boxes and the transport box.
pub const HELD: usize = BOX_COUNT * BOX_SLOTS + TRANSPORT_SLOTS;

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
    for index in 0..BOX_COUNT {
        let at = index * BOX_STRIDE;
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
            (MAIN_SIZE, TRANSPORT_SIZE, BANK_SIZE),
            (30_466, 6_990, 729_800)
        );
        let mut covered = vec![0u8; BLOB_SIZE];
        for range in BANK_RANGES
            .into_iter()
            .chain(MAIN_RANGES)
            .chain([TRANSPORT_RECORDS, TRANSPORT_TAGS])
        {
            for mark in &mut covered[range] {
                *mark += 1;
            }
        }
        assert!(covered.iter().all(|count| *count == 1));
        // The Miles state and the Pokédex are in the main file.
        use crate::native_blob::{MILES_OFFSET, REWARD_DATE_OFFSET};
        const { assert!(REWARD_DATE_OFFSET + 6 <= MAIN_RANGES[0].end) };
        const { assert!(MILES_OFFSET + 4 <= MAIN_RANGES[0].end) };
        assert_eq!((RECORDS.len(), POKEDEX.len()), (8 * 0x44, 0x7260));
        const { assert!(MAIN_RANGES[1].start <= DEX.start) };
        const { assert!(DEX.end <= MAIN_RANGES[1].end) };
        const { assert!(crate::native_blob::FRACTION_OFFSET + 2 <= MAIN_RANGES[0].end) };
    }

    #[test]
    fn split_and_join_return_every_byte() {
        let original = body();
        let main: Vec<Vec<u8>> = MAIN_RANGES
            .into_iter()
            .map(|range| original[range].to_vec())
            .collect();
        let (records, tags) = (
            original[TRANSPORT_RECORDS].to_vec(),
            original[TRANSPORT_TAGS].to_vec(),
        );
        let mut work = original.clone();
        let bank: Vec<u8> = BANK_RANGES
            .into_iter()
            .flat_map(|range| original[range].to_vec())
            .collect();
        compact(&mut work).unwrap();
        assert!(work[..BANK_SIZE] == bank);
        validate_bank(&work[..BANK_SIZE]).unwrap();
        // The payload begins with the first box.
        assert!(work[..BOX_STRIDE] == original[BOXES_START..BOXES_START + BOX_STRIDE]);
        // Stale bytes after the payload must not survive the round trip.
        work[BANK_SIZE..].fill(0xEE);
        expand(&mut work).unwrap();
        for range in MAIN_RANGES {
            assert!(work[range].iter().all(|byte| *byte == 0));
        }
        for (part, bytes) in main_parts(&mut work).unwrap().into_iter().zip(&main) {
            part.copy_from_slice(bytes);
        }
        work[TRANSPORT_RECORDS].copy_from_slice(&records);
        work[TRANSPORT_TAGS].copy_from_slice(&tags);
        assert!(work == original);
    }

    #[test]
    fn payload_and_header_are_checked_and_sizes_are_exact() {
        let mut work = body();
        validate_header(&work).unwrap();
        assert!(validate_header(&work[..BLOB_SIZE - 1]).is_err());
        work[0x15C] = 9;
        assert_eq!(
            validate_header(&work),
            Err(SectionError::Header(BlobError::UnsupportedFormat(9)))
        );
        work[0x15C] = 2;
        work[0x15E] = 99;
        assert_eq!(
            validate_header(&work),
            Err(SectionError::Header(BlobError::UnexpectedBoxCount(99)))
        );
        compact(&mut work).unwrap();
        validate_bank(&work[..BANK_SIZE]).unwrap();
        assert!(validate_bank(&work[..BANK_SIZE - 1]).is_err());
        assert!(validate_bank(&work).is_err());
        for size in [0, BANK_SIZE, BLOB_SIZE - 1, BLOB_SIZE + 1] {
            let mut short = vec![0; size];
            assert!(compact(&mut short).is_err());
            assert!(expand(&mut short).is_err());
            assert!(transport_count(&short).is_err());
            assert!(main_parts(&mut short).is_err());
        }
    }

    /// Stored-block order for each of the 24 orders, written independently of
    /// `BLOCK_POSITION`: entry `[order][position]` is the block stored there.
    const ORDERS: [&[u8; 4]; 24] = [
        b"ABCD", b"ABDC", b"ACBD", b"ACDB", b"ADBC", b"ADCB", b"BACD", b"BADC", b"BCAD", b"BCDA",
        b"BDAC", b"BDCA", b"CABD", b"CADB", b"CBAD", b"CBDA", b"CDAB", b"CDBA", b"DABC", b"DACB",
        b"DBAC", b"DBCA", b"DCAB", b"DCBA",
    ];

    /// A stored record of the plain blocks A..D, shuffled and encrypted by
    /// `key` the way the games do.
    fn stored(key: u32, plain: &[u16; 112]) -> [u8; RECORD_SIZE] {
        let order = ORDERS[(((key >> 13) & 31) % 24) as usize];
        let mut out = [0u8; RECORD_SIZE];
        out[..4].copy_from_slice(&key.to_le_bytes());
        let mut seed = key;
        for (position, &block) in order.iter().enumerate() {
            let block = usize::from(block - b'A');
            for (index, &word) in plain[block * 28..block * 28 + 28].iter().enumerate() {
                seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
                let at = 8 + (position * 28 + index) * 2;
                out[at..at + 2].copy_from_slice(&(word ^ (seed >> 16) as u16).to_le_bytes());
            }
        }
        out
    }

    /// `base` with `shuffle` in the key's order field, bits 13..18.
    fn key(base: u32, shuffle: u32) -> u32 {
        (base & !(31 << 13)) | (shuffle << 13)
    }

    /// The 32 values of the order field reach each of the 24 orders.
    fn every_order(base: u32) -> impl Iterator<Item = u32> {
        let keys = (0..32).map(move |shuffle| key(base, shuffle));
        let mut seen = [false; 24];
        for key in keys.clone() {
            seen[(((key >> 13) & 31) % 24) as usize] = true;
        }
        assert!(seen.iter().all(|&order| order));
        keys
    }

    /// A stored record with `species`.
    fn record(key: u32, species: u16) -> [u8; RECORD_SIZE] {
        let mut plain = [0u16; 112];
        plain[0] = species;
        plain[30] = 0x1111; // other blocks hold unrelated nonzero data
        plain[60] = 0x2222;
        plain[90] = 0x3333;
        stored(key, &plain)
    }

    #[test]
    fn species_is_found_for_every_block_order() {
        for key in every_order(0x1234_0155) {
            assert_eq!(stored_species(&record(key, 0)), 0, "key {key:#x}");
            assert_eq!(stored_species(&record(key, 151)), 151, "key {key:#x}");
            assert_eq!(stored_species(&record(key, 807)), 807, "key {key:#x}");
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
        stored(key, &plain)
    }

    #[test]
    fn identity_is_found_for_every_block_order_and_ignores_everything_else() {
        for key in every_order(0x4321_00aa) {
            let identity = (0x89ab_cdef, 0x1234, 0x5678);
            let expected = Some([key, 0x89ab_cdef, 0x5678_1234]);
            assert_eq!(
                stored_identity(&pokemon(key, 25, identity, 0)),
                expected,
                "key {key:#x}"
            );
            // Evolved, levelled, given an item: the same Pokémon.
            assert_eq!(
                stored_identity(&pokemon(key, 26, identity, 0x7e57)),
                expected,
                "key {key:#x}"
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
        assert_eq!(
            BOXES_START + BOX_COUNT * BOX_STRIDE,
            TRANSPORT_RECORDS.start
        );
        assert_eq!(BOX_STRIDE, BOX_SLOTS * RECORD_SIZE + 0x24 + 2);
        assert_eq!(HELD, 3030);

        let mut work = vec![0u8; BLOB_SIZE];
        work[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        work[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        // Every slot holds an encrypted empty record, as in a real Bank,
        // and the box names hold bytes that are not records.
        for index in 0..BOX_COUNT {
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
            // In the payload the boxes begin at 0.
            let at = index * BOX_STRIDE + slot * RECORD_SIZE;
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
