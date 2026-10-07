//! Shared Poké Miles accounting and redemption checks. Pure integer logic.
//!
//! One Bank-wide record earns `previous saved count * elapsed calendar days / 30`
//! Miles, carrying the remainder in thirtieths. The whole-Miles balance itself
//! stays in the native Bank body; this module never stores a second balance.
//! The accounting date only moves forward, so an earlier clock earns nothing
//! and never blocks a save. Native evidence is in docs/internals.md.

/// Native balance setter 001d59fc and getter 001d588c clamp to this value.
pub const MILES_CAP: u32 = 65_535;
/// Native accrual multiplies by 1/30 per stored Pokémon per day.
pub const DIVISOR: u32 = 30;
/// 100 regular boxes of 30 slots; transfer slots and game boxes do not count.
pub const MAX_STORED: u32 = 3_000;
/// Native claim state 0x16 offers redemption only above nine Miles.
pub const REDEEM_THRESHOLD: u32 = 10;
/// Native Gen 7 gift store 001d58b0 counts 48 slots.
pub const GEN7_GIFT_SLOTS: u32 = 48;
/// Record type written by the native Gen 7 Battle Point receipt.
pub const GEN7_BP_KIND: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewardError {
    InvalidDate,
    InvalidCount,
    InvalidFraction,
    InvalidRecord,
    Overflow,
    BelowThreshold,
    UnsupportedCurrency,
    BalanceMismatch,
    ReceiptMismatch,
}

/// A validated console-local calendar date. Ordering is chronological.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Years 1..=16383 match the native 14-bit packed year field.
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, RewardError> {
        if year == 0 || year > 0x3fff {
            return Err(RewardError::InvalidDate);
        }
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            _ => return Err(RewardError::InvalidDate),
        };
        if day == 0 || day > days {
            return Err(RewardError::InvalidDate);
        }
        Ok(Self { year, month, day })
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
    /// Days since 0000-03-01 in the proleptic Gregorian calendar.
    fn number(self) -> u32 {
        let (year, month) = (u32::from(self.year), u32::from(self.month));
        let year = if month <= 2 { year - 1 } else { year };
        let shifted = if month > 2 { month - 3 } else { month + 9 };
        year * 365 + year / 4 - year / 100
            + year / 400
            + (153 * shifted + 2) / 5
            + u32::from(self.day)
            - 1
    }
    /// Whole calendar days from `earlier` to `self`; zero when not later.
    pub fn days_since(self, earlier: Self) -> u32 {
        self.number().saturating_sub(earlier.number())
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
    pub fn new(
        accounted_through: Date,
        saved_count: u32,
        fraction: u32,
    ) -> Result<Self, RewardError> {
        if saved_count > MAX_STORED {
            return Err(RewardError::InvalidCount);
        }
        if fraction >= DIVISOR {
            return Err(RewardError::InvalidFraction);
        }
        Ok(Self {
            accounted_through,
            saved_count: saved_count as u16,
            fraction: fraction as u8,
        })
    }
    /// The latest day already paid for. It never moves backwards.
    pub const fn accounted_through(self) -> Date {
        self.accounted_through
    }
    pub const fn saved_count(self) -> u32 {
        self.saved_count as u32
    }
    pub const fn fraction(self) -> u32 {
        self.fraction as u32
    }
}

/// The rewards side file. Its 16-byte payload is everything about Miles:
/// u32 balance at 0; u8 state at 4 (0 no record yet, 1 record); u8 fraction at
/// 5; u16 saved count at 6; accounted-through date at 8 (u16 year, u8 month,
/// u8 day); zero at 12..16. With state 0, bytes 5..12 are zero.
pub const FILE: crate::sidecar::Kind = crate::sidecar::Kind {
    magic: *b"BKOFRWD1",
    capacity: RECORD_SIZE as u32,
};
pub const RECORD_SIZE: usize = 16;

/// The stored Miles state: the balance and the record it is calculated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    pub balance: u32,
    pub accounting: Option<Accounting>,
}
impl Stored {
    /// A Bank without a rewards file: no Miles and no record yet.
    pub const NONE: Self = Self {
        balance: 0,
        accounting: None,
    };

    pub fn encode(self) -> [u8; RECORD_SIZE] {
        let mut out = [0; RECORD_SIZE];
        out[..4].copy_from_slice(&self.balance.min(MILES_CAP).to_le_bytes());
        if let Some(record) = self.accounting {
            out[4] = 1;
            out[5] = record.fraction;
            out[6..8].copy_from_slice(&record.saved_count.to_le_bytes());
            out[8..10].copy_from_slice(&record.accounted_through.year.to_le_bytes());
            out[10] = record.accounted_through.month;
            out[11] = record.accounted_through.day;
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RewardError> {
        if bytes.len() != RECORD_SIZE || bytes[12..] != [0; 4] {
            return Err(RewardError::InvalidRecord);
        }
        let balance = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if balance > MILES_CAP {
            return Err(RewardError::InvalidRecord);
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
            _ => return Err(RewardError::InvalidRecord),
        };
        Ok(Self {
            balance,
            accounting,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    through: Date,
    fraction: u32,
    balance: u32,
    earned: u32,
}

fn advance(
    through: Date,
    fraction: u32,
    count: u32,
    balance: u32,
    today: Date,
) -> Result<Step, RewardError> {
    let elapsed = today.days_since(through);
    let numerator = u64::from(count)
        .checked_mul(u64::from(elapsed))
        .and_then(|earned| earned.checked_add(u64::from(fraction)))
        .ok_or(RewardError::Overflow)?;
    let whole = numerator / u64::from(DIVISOR);
    let balance = balance.min(MILES_CAP);
    // Whole Miles above the native limit are discarded; the remainder is kept.
    let credited = whole.min(u64::from(MILES_CAP - balance)) as u32;
    Ok(Step {
        through: through.max(today),
        fraction: (numerator % u64::from(DIVISOR)) as u32,
        balance: balance + credited,
        earned: credited,
    })
}

/// Provisional state of one loaded session. Nothing here is durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    committed: Option<Accounting>,
    through: Option<Date>,
    fraction: u32,
    balance: u32,
    earned: u32,
}

impl Entry {
    /// Balance to expose through the native reward flow.
    pub const fn balance(&self) -> u32 {
        self.balance
    }
    /// Whole Miles credited on entry, after applying the native limit.
    pub const fn earned(&self) -> u32 {
        self.earned
    }
    pub const fn committed(&self) -> Option<Accounting> {
        self.committed
    }
}

/// Calculates the entry quote from the committed record and stored balance.
/// New Miles are added to `balance`; the balance itself never depends on the
/// current count. A Bank without a record earns nothing until its first save.
pub fn enter(
    committed: Option<Accounting>,
    balance: u32,
    today: Date,
) -> Result<Entry, RewardError> {
    let Some(record) = committed else {
        return Ok(Entry {
            committed: None,
            through: None,
            fraction: 0,
            balance: balance.min(MILES_CAP),
            earned: 0,
        });
    };
    let step = advance(
        record.accounted_through,
        record.fraction(),
        record.saved_count(),
        balance,
        today,
    )?;
    Ok(Entry {
        committed,
        through: Some(step.through),
        fraction: step.fraction,
        balance: step.balance,
        earned: step.earned,
    })
}

/// The record and balance one Save and Quit must persist together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub accounting: Accounting,
    pub balance: u32,
    /// Miles credited between the entry quote and this save.
    pub earned: u32,
}

/// Accounts for days elapsed since entry using the same previous count, then
/// records `saved_count` for future earnings. `balance` is the live native
/// balance, already reduced by any confirmed redemption. A clock earlier than
/// the accounting date earns nothing and leaves that date unchanged.
pub fn settle(
    entry: &Entry,
    balance: u32,
    saved_count: u32,
    today: Date,
) -> Result<Settlement, RewardError> {
    if saved_count > MAX_STORED {
        return Err(RewardError::InvalidCount);
    }
    let (Some(record), Some(through)) = (entry.committed, entry.through) else {
        return Ok(Settlement {
            accounting: Accounting::new(today, saved_count, 0)?,
            balance: balance.min(MILES_CAP),
            earned: 0,
        });
    };
    let step = advance(
        through,
        entry.fraction,
        record.saved_count(),
        balance,
        today,
    )?;
    Ok(Settlement {
        accounting: Accounting::new(step.through, saved_count, step.fraction)?,
        balance: step.balance,
        earned: step.earned,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    Miles,
    BattlePoints,
}

/// What the selected game holds after the native claim state finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receipt {
    /// Gen 6 pending-gift buffer: flag bit 0x80 and the written halfword.
    Gen6 { flagged: bool, amount: u16 },
    /// Gen 7 gift store counts and the record at the first formerly free slot.
    Gen7 {
        count_before: u32,
        count_after: u32,
        kind: u8,
        amount: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub currency: Currency,
    pub transferred: u32,
    pub retained: u32,
}

/// Confirms the native debit and game receipt of one completed claim.
/// The Gen 7 caller ignores its insertion result, so the store is inspected.
pub fn verify_claim(
    currency: Currency,
    quoted: u32,
    balance_before: u32,
    balance_after: u32,
    receipt: Receipt,
) -> Result<Claim, RewardError> {
    if !(REDEEM_THRESHOLD..=MILES_CAP).contains(&quoted) {
        return Err(RewardError::BelowThreshold);
    }
    if balance_before != quoted {
        return Err(RewardError::BalanceMismatch);
    }
    let (transferred, retained) = match currency {
        Currency::Miles => (quoted, 0),
        Currency::BattlePoints => (quoted / REDEEM_THRESHOLD, quoted % REDEEM_THRESHOLD),
    };
    if balance_after != retained {
        return Err(RewardError::BalanceMismatch);
    }
    let received = match (currency, receipt) {
        (_, Receipt::Gen6 { flagged, amount }) => flagged && u32::from(amount) == transferred,
        (Currency::Miles, Receipt::Gen7 { .. }) => return Err(RewardError::UnsupportedCurrency),
        (
            Currency::BattlePoints,
            Receipt::Gen7 {
                count_before,
                count_after,
                kind,
                amount,
            },
        ) => {
            count_before < GEN7_GIFT_SLOTS
                && count_after == count_before + 1
                && kind == GEN7_BP_KIND
                && amount == transferred
        }
    };
    if !received {
        return Err(RewardError::ReceiptMismatch);
    }
    Ok(Claim {
        currency,
        transferred,
        retained,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: u16, month: u8, day: u8) -> Date {
        Date::new(year, month, day).unwrap()
    }
    fn record(day: Date, count: u32, fraction: u32) -> Accounting {
        Accounting::new(day, count, fraction).unwrap()
    }
    fn next_day(day: Date) -> Date {
        Date::new(day.year(), day.month(), day.day() + 1)
            .or_else(|_| Date::new(day.year(), day.month() + 1, 1))
            .or_else(|_| Date::new(day.year() + 1, 1, 1))
            .unwrap()
    }

    #[test]
    fn calendar_rejects_invalid_dates_and_counts_boundaries() {
        for (year, month, day) in [
            (0, 1, 1),
            (0x4000, 1, 1),
            (2026, 0, 1),
            (2026, 13, 1),
            (2026, 2, 29),
            (2100, 2, 29),
            (2026, 4, 31),
            (2026, 1, 0),
        ] {
            assert_eq!(Date::new(year, month, day), Err(RewardError::InvalidDate));
        }
        assert_eq!(date(2026, 3, 1).days_since(date(2026, 2, 28)), 1);
        assert_eq!(date(2028, 3, 1).days_since(date(2028, 2, 28)), 2);
        assert_eq!(date(2000, 3, 1).days_since(date(2000, 2, 28)), 2);
        assert_eq!(date(2027, 1, 1).days_since(date(2026, 12, 31)), 1);
        assert_eq!(date(2026, 11, 1).days_since(date(2026, 10, 31)), 1);
        assert_eq!(date(2027, 10, 6).days_since(date(2026, 10, 6)), 365);
        assert_eq!(date(2029, 1, 1).days_since(date(2028, 1, 1)), 366);
        assert_eq!(date(2026, 10, 5).days_since(date(2026, 10, 6)), 0);
        assert!(date(2026, 10, 6) < date(2026, 11, 1));
        assert!(date(2026, 12, 31) < date(2027, 1, 1));
    }

    #[test]
    fn required_counts_earn_the_documented_miles_and_fractions() {
        let start = date(2026, 10, 6);
        // (count, days, whole Miles, thirtieths)
        for (count, days, miles, fraction) in [
            (0, 5, 0, 0),
            (1, 1, 0, 1),
            (1, 30, 1, 0),
            (15, 1, 0, 15),
            (15, 2, 1, 0),
            (29, 1, 0, 29),
            (30, 1, 1, 0),
            (31, 1, 1, 1),
            (3000, 1, 100, 0),
            (3000, 7, 700, 0),
        ] {
            let mut today = start;
            for _ in 0..days {
                today = next_day(today);
            }
            let entry = enter(Some(record(start, count, 0)), 0, today).unwrap();
            assert_eq!((entry.balance(), entry.fraction), (miles, fraction));
            assert_eq!(entry.earned(), miles);
        }
    }

    #[test]
    fn same_day_sessions_and_repeated_saves_earn_nothing_extra() {
        let day = date(2026, 10, 6);
        let mut committed = Some(record(day, 30, 7));
        let mut balance = 12;
        for _ in 0..5 {
            let entry = enter(committed, balance, day).unwrap();
            assert_eq!((entry.balance(), entry.earned()), (12, 0));
            let saved = settle(&entry, entry.balance(), 30, day).unwrap();
            assert_eq!((saved.balance, saved.earned), (12, 0));
            assert_eq!(saved.accounting, record(day, 30, 7));
            committed = Some(saved.accounting);
            balance = saved.balance;
        }
    }

    #[test]
    fn midnight_counts_one_day_without_waiting_twenty_four_hours() {
        // Saved before midnight with 30 Pokémon, opened after midnight.
        let entry = enter(Some(record(date(2026, 12, 31), 30, 0)), 0, date(2027, 1, 1)).unwrap();
        assert_eq!(entry.balance(), 1);
    }

    #[test]
    fn first_save_initializes_without_history_and_keeps_native_balance() {
        let day = date(2026, 10, 6);
        let entry = enter(None, 4321, day).unwrap();
        assert_eq!((entry.balance(), entry.earned()), (4321, 0));
        // Even a much later first save has no previous count to earn from.
        let later = date(2030, 1, 1);
        let saved = settle(&entry, 4321, 3000, later).unwrap();
        assert_eq!(saved.balance, 4321);
        assert_eq!(saved.accounting, record(later, 3000, 0));
        let next = enter(Some(saved.accounting), saved.balance, date(2030, 1, 2)).unwrap();
        assert_eq!(next.balance(), 4421);
    }

    #[test]
    fn unredeemed_miles_stay_when_the_stored_count_changes() {
        // 300 Miles are waiting. The Bank is emptied, later refilled.
        let day = date(2026, 10, 6);
        let entry = enter(Some(record(day, 3000, 10)), 300, day).unwrap();
        let emptied = settle(&entry, entry.balance(), 0, day).unwrap();
        assert_eq!((emptied.balance, emptied.accounting.fraction()), (300, 10));
        let idle = enter(Some(emptied.accounting), emptied.balance, date(2027, 1, 1)).unwrap();
        assert_eq!((idle.balance(), idle.earned()), (300, 0));
        let refilled = settle(&idle, idle.balance(), 60, date(2027, 1, 1)).unwrap();
        let next = enter(
            Some(refilled.accounting),
            refilled.balance,
            date(2027, 1, 2),
        )
        .unwrap();
        // 10 carried thirtieths + 60 new ones: two Miles on top of the 300.
        assert_eq!((next.balance(), next.fraction), (302, 10));
    }

    #[test]
    fn fraction_carries_across_saves_and_previous_count_governs_earnings() {
        let mut day = date(2026, 10, 6);
        let mut committed = Some(record(day, 29, 0));
        let mut balance = 0;
        let mut total = 0;
        for _ in 0..30 {
            day = next_day(day);
            let entry = enter(committed, balance, day).unwrap();
            let saved = settle(&entry, entry.balance(), 29, day).unwrap();
            total += entry.earned() + saved.earned;
            committed = Some(saved.accounting);
            balance = saved.balance;
        }
        assert_eq!((balance, total), (29, 29));
        assert_eq!(committed.unwrap().fraction(), 0);

        let entry = enter(committed, balance, next_day(day)).unwrap();
        // Withdrawing everything changes only future earnings.
        let emptied = settle(&entry, entry.balance(), 0, next_day(day)).unwrap();
        assert_eq!((emptied.balance, emptied.accounting.fraction()), (29, 29));
    }

    #[test]
    fn session_spanning_midnight_adds_later_days_at_save() {
        let saved_day = date(2026, 10, 5);
        let entry = enter(Some(record(saved_day, 45, 0)), 3, date(2026, 10, 6)).unwrap();
        assert_eq!((entry.balance(), entry.fraction), (4, 15));
        let saved = settle(&entry, entry.balance(), 60, date(2026, 10, 7)).unwrap();
        assert_eq!((saved.balance, saved.earned), (6, 2));
        assert_eq!(saved.accounting, record(date(2026, 10, 7), 60, 0));
    }

    #[test]
    fn earnings_after_a_redemption_quote_stay_in_the_bank() {
        let entry = enter(
            Some(record(date(2026, 10, 1), 3000, 0)),
            0,
            date(2026, 10, 2),
        )
        .unwrap();
        assert_eq!(entry.balance(), 100);
        let claim = verify_claim(
            Currency::Miles,
            100,
            100,
            0,
            Receipt::Gen6 {
                flagged: true,
                amount: 100,
            },
        )
        .unwrap();
        assert_eq!((claim.transferred, claim.retained), (100, 0));
        // The session crosses midnight after the receipt was confirmed.
        let saved = settle(&entry, claim.retained, 3000, date(2026, 10, 3)).unwrap();
        assert_eq!((saved.balance, saved.earned), (100, 100));
    }

    #[test]
    fn cap_discards_whole_miles_but_keeps_the_fraction() {
        let entry = enter(
            Some(record(date(2026, 10, 1), 31, 3)),
            MILES_CAP - 1,
            date(2026, 10, 11),
        )
        .unwrap();
        // 3 + 310 thirtieths: ten whole Miles earned, one credited.
        assert_eq!(
            (entry.balance(), entry.earned(), entry.fraction),
            (MILES_CAP, 1, 13)
        );
        let over = enter(
            Some(record(date(2026, 10, 1), 1, 0)),
            70_000,
            date(2026, 10, 2),
        )
        .unwrap();
        assert_eq!(over.balance(), MILES_CAP);
        let long = enter(
            Some(record(date(1, 1, 1), 3000, 29)),
            0,
            date(0x3fff, 12, 31),
        )
        .unwrap();
        assert_eq!(long.balance(), MILES_CAP);
    }

    #[test]
    fn backward_clock_still_saves_earns_nothing_and_keeps_the_date() {
        let high = date(2026, 10, 6);
        let entry = enter(Some(record(high, 3000, 11)), 50, date(2026, 10, 1)).unwrap();
        assert_eq!(
            (entry.balance(), entry.earned(), entry.fraction),
            (50, 0, 11)
        );
        // Saving works and records the new count; the date does not go back.
        let saved = settle(&entry, 50, 2000, date(2026, 10, 2)).unwrap();
        assert_eq!(saved.balance, 50);
        assert_eq!(saved.accounting, record(high, 2000, 11));
        // Days already accounted are not paid again when the clock catches up.
        let caught_up = enter(Some(saved.accounting), 50, high).unwrap();
        assert_eq!(caught_up.earned(), 0);
        let after = enter(Some(saved.accounting), 50, date(2026, 10, 7)).unwrap();
        assert_eq!(after.balance(), 117);
    }

    #[test]
    fn clock_set_back_during_a_session_never_blocks_the_save() {
        // Entry credits one day, then the clock reads an earlier date at save.
        let entry = enter(
            Some(record(date(2026, 10, 5), 3000, 0)),
            0,
            date(2026, 10, 6),
        )
        .unwrap();
        assert_eq!(entry.balance(), 100);
        let saved = settle(&entry, 100, 3000, date(2026, 10, 1)).unwrap();
        assert_eq!((saved.balance, saved.earned), (100, 0));
        assert_eq!(saved.accounting, record(date(2026, 10, 6), 3000, 0));
    }

    #[test]
    fn jumping_forward_pays_once_then_nothing_until_that_date_passes() {
        let real = date(2026, 10, 6);
        let jumped = date(2026, 11, 5);
        let entry = enter(Some(record(real, 30, 0)), 0, jumped).unwrap();
        assert_eq!(entry.balance(), 30);
        let saved = settle(&entry, 30, 30, jumped).unwrap();
        // Back on the real date, and on every day up to the jumped date.
        for today in [real, date(2026, 10, 20), jumped] {
            let later = enter(Some(saved.accounting), saved.balance, today).unwrap();
            assert_eq!((later.balance(), later.earned()), (30, 0));
        }
        let after = enter(Some(saved.accounting), saved.balance, date(2026, 11, 6)).unwrap();
        assert_eq!(after.earned(), 1);
    }

    #[test]
    fn stored_record_layout_is_explicit_and_round_trips() {
        let stored = Stored {
            balance: 0x1234,
            accounting: Some(record(date(2026, 10, 6), 2999, 29)),
        };
        let bytes = stored.encode();
        assert_eq!(
            bytes,
            [0x34, 0x12, 0, 0, 1, 29, 0xb7, 0x0b, 0xea, 0x07, 10, 6, 0, 0, 0, 0]
        );
        assert_eq!(Stored::decode(&bytes), Ok(stored));
        assert_eq!(Stored::NONE.encode(), [0; 16]);
        assert_eq!(Stored::decode(&[0; 16]), Ok(Stored::NONE));
        assert_eq!(FILE.file_len(), 160);
        for (at, value) in [(2, 1), (4, 2), (5, 30), (7, 0x0c), (10, 13), (15, 1)] {
            let mut damaged = bytes;
            damaged[at] = value;
            assert!(Stored::decode(&damaged).is_err(), "byte {at}");
        }
        let mut stale = bytes;
        stale[4] = 0;
        assert!(Stored::decode(&stale).is_err());
        assert!(Stored::decode(&bytes[..15]).is_err());
    }

    #[test]
    fn records_reject_out_of_range_fields() {
        let day = date(2026, 10, 6);
        assert_eq!(
            Accounting::new(day, 3001, 0),
            Err(RewardError::InvalidCount)
        );
        assert_eq!(
            Accounting::new(day, 0, 30),
            Err(RewardError::InvalidFraction)
        );
        let entry = enter(Some(record(day, 1, 0)), 0, day).unwrap();
        assert_eq!(settle(&entry, 0, 3001, day), Err(RewardError::InvalidCount));
    }

    #[test]
    fn threshold_and_battle_point_conversion_follow_native_claims() {
        let gen6 = |amount| Receipt::Gen6 {
            flagged: true,
            amount,
        };
        for quoted in [0, 1, 9] {
            assert_eq!(
                verify_claim(Currency::Miles, quoted, quoted, 0, gen6(quoted as u16)),
                Err(RewardError::BelowThreshold)
            );
        }
        let exact = verify_claim(Currency::BattlePoints, 10, 10, 0, gen6(1)).unwrap();
        assert_eq!((exact.transferred, exact.retained), (1, 0));
        let leftover = verify_claim(Currency::BattlePoints, 1237, 1237, 7, gen6(123)).unwrap();
        assert_eq!((leftover.transferred, leftover.retained), (123, 7));
        let full = verify_claim(Currency::Miles, MILES_CAP, MILES_CAP, 0, gen6(u16::MAX)).unwrap();
        assert_eq!(full.transferred, MILES_CAP);
    }

    #[test]
    fn claims_need_the_expected_debit_and_receipt() {
        let inserted = Receipt::Gen7 {
            count_before: 47,
            count_after: 48,
            kind: GEN7_BP_KIND,
            amount: 12,
        };
        assert!(verify_claim(Currency::BattlePoints, 125, 125, 5, inserted).is_ok());
        assert_eq!(
            verify_claim(Currency::Miles, 125, 125, 0, inserted),
            Err(RewardError::UnsupportedCurrency)
        );
        // The ignored native insertion result: debit happened, no record.
        for failed in [
            Receipt::Gen7 {
                count_before: 47,
                count_after: 47,
                kind: GEN7_BP_KIND,
                amount: 12,
            },
            Receipt::Gen7 {
                count_before: 48,
                count_after: 49,
                kind: GEN7_BP_KIND,
                amount: 12,
            },
            Receipt::Gen7 {
                count_before: 3,
                count_after: 4,
                kind: 1,
                amount: 12,
            },
            Receipt::Gen7 {
                count_before: 3,
                count_after: 4,
                kind: GEN7_BP_KIND,
                amount: 13,
            },
            Receipt::Gen6 {
                flagged: false,
                amount: 12,
            },
            Receipt::Gen6 {
                flagged: true,
                amount: 125,
            },
        ] {
            assert_eq!(
                verify_claim(Currency::BattlePoints, 125, 125, 5, failed),
                Err(RewardError::ReceiptMismatch)
            );
        }
        assert_eq!(
            verify_claim(Currency::BattlePoints, 125, 125, 125, inserted),
            Err(RewardError::BalanceMismatch)
        );
        assert_eq!(
            verify_claim(Currency::BattlePoints, 125, 120, 5, inserted),
            Err(RewardError::BalanceMismatch)
        );
    }
}
