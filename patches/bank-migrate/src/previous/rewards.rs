//! Frozen: the record the previous release kept in its rewards side file.
//! Only the migration reads it.
//!
//! 16 bytes: the remainder in Pokémon-hours as a u16 at 4, and the day of the
//! last redemption at 8 (u16 year, u8 month, u8 day; all zero: never
//! redeemed). Bytes 0..4 and 6..8 are not read; 12..16 are zero.

use offline_core::rewards::Date;
pub use offline_core::rewards::Record;

pub const FILE: super::sidecar::Kind = super::sidecar::Kind {
    magic: *b"BKOFRWD1",
    capacity: RECORD_SIZE as u32,
};
pub const RECORD_SIZE: usize = 16;

pub fn encode(record: Record) -> [u8; RECORD_SIZE] {
    let mut out = [0; RECORD_SIZE];
    out[4..6].copy_from_slice(&record.fraction.to_le_bytes());
    if let Some(day) = record.redeemed {
        out[8..10].copy_from_slice(&day.year().to_le_bytes());
        out[10] = day.month();
        out[11] = day.day();
    }
    out
}

/// `None` for bytes that are not a record.
pub fn decode(bytes: &[u8]) -> Option<Record> {
    if bytes.len() != RECORD_SIZE || bytes[12..] != [0; 4] {
        return None;
    }
    let redeemed = if bytes[8..12] == [0; 4] {
        None
    } else {
        let year = u16::from_le_bytes([bytes[8], bytes[9]]);
        Some(Date::new(year, bytes[10], bytes[11]).ok()?)
    };
    Some(Record {
        fraction: u16::from_le_bytes([bytes[4], bytes[5]]),
        redeemed,
    })
}
