//! One-time conversion of a Bank an earlier release stored.
//!
//! Earlier releases stored the Bank in two other ways (`Source`): in four
//! files up to v0.2.1, and in ten from v0.3.0 to v0.4.0. Whichever is there
//! is read with the code of the release that wrote it (`v0_2_1`, `v0_4_0`),
//! assembled into the one native body, and written as a Bank of the current
//! files through `BankFiles`, the way the offline patch creates one. What
//! those releases kept in their rewards record goes into the body on the
//! way. Some of the new files have the names of old ones, which is why the
//! new files are first written under temporary names.
//!
//! Only this patch knows the earlier files, and it does nothing else: it
//! never opens the Bank for use.
//!
//! The steps, each safe to interrupt:
//!
//! 1. *Stage.* The old Bank is loaded the way its release loads it. It must
//!    be at rest: a save in progress, or a Pokédex or transport box that
//!    release would repair at its next save, are that release's to settle.
//!    Its body is written as a new Bank under temporary names, which is then
//!    loaded and must be the same body. Only then a marker file is created,
//!    which names the release. Until the marker exists the old files are
//!    untouched, and a start begins again.
//! 2. *Finish.* With the marker, the temporary files are the Bank, whatever
//!    they were made from, and at every start they must load as one before
//!    anything is removed. The files of both earlier ways are removed, the
//!    final files are written from the temporary ones (the journal records
//!    last) and compared, and must load as the same Bank; then the marker is
//!    removed, and then the temporary files.

use crate::{source::Source, v0_4_0::bank_files as old};
use bank_common::{
    bank_files::{self, BankFiles, FileName, Files, UnitFile, MAIN, MOVER, RECORD, SNAPSHOT},
    numbers::Label,
    session,
};
use offline_core::{crc32, native_blob::BLOB_SIZE, Phase, Storage, StoreError};

/// First number of the result screen when nothing went wrong; the second
/// number is the `Done`.
pub const DONE: u32 = 0x600d;
/// First number when the old Bank is not at rest: the release that stored
/// it has to settle it first.
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
    /// The files do not load as a Bank, or hold a Bank of both earlier ways.
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
    /// save, which its release repairs at its next Save and Quit. Nothing
    /// was changed.
    Unsettled,
    Failed(Step, Cause<E>),
}

/// What a run did, and the way of storing it found, when it found one.
#[derive(Debug, PartialEq, Eq)]
pub struct Report<E> {
    pub source: Option<Source>,
    pub result: Result<Done, Stopped<E>>,
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

/// The patch's name: behind "Ver. 1.5" on the start screen, and behind the
/// version on the result screen when no earlier Bank was found.
pub const PACKAGE: &[u8] = b"-migration";

/// The line above the numbers. For a Bank of an earlier way, that way and
/// the release converting it, as "v0.2.1-0.6.0" (`version` without its "v");
/// otherwise the version and the package, as "v0.6.0-migration". `None` for
/// a version that makes no label.
pub fn label(source: Option<Source>, version: &[u8]) -> Option<Label> {
    let Some(source) = source else {
        return Label::new(version, PACKAGE);
    };
    let version = version.strip_prefix(b"v").unwrap_or(version);
    let mut suffix = [0; Label::ROOM];
    let part = suffix.get_mut(1..1 + version.len())?;
    part.copy_from_slice(version);
    suffix[0] = b'-';
    Label::new(source.tag(), &suffix[..1 + version.len()])
}

const fn single(path: &'static str, len: u64) -> [UnitFile; 1] {
    [UnitFile::new(path, len)]
}
/// The current containers in the order they are put in place: the Bank
/// last, because its journal is what makes a Bank exist.
const ORDER: [FileName; 3] = [FileName::Main, FileName::Transport, FileName::Bank];
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
/// Says that the temporary files are a whole Bank, and which way it was
/// stored before: the tag of the `Source` in ASCII, zero after it. All
/// zero, as the release before this one left it, names none.
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
pub fn run<C: Card>(card: &mut C, buffer: &mut [u8]) -> Report<C::Error> {
    let mut source = None;
    let result = convert(card, buffer, &mut source);
    Report { source, result }
}

fn convert<C: Card>(
    card: &mut C,
    buffer: &mut [u8],
    source: &mut Option<Source>,
) -> Outcome<Done, C> {
    if card.exists(&MARKER).map_err(io(Step::ReadOld))? {
        *source = marked(card);
    } else {
        let Some(found) = find(card)? else {
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
        };
        *source = Some(found);
        stage(card, buffer, found)?;
    }
    finish(card, buffer)?;
    Ok(Done::Converted)
}

/// The earlier way the Bank on the card is stored in, if any. Files of both
/// are no Bank either of them wrote, and nothing is done with them.
fn find<C: Card>(card: &mut C) -> Outcome<Option<Source>, C> {
    let mut found = None;
    for source in Source::ALL {
        if source.there(card).map_err(io(Step::ReadOld))? {
            if found.is_some() {
                return Err(Stopped::Failed(Step::ReadOld, Cause::Damaged));
            }
            found = Some(source);
        }
    }
    Ok(found)
}

/// The marker's content for `source`.
fn mark(source: Source) -> [u8; 16] {
    let mut out = [0; 16];
    out[..source.tag().len()].copy_from_slice(source.tag());
    out
}
/// The way the marker names, read for the result screen only.
fn marked<C: Card>(card: &mut C) -> Option<Source> {
    let mut marker = card.open(&MARKER).ok()??;
    let mut bytes = [0; 16];
    marker.read(0, &mut bytes).ok()?;
    Source::ALL
        .into_iter()
        .find(|&source| mark(source) == bytes)
}

/// A card whose files are only read: every write is refused.
pub(crate) struct ReadOnly<S>(S);
pub(crate) enum Guarded<E> {
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
/// The files of an earlier way, only read: its containers as the frozen
/// code knows them, each made of the files `units` gives it.
pub(crate) struct Earlier<'a, C: Card> {
    card: &'a mut C,
    units: fn(old::FileName) -> &'static [UnitFile],
}
impl<'a, C: Card> Earlier<'a, C> {
    pub(crate) fn new(card: &'a mut C, units: fn(old::FileName) -> &'static [UnitFile]) -> Self {
        Self { card, units }
    }
}
impl<C: Card> old::Files for Earlier<'_, C> {
    type Storage = ReadOnly<C::Storage>;
    fn open(&mut self, file: old::FileName) -> Result<Option<Self::Storage>, Guarded<C::Error>> {
        let found = self.card.open((self.units)(file)).map_err(Guarded::Io)?;
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

/// The current containers under their own names.
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
fn stage<C: Card>(card: &mut C, buffer: &mut [u8], source: Source) -> Outcome<(), C> {
    let read = Step::ReadOld;
    let write = Step::WriteTemporary;
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(read, Cause::Damaged));
    };
    // The Bank the old files hold, read with the code that wrote them, as
    // the current release keeps it.
    source.read(card, body)?;
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
    if load(Temporary(card), body, write)? != converted {
        return Err(Stopped::Failed(write, Cause::Mismatch));
    }
    card.remove(&MARKER).map_err(io(write))?;
    let mut marker = card.create(&MARKER).map_err(io(write))?;
    marker.write(0, &mark(source)).map_err(io(write))?;
    marker.sync().map_err(io(write))?;
    Ok(())
}

/// Puts the temporary files in place of the old ones.
fn finish<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<(), C> {
    let read = Step::WriteTemporary;
    let write = Step::WriteNew;
    // The temporary files are all there is of the Bank once the old files
    // go: they must still load as one, at every start that goes on here.
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(read, Cause::Damaged));
    };
    let converted = load(Temporary(card), body, read)?;
    // Whichever way it was stored before: what is not there is not removed.
    for source in Source::ALL {
        for files in source.files() {
            card.remove(files).map_err(io(Step::RemoveOld))?;
        }
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
    // The final files must load as that Bank before what they were made
    // from goes.
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(write, Cause::Damaged));
    };
    if load(Current(card), body, write)? != converted {
        return Err(Stopped::Failed(write, Cause::Mismatch));
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
    use crate::v0_4_0::{self, into_body};
    use bank_common::testing::body;
    use offline_core::{
        native_blob::{self, FRACTION_OFFSET},
        sections::{TRANSPORT_RECORDS, TRANSPORT_TAGS},
    };
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        rc::Rc,
        vec,
        vec::Vec,
    };
    use v0_4_0::rewards::Record;

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
    /// The card as v0.4.0 uses it.
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

    const GAME: v0_4_0::GameIdentity = v0_4_0::GameIdentity {
        title_id: 0x0004_0000_0005_5e00,
        save_identity: [7; 32],
    };
    const RECORD_KEPT: Record = Record {
        fraction: 321,
        redeemed: None,
    };
    fn kept() -> Record {
        Record {
            redeemed: Some((2026, 10, 6)),
            ..RECORD_KEPT
        }
    }
    /// `body` with the record where the current release keeps it.
    fn with_record(mut body: Vec<u8>, record: Record) -> Vec<u8> {
        into_body(&mut body, record);
        body
    }
    /// A Bank of v0.4.0 on `card`, created and never saved.
    fn created(card: &Sd, first: &[u8]) {
        let mut bank = old::BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(None)));
        bank.initialize(&mut first.to_vec()).unwrap();
    }
    /// One Save and Quit of v0.4.0, up to the game's write.
    fn prepared(card: &Sd, saved: &[u8], record: Record) -> old::BankFiles<Before> {
        let mut bank = old::BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(Some(v0_4_0::Phase::Clean(_)))));
        bank.read(&mut vec![0; BLOB_SIZE]).unwrap();
        bank.prepare(&mut saved.to_vec(), record, GAME, [1; 32], [2; 32])
            .unwrap();
        bank
    }
    /// A card with a Bank v0.4.0 created and saved once, and
    /// the body the current release has to load from it.
    fn used(console: bool) -> (Sd, Vec<u8>) {
        let card = Sd::default();
        card.0.borrow_mut().console = console;
        created(&card, &body(0x31, 0x44, 0));
        let saved = body(0x52, 0x66, 3);
        let mut bank = prepared(&card, &saved, kept());
        bank.reconcile(v0_4_0::GameObservation::Present {
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
    /// Puts `bytes` into the rewards slot of the current save on `card`, a
    /// card of v0.4.0's files.
    fn write_record(card: &Sd, bytes: [u8; 16]) {
        let mut file = v0_4_0::sidecar::Sidecar::new(
            card.clone()
                .open(old::FileName::Rewards.units())
                .unwrap()
                .unwrap(),
            v0_4_0::rewards::FILE,
        );
        let slots = file.slots().unwrap();
        // The slot of the current save: the later of the two.
        let generation = |index: &usize| {
            let tag = slots[*index].and_then(|slot| slot.tag);
            tag.map(|tag| tag.generation)
        };
        let index = (0..slots.len()).max_by_key(generation).unwrap();
        let slot = slots[index].unwrap();
        file.write(index, slot.tag, slot.aux, slot.count, &[&bytes])
            .unwrap();
    }
    /// The same Bank as v0.2.1 stores it: each container one file, its
    /// units one after the other. A unit that is not there is a void slot.
    fn as_v0_2_1(card: &Sd) -> Sd {
        let files = card.files();
        let joined = Sd::default();
        let mut state = joined.0.borrow_mut();
        state.console = card.0.borrow().console;
        for (file, path) in [
            (old::FileName::Bank, "/bank.bin"),
            (old::FileName::Dex, "/dex.bin"),
            (old::FileName::Transport, "/transport.bin"),
            (old::FileName::Rewards, "/rewards.bin"),
        ] {
            let units = file.units();
            if units.iter().all(|unit| !files.contains_key(unit.path)) {
                continue;
            }
            let mut bytes = Vec::new();
            for unit in units {
                match files.get(unit.path) {
                    Some(unit) => bytes.extend_from_slice(unit),
                    None => bytes.resize(bytes.len() + unit.len as usize, 0),
                }
            }
            state.files.insert(path, bytes);
        }
        drop(state);
        joined
    }
    /// A record as v0.2.1 lays it out, written here byte by byte: the
    /// balance, state 1, the thirtieths, the count, the day accounted through.
    fn record_of_v0_2_1(balance: u32, fraction: u8, count: u16, day: (u16, u8, u8)) -> [u8; 16] {
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
    /// A card with a Bank v0.2.1 created and saved once, and the body the
    /// current release has to load from it.
    fn used_v0_2_1(console: bool) -> (Sd, Vec<u8>) {
        let (card, _) = used(console);
        let bytes = record_of_v0_2_1(1234, 17, 90, (2026, 10, 6));
        write_record(&card, bytes);
        let mut expected = body(0x52, 0x66, 3);
        let stored = crate::v0_2_1::rewards::Stored::decode(&bytes).unwrap();
        crate::v0_2_1::into_body(&mut expected, stored);
        (as_v0_2_1(&card.restarted(None)), expected)
    }
    fn report(card: &Sd) -> Report<()> {
        run(&mut card.clone(), &mut vec![0; BLOB_SIZE])
    }
    fn convert(card: &Sd) -> Result<Done, Stopped<()>> {
        report(card).result
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
    fn a_bank_of_v0_4_0_becomes_the_same_bank_with_its_miles_in_the_body() {
        for console in [false, true] {
            let (card, body) = used(console);
            // Ten files: every one of its containers twice.
            assert_eq!(card.files().len(), 10);
            assert_eq!(convert(&card), Ok(Done::Converted));
            assert!(only_the_new_files(&card));
            assert!(card.0.borrow().unreadable.is_empty());
            assert!(current(&card).unwrap() == body);
            // The current release reads the remainder of that Bank there; the
            // day of its last redemption is not kept.
            let view = native_blob::NativeBlobView::parse_layout(&body).unwrap();
            let now = offline_core::rewards::Record {
                fraction: 321,
                redeemed: None,
            };
            assert_eq!(view.rewards(), now);
            // The unused places are as in a new Bank, but for the remainder.
            for range in native_blob::UNUSED {
                for at in range {
                    let fraction = FRACTION_OFFSET..FRACTION_OFFSET + 2;
                    assert!(fraction.contains(&at) || body[at] == 0, "{at:#x}");
                }
            }
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
        let mut file = v0_4_0::sidecar::Sidecar::new(
            card.clone()
                .open(old::FileName::Transport.units())
                .unwrap()
                .unwrap(),
            v0_4_0::transport::KIND,
        );
        v0_4_0::transport::deliver(
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
    fn a_bank_of_v0_4_0_that_is_not_at_rest_is_left_to_that_release() {
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

    /// Cuts the conversion of `card` at every operation in turn, and has
    /// the next start go on from there; some of those are cut again.
    fn cut_everywhere(card: &Sd, body: &[u8], source: Source) {
        let whole = card.restarted(Some(usize::MAX));
        assert_eq!(
            report(&whole),
            Report {
                source: Some(source),
                result: Ok(Done::Converted)
            }
        );
        let total = whole.0.borrow().operations;
        assert!(total > 40);
        let reference = whole.files();
        let before = card.files();
        let mut resumed = 0;
        for cut in 0..total {
            let cut_card = card.restarted(Some(cut));
            assert!(convert(&cut_card).is_err(), "cut {cut}");
            let after = cut_card.restarted(None);
            let marker = after.files().get(MARKER[0].path).cloned();
            let expected = match marker {
                Some(bytes) => {
                    resumed += 1;
                    let readable = !after.0.borrow().unreadable.contains(MARKER[0].path);
                    (readable && bytes[..] == mark(source)).then_some(source)
                }
                None if source.there(&mut after.clone()).unwrap() => {
                    // Until the marker exists the old files are untouched.
                    for unit in source.files().into_iter().flatten() {
                        let path = unit.path;
                        assert!(
                            after.files().get(path) == before.get(path),
                            "cut {cut}: {path}"
                        );
                    }
                    Some(source)
                }
                None => None,
            };
            // The next start finishes it, whatever the cut left.
            let done = report(&after);
            assert!(
                matches!(done.result, Ok(Done::Converted | Done::AlreadyNew)),
                "cut {cut}: {done:?}"
            );
            assert_eq!(done.source, expected, "cut {cut}");
            assert!(after.files() == reference, "cut {cut}: files differ");
            assert!(after.0.borrow().unreadable.is_empty(), "cut {cut}");
            assert!(current(&after).unwrap() == body, "cut {cut}");
        }
        // Some cuts fell after the marker: those went on, not again.
        assert!(resumed > 10);
        // A start that goes on is cut too, and the one after it finishes.
        for first in (0..total).step_by(7) {
            for second in (0..total).step_by(11) {
                let cut_card = card.restarted(Some(first));
                assert!(convert(&cut_card).is_err());
                let again = cut_card.restarted(Some(second));
                let _ = convert(&again);
                let last = again.restarted(None);
                let done = convert(&last);
                assert!(
                    matches!(done, Ok(Done::Converted | Done::AlreadyNew)),
                    "cuts {first}, {second}: {done:?}"
                );
                assert!(last.files() == reference, "cuts {first}, {second}");
                assert!(current(&last).unwrap() == body, "cuts {first}, {second}");
            }
        }
    }

    #[test]
    fn a_cut_at_every_step_of_the_conversion_is_finished_by_the_next_start() {
        for console in [false, true] {
            let (card, body) = used(console);
            cut_everywhere(&card, &body, Source::V0_4_0);
            let (card, body) = used_v0_2_1(console);
            cut_everywhere(&card, &body, Source::V0_2_1);
        }
    }

    #[test]
    fn a_balance_that_v0_3_1_kept_in_its_record_comes_along() {
        // v0.3.1 wrote zero into the body and the balance into the first
        // four bytes of its record; the release after it reads both.
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        let mut saved = body(0x52, 0x66, 3);
        saved[0x170..0x174].fill(0);
        let mut bank = prepared(&card, &saved, kept());
        bank.reconcile(v0_4_0::GameObservation::Present {
            game: GAME,
            fingerprint: [2; 32],
        })
        .unwrap();
        bank.tidy_transport().unwrap();
        drop(bank);
        let mut earlier = v0_4_0::rewards::encode(kept());
        earlier[..4].copy_from_slice(&1234u32.to_le_bytes());
        write_record(&card, earlier);
        let card = card.restarted(None);
        assert_eq!(convert(&card), Ok(Done::Converted));
        let mut expected = with_record(saved, kept());
        expected[0x170..0x174].copy_from_slice(&1234u32.to_le_bytes());
        assert!(current(&card).unwrap() == expected);
        // A record of that release itself adds nothing.
        assert_eq!(v0_4_0::rewards::balance_beside(7, &[0; 16]), None);
    }

    #[test]
    fn a_bank_without_rewards_files_is_converted_as_that_release_reads_it() {
        let (card, body) = used(false);
        for unit in old::FileName::Rewards.units() {
            card.0.borrow_mut().files.remove(unit.path);
        }
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_new_files(&card));
        // No remainder, never redeemed.
        assert!(current(&card).unwrap() == with_record(body, Record::NONE));
    }

    #[test]
    fn a_run_that_goes_on_removes_nothing_unless_the_temporary_bank_still_loads() {
        let buffer = &mut vec![0; BLOB_SIZE];
        let (staged, body) = used(false);
        stage(&mut staged.clone(), buffer, Source::V0_4_0).unwrap();
        let damage = |change: &dyn Fn(&mut State)| {
            let card = staged.restarted(None);
            change(&mut card.0.borrow_mut());
            card
        };
        let cases = [
            // One byte of the boxes, of the main file, of a journal record.
            (
                damage(&|state| state.files.get_mut("/m2.tmp").unwrap()[77] ^= 1),
                Cause::Damaged,
            ),
            (
                damage(&|state| state.files.get_mut("/m4.tmp").unwrap()[77] ^= 1),
                Cause::Damaged,
            ),
            (
                damage(&|state| {
                    state.files.get_mut("/m0.tmp").unwrap()[77] ^= 1;
                    state.files.get_mut("/m1.tmp").unwrap()[77] ^= 1;
                }),
                Cause::Damaged,
            ),
            // The temporary Bank gone.
            (
                damage(&|state| {
                    for unit in &TEMPORARY_BANK {
                        state.files.remove(unit.path);
                    }
                }),
                Cause::Missing,
            ),
        ];
        for (card, cause) in cases {
            let files = card.files();
            assert_eq!(
                convert(&card),
                Err(Stopped::Failed(Step::WriteTemporary, cause))
            );
            // The old Bank is still there, whole.
            assert!(card.files() == files);
            let mut bank = old::BankFiles::new(Before(card.clone()));
            assert!(matches!(bank.open(), Ok(Some(v0_4_0::Phase::Clean(_)))));
            let mut staging = vec![0; BLOB_SIZE];
            bank.read(&mut staging).unwrap();
            assert!(with_record(staging, kept()) == body);
        }
        // Undamaged, the same card is finished.
        assert_eq!(convert(&staged), Ok(Done::Converted));
        assert!(current(&staged).unwrap() == body);
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

    #[test]
    fn a_bank_of_v0_2_1_becomes_the_same_bank_with_its_miles_in_the_body() {
        for console in [false, true] {
            let (card, body) = used_v0_2_1(console);
            // Four files: one per container.
            assert_eq!(card.files().len(), 4);
            assert_eq!(
                report(&card),
                Report {
                    source: Some(Source::V0_2_1),
                    result: Ok(Done::Converted)
                }
            );
            assert!(only_the_new_files(&card));
            assert!(card.0.borrow().unreadable.is_empty());
            assert!(current(&card).unwrap() == body);
            // The record's balance and thirtieths are in the body.
            let view = native_blob::NativeBlobView::parse_layout(&body).unwrap();
            assert_eq!(view.miles(), 1234);
            assert_eq!(view.rewards().fraction, 17 * 24);
            // A second run finds nothing to do and changes nothing.
            let files = card.files();
            assert_eq!(
                report(&card),
                Report {
                    source: None,
                    result: Ok(Done::AlreadyNew)
                }
            );
            assert!(card.files() == files);
        }
    }

    #[test]
    fn a_bank_of_v0_2_1_that_was_never_saved_is_converted_too() {
        let card = Sd::default();
        let first = body(0x31, 0x44, 2);
        created(&card, &first);
        let card = as_v0_2_1(&card);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_new_files(&card));
        // No Miles and no record yet: nothing paid until the first save.
        let mut expected = first;
        crate::v0_2_1::into_body(&mut expected, crate::v0_2_1::rewards::Stored::NONE);
        assert!(current(&card).unwrap() == expected);
    }

    #[test]
    fn a_damaged_spare_half_of_the_bank_file_of_v0_2_1_is_not_needed() {
        let (card, body) = used_v0_2_1(false);
        let records = 2 * old::RECORD as usize;
        let snapshot = old::SNAPSHOT as usize;
        let mut converted = 0;
        for start in [records, records + snapshot] {
            let torn = card.restarted(None);
            let middle = start + snapshot / 2;
            torn.0
                .borrow_mut()
                .torn
                .push(("/bank.bin", middle..middle + 512));
            let files = torn.files();
            match convert(&torn) {
                Ok(Done::Converted) => {
                    converted += 1;
                    assert!(only_the_new_files(&torn));
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

    #[test]
    fn a_bank_of_v0_2_1_that_is_not_at_rest_is_left_to_that_release() {
        let refused = |card: &Sd, stopped| {
            let files = card.files();
            assert_eq!(
                report(card),
                Report {
                    source: Some(Source::V0_2_1),
                    result: Err(stopped)
                }
            );
            assert!(card.files() == files);
        };
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        drop(prepared(&card, &body(0x52, 0x66, 1), kept()));
        refused(&as_v0_2_1(&card), Stopped::InProgress);
        for container in [old::FileName::Dex, old::FileName::Transport] {
            let (card, _) = used(false);
            for unit in container.units() {
                card.0.borrow_mut().files.remove(unit.path);
            }
            refused(&as_v0_2_1(&card), Stopped::Unsettled);
        }
    }

    #[test]
    fn files_of_both_earlier_ways_are_left_as_they_are() {
        let (card, _) = used_v0_2_1(false);
        let (other, _) = used(false);
        for unit in &old::FileName::Bank.units()[..2] {
            let bytes = other.files()[unit.path].clone();
            card.0.borrow_mut().files.insert(unit.path, bytes);
        }
        let files = card.files();
        assert_eq!(
            report(&card),
            Report {
                source: None,
                result: Err(Stopped::Failed(Step::ReadOld, Cause::Damaged))
            }
        );
        assert!(card.files() == files);
    }

    #[test]
    fn a_cut_run_of_the_migration_v0_4_0_shipped_is_left_as_it_is() {
        // That migration cut the files of v0.2.1 into those of v0.3.0, under
        // the same marker and temporary names, with the lengths of v0.3.0.
        let (card, _) = used_v0_2_1(false);
        let (split, _) = used(false);
        let units = [
            old::FileName::Bank,
            old::FileName::Dex,
            old::FileName::Transport,
            old::FileName::Rewards,
        ]
        .map(old::FileName::units)
        .concat();
        let names = [
            "/m0.tmp", "/m1.tmp", "/m2.tmp", "/m3.tmp", "/m4.tmp", "/m5.tmp", "/m6.tmp", "/m7.tmp",
            "/m8.tmp", "/m9.tmp",
        ];
        {
            let mut state = card.0.borrow_mut();
            state.files.insert(MARKER[0].path, vec![0; 16]);
            for (name, unit) in names.into_iter().zip(&units) {
                state.files.insert(name, split.files()[unit.path].clone());
            }
            // Its finish had begun.
            state.files.remove("/dex.bin");
        }
        let files = card.files();
        assert_eq!(
            report(&card),
            Report {
                source: None,
                result: Err(Stopped::Failed(Step::WriteTemporary, Cause::Io(())))
            }
        );
        assert!(card.files() == files);
    }

    #[test]
    fn a_cut_run_of_the_migration_v0_5_0_shipped_is_finished() {
        let (card, body) = used(false);
        stage(&mut card.clone(), &mut vec![0; BLOB_SIZE], Source::V0_4_0).unwrap();
        // That release's marker names no way of storing.
        card.0
            .borrow_mut()
            .files
            .insert(MARKER[0].path, vec![0; 16]);
        assert_eq!(
            report(&card),
            Report {
                source: None,
                result: Ok(Done::Converted)
            }
        );
        assert!(only_the_new_files(&card));
        assert!(current(&card).unwrap() == body);
    }

    #[test]
    fn the_line_names_the_way_converted_and_the_release() {
        let text = |source, version: &[u8]| {
            let label: Label = label(source, version)?;
            Some(std::string::String::from_utf16(label.units()).unwrap())
        };
        let v0_2_1 = Some(Source::V0_2_1);
        let v0_4_0 = Some(Source::V0_4_0);
        assert_eq!(text(v0_2_1, b"v0.6.0").as_deref(), Some("v0.2.1-0.6.0"));
        assert_eq!(text(v0_4_0, b"v0.6.0").as_deref(), Some("v0.4.0-0.6.0"));
        assert_eq!(
            text(v0_4_0, b"v0.6.0-draft").as_deref(),
            Some("v0.4.0-0.6.0-draft")
        );
        assert_eq!(text(v0_2_1, b"dev").as_deref(), Some("v0.2.1-dev"));
        assert_eq!(text(None, b"v0.6.0").as_deref(), Some("v0.6.0-migration"));
        assert_eq!(text(None, b"dev").as_deref(), Some("dev-migration"));
        // The longest version that fits behind a tag, and what does not.
        assert!(text(v0_2_1, &[b'1'; 24]).is_some());
        assert_eq!(text(v0_2_1, &[b'1'; 25]), None);
        assert_eq!(text(v0_2_1, b"v0.6.0 x"), None);
    }
}
