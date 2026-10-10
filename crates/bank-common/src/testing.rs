//! A simulated card with Bank's files on it, for the PC tests of this crate
//! and of the patches built on it (feature `test-support`).

extern crate std;
use crate::bank_files::{BankFiles, FileName, Files, Loaded};
use offline_core::{
    native_blob::BLOB_SIZE,
    rewards::{Accounting, Date, Stored},
    sections::{DEX, MILES, RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_TAGS},
    sidecar::Sidecar,
    transport, Fingerprint, GameIdentity, GameObservation, Storage,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    vec,
    vec::Vec,
};

/// A unit file: (container, index of the unit in it).
pub type UnitId = (u8, usize);

/// The card. `files` holds each container's bytes as one range; the sets
/// say what state each of its unit files is in.
#[derive(Default)]
pub struct Disk {
    pub files: BTreeMap<u8, Vec<u8>>,
    /// Unit files that do not exist. They read as zeros.
    pub absent: BTreeSet<UnitId>,
    /// Unit files an interrupted write left behind: nothing in them can
    /// be read or written until they are recreated.
    pub unreadable: BTreeSet<UnitId>,
    /// Unit files with writes that no sync has made durable yet.
    pub unsynced: BTreeSet<UnitId>,
    /// The console's behaviour: a cut leaves every unit file with
    /// unsynced writes unreadable. Otherwise half of the cut write
    /// arrives and everything stays readable.
    pub console: bool,
    /// Storage operations allowed before a simulated power cut.
    pub budget: Option<usize>,
    pub operations: usize,
    /// Containers Bank's side holds open right now.
    pub open: usize,
}
#[derive(Clone, Default)]
pub struct Shared(pub Rc<RefCell<Disk>>);
/// An open container. The third field says that it counts as one of
/// Bank's; the Transporter patch is another program with handles of its own.
pub struct Handle(pub Shared, pub FileName, bool);
/// The console gives out only so many handles, and a container takes one
/// archive handle and one open file. Bank holds its Bank container and one
/// other at a time, as it did when every container was one file.
pub const MOST_CONTAINERS: usize = 2;
impl Handle {
    fn held(disk: &Shared, file: FileName) -> Self {
        let mut state = disk.0.borrow_mut();
        state.open += 1;
        assert!(
            state.open <= MOST_CONTAINERS,
            "{} containers open at once",
            state.open
        );
        drop(state);
        Self(disk.clone(), file, true)
    }
    /// The same container in the hands of the Transporter patch.
    fn visiting(mut self) -> Self {
        if core::mem::take(&mut self.2) {
            (self.0).0.borrow_mut().open -= 1;
        }
        self
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        if self.2 {
            (self.0).0.borrow_mut().open -= 1;
        }
    }
}
impl Shared {
    pub(crate) fn tick(&self) -> Result<(), ()> {
        let mut disk = self.0.borrow_mut();
        if let Some(budget) = &mut disk.budget {
            if *budget == 0 {
                if disk.console {
                    let lost: Vec<UnitId> = disk.unsynced.iter().copied().collect();
                    disk.unreadable.extend(lost);
                }
                return Err(());
            }
            *budget -= 1;
        }
        disk.operations += 1;
        Ok(())
    }
    pub fn copy(&self) -> Self {
        let disk = self.0.borrow();
        Self(Rc::new(RefCell::new(Disk {
            files: disk.files.clone(),
            absent: disk.absent.clone(),
            unreadable: disk.unreadable.clone(),
            unsynced: BTreeSet::new(),
            console: disk.console,
            budget: None,
            operations: 0,
            open: 0,
        })))
    }
    /// A copy that behaves like the console, or like the simple card.
    pub fn like(&self, console: bool) -> Self {
        let copy = self.copy();
        copy.0.borrow_mut().console = console;
        copy
    }
    pub fn cut_after(&self, budget: usize) -> Self {
        let copy = self.copy();
        copy.0.borrow_mut().budget = Some(budget);
        copy
    }
    pub fn reboot(&self) -> Self {
        self.copy()
    }
}
impl Handle {
    /// The unit files that `at..at + length` lies in.
    pub fn units(&self, at: u64, length: u64) -> Vec<UnitId> {
        let mut start = 0;
        let mut out = Vec::new();
        for (index, unit) in self.1.units().iter().enumerate() {
            if at < start + unit.len && start < at + length {
                out.push((self.1 as u8, index));
            }
            start += unit.len;
        }
        out
    }
    /// The one unit file that is exactly `at..at + length`.
    pub fn unit(&self, at: u64, length: u64) -> Option<UnitId> {
        let mut start = 0;
        for (index, unit) in self.1.units().iter().enumerate() {
            if (start, unit.len) == (at, length) {
                return Some((self.1 as u8, index));
            }
            start += unit.len;
        }
        None
    }
}
impl Storage for Handle {
    type Error = ();
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
        let units = self.units(at, out.len() as u64);
        let disk = (self.0).0.borrow();
        if units.iter().any(|unit| disk.unreadable.contains(unit)) {
            return Err(());
        }
        let file = &disk.files[&(self.1 as u8)];
        out.copy_from_slice(&file[at as usize..at as usize + out.len()]);
        Ok(())
    }
    fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
        let units = self.units(at, bytes.len() as u64);
        let [unit] = units[..] else {
            panic!("a write must lie in one unit file");
        };
        if (self.0).0.borrow().unreadable.contains(&unit) {
            return Err(());
        }
        if (self.0).0.borrow().absent.contains(&unit) {
            // The first write creates the file; that is a step of its own.
            (self.0).0.borrow_mut().unsynced.insert(unit);
            self.0.tick()?;
            (self.0).0.borrow_mut().absent.remove(&unit);
        }
        (self.0).0.borrow_mut().unsynced.insert(unit);
        // On the simple card a cut write still lands its first half.
        let cut = self.0.tick().is_err();
        let length = if cut { bytes.len() / 2 } else { bytes.len() };
        let mut disk = (self.0).0.borrow_mut();
        let file = disk.files.get_mut(&(self.1 as u8)).unwrap();
        file[at as usize..at as usize + length].copy_from_slice(&bytes[..length]);
        if cut {
            Err(())
        } else {
            Ok(())
        }
    }
    fn sync(&mut self) -> Result<(), ()> {
        self.0.tick()?;
        let file = self.1 as u8;
        (self.0)
            .0
            .borrow_mut()
            .unsynced
            .retain(|unit| unit.0 != file);
        Ok(())
    }
    fn recreate(&mut self, at: u64, length: u64) -> Result<(), ()> {
        let unit = self
            .unit(at, length)
            .expect("only a whole unit is recreated");
        self.0.tick()?;
        let mut disk = (self.0).0.borrow_mut();
        let file = disk.files.get_mut(&(self.1 as u8)).unwrap();
        file[at as usize..(at + length) as usize].fill(0);
        disk.unreadable.remove(&unit);
        disk.unsynced.remove(&unit);
        disk.absent.insert(unit);
        Ok(())
    }
}
impl Files for Shared {
    type Storage = Handle;
    /// `None` when not one unit file of the container exists.
    fn open(&mut self, file: FileName) -> Result<Option<Handle>, ()> {
        let disk = self.0.borrow();
        let Some(bytes) = disk.files.get(&(file as u8)) else {
            return Ok(None);
        };
        assert_eq!(bytes.len() as u64, file.size());
        let exists = (0..file.units().len()).any(|unit| !disk.absent.contains(&(file as u8, unit)));
        drop(disk);
        Ok(exists.then(|| Handle::held(self, file)))
    }
    /// Creates the unit files one after the other.
    fn create(&mut self, file: FileName) -> Result<Handle, ()> {
        let count = file.units().len();
        {
            let mut disk = self.0.borrow_mut();
            let old = disk.files.insert(file as u8, vec![0; file.size() as usize]);
            assert!(old
                .is_none_or(|_| (0..count).all(|unit| disk.absent.contains(&(file as u8, unit)))));
            for unit in 0..count {
                disk.unreadable.remove(&(file as u8, unit));
                disk.absent.insert((file as u8, unit));
            }
        }
        for unit in 0..count {
            self.0.borrow_mut().unsynced.insert((file as u8, unit));
            self.tick()?;
            let mut disk = self.0.borrow_mut();
            disk.absent.remove(&(file as u8, unit));
            disk.unsynced.remove(&(file as u8, unit));
        }
        Ok(Handle::held(self, file))
    }
}

/// What the Transporter patch has of the transport container: it reads
/// and writes the files that are there and can neither create nor
/// replace one.
pub struct Visitor(pub Handle);
impl Storage for Visitor {
    type Error = ();
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
        self.0.read(at, out)
    }
    fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
        let units = self.0.units(at, bytes.len() as u64);
        if units
            .iter()
            .any(|unit| ((self.0).0).0.borrow().absent.contains(unit))
        {
            return Err(());
        }
        self.0.write(at, bytes)
    }
    fn sync(&mut self) -> Result<(), ()> {
        self.0.sync()
    }
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), ()> {
        Err(())
    }
}

pub const GAME: GameIdentity = GameIdentity {
    title_id: 0x0004_0000_0005_5e00,
    save_identity: [7; 32],
};
pub const BEFORE: Fingerprint = [1; 32];
pub const AFTER: Fingerprint = [2; 32];
pub fn observed(fingerprint: Fingerprint) -> GameObservation {
    GameObservation::Present {
        game: GAME,
        fingerprint,
    }
}

/// A stored record whose species word decrypts to `species` (block A first,
/// shuffle value 0).
pub fn record(species: u16) -> [u8; RECORD_SIZE] {
    let mut out = [0u8; RECORD_SIZE];
    let key = 0x0000_0155u32;
    out[..4].copy_from_slice(&key.to_le_bytes());
    let mut seed = key;
    for index in 0..112 {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        let plain = if index == 0 { species } else { 0 };
        out[8 + index * 2..10 + index * 2]
            .copy_from_slice(&(plain ^ (seed >> 16) as u16).to_le_bytes());
    }
    out
}
/// A full native body. `boxes` and `dex` mark their regions; the transport
/// box holds `pokemon` occupied slots followed by blank records.
pub fn body(boxes: u8, dex: u8, pokemon: usize) -> Vec<u8> {
    let mut bytes = vec![boxes; BLOB_SIZE];
    bytes[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
    bytes[MILES].fill(0);
    bytes[DEX].fill(dex);
    for slot in 0..30 {
        let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
        let species = if slot < pokemon { 25 + slot as u16 } else { 0 };
        bytes[at..at + RECORD_SIZE].copy_from_slice(&record(species));
        bytes[TRANSPORT_TAGS.start + slot] = if slot < pokemon { 2 } else { 0 };
    }
    bytes
}
pub fn date(day: u8) -> Date {
    Date::new(2026, 10, day).unwrap()
}
pub fn stored(balance: u32, day: u8, count: u32) -> Stored {
    Stored {
        balance,
        accounting: Some(Accounting::new(date(day), count, 0).unwrap()),
    }
}
/// Fills what the main thread fills: defaults for missing regions.
pub fn load(files: &mut BankFiles<Shared>) -> (Vec<u8>, Loaded) {
    let mut staging = vec![0xEE; BLOB_SIZE];
    let loaded = files.read(&mut staging).unwrap();
    let blank = body(0, 0, 0);
    let from = if loaded.transport_missing { 0 } else { 30 };
    for slot in from..30 {
        let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
        staging[at..at + RECORD_SIZE].copy_from_slice(&blank[at..at + RECORD_SIZE]);
        staging[TRANSPORT_TAGS.start + slot] = 0;
    }
    (staging, loaded)
}
pub fn fresh(body: &[u8]) -> Shared {
    let disk = Shared::default();
    let mut files = BankFiles::new(disk.clone());
    assert_eq!(files.open(), Ok(None));
    let mut staging = body.to_vec();
    files.initialize(&mut staging).unwrap();
    disk
}
pub fn opened(disk: &Shared) -> BankFiles<Shared> {
    let mut files = BankFiles::new(disk.clone());
    files.open().unwrap().unwrap();
    files
}
/// The transport container as the Transporter patch finds it.
pub fn visit(disk: &Shared) -> Option<Sidecar<Visitor>> {
    let storage = disk.clone().open(FileName::Transport).unwrap()?;
    Some(Sidecar::new(Visitor(storage.visiting()), transport::KIND))
}
/// Delivers with the very function the Transporter patch runs. `None`
/// when it refuses or cannot write.
pub fn try_deliver(disk: &Shared, pokemon: usize) -> Option<u32> {
    let source = body(0, 0, pokemon);
    let done = transport::deliver(
        &mut visit(disk)?,
        &source[TRANSPORT_RECORDS],
        &source[TRANSPORT_TAGS],
    )
    .ok()?;
    assert_eq!(done.count, pokemon as u32);
    Some(done.id)
}
pub fn deliver(disk: &Shared, pokemon: usize) -> u32 {
    try_deliver(disk, pokemon).expect("Transporter must be allowed to deliver")
}
