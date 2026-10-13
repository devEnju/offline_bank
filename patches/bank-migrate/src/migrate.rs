//! One-time conversion of the Bank the previous release stored.
//!
//! The previous release keeps the Bank in ten files: the journal and the
//! boxes, the Pokédex, the transport box and a rewards record, each twice.
//! Its files are read here with its own code (`previous`), assembled into
//! the one native body they are cut from, and written as a Bank of the
//! current files through `BankFiles`, the way the offline patch creates one.
//! What that release kept in its rewards record goes into the body on the
//! way. Most of the new files have the names of old ones, which is why the
//! new files are first written under temporary names.
//!
//! Only this patch knows the earlier files, and it does nothing else: it
//! never opens the Bank for use.
//!
//! The steps, each safe to interrupt:
//!
//! 1. *Stage.* The old Bank is loaded the way the previous release loads
//!    it. It must be at rest: a save in progress, or a Pokédex or transport
//!    box that release would repair at its next save, are that release's to
//!    settle. Its body is written as a new Bank under temporary names, which
//!    is then loaded and must be the same body. Only then a marker file is
//!    created. Until the marker exists the old files are untouched, and a
//!    start begins again.
//! 2. *Finish.* With the marker, the temporary files are the Bank. The old
//!    files are removed, the final files are written from the temporary ones
//!    (the journal records last) and compared, the marker is removed, and
//!    then the temporary files.

use crate::previous::{self, bank_files as old};
use bank_common::{
    bank_files::{self, BankFiles, FileName, Files, UnitFile, MAIN, MOVER, RECORD, SNAPSHOT},
    session,
};
use offline_core::{
    crc32,
    native_blob::{BLOB_SIZE, FRACTION_OFFSET, REDEEMED_OFFSET},
    Phase, Storage, StoreError,
};

/// First number of the result screen when nothing went wrong; the second
/// number is the `Done`.
pub const DONE: u32 = 0x600d;
/// First number when the old Bank is not at rest: the previous release has
/// to settle it first.
pub const REFUSED: u32 = 0xbad;
/// First number of a failed step, plus the `Step`.
pub const FAILED: u32 = 0xba0;

/// The files of the card, by path.
pub trait Card {
    type Error;
    type Storage: Storage<Error = Self::Error>;
    /// Whether every one of `files` is there with exactly its size.
    fn exists(&mut self, files: &'static [UnitFile]) -> Result<bool, Self::Error>;
    /// `None` when none of `files` is there.
    fn open(&mut self, files: &'static [UnitFile]) -> Result<Option<Self::Storage>, Self::Error>;
    /// Creates every one of `files` anew.
    fn create(&mut self, files: &'static [UnitFile]) -> Result<Self::Storage, Self::Error>;
    /// Removes those of `files` that are there.
    fn remove(&mut self, files: &'static [UnitFile]) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Done {
    /// Neither an old Bank nor a new one.
    NoBank = 0,
    Converted = 1,
    /// A Bank in the new files, and nothing to convert.
    AlreadyNew = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Step {
    ReadOld = 1,
    WriteTemporary = 2,
    RemoveOld = 3,
    WriteNew = 4,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cause<E> {
    Io(E),
    /// The files do not load as a Bank.
    Damaged,
    /// A copy does not read back as what was copied, or the copies do not
    /// load as the Bank the old files held.
    Mismatch,
    /// A file that has to be there is not.
    Missing,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stopped<E> {
    /// The old Bank has a save in progress. Nothing was changed.
    InProgress,
    /// The old Bank has no Pokédex or no transport box for its current
    /// save, which the previous release repairs at its next Save and Quit.
    /// Nothing was changed.
    Unsettled,
    Failed(Step, Cause<E>),
}

/// The two numbers of the result screen. `diagnostic` gives the console's
/// code for a file error.
pub fn numbers<E>(result: Result<Done, Stopped<E>>, diagnostic: impl Fn(E) -> u32) -> [u32; 2] {
    match result {
        Ok(done) => [DONE, done as u32],
        Err(Stopped::InProgress) => [REFUSED, 7],
        Err(Stopped::Unsettled) => [REFUSED, 8],
        Err(Stopped::Failed(step, cause)) => [
            FAILED + step as u32,
            match cause {
                Cause::Io(error) => diagnostic(error),
                Cause::Damaged => 5,
                Cause::Mismatch => 1,
                Cause::Missing => 2,
            },
        ],
    }
}

const fn single(path: &'static str, len: u64) -> [UnitFile; 1] {
    [UnitFile::new(path, len)]
}
/// The current containers in the order they are put in place: the Bank
/// last, because its journal is what makes a Bank exist.
const ORDER: [FileName; 3] = [FileName::Main, FileName::Transport, FileName::Bank];
/// The previous release's containers.
const PREVIOUS: [old::FileName; 4] = [
    old::FileName::Dex,
    old::FileName::Rewards,
    old::FileName::Transport,
    old::FileName::Bank,
];
/// Files that only the previous release has. Where one is, its Bank is.
fn only_previous() -> &'static [UnitFile] {
    old::FileName::Rewards.units()
}
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
static MARKER: [UnitFile; 1] = single("/migrate.ok", 16);

fn temporary(file: FileName) -> &'static [UnitFile] {
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

/// Converts what is there, or goes on where an interrupted run stopped.
/// `buffer` is working room of at least one native Bank body.
pub fn run<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<Done, C> {
    if !card.exists(&MARKER).map_err(io(Step::ReadOld))? {
        let previous = card.open(only_previous()).map_err(io(Step::ReadOld))?;
        if previous.is_none() {
            // What a finished run may have left when it was cut at its end.
            for file in ORDER {
                card.remove(temporary(file)).map_err(io(Step::WriteNew))?;
            }
            let new = card
                .open(FileName::Bank.units())
                .map_err(io(Step::ReadOld))?;
            return Ok(if new.is_some() {
                Done::AlreadyNew
            } else {
                Done::NoBank
            });
        }
        drop(previous);
        stage(card, buffer)?;
    }
    finish(card, buffer)?;
    Ok(Done::Converted)
}

/// A card whose files are only read: every write is refused.
struct ReadOnly<S>(S);
enum Guarded<E> {
    Io(E),
    ReadOnly,
}
impl<S: Storage> Storage for ReadOnly<S> {
    type Error = Guarded<S::Error>;
    fn read(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read(offset, out).map_err(Guarded::Io)
    }
    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
    fn sync(&mut self) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
}
/// The previous release's files, only read.
struct Previous<'a, C: Card>(&'a mut C);
impl<C: Card> old::Files for Previous<'_, C> {
    type Storage = ReadOnly<C::Storage>;
    fn open(&mut self, file: old::FileName) -> Result<Option<Self::Storage>, Guarded<C::Error>> {
        let found = self.0.open(file.units()).map_err(Guarded::Io)?;
        Ok(found.map(ReadOnly))
    }
    fn create(&mut self, _: old::FileName) -> Result<Self::Storage, Guarded<C::Error>> {
        Err(Guarded::ReadOnly)
    }
}
/// The current containers under their temporary names.
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

/// Why the previous release's code could not read its Bank.
fn unread<E>(step: Step) -> impl Fn(old::Error<Guarded<E>>) -> Stopped<E> {
    move |error| {
        Stopped::Failed(
            step,
            match error {
                old::Error::Io(Guarded::Io(error))
                | old::Error::Bank(previous::session::Error::Storage(previous::StoreError::Io(
                    Guarded::Io(error),
                ))) => Cause::Io(error),
                _ => Cause::Damaged,
            },
        )
    }
}
/// Why the current code could not write or read the new Bank.
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

/// Reads the old Bank into one native body, writes that body as a new Bank
/// under temporary names, and marks it complete. The old files are only read.
fn stage<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<(), C> {
    let read = Step::ReadOld;
    let write = Step::WriteTemporary;
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(read, Cause::Damaged));
    };
    // The Bank the old files hold, read with the code that wrote them.
    let mut bank = old::BankFiles::new(Previous(card));
    match bank.open().map_err(unread(read))? {
        Some(previous::Phase::Clean(_)) => {}
        Some(previous::Phase::Prepared(_)) => return Err(Stopped::InProgress),
        None => return Err(Stopped::Failed(read, Cause::Damaged)),
    }
    let loaded = bank.read(body).map_err(unread(read))?;
    drop(bank);
    // The previous release fills these from the running Bank and writes them
    // at its next save. Only it can; a waiting delivery it has taken.
    if loaded.dex_missing || loaded.transport_missing {
        return Err(Stopped::Unsettled);
    }
    // What it kept beside the body now lives in the body.
    let (redeemed, fraction) = loaded.rewards.to_fields();
    body[REDEEMED_OFFSET..REDEEMED_OFFSET + 4].copy_from_slice(&redeemed);
    body[FRACTION_OFFSET..FRACTION_OFFSET + 2].copy_from_slice(&fraction);
    let converted = crc32(body);

    for file in ORDER {
        card.remove(temporary(file)).map_err(io(write))?;
    }
    let mut bank = BankFiles::new(Temporary(card));
    if bank.open().map_err(unwritten(write))?.is_some() {
        return Err(Stopped::Failed(write, Cause::Damaged));
    }
    bank.initialize(body).map_err(unwritten(write))?;
    drop(bank);
    // The new files must load as the Bank the old files were.
    let mut bank = BankFiles::new(Temporary(card));
    let Some(Phase::Clean(_)) = bank.open().map_err(unwritten(write))? else {
        return Err(Stopped::Failed(write, Cause::Mismatch));
    };
    let loaded = bank.read(body).map_err(unwritten(write))?;
    drop(bank);
    if loaded.transport_missing || crc32(body) != converted {
        return Err(Stopped::Failed(write, Cause::Mismatch));
    }
    card.remove(&MARKER).map_err(io(write))?;
    card.create(&MARKER).map_err(io(write))?;
    Ok(())
}

/// Puts the temporary files in place of the old ones.
fn finish<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<(), C> {
    let read = Step::WriteTemporary;
    let write = Step::WriteNew;
    for file in PREVIOUS {
        card.remove(file.units()).map_err(io(Step::RemoveOld))?;
    }
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
    // The marker first: without it the temporary files mean nothing.
    card.remove(&MARKER).map_err(io(write))?;
    for file in ORDER {
        card.remove(temporary(file)).map_err(io(write))?;
    }
    Ok(())
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
    extern crate std;
    use super::*;
    use bank_common::testing::body;
    use offline_core::{
        rewards::{Date, Record},
        sections::{TRANSPORT_RECORDS, TRANSPORT_TAGS},
    };
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        rc::Rc,
        vec,
        vec::Vec,
    };

    /// The card, by path. A file that is in `unreadable` was left by an
    /// interrupted write: it is there, but nothing in it reads or writes.
    #[derive(Default)]
    struct State {
        files: BTreeMap<&'static str, Vec<u8>>,
        unreadable: BTreeSet<&'static str>,
        /// Parts of a file that an interrupted write of the earlier version
        /// left unreadable: a read that touches one fails.
        torn: Vec<(&'static str, core::ops::Range<usize>)>,
        unsynced: BTreeSet<&'static str>,
        /// The console's behaviour on a cut; otherwise half a write lands.
        console: bool,
        budget: Option<usize>,
        operations: usize,
        /// Containers open right now.
        open: usize,
    }
    #[derive(Clone, Default)]
    struct Sd(Rc<RefCell<State>>);
    struct Open(Sd, &'static [UnitFile]);
    impl Open {
        /// The console gives out only so many handles; a container takes
        /// one archive handle and one open file. Two at once is what the
        /// Bank of one file per container used.
        fn held(card: &Sd, files: &'static [UnitFile]) -> Self {
            let mut state = card.0.borrow_mut();
            state.open += 1;
            assert!(state.open <= 2, "{} containers open at once", state.open);
            drop(state);
            Self(card.clone(), files)
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            (self.0).0.borrow_mut().open -= 1;
        }
    }
    impl Sd {
        fn tick(&self) -> Result<(), ()> {
            let mut state = self.0.borrow_mut();
            if let Some(budget) = &mut state.budget {
                if *budget == 0 {
                    if state.console {
                        let lost: Vec<_> = state.unsynced.iter().copied().collect();
                        state.unreadable.extend(lost);
                    }
                    return Err(());
                }
                *budget -= 1;
            }
            state.operations += 1;
            Ok(())
        }
        /// The card after a restart, with `budget` operations before a cut.
        fn restarted(&self, budget: Option<usize>) -> Self {
            let state = self.0.borrow();
            Self(Rc::new(RefCell::new(State {
                files: state.files.clone(),
                unreadable: state.unreadable.clone(),
                torn: state.torn.clone(),
                unsynced: BTreeSet::new(),
                console: state.console,
                budget,
                operations: 0,
                open: 0,
            })))
        }
        fn files(&self) -> BTreeMap<&'static str, Vec<u8>> {
            self.0.borrow().files.clone()
        }
    }
    impl Card for Sd {
        type Error = ();
        type Storage = Open;
        fn exists(&mut self, files: &'static [UnitFile]) -> Result<bool, ()> {
            let state = self.0.borrow();
            Ok(files.iter().all(|file| {
                state
                    .files
                    .get(file.path)
                    .is_some_and(|bytes| bytes.len() as u64 == file.len)
            }))
        }
        fn open(&mut self, files: &'static [UnitFile]) -> Result<Option<Open>, ()> {
            let state = self.0.borrow();
            let mut found = false;
            for file in files {
                if let Some(bytes) = state.files.get(file.path) {
                    if bytes.len() as u64 != file.len {
                        return Err(());
                    }
                    found = true;
                }
            }
            drop(state);
            Ok(found.then(|| Open::held(self, files)))
        }
        fn create(&mut self, files: &'static [UnitFile]) -> Result<Open, ()> {
            for file in files {
                if self.0.borrow().files.contains_key(file.path) {
                    return Err(());
                }
                // The file is there before it is filled and flushed.
                self.0
                    .borrow_mut()
                    .files
                    .insert(file.path, vec![0; file.len as usize]);
                self.0.borrow_mut().unsynced.insert(file.path);
                self.tick()?;
                self.0.borrow_mut().unsynced.remove(file.path);
            }
            Ok(Open::held(self, files))
        }
        fn remove(&mut self, files: &'static [UnitFile]) -> Result<(), ()> {
            for file in files {
                if self.0.borrow().files.contains_key(file.path) {
                    self.tick()?;
                    let mut state = self.0.borrow_mut();
                    state.files.remove(file.path);
                    state.unreadable.remove(file.path);
                    // A file created later under this name is another file.
                    state.torn.retain(|(path, _)| *path != file.path);
                }
            }
            Ok(())
        }
    }
    impl Open {
        /// The files `at..at + length` lies in, each with its start.
        fn touched(&self, at: u64, length: u64) -> Vec<(&'static UnitFile, u64)> {
            let mut start = 0;
            let mut out = Vec::new();
            for file in self.1 {
                if at < start + file.len && start < at + length {
                    out.push((file, start));
                }
                start += file.len;
            }
            out
        }
    }
    impl Storage for Open {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            let state = (self.0).0.borrow();
            for (file, start) in self.touched(at, out.len() as u64) {
                if state.unreadable.contains(file.path) {
                    return Err(());
                }
                let from = at.max(start);
                let to = (at + out.len() as u64).min(start + file.len);
                let (low, high) = ((from - start) as usize, (to - start) as usize);
                if state.torn.iter().any(|(path, range)| {
                    *path == file.path && range.start < high && low < range.end
                }) {
                    return Err(());
                }
                let part = &mut out[(from - at) as usize..(to - at) as usize];
                match state.files.get(file.path) {
                    Some(bytes) => {
                        part.copy_from_slice(&bytes[(from - start) as usize..(to - start) as usize])
                    }
                    None => part.fill(0),
                }
            }
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            let touched = self.touched(at, bytes.len() as u64);
            let [(file, start)] = touched[..] else {
                panic!("a write must lie in one file");
            };
            if (self.0).0.borrow().unreadable.contains(file.path) {
                return Err(());
            }
            if !(self.0).0.borrow().files.contains_key(file.path) {
                (self.0)
                    .0
                    .borrow_mut()
                    .files
                    .insert(file.path, vec![0; file.len as usize]);
                (self.0).0.borrow_mut().unsynced.insert(file.path);
                self.0.tick()?;
            }
            (self.0).0.borrow_mut().unsynced.insert(file.path);
            let cut = self.0.tick().is_err();
            let length = if cut { bytes.len() / 2 } else { bytes.len() };
            let mut state = (self.0).0.borrow_mut();
            let stored = state.files.get_mut(file.path).unwrap();
            let within = (at - start) as usize;
            stored[within..within + length].copy_from_slice(&bytes[..length]);
            if cut {
                Err(())
            } else {
                Ok(())
            }
        }
        fn sync(&mut self) -> Result<(), ()> {
            self.0.tick()?;
            let mut state = (self.0).0.borrow_mut();
            for file in self.1 {
                state.unsynced.remove(file.path);
            }
            Ok(())
        }
        /// Not used by the conversion, which replaces files by name; a
        /// save of the converted Bank may.
        fn recreate(&mut self, at: u64, length: u64) -> Result<(), ()> {
            let touched = self.touched(at, length);
            let [(file, start)] = touched[..] else {
                panic!("only a whole file is replaced");
            };
            assert_eq!((start, file.len), (at, length));
            self.0.tick()?;
            let mut state = (self.0).0.borrow_mut();
            state.files.remove(file.path);
            state.unreadable.remove(file.path);
            state.unsynced.remove(file.path);
            Ok(())
        }
    }
    /// The converted card as the offline patch uses it.
    struct Final(Sd);
    impl Files for Final {
        type Storage = Open;
        fn open(&mut self, file: FileName) -> Result<Option<Open>, ()> {
            self.0.open(file.units())
        }
        fn create(&mut self, file: FileName) -> Result<Open, ()> {
            self.0.create(file.units())
        }
    }
    /// The card as the previous release uses it.
    struct Before(Sd);
    impl old::Files for Before {
        type Storage = Open;
        fn open(&mut self, file: old::FileName) -> Result<Option<Open>, ()> {
            self.0.open(file.units())
        }
        fn create(&mut self, file: old::FileName) -> Result<Open, ()> {
            self.0.create(file.units())
        }
    }

    const GAME: previous::GameIdentity = previous::GameIdentity {
        title_id: 0x0004_0000_0005_5e00,
        save_identity: [7; 32],
    };
    const RECORD_KEPT: Record = Record {
        fraction: 321,
        redeemed: None,
    };
    fn kept() -> Record {
        Record {
            redeemed: Some(Date::new(2026, 10, 6).unwrap()),
            ..RECORD_KEPT
        }
    }
    /// `body` with the record where the current release keeps it.
    fn with_record(mut body: Vec<u8>, record: Record) -> Vec<u8> {
        let (redeemed, fraction) = record.to_fields();
        body[REDEEMED_OFFSET..REDEEMED_OFFSET + 4].copy_from_slice(&redeemed);
        body[FRACTION_OFFSET..FRACTION_OFFSET + 2].copy_from_slice(&fraction);
        body
    }
    /// A Bank of the previous release on `card`, created and never saved.
    fn created(card: &Sd, first: &[u8]) {
        let mut bank = old::BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(None)));
        bank.initialize(&mut first.to_vec()).unwrap();
    }
    /// One Save and Quit of the previous release, up to the game's write.
    fn prepared(card: &Sd, saved: &[u8], record: Record) -> old::BankFiles<Before> {
        let mut bank = old::BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(Some(previous::Phase::Clean(_)))));
        bank.read(&mut vec![0; BLOB_SIZE]).unwrap();
        bank.prepare(&mut saved.to_vec(), record, GAME, [1; 32], [2; 32])
            .unwrap();
        bank
    }
    /// A card with a Bank the previous release created and saved once, and
    /// the body the current release has to load from it.
    fn used(console: bool) -> (Sd, Vec<u8>) {
        let card = Sd::default();
        card.0.borrow_mut().console = console;
        created(&card, &body(0x31, 0x44, 0));
        let saved = body(0x52, 0x66, 3);
        let mut bank = prepared(&card, &saved, kept());
        bank.reconcile(previous::GameObservation::Present {
            game: GAME,
            fingerprint: [2; 32],
        })
        .unwrap();
        bank.tidy_transport().unwrap();
        drop(bank);
        (card.restarted(None), with_record(saved, kept()))
    }
    /// The Bank the current release finds on `card`.
    fn current(card: &Sd) -> Option<Vec<u8>> {
        let mut bank = BankFiles::new(Final(card.clone()));
        let Some(Phase::Clean(_)) = bank.open().unwrap() else {
            return None;
        };
        let mut staging = vec![0; BLOB_SIZE];
        let loaded = bank.read(&mut staging).unwrap();
        assert!(!loaded.transport_missing);
        Some(staging)
    }
    fn convert(card: &Sd) -> Result<Done, Stopped<()>> {
        run(&mut card.clone(), &mut vec![0; BLOB_SIZE])
    }
    /// Nothing but the current release's files is on `card`.
    fn only_the_new_files(card: &Sd) -> bool {
        let files = card.files();
        let expected: Vec<_> = ORDER
            .iter()
            .flat_map(|file| file.units())
            .map(|unit| (unit.path, unit.len as usize))
            .collect();
        files.len() == expected.len()
            && expected
                .iter()
                .all(|(path, len)| files.get(path).is_some_and(|bytes| bytes.len() == *len))
    }

    #[test]
    fn a_bank_of_the_previous_release_becomes_the_same_bank_with_its_miles_in_the_body() {
        for console in [false, true] {
            let (card, body) = used(console);
            // Ten files: every one of its containers twice.
            assert_eq!(card.files().len(), 10);
            assert_eq!(convert(&card), Ok(Done::Converted));
            assert!(only_the_new_files(&card));
            assert!(card.0.borrow().unreadable.is_empty());
            assert!(current(&card).unwrap() == body);
            // A second run finds nothing to do and changes nothing.
            let files = card.files();
            assert_eq!(convert(&card), Ok(Done::AlreadyNew));
            assert!(card.files() == files);
        }
    }

    #[test]
    fn a_bank_that_was_never_saved_is_converted_too() {
        let card = Sd::default();
        let first = body(0x31, 0x44, 2);
        created(&card, &first);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_new_files(&card));
        // Its rewards record is "none": no remainder, never redeemed.
        assert!(current(&card).unwrap() == with_record(first, Record::NONE));
    }

    #[test]
    fn a_waiting_delivery_is_taken_into_the_transport_box() {
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        let delivered = body(0, 0, 4);
        let mut file = previous::sidecar::Sidecar::new(
            card.clone()
                .open(old::FileName::Transport.units())
                .unwrap()
                .unwrap(),
            previous::transport::KIND,
        );
        previous::transport::deliver(
            &mut file,
            &delivered[TRANSPORT_RECORDS],
            &delivered[TRANSPORT_TAGS],
        )
        .unwrap();
        drop(file);
        assert_eq!(convert(&card), Ok(Done::Converted));
        let mut expected = with_record(body(0x31, 0x44, 0), Record::NONE);
        expected[TRANSPORT_RECORDS].copy_from_slice(&delivered[TRANSPORT_RECORDS]);
        expected[TRANSPORT_TAGS].copy_from_slice(&delivered[TRANSPORT_TAGS]);
        assert!(current(&card).unwrap() == expected);
    }

    #[test]
    fn a_bank_that_is_not_at_rest_is_left_to_the_previous_release() {
        // A save in progress.
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        drop(prepared(&card, &body(0x52, 0x66, 1), kept()));
        let files = card.files();
        assert_eq!(convert(&card), Err(Stopped::InProgress));
        assert!(card.files() == files);
        // No Pokédex for the current save: that release writes one at its
        // next Save and Quit.
        let (card, _) = used(false);
        for unit in old::FileName::Dex.units() {
            card.0.borrow_mut().files.remove(unit.path);
        }
        let files = card.files();
        assert_eq!(convert(&card), Err(Stopped::Unsettled));
        assert!(card.files() == files);
        // The same without a transport box.
        let (card, _) = used(false);
        for unit in old::FileName::Transport.units() {
            card.0.borrow_mut().files.remove(unit.path);
        }
        let files = card.files();
        assert_eq!(convert(&card), Err(Stopped::Unsettled));
        assert!(card.files() == files);
    }

    #[test]
    fn a_damaged_bank_is_left_as_it_is() {
        let (card, _) = used(false);
        // Both journal records gone: nothing says which boxes are current.
        for unit in &old::FileName::Bank.units()[..2] {
            card.0
                .borrow_mut()
                .files
                .get_mut(unit.path)
                .unwrap()
                .fill(0xa5);
        }
        let files = card.files();
        assert_eq!(
            convert(&card),
            Err(Stopped::Failed(Step::ReadOld, Cause::Damaged))
        );
        assert!(card.files() == files);
    }

    #[test]
    fn a_cut_at_every_step_of_the_conversion_is_finished_by_the_next_start() {
        for console in [false, true] {
            let (card, body) = used(console);
            let whole = card.restarted(Some(usize::MAX));
            assert_eq!(convert(&whole), Ok(Done::Converted));
            let total = whole.0.borrow().operations;
            assert!(total > 40);
            let reference = whole.files();
            let mut resumed = 0;
            for cut in 0..total {
                let cut_card = card.restarted(Some(cut));
                assert!(convert(&cut_card).is_err(), "cut {cut}");
                let after = cut_card.restarted(None);
                if after.0.borrow().files.contains_key(MARKER[0].path) {
                    resumed += 1;
                } else if after.0.borrow().files.contains_key(only_previous()[0].path) {
                    // Until the marker exists the old Bank is whole.
                    let mut bank = old::BankFiles::new(Before(after.clone()));
                    assert!(matches!(bank.open(), Ok(Some(previous::Phase::Clean(_)))));
                    let mut staging = vec![0; BLOB_SIZE];
                    let loaded = bank.read(&mut staging).unwrap();
                    assert_eq!(loaded.rewards, kept(), "cut {cut}");
                    assert!(with_record(staging, kept()) == body, "cut {cut}");
                }
                // The next start finishes it, whatever the cut left.
                let done = convert(&after);
                assert!(
                    matches!(done, Ok(Done::Converted | Done::AlreadyNew)),
                    "cut {cut}: {done:?}"
                );
                assert!(after.files() == reference, "cut {cut}: files differ");
                assert!(after.0.borrow().unreadable.is_empty(), "cut {cut}");
                assert!(current(&after).unwrap() == body, "cut {cut}");
            }
            // Some cuts fell after the marker: those went on, not again.
            assert!(resumed > 10);
        }
    }

    #[test]
    fn every_result_has_its_two_numbers() {
        let code = |_: ()| 0xd900_458b;
        assert_eq!(numbers(Ok(Done::NoBank), code), [0x600d, 0]);
        assert_eq!(numbers(Ok(Done::Converted), code), [0x600d, 1]);
        assert_eq!(numbers(Ok(Done::AlreadyNew), code), [0x600d, 2]);
        assert_eq!(numbers(Err(Stopped::InProgress), code), [0xbad, 7]);
        assert_eq!(numbers(Err(Stopped::Unsettled), code), [0xbad, 8]);
        for (step, first) in [
            (Step::ReadOld, 0xba1),
            (Step::WriteTemporary, 0xba2),
            (Step::RemoveOld, 0xba3),
            (Step::WriteNew, 0xba4),
        ] {
            let failed = |cause| numbers(Err(Stopped::Failed(step, cause)), code);
            assert_eq!(failed(Cause::Io(())), [first, 0xd900_458b]);
            assert_eq!(failed(Cause::Mismatch), [first, 1]);
            assert_eq!(failed(Cause::Missing), [first, 2]);
            assert_eq!(failed(Cause::Damaged), [first, 5]);
        }
    }

    #[test]
    fn nothing_to_convert_changes_nothing() {
        let empty = Sd::default();
        assert_eq!(convert(&empty), Ok(Done::NoBank));
        assert!(empty.files().is_empty());
        // A Bank of the current release.
        let card = Sd::default();
        let mut bank = BankFiles::new(Final(card.clone()));
        assert!(matches!(bank.open(), Ok(None)));
        bank.initialize(&mut body(0x31, 0x44, 0)).unwrap();
        drop(bank);
        let files = card.files();
        assert_eq!(convert(&card), Ok(Done::AlreadyNew));
        assert!(card.files() == files);
    }
}
