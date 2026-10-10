//! One-time conversion of the Bank that v0.2.1 stored, for v0.3.0.
//!
//! Version 0.2.1 kept each container in one file: `/bank.bin`,
//! `/dex.bin`, `/transport.bin`, `/rewards.bin`. Every unit of a container is
//! a file of its own now (`bank_files`), and the containers themselves are
//! byte for byte what they were, so converting is cutting the four files at
//! their unit boundaries. Three of the new files have the names of old ones,
//! which is why the new files are first written under temporary names.
//!
//! Only this patch knows the earlier files, and it does nothing else: it
//! never opens the Bank. The offline patch refuses them by their size.
//!
//! The steps, each safe to interrupt:
//!
//! 1. *Stage.* The old Bank is loaded the way the offline patch loads a
//!    Bank; it must load and have no save in progress. Every unit of the old
//!    files that can be read is copied into a temporary file of its own and
//!    read back. A unit that cannot be read is left out: if the Bank loaded,
//!    it is a spare one, which an interrupted save of v0.2.1 can leave
//!    damaged, and the offline patch writes it anew at the next save. The
//!    Bank is then loaded from the temporary files and must be the same.
//!    Only then a marker file is created. Until the marker exists the old
//!    files are untouched, and a start begins again.
//! 2. *Finish.* With the marker, the temporary files are the Bank. The old
//!    files are removed, the final files are written from the temporary ones
//!    (the journal records last) and compared, the marker is removed, and
//!    then the temporary files.

use bank_common::{
    bank_files::{self, BankFiles, FileName, Files, Loaded, UnitFile, RECORD, SNAPSHOT},
    session,
};
use offline_core::{
    crc32, native_blob::BLOB_SIZE, rewards, sections::DEX_FILE, transport, Phase, Storage,
    StoreError,
};

/// First number of the result screen when nothing went wrong; the second
/// number is the `Done`.
pub const DONE: u32 = 0x600d;
/// First number when the old Bank has a save in progress. Version 0.2.1,
/// which began it, has to finish it.
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
    Failed(Step, Cause<E>),
}

/// The two numbers of the result screen. `diagnostic` gives the console's
/// code for a file error.
pub fn numbers<E>(result: Result<Done, Stopped<E>>, diagnostic: impl Fn(E) -> u32) -> [u32; 2] {
    match result {
        Ok(done) => [DONE, done as u32],
        Err(Stopped::InProgress) => [REFUSED, 7],
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
/// The containers in the order they are converted: the Bank last, because
/// its journal is what makes a Bank exist.
const ORDER: [FileName; 4] = [
    FileName::Dex,
    FileName::Rewards,
    FileName::Transport,
    FileName::Bank,
];
static OLD_BANK: [UnitFile; 1] = single("/bank.bin", session::BANK_LAYOUT.file_len());
static OLD_DEX: [UnitFile; 1] = single("/dex.bin", DEX_FILE.file_len());
static OLD_TRANSPORT: [UnitFile; 1] = single("/transport.bin", transport::KIND.file_len());
static OLD_REWARDS: [UnitFile; 1] = single("/rewards.bin", rewards::FILE.file_len());
static TEMPORARY_BANK: [UnitFile; 4] = [
    UnitFile::new("/m0.tmp", RECORD),
    UnitFile::new("/m1.tmp", RECORD),
    UnitFile::new("/m2.tmp", SNAPSHOT),
    UnitFile::new("/m3.tmp", SNAPSHOT),
];
static TEMPORARY_DEX: [UnitFile; 2] = [
    UnitFile::new("/m4.tmp", DEX_FILE.slot_len()),
    UnitFile::new("/m5.tmp", DEX_FILE.slot_len()),
];
static TEMPORARY_TRANSPORT: [UnitFile; 2] = [
    UnitFile::new("/m6.tmp", transport::KIND.slot_len()),
    UnitFile::new("/m7.tmp", transport::KIND.slot_len()),
];
static TEMPORARY_REWARDS: [UnitFile; 2] = [
    UnitFile::new("/m8.tmp", rewards::FILE.slot_len()),
    UnitFile::new("/m9.tmp", rewards::FILE.slot_len()),
];
static MARKER: [UnitFile; 1] = single("/migrate.ok", 16);

fn old(file: FileName) -> &'static [UnitFile] {
    match file {
        FileName::Bank => &OLD_BANK,
        FileName::Dex => &OLD_DEX,
        FileName::Transport => &OLD_TRANSPORT,
        FileName::Rewards => &OLD_REWARDS,
    }
}
fn temporary(file: FileName) -> &'static [UnitFile] {
    match file {
        FileName::Bank => &TEMPORARY_BANK,
        FileName::Dex => &TEMPORARY_DEX,
        FileName::Transport => &TEMPORARY_TRANSPORT,
        FileName::Rewards => &TEMPORARY_REWARDS,
    }
}

type Outcome<T, C> = Result<T, Stopped<<C as Card>::Error>>;

fn io<E>(step: Step) -> impl Fn(E) -> Stopped<E> {
    move |error| Stopped::Failed(step, Cause::Io(error))
}

/// Converts what is there, or goes on where an interrupted run stopped.
/// `buffer` is working room of at least one Bank payload.
pub fn run<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<Done, C> {
    if !card.exists(&MARKER).map_err(io(Step::ReadOld))? {
        if !card.exists(&OLD_BANK).map_err(io(Step::ReadOld))? {
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
/// The four containers under the names `names` gives them, for reading.
struct View<'a, C: Card> {
    card: &'a mut C,
    names: fn(FileName) -> &'static [UnitFile],
}
impl<C: Card> Files for View<'_, C> {
    type Storage = ReadOnly<C::Storage>;
    fn open(&mut self, file: FileName) -> Result<Option<Self::Storage>, Guarded<C::Error>> {
        let found = self.card.open((self.names)(file)).map_err(Guarded::Io)?;
        Ok(found.map(ReadOnly))
    }
    fn create(&mut self, _: FileName) -> Result<Self::Storage, Guarded<C::Error>> {
        Err(Guarded::ReadOnly)
    }
}

/// What a Bank is when it is loaded: where its journal stands, what was
/// found beside the boxes, and every byte of the assembled body.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    phase: Phase,
    loaded: Loaded,
    body: u32,
}

/// Loads the Bank under `names` as the offline patch loads one. It must be
/// at rest: a save in progress is the old version's to finish.
fn inspect<C: Card>(
    card: &mut C,
    names: fn(FileName) -> &'static [UnitFile],
    buffer: &mut [u8],
    step: Step,
) -> Outcome<Seen, C> {
    let failed = |error| {
        Stopped::Failed(
            step,
            match error {
                bank_files::Error::Io(Guarded::Io(error))
                | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(Guarded::Io(
                    error,
                )))) => Cause::Io(error),
                _ => Cause::Damaged,
            },
        )
    };
    let damaged = Stopped::Failed(step, Cause::Damaged);
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(damaged);
    };
    let mut files = BankFiles::new(View { card, names });
    let phase = match files.open().map_err(failed)? {
        Some(phase @ Phase::Clean(_)) => phase,
        Some(Phase::Prepared(_)) => return Err(Stopped::InProgress),
        None => return Err(damaged),
    };
    let loaded = files.read(body).map_err(failed)?;
    Ok(Seen {
        phase,
        loaded,
        body: crc32(body),
    })
}

/// Copies the old files into temporary ones in the new cut, then marks them
/// complete. The old files are only read.
fn stage<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<(), C> {
    let read = Step::ReadOld;
    let write = Step::WriteTemporary;
    // The Bank the old files hold. Whatever this needed could be read.
    let before = inspect(card, old, buffer, read)?;
    for file in ORDER {
        card.remove(temporary(file)).map_err(io(write))?;
        // A side file that was never there stays absent.
        let Some(mut from) = card.open(old(file)).map_err(io(read))? else {
            continue;
        };
        let mut start = 0;
        for unit in temporary(file) {
            let piece = buffer
                .get_mut(..unit.len as usize)
                .ok_or(Stopped::Failed(read, Cause::Damaged))?;
            // A unit that does not read is left out. The load after the
            // loop decides whether the Bank can do without it.
            if from.read(start, piece).is_ok() {
                let copied = crc32(piece);
                let mut to = card
                    .create(core::slice::from_ref(unit))
                    .map_err(io(write))?;
                to.write(0, piece).map_err(io(write))?;
                to.sync().map_err(io(write))?;
                to.read(0, piece).map_err(io(write))?;
                if crc32(piece) != copied {
                    return Err(Stopped::Failed(write, Cause::Mismatch));
                }
            }
            start += unit.len;
        }
    }
    // The converted files must be the Bank the old files were.
    if inspect(card, temporary, buffer, write)? != before {
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
    for file in ORDER {
        card.remove(old(file)).map_err(io(Step::RemoveOld))?;
    }
    for file in ORDER {
        card.remove(file.units()).map_err(io(write))?;
        let Some(mut from) = card.open(temporary(file)).map_err(io(read))? else {
            if file == FileName::Bank {
                return Err(Stopped::Failed(read, Cause::Missing));
            }
            continue;
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
    use bank_common::testing::{
        body, deliver, fresh, load, observed, opened, stored, Shared, AFTER, BEFORE, GAME,
    };
    use offline_core::GameObservation;
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
    /// The Bank the card holds under `names`, as the conversion sees it.
    fn seen(card: &Sd, names: fn(FileName) -> &'static [UnitFile]) -> Result<Seen, Stopped<()>> {
        inspect(
            &mut card.clone(),
            names,
            &mut vec![0; BLOB_SIZE],
            Step::ReadOld,
        )
    }
    /// Every unit of the old files: its file, and where in it the unit lies.
    fn old_units() -> Vec<(FileName, usize, &'static str, core::ops::Range<usize>)> {
        let mut out = Vec::new();
        for file in CONTAINERS {
            let mut start = 0;
            for (index, unit) in file.units().iter().enumerate() {
                let end = start + unit.len as usize;
                out.push((file, index, old(file)[0].path, start..end));
                start = end;
            }
        }
        out
    }

    const CONTAINERS: [FileName; 4] = [
        FileName::Bank,
        FileName::Dex,
        FileName::Transport,
        FileName::Rewards,
    ];

    /// The four files an earlier version left for the Bank on `disk`: each
    /// container as one file.
    fn old_card(disk: &Shared, console: bool) -> Sd {
        let card = Sd::default();
        card.0.borrow_mut().console = console;
        for file in CONTAINERS {
            if let Some(bytes) = disk.0.borrow().files.get(&(file as u8)) {
                card.0
                    .borrow_mut()
                    .files
                    .insert(old(file)[0].path, bytes.clone());
            }
        }
        card
    }
    /// What a finished conversion leaves: every container cut into its
    /// files, and nothing else.
    fn converted(disk: &Shared) -> BTreeMap<&'static str, Vec<u8>> {
        let mut out = BTreeMap::new();
        for file in CONTAINERS {
            let Some(bytes) = disk.0.borrow().files.get(&(file as u8)).cloned() else {
                continue;
            };
            let mut start = 0;
            for unit in file.units() {
                out.insert(unit.path, bytes[start..start + unit.len as usize].to_vec());
                start += unit.len as usize;
            }
        }
        out
    }
    fn convert(card: &Sd) -> Result<Done, Stopped<()>> {
        run(&mut card.clone(), &mut vec![0; BLOB_SIZE])
    }
    /// A Bank that was saved once, with a delivery waiting in its box.
    fn used_bank() -> (Shared, Vec<u8>) {
        let disk = fresh(&body(0, 0, 0));
        let saved = body(0x52, 0x66, 0);
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut saved.clone(), stored(7, 1, 3000), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(observed(AFTER)).unwrap();
        files.tidy_transport().unwrap();
        deliver(&disk, 4);
        (disk, body(0x52, 0x66, 4))
    }

    #[test]
    fn the_old_files_are_the_containers_as_one_file_each() {
        let lengths = CONTAINERS.map(|file| (old(file)[0].path, old(file)[0].len));
        assert_eq!(
            lengths,
            [
                ("/bank.bin", 1_461_332),
                ("/dex.bin", 59_776),
                ("/transport.bin", 14_112),
                ("/rewards.bin", 160),
            ]
        );
        for file in CONTAINERS {
            assert_eq!(old(file)[0].len, file.size());
            let cut: Vec<u64> = temporary(file).iter().map(|unit| unit.len).collect();
            let new: Vec<u64> = file.units().iter().map(|unit| unit.len).collect();
            assert_eq!(cut, new);
        }
    }

    #[test]
    fn an_old_bank_is_cut_into_the_new_files_byte_for_byte() {
        let (disk, shown) = used_bank();
        for console in [false, true] {
            let card = old_card(&disk, console);
            assert_eq!(convert(&card), Ok(Done::Converted));
            assert!(card.files() == converted(&disk));
            assert!(card.0.borrow().unreadable.is_empty());
            // A second run finds nothing to do and changes nothing.
            assert_eq!(convert(&card), Ok(Done::AlreadyNew));
            assert!(card.files() == converted(&disk));
        }
        // And those bytes are the Bank: boxes, Pokédex, Miles, the delivery.
        let (bytes, loaded) = load(&mut opened(&disk));
        assert!(bytes == shown);
        assert_eq!((loaded.delivered, loaded.rewards), (4, stored(7, 1, 3000)));
    }

    #[test]
    fn a_cut_at_every_step_of_the_conversion_is_finished_by_the_next_start() {
        let (disk, _) = used_bank();
        for console in [false, true] {
            let whole = old_card(&disk, console).restarted(Some(usize::MAX));
            assert_eq!(convert(&whole), Ok(Done::Converted));
            let total = whole.0.borrow().operations;
            assert!(total > 60);
            let mut resumed = 0;
            for cut in 0..total {
                let card = old_card(&disk, console).restarted(Some(cut));
                assert!(convert(&card).is_err(), "cut {cut}");
                // Until the marker exists the old Bank is whole. Past the
                // marker's removal the new files are complete, and only
                // temporary ones are left to clear away.
                let card = card.restarted(None);
                let marked = card.0.borrow().files.contains_key(MARKER[0].path);
                let complete = card
                    .files()
                    .get("/bank.bin")
                    .is_some_and(|bytes| bytes.len() as u64 == SNAPSHOT);
                if marked {
                    resumed += 1;
                } else if !complete {
                    for file in CONTAINERS {
                        let path = old(file)[0].path;
                        let kept = card.0.borrow().files.get(path).cloned();
                        let was = disk.0.borrow().files.get(&(file as u8)).cloned();
                        assert!(kept == was, "cut {cut}: {path} changed before the marker");
                    }
                }
                let done = convert(&card)
                    .unwrap_or_else(|error| panic!("start after cut {cut}: {error:?}"));
                assert!(
                    matches!(done, Done::Converted | Done::AlreadyNew),
                    "cut {cut}"
                );
                assert!(card.files() == converted(&disk), "cut {cut}: files differ");
                assert!(card.0.borrow().unreadable.is_empty(), "cut {cut}");
            }
            // Cuts fell on both sides of the marker.
            assert!(resumed > 10 && resumed < total - 10, "{resumed} of {total}");
        }
    }

    #[test]
    fn a_bank_with_a_save_in_progress_or_with_damage_is_left_as_it_is() {
        let (disk, _) = used_bank();
        // A Save and Quit that was interrupted under the earlier version.
        let pending = disk.reboot();
        let mut files = opened(&pending);
        load(&mut files);
        files
            .prepare(
                &mut body(0x11, 0x22, 4),
                stored(8, 2, 1),
                GAME,
                [5; 32],
                [6; 32],
            )
            .unwrap();
        let card = old_card(&pending, false);
        let before = card.files();
        assert_eq!(convert(&card), Err(Stopped::InProgress));
        assert!(card.files() == before);

        // The boxes in use fail their checksum.
        let card = old_card(&disk, false);
        let at = session::BANK_LAYOUT.file_len() as usize - 100;
        card.0.borrow_mut().files.get_mut("/bank.bin").unwrap()[at] ^= 1;
        card.0.borrow_mut().files.get_mut("/bank.bin").unwrap()[500] ^= 1;
        let before = card.files();
        assert_eq!(
            convert(&card),
            Err(Stopped::Failed(Step::ReadOld, Cause::Damaged))
        );
        assert!(card.files() == before);
    }

    #[test]
    fn a_bank_that_loads_is_converted_whatever_else_is_unreadable_and_loads_the_same() {
        let (disk, _) = used_bank();
        let want = seen(&old_card(&disk, true), old).unwrap();
        let mut converted_despite_damage = 0;
        for (file, index, path, range) in old_units() {
            let card = old_card(&disk, true);
            card.0.borrow_mut().torn.push((path, range));
            let before = card.files();
            match seen(&card, old) {
                // What the Bank needs cannot be read: nothing is changed.
                Err(error) => {
                    assert_eq!(convert(&card), Err(error), "{file:?} {index}");
                    assert!(card.files() == before, "{file:?} {index}");
                }
                // It loads without that unit: it is converted, and the new
                // files load as exactly the same Bank.
                Ok(loads) => {
                    assert_eq!(convert(&card), Ok(Done::Converted), "{file:?} {index}");
                    assert_eq!(seen(&card, FileName::units), Ok(loads), "{file:?} {index}");
                    assert!(!card.files().contains_key("/transport.bin"));
                    converted_despite_damage += 1;
                }
            }
        }
        // Most units are spare at any time; the Bank did without them.
        assert!(converted_despite_damage >= 5, "{converted_despite_damage}");

        // The cases by name. One of the two sets of boxes is in use and one
        // is spare: the spare one unreadable changes nothing of the Bank.
        let boxes: Vec<_> = old_units()
            .into_iter()
            .filter(|(file, index, ..)| *file == FileName::Bank && *index >= 2)
            .collect();
        let outcomes: Vec<_> = boxes
            .iter()
            .map(|(_, _, path, range)| {
                let card = old_card(&disk, true);
                card.0.borrow_mut().torn.push((path, range.clone()));
                let done = convert(&card);
                if done.is_ok() {
                    assert_eq!(seen(&card, FileName::units).as_ref(), Ok(&want));
                }
                done
            })
            .collect();
        assert!(outcomes.contains(&Ok(Done::Converted)));
        assert!(outcomes.contains(&Err(Stopped::Failed(Step::ReadOld, Cause::Io(())))));
        // One record copy unreadable: the other one is the Bank's record.
        for (_, _, path, range) in old_units()
            .into_iter()
            .filter(|(file, index, ..)| *file == FileName::Bank && *index < 2)
        {
            let card = old_card(&disk, true);
            card.0.borrow_mut().torn.push((path, range));
            assert_eq!(convert(&card), Ok(Done::Converted));
            assert_eq!(seen(&card, FileName::units).as_ref(), Ok(&want));
        }
        // Both record copies unreadable: no Bank can be told from that.
        let card = old_card(&disk, true);
        for (_, _, path, range) in old_units()
            .into_iter()
            .filter(|(file, index, ..)| *file == FileName::Bank && *index < 2)
        {
            card.0.borrow_mut().torn.push((path, range));
        }
        let before = card.files();
        assert!(convert(&card).is_err());
        assert!(card.files() == before);
    }

    #[test]
    fn a_bank_converted_without_its_spare_boxes_saves_and_loads_again() {
        let (disk, _) = used_bank();
        for (file, index, path, range) in old_units() {
            if file != FileName::Bank || index < 2 {
                continue;
            }
            let card = old_card(&disk, true);
            card.0.borrow_mut().torn.push((path, range));
            if convert(&card).is_err() {
                continue; // the boxes in use
            }
            let mut files = BankFiles::new(Final(card.clone()));
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
            let mut staging = vec![0; BLOB_SIZE];
            assert_eq!(files.read(&mut staging).unwrap().delivered, 4);
            // The next Save and Quit writes into the spare files.
            let saved = body(0x77, 0x66, 4);
            files
                .prepare(
                    &mut saved.clone(),
                    stored(9, 2, 2900),
                    GAME,
                    [5; 32],
                    [6; 32],
                )
                .unwrap();
            files
                .reconcile(GameObservation::Present {
                    game: GAME,
                    fingerprint: [6; 32],
                })
                .unwrap();
            files.tidy_transport().unwrap();
            drop(files);
            let mut files = BankFiles::new(Final(card.restarted(None)));
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
            let loaded = files.read(&mut staging).unwrap();
            assert_eq!(loaded.rewards, stored(9, 2, 2900));
            return;
        }
        panic!("one set of boxes is always spare");
    }

    #[test]
    fn a_conversion_that_failed_while_writing_the_new_files_is_continued() {
        // As on a console with the first build of this patch: the marker is
        // set, the old files are gone, the smaller containers are in their
        // new files, and writing the Bank's own files failed.
        let (disk, _) = used_bank();
        let whole = old_card(&disk, true).restarted(Some(usize::MAX));
        assert_eq!(convert(&whole), Ok(Done::Converted));
        let total = whole.0.borrow().operations;
        let stuck = (0..total)
            .map(|cut| {
                let card = old_card(&disk, true).restarted(Some(cut));
                assert!(convert(&card).is_err());
                card.restarted(None)
            })
            .find(|card| {
                let files = card.files();
                files.contains_key(MARKER[0].path)
                    && !files.contains_key("/transport.bin")
                    && files.contains_key("/mover.alt.bin")
                    && files.contains_key("/journal.bin")
            })
            .expect("a cut while the Bank's own files are written");
        assert_eq!(convert(&stuck), Ok(Done::Converted));
        assert!(stuck.files() == converted(&disk));
        assert_eq!(convert(&stuck), Ok(Done::AlreadyNew));
    }

    #[test]
    fn every_result_has_its_two_numbers() {
        let code = |_: ()| 0xd900_458b;
        assert_eq!(numbers(Ok(Done::NoBank), code), [0x600d, 0]);
        assert_eq!(numbers(Ok(Done::Converted), code), [0x600d, 1]);
        assert_eq!(numbers(Ok(Done::AlreadyNew), code), [0x600d, 2]);
        assert_eq!(numbers(Err(Stopped::InProgress), code), [0xbad, 7]);
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

        // A side file that was never there stays absent.
        let (disk, _) = used_bank();
        let without = disk.reboot();
        without.0.borrow_mut().files.remove(&(FileName::Dex as u8));
        let card = old_card(&without, true);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(card.files() == converted(&without));
        assert!(!card.files().contains_key("/dex.bin"));

        // What a cut at the very end may leave is cleared away.
        let card = Sd::default();
        card.0.borrow_mut().files = converted(&disk);
        card.0.borrow_mut().files.insert("/m4.tmp", vec![1; 29_888]);
        assert_eq!(convert(&card), Ok(Done::AlreadyNew));
        assert!(card.files() == converted(&disk));
    }
}
