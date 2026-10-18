//! The simulated card of the migration's tests, what the tests check of
//! the target on it, and what each folder gives the tests that run for every
//! earlier way (`Fixtures`).
extern crate std;

use crate::{
    migrate::{run, Card, Done, Report, Stopped, MARKER},
    target,
};
use bank_common::bank_files::{BankFiles, FileName, Files, UnitFile};
use offline_core::{native_blob::BLOB_SIZE, Phase, Storage};
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
pub(crate) struct State {
    pub(crate) files: BTreeMap<&'static str, Vec<u8>>,
    pub(crate) unreadable: BTreeSet<&'static str>,
    /// Parts of a file that an interrupted write of the earlier version
    /// left unreadable: a read that touches one fails.
    pub(crate) torn: Vec<(&'static str, core::ops::Range<usize>)>,
    pub(crate) unsynced: BTreeSet<&'static str>,
    /// The console's behaviour on a cut; otherwise half a write lands.
    pub(crate) console: bool,
    pub(crate) budget: Option<usize>,
    pub(crate) operations: usize,
    /// Containers open right now.
    pub(crate) open: usize,
}
#[derive(Clone, Default)]
pub(crate) struct Sd(pub(crate) Rc<RefCell<State>>);
pub(crate) struct Open(Sd, &'static [UnitFile]);
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
    pub(crate) fn tick(&self) -> Result<(), ()> {
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
    pub(crate) fn restarted(&self, budget: Option<usize>) -> Self {
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
    pub(crate) fn files(&self) -> BTreeMap<&'static str, Vec<u8>> {
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
            if state
                .torn
                .iter()
                .any(|(path, range)| *path == file.path && range.start < high && low < range.end)
            {
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
pub(crate) struct Final(pub(crate) Sd);
impl Files for Final {
    type Storage = Open;
    fn open(&mut self, file: FileName) -> Result<Option<Open>, ()> {
        self.0.open(file.units())
    }
    fn create(&mut self, file: FileName) -> Result<Open, ()> {
        self.0.create(file.units())
    }
}

/// The Bank the current release finds on `card`.
pub(crate) fn current(card: &Sd) -> Option<Vec<u8>> {
    let mut bank = BankFiles::new(Final(card.clone()));
    let Some(Phase::Clean(_)) = bank.open().unwrap() else {
        return None;
    };
    let mut staging = vec![0; BLOB_SIZE];
    let loaded = bank.read(&mut staging).unwrap();
    assert!(!loaded.transport_missing);
    Some(staging)
}

/// Nothing but the files of the target is on `card`.
pub(crate) fn only_the_target_files(card: &Sd) -> bool {
    let files = card.files();
    let expected: Vec<_> = target::ORDER
        .iter()
        .flat_map(|file| file.units())
        .map(|unit| (unit.path, unit.len as usize))
        .collect();
    files.len() == expected.len()
        && expected
            .iter()
            .all(|(path, len)| files.get(path).is_some_and(|bytes| bytes.len() == *len))
}

/// Whether `path` is one of the run's own files: a temporary one or the
/// marker.
pub(crate) fn of_the_run(path: &str) -> bool {
    path == MARKER[0].path
        || target::ORDER
            .iter()
            .any(|&file| target::temporary(file).iter().any(|unit| unit.path == path))
}

pub(crate) fn report(card: &Sd) -> Report<()> {
    run(&mut card.clone(), &mut vec![0; BLOB_SIZE])
}
pub(crate) fn convert(card: &Sd) -> Result<Done, Stopped<()>> {
    report(card).result
}

/// What a folder gives the tests that run for every way (`Source::fixtures`).
pub(crate) struct Fixtures {
    /// A card with a Bank of that way, created and saved once, and the body
    /// the target has to load after converting it.
    pub(crate) used: fn(console: bool) -> (Sd, Vec<u8>),
    /// A card with a save in progress.
    pub(crate) in_progress: fn() -> Sd,
    /// Cards that lack what that release repairs at its next save.
    pub(crate) unsettled: fn() -> Vec<Sd>,
}
