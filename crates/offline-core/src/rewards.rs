//! Shared Poké Miles accounting and redemption checks. Pure integer logic.
//!
//! One stored Pokémon earns one Mile in 30 days, counted by the hour:
//! `count * clock hours / 720`, with the remainder carried in Pokémon-hours.
//! The whole-Miles balance and the date of the last save are the native
//! fields of the Bank body. The remainder and the day of the last redemption
//! are kept in two places of that body the original never reaches offline
//! (`native_blob`). Neither date ever moves back, so an earlier clock
//! earns nothing and never blocks a save. Native evidence is in
//! docs/internals.md.

/// Native balance setter 001d59fc and getter 001d588c clamp to this value.
pub const MILES_CAP: u32 = 65_535;
/// Pokémon-hours in one Mile: the native accrual multiplies by 1/30 per
/// stored Pokémon and by 1/24 per hour.
pub const UNIT: u32 = 720;
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

/// A console-local date and time to the minute, as the native reward date
/// holds it. Ordering is chronological.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Moment {
    day: Date,
    hour: u8,
    minute: u8,
}

impl Moment {
    /// Size of the native reward date: u16 year, then month, day, hour and
    /// minute as one byte each (native 001d5880 points at it).
    pub const NATIVE_SIZE: usize = 6;

    pub fn new(day: Date, hour: u8, minute: u8) -> Result<Self, RewardError> {
        if hour > 23 || minute > 59 {
            return Err(RewardError::InvalidDate);
        }
        Ok(Self { day, hour, minute })
    }
    pub const fn day(self) -> Date {
        self.day
    }
    /// The native reward date. Anything that is not a date and time is "no
    /// date yet", as the original takes a month outside 1..=12.
    pub fn from_native(bytes: &[u8; Self::NATIVE_SIZE]) -> Option<Self> {
        let day = Date::new(u16::from_le_bytes([bytes[0], bytes[1]]), bytes[2], bytes[3]).ok()?;
        Self::new(day, bytes[4], bytes[5]).ok()
    }
    pub fn to_native(self) -> [u8; Self::NATIVE_SIZE] {
        let year = self.day.year.to_le_bytes();
        [
            year[0],
            year[1],
            self.day.month,
            self.day.day,
            self.hour,
            self.minute,
        ]
    }
    /// Full clock hours crossed from `earlier` to `self`; zero when not
    /// later. Every hour is crossed once, whenever the saves in it are made.
    pub fn hours_since(self, earlier: Self) -> u32 {
        let hours = |moment: Self| moment.day.number() * 24 + u32::from(moment.hour);
        hours(self).saturating_sub(hours(earlier))
    }
}

/// What the patch keeps for the Miles beside the native balance and date: the
/// remainder and the day of the last redemption. Both live in the Bank body
/// (`native_blob::FRACTION_OFFSET`, `REDEEMED_OFFSET`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// Pokémon-hours not yet a whole Mile. A save writes less than `UNIT`; a
    /// larger number is paid out at the next load like any other.
    pub fraction: u16,
    /// The day of the last redemption. It never moves backwards.
    pub redeemed: Option<Date>,
}
impl Record {
    /// A new Bank: no remainder, never redeemed.
    pub const NONE: Self = Self {
        fraction: 0,
        redeemed: None,
    };

    /// The two fields as the body holds them: the day as u16 year, month,
    /// day, and the remainder as a u16. Anything that is not a date is "never
    /// redeemed": a new Bank has zeros there, and a Bank from the server the
    /// count the original kept in that place.
    pub fn from_fields(redeemed: [u8; 4], fraction: [u8; 2]) -> Self {
        let year = u16::from_le_bytes([redeemed[0], redeemed[1]]);
        Self {
            fraction: u16::from_le_bytes(fraction),
            redeemed: Date::new(year, redeemed[2], redeemed[3]).ok(),
        }
    }

    /// Inverse of `from_fields`; no day is four zero bytes.
    pub fn to_fields(self) -> ([u8; 4], [u8; 2]) {
        let mut redeemed = [0; 4];
        if let Some(day) = self.redeemed {
            redeemed[..2].copy_from_slice(&day.year.to_le_bytes());
            redeemed[2] = day.month;
            redeemed[3] = day.day;
        }
        (redeemed, self.fraction.to_le_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    fraction: u32,
    balance: u32,
    earned: u32,
}

fn accrue(fraction: u32, count: u32, hours: u32, balance: u32) -> Step {
    let total = u64::from(count) * u64::from(hours) + u64::from(fraction);
    let whole = total / u64::from(UNIT);
    let balance = balance.min(MILES_CAP);
    // Whole Miles above the native limit are discarded; the remainder is kept.
    let credited = whole.min(u64::from(MILES_CAP - balance)) as u32;
    Step {
        fraction: (total % u64::from(UNIT)) as u32,
        balance: balance + credited,
        earned: credited,
    }
}

/// Provisional state of one loaded session. Nothing here is durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    through: Moment,
    today: Date,
    saved: Option<Date>,
    redeemed: Option<Date>,
    count: u32,
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
    /// What the reward screens have to say for `balance`, the live native
    /// balance of this session.
    pub fn dialog(&self, balance: u32) -> Dialog {
        dialog(self.redeemed, self.saved, self.today, balance)
    }
}

/// Calculates the entry quote: the hours from the stored date to `now` at
/// `count`, the Pokémon in the boxes as loaded, on top of the stored balance
/// and remainder. A Bank without a stored date earns nothing yet.
pub fn enter(
    record: Record,
    stored: Option<Moment>,
    balance: u32,
    count: u32,
    now: Moment,
) -> Result<Entry, RewardError> {
    if count > MAX_STORED {
        return Err(RewardError::InvalidCount);
    }
    let hours = stored.map_or(0, |stored| now.hours_since(stored));
    let step = accrue(u32::from(record.fraction), count, hours, balance);
    Ok(Entry {
        through: stored.map_or(now, |stored| stored.max(now)),
        today: now.day,
        saved: stored.map(Moment::day),
        redeemed: record.redeemed,
        count,
        fraction: step.fraction,
        balance: step.balance,
        earned: step.earned,
    })
}

/// What one Save and Quit must persist together: the balance and the date in
/// the Bank body, and the record in its two places there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub balance: u32,
    pub date: Moment,
    pub record: Record,
    /// Miles credited between the entry quote and this save.
    pub earned: u32,
}

/// Accounts for the hours elapsed since entry at the count the Bank was
/// loaded with. `balance` is the live native balance, already reduced by any
/// confirmed redemption; `redeemed` says whether one happened. A clock
/// earlier than a stored date earns nothing and moves neither date back.
pub fn settle(entry: &Entry, balance: u32, now: Moment, redeemed: bool) -> Settlement {
    let step = accrue(
        entry.fraction,
        entry.count,
        now.hours_since(entry.through),
        balance,
    );
    let day = match (redeemed, entry.redeemed) {
        (false, day) => day,
        (true, Some(day)) => Some(day.max(now.day)),
        (true, None) => Some(now.day),
    };
    Settlement {
        balance: step.balance,
        date: entry.through.max(now),
        record: Record {
            // Below `UNIT` after every accrual.
            fraction: step.fraction as u16,
            redeemed: day,
        },
        earned: step.earned,
    }
}

/// What the reward screens say after a game was loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// Nothing: the boxes open directly.
    Silent,
    /// The original message for a balance that cannot be redeemed yet.
    Message,
    /// The original message and the redemption choices.
    Offer,
}

/// Decides the reward screens from the day of the last redemption, the day
/// of the last save and the balance. Nothing is said without a balance or
/// after a redemption today. Ten or more are offered at every opening until
/// they are redeemed. Fewer are announced once a day: saving on that day
/// makes it the day of the last save. A stored day later than `today`, after
/// the clock was set back, counts as today.
pub fn dialog(redeemed: Option<Date>, saved: Option<Date>, today: Date, balance: u32) -> Dialog {
    let not_earlier = |day: Option<Date>| day.is_some_and(|day| day >= today);
    if balance == 0 || not_earlier(redeemed) {
        Dialog::Silent
    } else if balance >= REDEEM_THRESHOLD {
        Dialog::Offer
    } else if not_earlier(saved) {
        Dialog::Silent
    } else {
        Dialog::Message
    }
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
    /// A moment in October 2026.
    fn at(day: u8, hour: u8, minute: u8) -> Moment {
        Moment::new(date(2026, 10, day), hour, minute).unwrap()
    }
    fn record(fraction: u16, redeemed: Option<u8>) -> Record {
        Record {
            fraction,
            redeemed: redeemed.map(|day| date(2026, 10, day)),
        }
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
    fn hours_are_the_clock_hours_crossed_and_native_dates_round_trip() {
        assert_eq!(at(6, 10, 55).hours_since(at(6, 10, 5)), 0);
        assert_eq!(at(6, 11, 1).hours_since(at(6, 10, 55)), 1);
        assert_eq!(at(6, 11, 59).hours_since(at(6, 11, 1)), 0);
        assert_eq!(at(7, 0, 1).hours_since(at(6, 23, 59)), 1);
        assert_eq!(at(7, 9, 0).hours_since(at(6, 18, 10)), 15);
        assert_eq!(at(31, 12, 0).hours_since(at(1, 12, 0)), 720);
        assert_eq!(at(6, 10, 0).hours_since(at(6, 11, 0)), 0);
        let new_year = Moment::new(date(2027, 1, 1), 0, 0).unwrap();
        let old_year = Moment::new(date(2026, 12, 31), 23, 0).unwrap();
        assert_eq!(new_year.hours_since(old_year), 1);
        assert!(at(6, 10, 5) < at(6, 10, 55) && at(6, 23, 59) < at(7, 0, 0));

        let bytes = [0xea, 0x07, 10, 6, 18, 10];
        assert_eq!(Moment::from_native(&bytes), Some(at(6, 18, 10)));
        assert_eq!(at(6, 18, 10).to_native(), bytes);
        for invalid in [
            [0; 6],
            [0xea, 0x07, 0, 6, 18, 10],
            [0xea, 0x07, 13, 6, 18, 10],
            [0xea, 0x07, 2, 30, 18, 10],
            [0xea, 0x07, 10, 6, 24, 10],
            [0xea, 0x07, 10, 6, 18, 60],
        ] {
            assert_eq!(Moment::from_native(&invalid), None);
        }
        assert!(Moment::new(date(2026, 10, 6), 24, 0).is_err());
    }

    #[test]
    fn thirty_pokemon_earn_one_mile_a_day_and_the_rest_is_carried() {
        // (count, hours, whole Miles, Pokémon-hours left)
        for (count, hours, miles, fraction) in [
            (0, 100, 0, 0),
            (1, 719, 0, 719),
            (1, 720, 1, 0),
            (7, 24, 0, 168),
            (15, 24, 0, 360),
            (15, 48, 1, 0),
            (29, 24, 0, 696),
            (30, 24, 1, 0),
            (31, 24, 1, 24),
            (720, 1, 1, 0),
            (3000, 1, 4, 120),
            (3000, 24, 100, 0),
            (3000, 168, 700, 0),
        ] {
            let step = accrue(0, count, hours, 0);
            assert_eq!((step.balance, step.fraction), (miles, fraction));
            assert_eq!(step.earned, miles);
        }
    }

    #[test]
    fn an_odd_count_pays_exactly_over_thirty_daily_saves() {
        let (mut balance, mut stored, mut kept) = (0, at(1, 9, 0), Record::NONE);
        for day in 2..=31 {
            let entry = enter(kept, Some(stored), balance, 7, at(day, 9, 0)).unwrap();
            let saved = settle(&entry, entry.balance(), at(day, 9, 20), false);
            (balance, stored, kept) = (saved.balance, saved.date, saved.record);
        }
        // 7 Pokémon for 720 hours: one Mile each, nothing left over.
        assert_eq!((balance, kept.fraction), (7, 0));
        assert_eq!(stored, at(31, 9, 20));
    }

    #[test]
    fn hours_before_a_save_count_at_the_old_number_and_those_after_at_the_new() {
        // Saved at 10:00 with 1,200 Pokémon after a redemption left 8 Miles.
        let first = enter(
            record(0, Some(6)),
            Some(at(6, 10, 0)),
            8,
            1200,
            at(6, 18, 0),
        )
        .unwrap();
        assert_eq!(
            (first.balance(), first.earned(), first.fraction),
            (21, 13, 240)
        );
        assert_eq!(first.dialog(first.balance()), Dialog::Silent);
        // 300 are deposited; the save ten minutes later crosses no hour.
        let saved = settle(&first, first.balance(), at(6, 18, 10), false);
        assert_eq!((saved.balance, saved.earned), (21, 0));
        assert_eq!(
            (saved.date, saved.record),
            (at(6, 18, 10), record(240, Some(6)))
        );
        // The next morning the 15 hours are paid for 1,500.
        let next = enter(
            saved.record,
            Some(saved.date),
            saved.balance,
            1500,
            at(7, 9, 0),
        )
        .unwrap();
        assert_eq!(
            (next.balance(), next.earned(), next.fraction),
            (52, 31, 420)
        );
        assert_eq!(next.dialog(next.balance()), Dialog::Offer);
    }

    #[test]
    fn a_session_pays_its_own_hours_at_save_with_the_count_it_was_loaded_with() {
        let entry = enter(Record::NONE, Some(at(5, 20, 0)), 0, 720, at(6, 8, 0)).unwrap();
        assert_eq!(entry.balance(), 12);
        // All twelve are redeemed, then the session stays open until 14:40.
        let saved = settle(&entry, 0, at(6, 14, 40), true);
        assert_eq!((saved.balance, saved.earned), (6, 6));
        assert_eq!(
            (saved.date, saved.record),
            (at(6, 14, 40), record(0, Some(6)))
        );
        // Saving twice in one hour pays nothing twice.
        let again = enter(saved.record, Some(saved.date), 6, 720, at(6, 14, 50)).unwrap();
        let resaved = settle(&again, again.balance(), at(6, 14, 59), false);
        assert_eq!((again.earned(), resaved.earned, resaved.balance), (0, 0, 6));
    }

    #[test]
    fn a_bank_without_a_date_earns_nothing_until_it_was_saved() {
        let entry = enter(Record::NONE, None, 4321, 3000, at(6, 12, 0)).unwrap();
        assert_eq!((entry.balance(), entry.earned()), (4321, 0));
        assert_eq!(entry.dialog(entry.balance()), Dialog::Offer);
        let saved = settle(&entry, 4321, at(6, 12, 30), false);
        assert_eq!((saved.balance, saved.date), (4321, at(6, 12, 30)));
        assert_eq!(saved.record, Record::NONE);
        let next = enter(saved.record, Some(saved.date), 4321, 3000, at(7, 12, 0)).unwrap();
        assert_eq!(next.balance(), 4421);
    }

    #[test]
    fn cap_discards_whole_miles_but_keeps_the_fraction() {
        let entry = enter(
            record(100, None),
            Some(at(1, 0, 0)),
            MILES_CAP - 1,
            31,
            at(11, 0, 0),
        )
        .unwrap();
        // 100 + 31 * 240 Pokémon-hours: ten whole Miles earned, one credited.
        assert_eq!(
            (entry.balance(), entry.earned(), entry.fraction),
            (MILES_CAP, 1, 340)
        );
        let over = enter(Record::NONE, Some(at(1, 0, 0)), 70_000, 1, at(2, 0, 0)).unwrap();
        assert_eq!(over.balance(), MILES_CAP);
        let first = Moment::new(date(1, 1, 1), 0, 0).unwrap();
        let last = Moment::new(date(0x3fff, 12, 31), 23, 59).unwrap();
        let long = enter(record(u16::MAX, None), Some(first), 0, 3000, last).unwrap();
        assert_eq!(long.balance(), MILES_CAP);
        assert!(long.fraction < UNIT);
    }

    #[test]
    fn neither_date_goes_back_when_the_clock_does() {
        let stored = at(6, 12, 0);
        let entry = enter(record(11, Some(6)), Some(stored), 50, 3000, at(1, 8, 0)).unwrap();
        assert_eq!(
            (entry.balance(), entry.earned(), entry.fraction),
            (50, 0, 11)
        );
        // A stored day ahead of the clock counts as today: nothing is said.
        assert_eq!(entry.dialog(entry.balance()), Dialog::Silent);
        // Saving works; the date stays, also after a redemption.
        let saved = settle(&entry, 50, at(2, 8, 0), true);
        assert_eq!((saved.balance, saved.earned), (50, 0));
        assert_eq!((saved.date, saved.record), (stored, record(11, Some(6))));
        // Hours already accounted are not paid again when the clock catches up.
        let caught_up = enter(saved.record, Some(saved.date), 50, 3000, stored).unwrap();
        assert_eq!(caught_up.earned(), 0);
        let after = enter(saved.record, Some(saved.date), 50, 3000, at(6, 13, 0)).unwrap();
        assert_eq!((after.earned(), after.fraction), (4, 131));
        // A redemption moves its day forward, and only forward.
        let later = settle(&after, 0, at(8, 9, 0), true);
        assert_eq!(later.record.redeemed, Some(date(2026, 10, 8)));
        let kept = settle(&after, after.balance(), at(8, 9, 0), false);
        assert_eq!(kept.record.redeemed, Some(date(2026, 10, 6)));
    }

    #[test]
    fn jumping_forward_pays_once_then_nothing_until_that_date_passes() {
        let (real, jumped) = (at(6, 12, 0), at(16, 12, 0));
        let entry = enter(Record::NONE, Some(real), 0, 30, jumped).unwrap();
        assert_eq!(entry.balance(), 10);
        let saved = settle(&entry, 10, jumped, false);
        assert_eq!(saved.date, jumped);
        // Back on the real date, and at every moment up to the jumped one.
        for now in [real, at(10, 0, 0), jumped] {
            let later = enter(saved.record, Some(saved.date), saved.balance, 30, now).unwrap();
            assert_eq!((later.balance(), later.earned()), (10, 0));
        }
        let after = enter(saved.record, Some(saved.date), 10, 30, at(17, 12, 0)).unwrap();
        assert_eq!(after.earned(), 1);
    }

    #[test]
    fn the_two_fields_of_the_record_are_explicit_and_round_trip() {
        let kept = Record {
            fraction: 719,
            redeemed: Some(date(2026, 10, 6)),
        };
        let fields = kept.to_fields();
        assert_eq!(fields, ([0xea, 0x07, 10, 6], [0xcf, 0x02]));
        assert_eq!(Record::from_fields(fields.0, fields.1), kept);
        assert_eq!(Record::NONE.to_fields(), ([0; 4], [0; 2]));
        assert_eq!(Record::from_fields([0; 4], [0; 2]), Record::NONE);
        // A remainder without a redemption is a record too.
        let (day, rest) = record(5, None).to_fields();
        assert_eq!(Record::from_fields(day, rest), record(5, None));
        // What is not a date is no redemption: the count the original kept
        // in that place, as a Bank from the server brings it, or damage.
        for not_a_date in [
            [2, 0, 0, 0],
            [0xff; 4],
            [0xea, 0x07, 13, 6],
            [0xea, 0x07, 10, 32],
        ] {
            assert_eq!(Record::from_fields(not_a_date, [7, 0]), record(7, None));
        }
        // A remainder above one Mile is paid out at the next load.
        let large = Record::from_fields([0; 4], 7425u16.to_le_bytes());
        let entry = enter(large, Some(at(6, 12, 0)), 0, 0, at(6, 12, 30)).unwrap();
        assert_eq!((entry.balance(), entry.fraction), (10, 225));
    }

    #[test]
    fn counts_above_the_boxes_are_rejected() {
        assert_eq!(
            enter(Record::NONE, None, 0, MAX_STORED + 1, at(6, 0, 0)),
            Err(RewardError::InvalidCount)
        );
        assert!(enter(Record::NONE, None, 0, MAX_STORED, at(6, 0, 0)).is_ok());
    }

    #[test]
    fn reward_screens_follow_the_redeemed_day_the_saved_day_and_the_balance() {
        let today = date(2026, 10, 6);
        let (yesterday, tomorrow) = (Some(date(2026, 10, 5)), Some(date(2026, 10, 7)));
        let now = Some(today);
        for saved in [None, yesterday, now, tomorrow] {
            // Without a balance nothing is said, whatever the dates.
            for redeemed in [None, yesterday, now, tomorrow] {
                assert_eq!(dialog(redeemed, saved, today, 0), Dialog::Silent);
            }
            // After a redemption today nothing is said, whatever the balance.
            for redeemed in [now, tomorrow] {
                for balance in [1, 9, 10, 500] {
                    assert_eq!(dialog(redeemed, saved, today, balance), Dialog::Silent);
                }
            }
            // Ten or more are offered until they are redeemed.
            for redeemed in [None, yesterday] {
                for balance in [10, 11, MILES_CAP] {
                    assert_eq!(dialog(redeemed, saved, today, balance), Dialog::Offer);
                }
            }
        }
        // Fewer are announced once a day: until a save on that day.
        for redeemed in [None, yesterday] {
            for balance in 1..10 {
                for saved in [None, yesterday] {
                    assert_eq!(dialog(redeemed, saved, today, balance), Dialog::Message);
                }
                for saved in [now, tomorrow] {
                    assert_eq!(dialog(redeemed, saved, today, balance), Dialog::Silent);
                }
            }
        }
    }

    #[test]
    fn a_small_balance_is_announced_once_a_day_and_an_offer_until_it_is_taken() {
        // 15 Pokémon, opened daily: the first Mile comes on the third day.
        let second = enter(Record::NONE, Some(at(1, 9, 0)), 0, 15, at(2, 9, 0)).unwrap();
        assert_eq!((second.balance(), second.fraction), (0, 360));
        assert_eq!(second.dialog(0), Dialog::Silent);
        let saved = settle(&second, 0, at(2, 9, 5), false);
        let third = enter(saved.record, Some(saved.date), 0, 15, at(3, 9, 0)).unwrap();
        assert_eq!(third.balance(), 1);
        assert_eq!(third.dialog(1), Dialog::Message);
        // Leaving without saving announces it again; a save does not.
        let saved = settle(&third, 1, at(3, 9, 5), false);
        let again = enter(saved.record, Some(saved.date), 1, 15, at(3, 20, 0)).unwrap();
        assert_eq!(again.dialog(again.balance()), Dialog::Silent);
        let next = enter(saved.record, Some(saved.date), 1, 15, at(4, 9, 0)).unwrap();
        assert_eq!(next.dialog(next.balance()), Dialog::Message);

        // 720 Pokémon: twelve Miles are declined and saved, then offered
        // again with what the day added.
        let morning = enter(record(0, Some(5)), Some(at(5, 20, 0)), 0, 720, at(6, 8, 0)).unwrap();
        assert_eq!(morning.dialog(morning.balance()), Dialog::Offer);
        let declined = settle(&morning, 12, at(6, 8, 30), false);
        let noon = enter(declined.record, Some(declined.date), 12, 720, at(6, 15, 0)).unwrap();
        assert_eq!((noon.balance(), noon.dialog(19)), (19, Dialog::Offer));
        // Battle Points leave nine: quiet for the rest of the day, although
        // the balance passes ten again, and offered the next morning.
        let taken = settle(&noon, 9, at(6, 15, 10), true);
        let evening = enter(taken.record, Some(taken.date), 9, 720, at(6, 20, 0)).unwrap();
        assert_eq!(
            (evening.balance(), evening.dialog(14)),
            (14, Dialog::Silent)
        );
        let tomorrow = enter(taken.record, Some(taken.date), 9, 720, at(7, 8, 0)).unwrap();
        assert_eq!(
            (tomorrow.balance(), tomorrow.dialog(26)),
            (26, Dialog::Offer)
        );
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
