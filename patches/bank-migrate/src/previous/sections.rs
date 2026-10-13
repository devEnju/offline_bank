//! Frozen: the previous release's `sections.rs`. Only the migration reads it.
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

use super::{checksum::Crc32, native_blob::BLOB_SIZE, sidecar::Kind};
use core::ops::Range;

pub const TRANSPORT_SLOTS: usize = 30;
pub const RECORD_SIZE: usize = 0xE8;
pub const TRANSPORT_RECORDS: Range<usize> = 0xAAF14..0xACA44;
pub const TRANSPORT_TAGS: Range<usize> = 0xAD5FC..0xAD61A;
pub const DEX: Range<usize> = 0xAD61C..0xB4A9C;
/// The native whole-Miles field. The Bank file stores it as the body has it;
/// the remainder below one Mile lives in the rewards file.
pub const MILES: Range<usize> = 0x170..0x174;
/// The native reward date: the date and time up to which Miles are counted.
pub const REWARD_DATE: Range<usize> = 0x160..0x166;

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
