//! A simulated card with Bank's files on it, for the PC tests of this crate
//! and of the patches built on it (feature `test-support`).

extern crate std;
use crate::bank_files::{BankFiles, Cleaned, FileName, Files, Loaded, Rest};
use offline_core::{
    native_blob::BLOB_SIZE,
    sections::{DEX, RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_TAGS},
    transport::{self, Delivery},
    Fingerprint, GameIdentity, Storage,
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
    /// Pairs of files the Transporter patch holds open right now.
    pub visitors: usize,
    /// The card has no room: a file that is not there cannot be made.
    pub full: bool,
    /// The plain files of the SD card (`sdfiles::Sd`), by path without
    /// the terminator, and its folders.
    pub sd: BTreeMap<Vec<u8>, Vec<u8>>,
    pub sd_dirs: BTreeSet<Vec<u8>>,
    /// The SD card has no room for a new plain file.
    pub sd_full: bool,
    /// The files of folders of Bank's extdata (`ExtCard`), by path, each
    /// with the size it was made with, and those folders.
    pub ext: BTreeMap<Vec<u8>, Vec<u8>>,
    pub ext_dirs: BTreeSet<Vec<u8>>,
    /// How far each of those files has been written: what lies behind does
    /// not read, as on the console.
    pub ext_written: BTreeMap<Vec<u8>, usize>,
    /// Those a cut left unreadable.
    pub ext_damaged: BTreeSet<Vec<u8>>,
    /// How many reads of the SD card's plain files and of those files were
    /// made since this copy of the card was taken.
    pub reads: usize,
    /// The reads, by their number in `reads`, that fail once.
    pub fail_reads: BTreeSet<usize>,
}

/// What the simulated card answers an operation it refuses with: whether it
/// refused for lack of space (`fs::NoRoom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub full: bool,
}
impl Refused {
    pub const FAILED: Self = Self { full: false };
    pub const FULL: Self = Self { full: true };
}
impl crate::fs::NoRoom for Refused {
    fn no_room(&self) -> bool {
        self.full
    }
}
#[derive(Clone, Default)]
pub struct Shared(pub Rc<RefCell<Disk>>);
/// An open container, and how it is held.
pub struct Handle(pub Shared, pub FileName, Hold);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// One of Bank's, with its handles open: it counts in `Disk::open`.
    Bank,
    /// One of Bank's that rests: it holds nothing open until its next use.
    Resting,
    /// The Transporter patch's, another program with handles of its own.
    Visitor,
}
/// The console gives out only so many handles, and a container takes one
/// archive handle and one open file. Bank holds its Bank container and one
/// other at a time, as it did when every container was one file.
pub const MOST_CONTAINERS: usize = 2;
impl Handle {
    fn count(disk: &Shared) {
        let mut state = disk.0.borrow_mut();
        state.open += 1;
        assert!(
            state.open <= MOST_CONTAINERS,
            "{} containers open at once",
            state.open
        );
    }
    fn held(disk: &Shared, file: FileName) -> Self {
        Self::count(disk);
        Self(disk.clone(), file, Hold::Bank)
    }
    /// The same container in the hands of the Transporter patch.
    fn visiting(mut self) -> Self {
        self.release();
        self.2 = Hold::Visitor;
        self
    }
    fn release(&mut self) {
        if self.2 == Hold::Bank {
            (self.0).0.borrow_mut().open -= 1;
        }
    }
    /// A container that rests opens its handles again when it is used.
    fn take_up(&mut self) {
        if self.2 == Hold::Resting {
            Self::count(&self.0);
            self.2 = Hold::Bank;
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.release();
    }
}
impl Rest for Handle {
    /// The open unit file is flushed if it holds writes, as on the console.
    fn rest(&mut self) -> Result<(), Refused> {
        if self.2 != Hold::Bank {
            return Ok(());
        }
        let file = self.1 as u8;
        if (self.0)
            .0
            .borrow()
            .unsynced
            .iter()
            .any(|unit| unit.0 == file)
        {
            self.sync()?;
        }
        self.release();
        self.2 = Hold::Resting;
        Ok(())
    }
}
impl Shared {
    pub(crate) fn tick(&self) -> Result<(), Refused> {
        let mut disk = self.0.borrow_mut();
        if let Some(budget) = &mut disk.budget {
            if *budget == 0 {
                if disk.console {
                    let lost: Vec<UnitId> = disk.unsynced.iter().copied().collect();
                    disk.unreadable.extend(lost);
                }
                return Err(Refused::FAILED);
            }
            *budget -= 1;
        }
        disk.operations += 1;
        Ok(())
    }
    /// Counts a read of a plain file or of a folder's file: whether it is
    /// one that fails.
    fn read_fails(&self) -> bool {
        let mut disk = self.0.borrow_mut();
        let read = disk.reads;
        disk.reads += 1;
        disk.fail_reads.contains(&read)
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
            visitors: 0,
            full: disk.full,
            sd: disk.sd.clone(),
            sd_dirs: disk.sd_dirs.clone(),
            sd_full: disk.sd_full,
            ext: disk.ext.clone(),
            ext_dirs: disk.ext_dirs.clone(),
            ext_written: disk.ext_written.clone(),
            ext_damaged: disk.ext_damaged.clone(),
            reads: 0,
            fail_reads: BTreeSet::new(),
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
    type Error = Refused;
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), Refused> {
        self.take_up();
        let units = self.units(at, out.len() as u64);
        let disk = (self.0).0.borrow();
        if units.iter().any(|unit| disk.unreadable.contains(unit)) {
            return Err(Refused::FAILED);
        }
        let file = &disk.files[&(self.1 as u8)];
        out.copy_from_slice(&file[at as usize..at as usize + out.len()]);
        Ok(())
    }
    fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), Refused> {
        self.take_up();
        let units = self.units(at, bytes.len() as u64);
        let [unit] = units[..] else {
            panic!("a write must lie in one unit file");
        };
        if (self.0).0.borrow().unreadable.contains(&unit) {
            return Err(Refused::FAILED);
        }
        if (self.0).0.borrow().absent.contains(&unit) {
            if (self.0).0.borrow().full {
                return Err(Refused::FULL);
            }
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
            Err(Refused::FAILED)
        } else {
            Ok(())
        }
    }
    fn sync(&mut self) -> Result<(), Refused> {
        self.take_up();
        self.0.tick()?;
        let file = self.1 as u8;
        (self.0)
            .0
            .borrow_mut()
            .unsynced
            .retain(|unit| unit.0 != file);
        Ok(())
    }
    fn recreate(&mut self, at: u64, length: u64) -> Result<(), Refused> {
        self.take_up();
        let unit = self
            .unit(at, length)
            .expect("only a whole unit is recreated");
        if (self.0).0.borrow().full {
            return Err(Refused::FULL);
        }
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
    fn open(&mut self, file: FileName) -> Result<Option<Handle>, Refused> {
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
    fn create(&mut self, file: FileName) -> Result<Handle, Refused> {
        let count = file.units().len();
        if self.0.borrow().full {
            return Err(Refused::FULL);
        }
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

/// What the Transporter patch has of a container of Bank's: it reads and
/// writes the files that are there, cannot use one that is not, and can
/// neither create nor replace one. It holds the files of one container at a time.
pub struct Visitor(Handle);
impl Visitor {
    fn of(disk: &Shared, file: FileName) -> Option<Self> {
        let handle = disk.clone().open(file).unwrap()?.visiting();
        let mut state = disk.0.borrow_mut();
        state.visitors += 1;
        assert_eq!(
            state.visitors, 1,
            "Transporter holds two containers at once"
        );
        Some(Self(handle))
    }
}
impl Drop for Visitor {
    fn drop(&mut self) {
        ((self.0).0).0.borrow_mut().visitors -= 1;
    }
}
impl Storage for Visitor {
    type Error = Refused;
    /// A unit file that is not there is not read (`fs::Split::visit`).
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), Refused> {
        let units = self.0.units(at, out.len() as u64);
        if units
            .iter()
            .any(|unit| ((self.0).0).0.borrow().absent.contains(unit))
        {
            return Err(Refused::FAILED);
        }
        self.0.read(at, out)
    }
    fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), Refused> {
        let units = self.0.units(at, bytes.len() as u64);
        if units
            .iter()
            .any(|unit| ((self.0).0).0.borrow().absent.contains(unit))
        {
            return Err(Refused::FAILED);
        }
        self.0.write(at, bytes)
    }
    fn sync(&mut self) -> Result<(), Refused> {
        self.0.sync()
    }
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), Refused> {
        Err(Refused::FAILED)
    }
}

pub const GAME: GameIdentity = GameIdentity {
    title_id: 0x0004_0000_0005_5e00,
    save_identity: [7; 32],
};
pub const BEFORE: Fingerprint = [1; 32];
pub const AFTER: Fingerprint = [2; 32];

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
/// A full native body, clean as every write leaves one (`sections::clean`).
/// `boxes` and `dex` mark their regions; the transport box holds `pokemon`
/// occupied slots followed by blank records.
pub fn body(boxes: u8, dex: u8, pokemon: usize) -> Vec<u8> {
    let mut bytes = vec![boxes; BLOB_SIZE];
    bytes[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
    bytes[DEX].fill(dex);
    for slot in 0..30 {
        let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
        let species = if slot < pokemon { 25 + slot as u16 } else { 0 };
        bytes[at..at + RECORD_SIZE].copy_from_slice(&record(species));
        bytes[TRANSPORT_TAGS.start + slot] = if slot < pokemon { 2 } else { 0 };
    }
    offline_core::sections::clean(&mut bytes).unwrap();
    bytes
}
/// A stored Pokémon of species 133 whose identity is `id`, below 0x1000:
/// its encryption constant holds it.
pub fn pokemon(id: u32) -> [u8; RECORD_SIZE] {
    let mut out = [0u8; RECORD_SIZE];
    // Bits 13..18 of the key choose the block order; they stay zero.
    let key = id << 20 | 0x155;
    out[..4].copy_from_slice(&key.to_le_bytes());
    let mut seed = key;
    for index in 0..112 {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        let plain = if index == 0 { 133 } else { 0 };
        out[8 + index * 2..10 + index * 2]
            .copy_from_slice(&(plain ^ (seed >> 16) as u16).to_le_bytes());
    }
    out
}
/// A body whose boxes hold exactly `boxed`, each at (box, slot), and whose
/// transport box holds `transport` Pokémon; `fill` marks the rest.
pub fn with_boxes(
    fill: u8,
    boxed: &[(usize, usize, [u8; RECORD_SIZE])],
    transport: usize,
) -> Vec<u8> {
    let mut bytes = body(fill, 0x44, transport);
    let place = |index: usize, slot: usize| 0x17C + index * 0x1B56 + slot * RECORD_SIZE;
    for index in 0..100 {
        for slot in 0..30 {
            let at = place(index, slot);
            bytes[at..at + RECORD_SIZE].copy_from_slice(&record(0));
        }
    }
    for (index, slot, record) in boxed {
        let at = place(*index, *slot);
        bytes[at..at + RECORD_SIZE].copy_from_slice(record);
    }
    offline_core::sections::clean(&mut bytes).unwrap();
    bytes
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
    files
        .initialize(Cleaned::new(&mut staging).unwrap())
        .unwrap();
    disk
}
pub fn opened(disk: &Shared) -> BankFiles<Shared> {
    let mut files = BankFiles::new(disk.clone());
    files.open().unwrap().unwrap();
    files
}
/// Bank's files as the Transporter patch reaches them, the simulated twin
/// of `fs::Visit`. Of the Bank container it touches the two journal records
/// only.
pub struct Visit(pub Shared);
impl transport::Files for Visit {
    type Storage = Visitor;
    fn journal(&mut self) -> Option<Visitor> {
        Visitor::of(&self.0, FileName::Bank)
    }
    fn mover(&mut self) -> Option<Visitor> {
        Visitor::of(&self.0, FileName::Transport)
    }
}
/// Whether the record in force leaves its save in progress to
/// Transporter's question (`offline_core::Metadata::ask`).
pub fn asked(disk: &Shared) -> bool {
    in_force(disk).is_some_and(|record| record.ask)
}
/// Whether the record in force has its save in progress unlocked
/// (`offline_core::Metadata::unlocked`).
pub fn unlocked(disk: &Shared) -> bool {
    in_force(disk).is_some_and(|record| record.unlocked)
}
fn in_force(disk: &Shared) -> Option<offline_core::Metadata> {
    let journal = transport::Files::journal(&mut Visit(disk.clone())).expect("no journal");
    offline_core::Journal::new(journal)
        .load()
        .expect("journal")
        .map(|head| *head.metadata())
}
/// Transporter's check before a session, as it answers it.
pub fn transport_check(disk: &Shared) -> Result<(), transport::Refusal> {
    transport::check(&mut Visit(disk.clone()))
}
/// Transporter's check before a session: whether it may deliver now.
pub fn transport_ok(disk: &Shared) -> bool {
    transport::check(&mut Visit(disk.clone())).is_ok()
}
/// Delivers with the very function the Transporter patch runs. `None`
/// when it refuses or cannot write.
pub fn try_deliver(disk: &Shared, pokemon: usize) -> Option<Delivery> {
    let source = body(0, 0, pokemon);
    let done = transport::deliver(
        &mut Visit(disk.clone()),
        &source[TRANSPORT_RECORDS],
        &source[TRANSPORT_TAGS],
    )
    .ok()?;
    assert_eq!(done.count, pokemon as u32);
    Some(done)
}
pub fn deliver(disk: &Shared, pokemon: usize) -> Delivery {
    try_deliver(disk, pokemon).expect("Transporter must be allowed to deliver")
}

/// What the SD card's files and folders do.
impl crate::sdfiles::Store for Shared {
    type Error = Refused;
    fn create_dir(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.tick()?;
        self.0.borrow_mut().sd_dirs.insert(path.text().to_vec());
        Ok(())
    }
    fn delete(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.tick()?;
        self.0.borrow_mut().sd.remove(path.text());
        Ok(())
    }
    fn list(
        &mut self,
        path: &crate::sdfiles::Path,
        each: &mut dyn FnMut(&[u8], bool),
    ) -> Result<(), Refused> {
        let inside = [path.text(), b"/"].concat();
        let disk = self.0.borrow();
        let files = entries(&inside, &mut disk.sd.keys());
        let folders = entries(&inside, &mut disk.sd_dirs.iter());
        drop(disk);
        for name in files {
            each(&name, false);
        }
        for name in folders {
            each(&name, true);
        }
        Ok(())
    }
    fn remove_dir(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.tick()?;
        let mut disk = self.0.borrow_mut();
        let inside = [path.text(), b"/"].concat();
        if disk.sd.keys().any(|key| key.starts_with(&inside)) {
            return Err(Refused::FAILED);
        }
        disk.sd_dirs.remove(path.text());
        Ok(())
    }
    fn no_room(error: &Refused) -> bool {
        error.full
    }
    fn code(_: &Refused) -> u32 {
        0
    }
}

/// The names directly inside the folder whose path ends in `inside`.
fn entries(inside: &[u8], keys: &mut dyn Iterator<Item = &Vec<u8>>) -> Vec<Vec<u8>> {
    keys.filter_map(|key| key.strip_prefix(inside))
        .filter(|name| !name.contains(&b'/'))
        .map(|name| name.to_vec())
        .collect()
}

/// The plain files of the SD card. Every call is one operation of the
/// card's budget, but a write: it deletes, creates and writes, three
/// operations, as the console's `put` does. A cut at its second leaves no
/// file, one at its third the file there with the wrong bytes, as a torn
/// write does. A file goes only into a folder that is there.
impl crate::sdfiles::Sd for Shared {
    fn put(&mut self, path: &crate::sdfiles::Path, bytes: &[u8]) -> Result<(), Refused> {
        let key = path.text().to_vec();
        let folder = key[..key.iter().rposition(|byte| *byte == b'/').unwrap_or(0)].to_vec();
        assert!(
            folder.is_empty() || self.0.borrow().sd_dirs.contains(&folder),
            "no folder for {path:?}"
        );
        self.tick()?;
        self.0.borrow_mut().sd.remove(&key);
        if self.0.borrow().sd_full {
            return Err(Refused::FULL);
        }
        self.tick()?;
        if let Err(error) = self.tick() {
            let mut torn = bytes.to_vec();
            torn.iter_mut().for_each(|byte| *byte = !*byte);
            self.0.borrow_mut().sd.insert(key, torn);
            return Err(error);
        }
        self.0.borrow_mut().sd.insert(key, bytes.to_vec());
        Ok(())
    }
    fn get(
        &mut self,
        path: &crate::sdfiles::Path,
        out: &mut [u8],
    ) -> Result<crate::sdfiles::Found, Refused> {
        use crate::sdfiles::Found;
        if self.read_fails() {
            return Err(Refused::FAILED);
        }
        let disk = self.0.borrow();
        Ok(match disk.sd.get(path.text()) {
            None => Found::Absent,
            Some(bytes) if bytes.len() != out.len() => Found::OtherSize,
            Some(bytes) => {
                out.copy_from_slice(bytes);
                Found::Read
            }
        })
    }
    /// One operation: the folder and everything in it, or nothing.
    fn rename_dir(
        &mut self,
        from: &crate::sdfiles::Path,
        to: &crate::sdfiles::Path,
    ) -> Result<(), Refused> {
        self.tick()?;
        let mut disk = self.0.borrow_mut();
        let (from, to) = (from.text().to_vec(), to.text().to_vec());
        assert!(
            !disk.sd_dirs.contains(&to) && !disk.sd.contains_key(&to),
            "rename onto {to:?}"
        );
        if !disk.sd_dirs.remove(&from) {
            return Err(Refused::FAILED);
        }
        disk.sd_dirs.insert(to.clone());
        let inside = [&from[..], b"/"].concat();
        let moved: Vec<Vec<u8>> = disk
            .sd
            .keys()
            .filter(|key| key.starts_with(&inside))
            .cloned()
            .collect();
        for key in moved {
            let bytes = disk.sd.remove(&key).unwrap();
            disk.sd
                .insert([&to[..], &key[from.len()..]].concat(), bytes);
        }
        Ok(())
    }
}

/// The folders of Bank's extdata on the simulated card, by the limits of
/// that archive: names of 16 characters at most, 10 folders, 100 files with
/// the Bank's 8, and a file keeps the size it was made with. Every call is
/// one operation of the card's budget; a cut in a write leaves the file
/// unreadable, as on the console.
pub struct ExtCard(pub Shared);

/// The Bank's own files in the archive's count of 100.
const BANK_UNITS: usize = 8;

impl crate::sdfiles::Store for ExtCard {
    type Error = Refused;
    fn create_dir(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.0.tick()?;
        let key = path.text().to_vec();
        assert!(names_fit(&key), "name too long: {path:?}");
        let mut disk = self.0 .0.borrow_mut();
        disk.ext_dirs.insert(key);
        assert!(disk.ext_dirs.len() <= 10, "more than 10 folders");
        Ok(())
    }
    fn delete(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.0.tick()?;
        let mut disk = self.0 .0.borrow_mut();
        disk.ext.remove(path.text());
        disk.ext_written.remove(path.text());
        disk.ext_damaged.remove(path.text());
        Ok(())
    }
    fn list(
        &mut self,
        path: &crate::sdfiles::Path,
        each: &mut dyn FnMut(&[u8], bool),
    ) -> Result<(), Refused> {
        let inside = if path.text() == b"/" {
            b"/".to_vec()
        } else {
            [path.text(), b"/"].concat()
        };
        let disk = self.0 .0.borrow();
        let files = entries(&inside, &mut disk.ext.keys());
        let folders = entries(&inside, &mut disk.ext_dirs.iter());
        drop(disk);
        for name in files {
            each(&name, false);
        }
        for name in folders {
            each(&name, true);
        }
        Ok(())
    }
    fn remove_dir(&mut self, path: &crate::sdfiles::Path) -> Result<(), Refused> {
        self.0.tick()?;
        let mut disk = self.0 .0.borrow_mut();
        let inside = [path.text(), b"/"].concat();
        if disk.ext.keys().any(|key| key.starts_with(&inside)) {
            return Err(Refused::FAILED);
        }
        disk.ext_dirs.remove(path.text());
        Ok(())
    }
    fn no_room(error: &Refused) -> bool {
        error.full
    }
    fn code(_: &Refused) -> u32 {
        0
    }
}

/// Whether every part of the path is a name the archive stores.
fn names_fit(path: &[u8]) -> bool {
    path.split(|byte| *byte == b'/')
        .all(|name| name.len() <= crate::bank_files::UnitFile::MAX_NAME)
}

impl crate::sdfiles::Extdata for ExtCard {
    fn create(&mut self, path: &crate::sdfiles::Path, length: u32) -> Result<(), Refused> {
        let key = path.text().to_vec();
        assert!(names_fit(&key), "name too long: {path:?}");
        assert!(length > 0, "the console opens no file of size 0");
        let folder = key[..key.iter().rposition(|byte| *byte == b'/').unwrap_or(0)].to_vec();
        assert!(
            self.0 .0.borrow().ext_dirs.contains(&folder),
            "no folder for {path:?}"
        );
        crate::sdfiles::Store::delete(self, path)?;
        if self.0 .0.borrow().sd_full {
            return Err(Refused::FULL);
        }
        self.0.tick()?;
        let mut disk = self.0 .0.borrow_mut();
        disk.ext.insert(key.clone(), vec![0; length as usize]);
        disk.ext_written.insert(key, 0);
        assert!(disk.ext.len() + BANK_UNITS <= 100, "more than 100 files");
        Ok(())
    }
    fn write_at(
        &mut self,
        path: &crate::sdfiles::Path,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), Refused> {
        let key = path.text().to_vec();
        if let Err(error) = self.0.tick() {
            self.0 .0.borrow_mut().ext_damaged.insert(key);
            return Err(error);
        }
        let mut disk = self.0 .0.borrow_mut();
        let start = offset as usize;
        let written = disk.ext_written.get(&key).copied().ok_or(Refused::FAILED)?;
        let file = disk.ext.get_mut(&key).ok_or(Refused::FAILED)?;
        assert!(start + bytes.len() <= file.len(), "a write past the end");
        file[start..start + bytes.len()].copy_from_slice(bytes);
        if start <= written {
            disk.ext_written
                .insert(key, written.max(start + bytes.len()));
        }
        Ok(())
    }
    fn read_at(
        &mut self,
        path: &crate::sdfiles::Path,
        offset: u32,
        out: &mut [u8],
    ) -> Result<crate::sdfiles::Piece, Refused> {
        use crate::sdfiles::Piece;
        if self.0.read_fails() {
            return Err(Refused::FAILED);
        }
        let disk = self.0 .0.borrow();
        let key = path.text();
        let Some(file) = disk.ext.get(key) else {
            return Ok(Piece::Missing);
        };
        let (start, end) = (offset as usize, offset as usize + out.len());
        if end > file.len() {
            return Ok(Piece::Missing);
        }
        if disk.ext_damaged.contains(key) || end > disk.ext_written.get(key).copied().unwrap_or(0) {
            return Err(Refused::FAILED);
        }
        out.copy_from_slice(&file[start..end]);
        Ok(Piece::Read)
    }
    fn rest(&mut self) {}
}
