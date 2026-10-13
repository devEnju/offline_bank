//! Frozen: the record the previous release kept in its rewards side file.
//! Only the migration reads it.
//!
//! 16 bytes: the remainder in Pokémon-hours as a u16 at 4, and the day of the
//! last redemption at 8 (u16 year, u8 month, u8 day; all zero: never
//! redeemed). Bytes 0..4 are a Miles balance to add to the body's: zero as
//! this release writes them, the balance itself as v0.3.1 did, which wrote
//! zero into the body. Bytes 6..8 are not read; 12..16 are zero.

/// What the record holds, as that release reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// Pokémon-hours not yet a whole Mile.
    pub fraction: u16,
    /// The day of the last redemption: year, month, day.
    pub redeemed: Option<(u16, u8, u8)>,
}
impl Record {
    /// A Bank without a rewards file: no remainder, never redeemed.
    pub const NONE: Self = Self {
        fraction: 0,
        redeemed: None,
    };
}

/// Most Miles a balance holds.
const MILES_CAP: u32 = 65_535;

/// The Miles balance for the body when the record beside it holds one in
/// its first four bytes: the sum of the two, up to the cap. `None` for a
/// record that holds none there, which leaves the body as it is.
pub fn balance_beside(body: u32, record: &[u8; RECORD_SIZE]) -> Option<u32> {
    let beside = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    (beside != 0).then(|| {
        body.min(MILES_CAP)
            .saturating_add(beside.min(MILES_CAP))
            .min(MILES_CAP)
    })
}

/// A calendar day as that release accepts one: years 1..=16383.
fn date(year: u16, month: u8, day: u8) -> Option<(u16, u8, u8)> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    let valid = (1..=0x3fff).contains(&year) && (1..=days).contains(&day);
    valid.then_some((year, month, day))
}

pub const FILE: super::sidecar::Kind = super::sidecar::Kind {
    magic: *b"BKOFRWD1",
    capacity: RECORD_SIZE as u32,
};
pub const RECORD_SIZE: usize = 16;

pub fn encode(record: Record) -> [u8; RECORD_SIZE] {
    let mut out = [0; RECORD_SIZE];
    out[4..6].copy_from_slice(&record.fraction.to_le_bytes());
    if let Some((year, month, day)) = record.redeemed {
        out[8..10].copy_from_slice(&year.to_le_bytes());
        out[10] = month;
        out[11] = day;
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
        Some(date(year, bytes[10], bytes[11])?)
    };
    Some(Record {
        fraction: u16::from_le_bytes([bytes[4], bytes[5]]),
        redeemed,
    })
}
