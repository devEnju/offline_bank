//! Frozen: the record v0.2.1 kept in its rewards file, as its
//! `offline-core/src/rewards.rs` reads it. Only the migration reads it.
//!
//! 16 bytes: u32 balance at 0; u8 state at 4 (0 no record yet, 1 record);
//! u8 fraction at 5; u16 saved count at 6; accounted-through date at 8 (u16
//! year, u8 month, u8 day); zero at 12..16. With state 0, bytes 5..12 are
//! zero. The record holds the whole Miles balance; the body holds zero.

/// Native balance setter 001d59fc and getter 001d588c clamp to this value.
pub const MILES_CAP: u32 = 65_535;
/// Native accrual multiplies by 1/30 per stored Pokémon per day.
pub const DIVISOR: u32 = 30;
/// 100 regular boxes of 30 slots; transfer slots and game boxes do not count.
pub const MAX_STORED: u32 = 3_000;
pub const RECORD_SIZE: usize = 16;

/// A validated console-local calendar date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Years 1..=16383 match the native 14-bit packed year field.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year == 0 || year > 0x3fff {
            return None;
        }
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            _ => return None,
        };
        if day == 0 || day > days {
            return None;
        }
        Some(Self { year, month, day })
    }
    pub const fn year(self) -> u16 {
        self.year
    }
    pub const fn month(self) -> u8 {
        self.month
    }
    pub const fn day(self) -> u8 {
        self.day
    }
}

/// The committed record stored beside the Bank body in every snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accounting {
    accounted_through: Date,
    saved_count: u16,
    fraction: u8,
}

impl Accounting {
    pub fn new(accounted_through: Date, saved_count: u32, fraction: u32) -> Option<Self> {
        if saved_count > MAX_STORED || fraction >= DIVISOR {
            return None;
        }
        Some(Self {
            accounted_through,
            saved_count: saved_count as u16,
            fraction: fraction as u8,
        })
    }
    /// The latest day already paid for.
    pub const fn accounted_through(self) -> Date {
        self.accounted_through
    }
    pub const fn saved_count(self) -> u32 {
        self.saved_count as u32
    }
    /// Thirtieths of a Mile not yet paid.
    pub const fn fraction(self) -> u32 {
        self.fraction as u32
    }
}

/// The stored Miles state: the balance and the record it is calculated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    pub balance: u32,
    pub accounting: Option<Accounting>,
}

impl Stored {
    /// A Bank without a rewards file, or with one v0.2.1 does not read: no
    /// Miles and no record yet.
    pub const NONE: Self = Self {
        balance: 0,
        accounting: None,
    };

    /// `None` for bytes that are not a record; v0.2.1 then loads `NONE`.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != RECORD_SIZE || bytes[12..] != [0; 4] {
            return None;
        }
        let balance = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if balance > MILES_CAP {
            return None;
        }
        let accounting = match bytes[4] {
            0 if bytes[5..12] == [0; 7] => None,
            1 => Some(Accounting::new(
                Date::new(
                    u16::from_le_bytes([bytes[8], bytes[9]]),
                    bytes[10],
                    bytes[11],
                )?,
                u32::from(u16::from_le_bytes([bytes[6], bytes[7]])),
                u32::from(bytes[5]),
            )?),
            _ => return None,
        };
        Some(Self {
            balance,
            accounting,
        })
    }
}
