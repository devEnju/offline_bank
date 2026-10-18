//! The target: the files the current release keeps the Bank in, and the
//! only part of the migration that knows them. It writes a body as a Bank
//! under temporary names, reads it back, and puts it in place of the files
//! of any earlier way. Its names and sizes are `bank-common`'s, so it follows
//! the current files by itself; when they change, so does this module.
//!
//! An earlier way meets the target only through the native body, which the
//! original defines, and `Miles`: what the current release keeps in the
//! body beside the native data.

use crate::migrate::{Card, Cause, Step, Stopped};
use bank_common::{
    bank_files::{self, BankFiles, FileName, Files, UnitFile, MAIN, MOVER, RECORD, SNAPSHOT},
    session,
};
use offline_core::{
    crc32,
    native_blob::{self, BLOB_SIZE, FRACTION_OFFSET, MILES_OFFSET, REWARD_DATE_OFFSET},
    rewards::Moment,
    Phase, Storage, StoreError,
};

/// What the current release keeps of the Miles in the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Miles {
    /// Whole Miles.
    pub balance: u32,
    /// The reward date as the body holds it (`Moment::to_native`), from
    /// which hours are counted. What is not a date, as all zero, is "no date
    /// yet": nothing is earned until the first save.
    pub from: [u8; Moment::NATIVE_SIZE],
    /// Pokémon-hours not yet a whole Mile.
    pub remainder: u16,
}

impl Miles {
    /// No date yet.
    pub const NO_DATE: [u8; Moment::NATIVE_SIZE] = [0; Moment::NATIVE_SIZE];

    /// The Miles as `body` holds them.
    pub fn in_body(body: &[u8]) -> Self {
        let at = |offset: usize, len: usize| &body[offset..offset + len];
        Self {
            balance: u32::from_le_bytes(at(MILES_OFFSET, 4).try_into().unwrap()),
            from: at(REWARD_DATE_OFFSET, Moment::NATIVE_SIZE)
                .try_into()
                .unwrap(),
            remainder: u16::from_le_bytes(at(FRACTION_OFFSET, 2).try_into().unwrap()),
        }
    }
}

/// Makes `body` what the current release keeps: the unused places as a new
/// Bank has them (`native_blob::clear_unused`), with `miles` in theirs.
/// Every earlier way finishes its body here.
pub fn into_body(body: &mut [u8], miles: Miles) {
    native_blob::clear_unused(body);
    body[MILES_OFFSET..MILES_OFFSET + 4].copy_from_slice(&miles.balance.to_le_bytes());
    body[REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + Moment::NATIVE_SIZE].copy_from_slice(&miles.from);
    body[FRACTION_OFFSET..FRACTION_OFFSET + 2].copy_from_slice(&miles.remainder.to_le_bytes());
}

/// The containers in the order they are put in place: the Bank last,
/// because its journal is what makes a Bank exist.
pub(crate) const ORDER: [FileName; 3] = [FileName::Main, FileName::Transport, FileName::Bank];
static TEMPORARY_BANK: [UnitFile; 4] = [
    UnitFile::new("/m0.tmp", RECORD),
    UnitFile::new("/m1.tmp", RECORD),
    UnitFile::new("/m2.tmp", SNAPSHOT),
    UnitFile::new("/m3.tmp", SNAPSHOT),
];
static TEMPORARY_MAIN: [UnitFile; 2] = [
    UnitFile::new("/m4.tmp", MAIN),
    UnitFile::new("/m5.tmp", MAIN),
];
static TEMPORARY_TRANSPORT: [UnitFile; 2] = [
    UnitFile::new("/m6.tmp", MOVER),
    UnitFile::new("/m7.tmp", MOVER),
];

/// A container's files under their temporary names.
pub(crate) fn temporary(file: FileName) -> &'static [UnitFile] {
    match file {
        FileName::Bank => &TEMPORARY_BANK,
        FileName::Main => &TEMPORARY_MAIN,
        FileName::Transport => &TEMPORARY_TRANSPORT,
    }
}

type Outcome<T, C> = Result<T, Stopped<<C as Card>::Error>>;

fn io<E>(step: Step) -> impl Fn(E) -> Stopped<E> {
    move |error| Stopped::Failed(step, Cause::Io(error))
}

/// The containers under their temporary names.
struct Temporary<'a, C: Card>(&'a mut C);
impl<C: Card> Files for Temporary<'_, C> {
    type Storage = C::Storage;
    fn open(&mut self, file: FileName) -> Result<Option<Self::Storage>, C::Error> {
        self.0.open(temporary(file))
    }
    fn create(&mut self, file: FileName) -> Result<Self::Storage, C::Error> {
        self.0.create(temporary(file))
    }
}

/// The containers under their own names.
struct Current<'a, C: Card>(&'a mut C);
impl<C: Card> Files for Current<'_, C> {
    type Storage = C::Storage;
    fn open(&mut self, file: FileName) -> Result<Option<Self::Storage>, C::Error> {
        self.0.open(file.units())
    }
    fn create(&mut self, file: FileName) -> Result<Self::Storage, C::Error> {
        self.0.create(file.units())
    }
}

/// Whether a Bank of the current files is there.
pub(crate) fn exists<C: Card>(card: &mut C) -> Outcome<bool, C> {
    let found = card
        .open(FileName::Bank.units())
        .map_err(io(Step::ReadOld))?;
    Ok(found.is_some())
}

/// Removes what of the temporary files is there.
pub(crate) fn remove_temporary<C: Card>(card: &mut C) -> Outcome<(), C> {
    for file in ORDER {
        card.remove(temporary(file)).map_err(io(Step::WriteNew))?;
    }
    Ok(())
}

/// Writes `body` as a new Bank under the temporary names, anew, and reads it
/// back: it must load as the same body, whose checksum it answers. `body` is
/// used as the working room of the writing.
pub(crate) fn write_temporary<C: Card>(card: &mut C, body: &mut [u8]) -> Outcome<u32, C> {
    let write = Step::WriteTemporary;
    let written = crc32(body);
    for file in ORDER {
        card.remove(temporary(file)).map_err(io(write))?;
    }
    let mut bank = BankFiles::new(Temporary(card));
    if bank.open().map_err(unwritten(write))?.is_some() {
        return Err(Stopped::Failed(write, Cause::Damaged));
    }
    bank.initialize(body).map_err(unwritten(write))?;
    drop(bank);
    if load(Temporary(card), body, write)? != written {
        return Err(Stopped::Failed(write, Cause::Mismatch));
    }
    Ok(written)
}

/// The checksum of the body the temporary files hold. They must load as
/// one Bank, whole and at rest.
pub(crate) fn temporary_bank<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<u32, C> {
    let read = Step::WriteTemporary;
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(read, Cause::Damaged));
    };
    load(Temporary(card), body, read)
}

/// Writes the final files from the temporary ones and compares them, the
/// journal records last; they must then load as the body with checksum
/// `converted`. `buffer` is working room of at least one body.
pub(crate) fn put_in_place<C: Card>(
    card: &mut C,
    buffer: &mut [u8],
    converted: u32,
) -> Outcome<(), C> {
    let read = Step::WriteTemporary;
    let write = Step::WriteNew;
    for file in ORDER {
        card.remove(file.units()).map_err(io(write))?;
        let Some(mut from) = card.open(temporary(file)).map_err(io(read))? else {
            return Err(Stopped::Failed(read, Cause::Missing));
        };
        let mut to = card.create(file.units()).map_err(io(write))?;
        let units = file.units();
        if file == FileName::Bank {
            // The snapshots first, the journal records last: until a record
            // is there, the new files hold no Bank.
            copy(&mut from, &mut to, units, 2..4, buffer, read, write)?;
            to.sync().map_err(io(write))?;
            copy(&mut from, &mut to, units, 0..2, buffer, read, write)?;
        } else {
            copy(
                &mut from,
                &mut to,
                units,
                0..units.len(),
                buffer,
                read,
                write,
            )?;
        }
        to.sync().map_err(io(write))?;
        same(&mut from, &mut to, file.size(), buffer, read, write)?;
    }
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(write, Cause::Damaged));
    };
    if load(Current(card), body, write)? != converted {
        return Err(Stopped::Failed(write, Cause::Mismatch));
    }
    Ok(())
}

/// Loads the Bank in `files` into `body` and answers the checksum of the
/// body. It has to be whole and at rest; nothing is written.
fn load<F: Files>(
    files: F,
    body: &mut [u8],
    step: Step,
) -> Result<u32, Stopped<<F::Storage as Storage>::Error>> {
    let mut bank = BankFiles::new(files);
    match bank.open().map_err(unwritten(step))? {
        Some(Phase::Clean(_)) => {}
        Some(Phase::Prepared(_)) => return Err(Stopped::Failed(step, Cause::Mismatch)),
        None => return Err(Stopped::Failed(step, Cause::Missing)),
    }
    if bank.read(body).map_err(unwritten(step))?.transport_missing {
        return Err(Stopped::Failed(step, Cause::Mismatch));
    }
    Ok(crc32(body))
}

/// Why the current code could not write or read the Bank.
fn unwritten<E>(step: Step) -> impl Fn(bank_files::Error<E>) -> Stopped<E> {
    move |error| {
        Stopped::Failed(
            step,
            match error {
                bank_files::Error::Io(error)
                | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(error))) => {
                    Cause::Io(error)
                }
                _ => Cause::Damaged,
            },
        )
    }
}

/// Copies the units `range` of a container, never across a unit boundary.
fn copy<S: Storage>(
    from: &mut S,
    to: &mut S,
    units: &[UnitFile],
    range: core::ops::Range<usize>,
    buffer: &mut [u8],
    read: Step,
    write: Step,
) -> Result<(), Stopped<S::Error>> {
    let mut start = 0;
    for (index, unit) in units.iter().enumerate() {
        if range.contains(&index) {
            let mut done = 0;
            while done < unit.len {
                let count = (unit.len - done).min(buffer.len() as u64) as usize;
                let chunk = &mut buffer[..count];
                from.read(start + done, chunk).map_err(io(read))?;
                to.write(start + done, chunk).map_err(io(write))?;
                done += count as u64;
            }
        }
        start += unit.len;
    }
    Ok(())
}

/// Reads both containers again and compares them.
fn same<S: Storage>(
    from: &mut S,
    to: &mut S,
    length: u64,
    buffer: &mut [u8],
    read: Step,
    write: Step,
) -> Result<(), Stopped<S::Error>> {
    let (first, second) = buffer.split_at_mut(buffer.len() / 2);
    let mut done = 0;
    while done < length {
        let count = (length - done).min(first.len() as u64) as usize;
        from.read(done, &mut first[..count]).map_err(io(read))?;
        to.read(done, &mut second[..count]).map_err(io(write))?;
        if first[..count] != second[..count] {
            return Err(Stopped::Failed(write, Cause::Mismatch));
        }
        done += count as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank_common::testing::body;
    use offline_core::rewards::Date;

    #[test]
    fn the_miles_go_into_their_places_and_only_there() {
        let original = body(0x31, 0x44, 2);
        let from = Moment::new(Date::new(2026, 10, 6).unwrap(), 17, 45).unwrap();
        let miles = Miles {
            balance: 1234,
            from: from.to_native(),
            remainder: 719,
        };
        let mut converted = original.clone();
        into_body(&mut converted, miles);
        assert_eq!(Miles::in_body(&converted), miles);
        let view = native_blob::NativeBlobView::parse_layout(&converted).unwrap();
        assert_eq!((view.miles(), view.reward_date()), (1234, Some(from)));
        assert_eq!(view.rewards().fraction, 719);
        // Beside them only the unused places change, to what a new Bank has.
        let mut cleared = original.clone();
        native_blob::clear_unused(&mut cleared);
        let places = REWARD_DATE_OFFSET..MILES_OFFSET + 4;
        for (at, (was, is)) in cleared.iter().zip(&converted).enumerate() {
            assert!(places.contains(&at) || was == is, "{at:#x}");
        }
        // What the body holds comes back as it was, date or not.
        let mut again = original.clone();
        into_body(&mut again, Miles::in_body(&original));
        assert_eq!(Miles::in_body(&again), Miles::in_body(&original));
        assert_eq!(
            &again[REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + 6],
            &original[REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + 6]
        );
    }
}
