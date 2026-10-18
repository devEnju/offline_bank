//! The Bank as releases up to v0.2.1 stored it (tag v0.2.1, 8104e4a).
//!
//! Four files, one per container: `/bank.bin` (the journal records and both
//! snapshots), `/dex.bin`, `/transport.bin` and `/rewards.bin` (two slots
//! each). The containers are those of v0.3.0 to v0.4.0 byte for byte; those
//! releases cut each into a file per unit. So they are read with v0.4.0's
//! frozen code (`v0_4_0::Containers`, `v0_4_0::settled`), each container as
//! the one file it is here. Only the rewards record means something else:
//! v0.2.1's own (`rewards`).
#![allow(dead_code)]

pub mod rewards;

use crate::{
    migrate::{Card, Stopped},
    source,
    target::{self, Miles},
    v0_4_0::{self, bank_files::FileName, Containers},
};
use bank_common::bank_files::UnitFile;
use offline_core::rewards::{Date, Moment, UNIT};
use rewards::{Stored, DIVISOR};

// The lengths are v0.2.1's own (`session::BANK_LAYOUT.file_len()` and each
// side kind's `file_len()` at its tag), not computed from the code here.
static BANK: [UnitFile; 1] = [UnitFile::new("/bank.bin", 0x16_4c54)];
static DEX: [UnitFile; 1] = [UnitFile::new("/dex.bin", 0xe980)];
static TRANSPORT: [UnitFile; 1] = [UnitFile::new("/transport.bin", 0x3720)];
static REWARDS: [UnitFile; 1] = [UnitFile::new("/rewards.bin", 0xa0)];

fn units(file: FileName) -> &'static [UnitFile] {
    match file {
        FileName::Bank => &BANK,
        FileName::Dex => &DEX,
        FileName::Transport => &TRANSPORT,
        FileName::Rewards => &REWARDS,
    }
}

pub(crate) struct Way;
impl source::Way for Way {
    const TAG: &'static [u8] = b"v0.2.1";
    /// Its `/bank.bin`: the current release finds no journal record.
    const REFUSED: u32 = 0x16_4c54;

    /// Its Bank file, which holds its journal. A current `/bank.bin` has
    /// another size.
    fn there<C: Card>(card: &mut C) -> Result<bool, C::Error> {
        card.exists(&BANK)
    }

    fn read<C: Card>(card: &mut C, body: &mut [u8]) -> Result<(), Stopped<C::Error>> {
        let loaded = v0_4_0::settled(Containers::new(card, units), body)?;
        // A record v0.2.1 does not read is none to it: no Miles, no record.
        let stored = loaded
            .record
            .and_then(|bytes| Stored::decode(&bytes))
            .unwrap_or(Stored::NONE);
        into_body(body, stored);
        Ok(())
    }

    fn remove<C: Card>(card: &mut C) -> Result<(), C::Error> {
        for files in [&DEX, &REWARDS, &TRANSPORT, &BANK] {
            card.remove(files)?;
        }
        Ok(())
    }
}

/// The Miles as v0.2.1 kept them, as the target keeps them. The balance is
/// the record's: v0.2.1 kept zero in the body. Accrual goes on from the
/// start of the day the record is accounted through, and its thirtieths
/// become Pokémon-hours. Without a record v0.2.1 paid nothing until the
/// first save, which the current release does for a body without a date.
pub(crate) fn into_body(body: &mut [u8], stored: Stored) {
    let start = stored.accounting.and_then(|record| {
        let through = record.accounted_through();
        let day = Date::new(through.year(), through.month(), through.day()).ok()?;
        Some((record, Moment::new(day, 0, 0).ok()?))
    });
    let miles = match start {
        Some((record, start)) => Miles {
            balance: stored.balance,
            from: start.to_native(),
            remainder: (record.fraction() * (UNIT / DIVISOR)) as u16,
        },
        None => Miles {
            balance: stored.balance,
            from: Miles::NO_DATE,
            remainder: 0,
        },
    };
    target::into_body(body, miles);
}

/// Banks of this way for the tests: v0.4.0's, joined into single files.
#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use super::*;
    use crate::{
        migrate::{Cause, Done, Step},
        testing::{convert, current, only_the_target_files, Fixtures, Sd},
        v0_4_0::{
            rewards::FILE, sections::DEX_FILE, session::BANK_LAYOUT, tests as before, transport,
        },
    };
    use bank_common::testing::body;
    use offline_core::native_blob::{self, MILES_OFFSET, REWARD_DATE_OFFSET};
    use std::vec::Vec;

    #[test]
    fn its_files_have_the_lengths_of_v0_2_1() {
        // The containers v0.4.0's code reads are as long as v0.2.1's files.
        for file in [
            FileName::Bank,
            FileName::Dex,
            FileName::Transport,
            FileName::Rewards,
        ] {
            assert_eq!(units(file)[0].len, file.size(), "{file:?}");
        }
        assert_eq!(BANK_LAYOUT.file_len(), 0x16_4c54);
        assert_eq!(DEX_FILE.file_len(), 0xe980);
        assert_eq!(transport::KIND.file_len(), 0x3720);
        assert_eq!(FILE.file_len(), 0xa0);
    }

    /// A record as v0.2.1 lays it out, written here byte by byte.
    fn record(balance: u32, fraction: u8, count: u16, day: (u16, u8, u8)) -> [u8; 16] {
        let mut out = [0; 16];
        out[..4].copy_from_slice(&balance.to_le_bytes());
        out[4] = 1;
        out[5] = fraction;
        out[6..8].copy_from_slice(&count.to_le_bytes());
        out[8..10].copy_from_slice(&day.0.to_le_bytes());
        out[10] = day.1;
        out[11] = day.2;
        out
    }

    #[test]
    fn every_record_v0_2_1_reads_is_read_so() {
        for fraction in 0..30 {
            let stored = Stored::decode(&record(1234, fraction, 90, (2026, 10, 6))).unwrap();
            assert_eq!(stored.balance, 1234);
            let accounting = stored.accounting.unwrap();
            assert_eq!(accounting.fraction(), u32::from(fraction));
            assert_eq!(accounting.saved_count(), 90);
            let day = accounting.accounted_through();
            assert_eq!((day.year(), day.month(), day.day()), (2026, 10, 6));
        }
        // State 0: a balance and no record yet.
        let mut none = [0; 16];
        none[..4].copy_from_slice(&40u32.to_le_bytes());
        assert_eq!(
            Stored::decode(&none),
            Some(Stored {
                balance: 40,
                accounting: None
            })
        );
        // What it refuses: a remainder of a whole Mile, too many Pokémon, no
        // date, an unknown state, bytes after the record, a balance above
        // the cap, state 0 with anything beside the balance.
        let bad = [
            record(1, 30, 90, (2026, 10, 6)),
            record(1, 0, 3001, (2026, 10, 6)),
            record(1, 0, 90, (2026, 2, 30)),
            record(1, 0, 90, (0, 1, 1)),
            {
                let mut state = record(1, 0, 90, (2026, 10, 6));
                state[4] = 2;
                state
            },
            {
                let mut tail = record(1, 0, 90, (2026, 10, 6));
                tail[15] = 1;
                tail
            },
            record(65_536, 0, 90, (2026, 10, 6)),
            {
                let mut early = [0; 16];
                early[6] = 1;
                early
            },
        ];
        for bytes in bad {
            assert_eq!(Stored::decode(&bytes), None, "{bytes:?}");
        }
        assert_eq!(Stored::decode(&[0; 15]), None);
    }

    fn miles(body: &[u8]) -> (u32, Option<Moment>, offline_core::rewards::Record) {
        let view = native_blob::NativeBlobView::parse_layout(body).unwrap();
        (view.miles(), view.reward_date(), view.rewards())
    }

    #[test]
    fn the_miles_go_on_from_the_day_v0_2_1_paid_through() {
        let body = bank_common::testing::body(0x31, 0x44, 0);
        for fraction in 0..30u8 {
            let mut converted = body.clone();
            converted[REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + 6]
                .copy_from_slice(&[0xea, 0x07, 10, 6, 17, 45]);
            let stored = Stored::decode(&record(1234, fraction, 90, (2026, 10, 6))).unwrap();
            into_body(&mut converted, stored);
            let (balance, date, kept) = miles(&converted);
            assert_eq!(balance, 1234);
            let start = Moment::new(Date::new(2026, 10, 6).unwrap(), 0, 0).unwrap();
            assert_eq!(date, Some(start));
            assert_eq!(kept.fraction, u16::from(fraction) * 24);
            assert_eq!(kept.redeemed, None);
            // Beside the places a new Bank has clear, only the Miles places
            // differ from the body.
            let mut cleared = body.clone();
            native_blob::clear_unused(&mut cleared);
            for (at, (was, is)) in cleared.iter().zip(&converted).enumerate() {
                let miles = REWARD_DATE_OFFSET..MILES_OFFSET + 4;
                assert!(miles.contains(&at) || was == is, "{at:#x}");
            }
        }
    }

    #[test]
    fn what_v0_2_1_would_have_paid_is_paid_and_no_more() {
        use offline_core::rewards::{enter, Record};
        let through = (2026, 10, 6);
        for (count, fraction) in [(90u16, 0u8), (90, 29), (3000, 17), (1, 29), (0, 5)] {
            let mut body = bank_common::testing::body(0x31, 0x44, 0);
            let stored = Stored::decode(&record(10, fraction, count, through)).unwrap();
            into_body(&mut body, stored);
            let (balance, date, kept) = miles(&body);
            let later = [
                (0u32, (2026, 10, 6)),
                (1, (2026, 10, 7)),
                (2, (2026, 10, 8)),
                (30, (2026, 11, 5)),
                (365, (2027, 10, 6)),
            ];
            for (days, (year, month, on)) in later {
                let now = Moment::new(Date::new(year, month, on).unwrap(), 0, 0).unwrap();
                // v0.2.1 on that day: count × days / 30, the thirtieths on top.
                let thirtieths = u32::from(count) * days + u32::from(fraction);
                let expected = (10 + thirtieths / 30).min(65_535);
                let entry = enter(
                    Record {
                        fraction: kept.fraction,
                        redeemed: None,
                    },
                    date,
                    balance,
                    u32::from(count),
                    now,
                )
                .unwrap();
                assert_eq!(entry.balance(), expected, "{count} {fraction} {days}");
            }
        }
    }

    #[test]
    fn without_a_record_nothing_is_paid_until_the_first_save() {
        let mut body = bank_common::testing::body(0x31, 0x44, 0);
        body[REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + 6].copy_from_slice(&[0xe8, 0x07, 3, 1, 9, 0]);
        body[MILES_OFFSET..MILES_OFFSET + 4].copy_from_slice(&77u32.to_le_bytes());
        let mut none = body.clone();
        into_body(&mut none, Stored::NONE);
        let (balance, date, kept) = miles(&none);
        assert_eq!((balance, date, kept.fraction), (0, None, 0));
        // A balance kept without a record yet.
        into_body(
            &mut body,
            Stored {
                balance: 40,
                accounting: None,
            },
        );
        assert_eq!(miles(&body).0, 40);
        assert_eq!(miles(&body).1, None);
    }
    pub(crate) const FIXTURES: Fixtures = Fixtures {
        used,
        in_progress,
        unsettled,
    };

    /// The same Bank as v0.2.1 stores it: each container one file, its
    /// units one after the other. A unit that is not there is a void slot.
    pub(crate) fn as_v0_2_1(card: &Sd) -> Sd {
        let files = card.files();
        let joined = Sd::default();
        let mut state = joined.0.borrow_mut();
        state.console = card.0.borrow().console;
        for file in [
            FileName::Bank,
            FileName::Dex,
            FileName::Transport,
            FileName::Rewards,
        ] {
            let split = file.units();
            if split.iter().all(|unit| !files.contains_key(unit.path)) {
                continue;
            }
            let mut bytes = Vec::new();
            for unit in split {
                match files.get(unit.path) {
                    Some(unit) => bytes.extend_from_slice(unit),
                    None => bytes.resize(bytes.len() + unit.len as usize, 0),
                }
            }
            state.files.insert(units(file)[0].path, bytes);
        }
        drop(state);
        joined
    }
    /// A card with a Bank v0.2.1 created and saved once, and the body the
    /// current release has to load from it.
    pub(crate) fn used(console: bool) -> (Sd, Vec<u8>) {
        let (card, _) = before::used(console);
        let bytes = record(1234, 17, 90, (2026, 10, 6));
        before::write_record(&card, bytes);
        let mut expected = before::saved();
        into_body(&mut expected, Stored::decode(&bytes).unwrap());
        (as_v0_2_1(&card.restarted(None)), expected)
    }
    fn in_progress() -> Sd {
        as_v0_2_1(&(before::FIXTURES.in_progress)())
    }
    fn unsettled() -> Vec<Sd> {
        (before::FIXTURES.unsettled)()
            .iter()
            .map(as_v0_2_1)
            .collect()
    }

    #[test]
    fn its_record_goes_into_the_body() {
        let (card, body) = used(false);
        // Four files: one per container.
        assert_eq!(card.files().len(), 4);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(current(&card).unwrap() == body);
        // The record's balance and thirtieths are in the body.
        let view = native_blob::NativeBlobView::parse_layout(&body).unwrap();
        assert_eq!(view.miles(), 1234);
        assert_eq!(view.rewards().fraction, 17 * 24);
    }

    #[test]
    fn a_bank_that_was_never_saved_is_converted_too() {
        let card = Sd::default();
        let first = body(0x31, 0x44, 2);
        before::created(&card, &first);
        let card = as_v0_2_1(&card);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_target_files(&card));
        // No Miles and no record yet: nothing paid until the first save.
        let mut expected = first;
        into_body(&mut expected, Stored::NONE);
        assert!(current(&card).unwrap() == expected);
    }

    #[test]
    fn a_damaged_spare_half_of_the_bank_file_is_not_needed() {
        let (card, body) = used(false);
        let records = 2 * v0_4_0::bank_files::RECORD as usize;
        let snapshot = v0_4_0::bank_files::SNAPSHOT as usize;
        let mut converted = 0;
        for start in [records, records + snapshot] {
            let torn = card.restarted(None);
            let middle = start + snapshot / 2;
            torn.0
                .borrow_mut()
                .torn
                .push((BANK[0].path, middle..middle + 512));
            let files = torn.files();
            match convert(&torn) {
                Ok(Done::Converted) => {
                    converted += 1;
                    assert!(only_the_target_files(&torn));
                    assert!(current(&torn).unwrap() == body);
                }
                Err(Stopped::Failed(Step::ReadOld, Cause::Io(()))) => {
                    assert!(torn.files() == files)
                }
                other => panic!("{other:?}"),
            }
        }
        // The half of the current save is needed; the other is not.
        assert_eq!(converted, 1);
    }
}
