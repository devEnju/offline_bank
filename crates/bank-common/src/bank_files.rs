//! The stored files and the order in which they are read and written.
//!
//! The Bank container carries the journal and the Bank file, and its journal
//! decides which save is current. The main file and the transport box are a
//! pair of copies each (`offline_core::pair`): the journal record names the
//! copy of each that belongs to a save, and its checksum. A save writes the
//! main file and the transport box into copies that are not in use first,
//! then prepares the journal with the Bank file; a load reads the copies the
//! record names. So all three always describe the same save.
//!
//! Every unit of a container is a file of its own (`UnitFile`): the two
//! journal records and the two copies of each file. An interrupted write can
//! make the file it went to unreadable, and that file is then never one that
//! holds anything still needed. The two files of a pair take turns; `.alt`
//! names the alternate of the two, not an older copy.
//!
//! This module is generic over storage so the whole sequence is host-tested.
//! In memory the main thread only ever sees the one native body.

use crate::session::{self, BankSession};
use offline_core::{
    files,
    moved::{self, Moved},
    native_blob::BLOB_SIZE,
    pair::{Pair, PairError},
    sections::{
        self, Identity, BANK_SIZE, HELD, MAIN_RANGES, MAIN_SIZE, RECORD_SIZE, TRANSPORT_RECORDS,
        TRANSPORT_SIZE, TRANSPORT_SLOTS, TRANSPORT_TAGS,
    },
    transport, Fingerprint, GameIdentity, Part, Phase, RecoveryDecision, Sides, Slot, Storage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FileName {
    Bank = 0,
    Main = 1,
    Transport = 2,
}
/// One file in SD extdata archive `0x00000C9B`: its path and its exact
/// allocation. A file of another size is an error and is never replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitFile {
    pub path: &'static str,
    pub len: u64,
}
impl UnitFile {
    /// Longest file name the 3DS stores in save data and extdata, without the
    /// leading `/`. A longer name makes creating the file fail on the console
    /// (`E0E046C7`), which no PC test can show.
    pub const MAX_NAME: usize = 16;
    pub const fn new(path: &'static str, len: u64) -> Self {
        assert!(path.len() - 1 <= Self::MAX_NAME);
        Self { path, len }
    }
}

/// Lengths of the files: a journal record, and one copy of the Bank file,
/// the main file and the transport box. Each is its data and nothing else.
pub const RECORD: u64 = offline_core::METADATA_SIZE as u64;
pub const SNAPSHOT: u64 = session::BANK_LAYOUT.snapshot_len();
pub const MAIN: u64 = MAIN_SIZE as u64;
pub const MOVER: u64 = transport::LEN as u64;
/// In the order of the container's layout: the journal records, then the
/// copies of the Bank file.
/// The names are `offline_core::files`, which Transporter reaches too.
static BANK_UNITS: [UnitFile; 4] = [
    UnitFile::new(files::JOURNAL[0], RECORD),
    UnitFile::new(files::JOURNAL[1], RECORD),
    UnitFile::new(files::BANK[0], SNAPSHOT),
    UnitFile::new(files::BANK[1], SNAPSHOT),
];
static MAIN_UNITS: [UnitFile; 2] = [
    UnitFile::new(files::MAIN[0], MAIN),
    UnitFile::new(files::MAIN[1], MAIN),
];
static TRANSPORT_UNITS: [UnitFile; 2] = [
    UnitFile::new(files::MOVER[0], MOVER),
    UnitFile::new(files::MOVER[1], MOVER),
];

impl FileName {
    /// The files of the container, in the order of its layout.
    pub fn units(self) -> &'static [UnitFile] {
        match self {
            Self::Bank => &BANK_UNITS,
            Self::Main => &MAIN_UNITS,
            Self::Transport => &TRANSPORT_UNITS,
        }
    }
    /// Length of the container: all its units.
    pub fn size(self) -> u64 {
        self.units().iter().map(|unit| unit.len).sum()
    }
}

/// Access to the containers by name. `open` returns `None` only when none of
/// a container's files exists; `create` makes all of them exclusively new and
/// zero-filled.
pub trait Files {
    type Storage: Storage;
    fn open(
        &mut self,
        file: FileName,
    ) -> Result<Option<Self::Storage>, <Self::Storage as Storage>::Error>;
    fn create(
        &mut self,
        file: FileName,
    ) -> Result<Self::Storage, <Self::Storage as Storage>::Error>;
}

/// A container that can let go of everything it holds open between uses
/// and takes it up again when it is next read or written.
pub trait Rest: Storage {
    fn rest(&mut self) -> Result<(), Self::Error>;
}

/// The files in Bank's extdata, opened through the session borrowed from
/// the original (`task::FS_SESSION`).
#[cfg(target_arch = "arm")]
pub struct ExtFiles {
    pub session: u32,
}
#[cfg(target_arch = "arm")]
impl Files for ExtFiles {
    type Storage = crate::fs::ExtdataStorage;
    fn open(&mut self, file: FileName) -> Result<Option<Self::Storage>, crate::fs::Error> {
        unsafe { crate::fs::ExtdataStorage::open_existing(self.session, file.units()) }
    }
    fn create(&mut self, file: FileName) -> Result<Self::Storage, crate::fs::Error> {
        unsafe { crate::fs::ExtdataStorage::create_new(self.session, file.units()) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Problem {
    /// The copy that belongs to the current save is missing or is not what
    /// the journal says it is.
    Damaged = 1,
    /// A delivery waits that could not be read: a save would drop it.
    Conflict = 3,
    /// The staged body is not a native body.
    Layout = 4,
    /// The Bank file is not open or has an unresolved transfer.
    NotReady = 5,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Io(E),
    Bank(session::Error<E>),
    Side(FileName, Problem),
}
type Outcome<T, F> = Result<T, Error<<<F as Files>::Storage as Storage>::Error>>;

#[cfg(feature = "test-build")]
use crate::rehearsal::{Rehearsal, TEAR_BOXES_PASSES, TEAR_RECORD_PASSES};

/// What a load found beside the Bank regions now in the staging buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// No transport box: every slot must be filled with the blank record.
    pub transport_missing: bool,
    /// Nonzero when a Transporter delivery became the box: its Pokémon count.
    /// The delivered box is complete and is shown as it is.
    pub delivered: u32,
}

pub struct BankFiles<F: Files> {
    files: F,
    bank: Option<BankSession<F::Storage>>,
    /// The delivery that waits was read at the last load and is the box shown.
    taken: bool,
    /// Test builds: what the next save rehearses.
    #[cfg(feature = "test-build")]
    rehearsal: Rehearsal,
}

fn side<E>(file: FileName) -> impl Fn(PairError<E>) -> Error<E> {
    move |error| match error {
        PairError::Io(error) => Error::Io(error),
        _ => Error::Side(file, Problem::Damaged),
    }
}

impl<F: Files> BankFiles<F>
where
    F::Storage: Rest,
{
    /// Lets go of the Bank file's archive handle and open file between two
    /// uses: what was written is flushed, and the file stays this Bank's,
    /// with the journal as it was read. The next use opens them again.
    pub fn rest(&mut self) -> Outcome<(), F> {
        match &mut self.bank {
            Some(bank) => bank.storage_mut().rest().map_err(Error::Io),
            None => Ok(()),
        }
    }
}

impl<F: Files> BankFiles<F> {
    pub const fn new(files: F) -> Self {
        Self {
            files,
            bank: None,
            taken: false,
            #[cfg(feature = "test-build")]
            rehearsal: Rehearsal::None,
        }
    }
    /// Test builds: what the next save rehearses. Only the two that write
    /// for a while are made here; a stop is the caller's.
    #[cfg(feature = "test-build")]
    pub fn rehearse(&mut self, rehearsal: Rehearsal) {
        self.rehearsal = rehearsal;
    }
    pub fn files_mut(&mut self) -> &mut F {
        &mut self.files
    }
    pub fn is_open(&self) -> bool {
        self.bank.is_some()
    }
    /// Releases the Bank file. The other files are never held open.
    pub fn close(&mut self) -> Option<F::Storage> {
        self.taken = false;
        self.bank.take().map(BankSession::into_storage)
    }

    fn bank(&mut self) -> Outcome<&mut BankSession<F::Storage>, F> {
        self.bank
            .as_mut()
            .ok_or(Error::Side(FileName::Bank, Problem::NotReady))
    }
    /// The two copies of the main file or of the transport box.
    fn open_side(&mut self, file: FileName) -> Outcome<Option<Pair<F::Storage>>, F> {
        let len = file.units()[0].len as u32;
        Ok(self
            .files
            .open(file)
            .map_err(Error::Io)?
            .map(|storage| Pair::new(storage, len)))
    }
    fn ensure_side(&mut self, file: FileName) -> Outcome<Pair<F::Storage>, F> {
        match self.open_side(file)? {
            Some(pair) => Ok(pair),
            None => Ok(Pair::new(
                self.files.create(file).map_err(Error::Io)?,
                file.units()[0].len as u32,
            )),
        }
    }

    /// Opens the Bank file. `None` means no Bank exists yet: the file is
    /// absent, or its creation was cut before a Bank became current in it.
    /// `initialize` finishes either.
    pub fn open(&mut self) -> Outcome<Option<Phase>, F> {
        if self.bank.is_none() {
            let Some(storage) = self.files.open(FileName::Bank).map_err(Error::Io)? else {
                return Ok(None);
            };
            match BankSession::open_existing(storage) {
                Ok(session) => self.bank = Some(session),
                Err(error) if BankSession::<F::Storage>::never_held_a_bank(&error) => {
                    return Ok(None)
                }
                Err(error) => return Err(Error::Bank(error)),
            }
        }
        self.phase().map(Some)
    }

    pub fn phase(&mut self) -> Outcome<Phase, F> {
        self.bank()?.phase().map_err(Error::Bank)
    }

    /// Writes the main file and the transport box of the next save from the
    /// full body in `staging`, into the copies `main` and `mover`, then
    /// compacts `staging[..BANK_SIZE]` into the Bank payload. Returns what the
    /// journal records of the two.
    fn write_sides(&mut self, staging: &mut [u8], main: Slot, mover: Slot) -> Outcome<Sides, F> {
        let layout = Error::Side(FileName::Bank, Problem::Layout);
        let Ok(held) = sections::transport_count(staging) else {
            return Err(layout);
        };
        let [header, middle, tail] = MAIN_RANGES;
        let mut file = self.ensure_side(FileName::Main)?;
        let main = file
            .write(main, &[&staging[header], &staging[middle], &staging[tail]])
            .map_err(side(FileName::Main))?;
        drop(file);
        let mut file = self.ensure_side(FileName::Transport)?;
        let mover = file
            .write(
                mover,
                &[&staging[TRANSPORT_RECORDS], &staging[TRANSPORT_TAGS]],
            )
            .map_err(side(FileName::Transport))?;
        drop(file);
        if sections::compact(staging).is_err() {
            return Err(layout);
        }
        Ok(Sides {
            main,
            mover,
            held: held as u8,
        })
    }

    /// Creates a new Bank from the full native body in `staging`. Afterwards
    /// `staging` holds the compact Bank payload, not a body. Files left by a
    /// creation that was cut are finished in place. Beside a Bank that
    /// exists, or one that cannot be read, nothing is written.
    pub fn initialize(&mut self, staging: &mut [u8]) -> Outcome<(), F> {
        if self.open()?.is_some() {
            return Err(Error::Side(FileName::Bank, Problem::NotReady));
        }
        self.taken = false;
        let sides = self.write_sides(staging, Slot::A, Slot::A)?;
        let payload = &staging[..BANK_SIZE];
        let session = match self.files.open(FileName::Bank).map_err(Error::Io)? {
            Some(storage) => BankSession::reinitialize_bytes(storage, payload, sides),
            None => {
                let storage = self.files.create(FileName::Bank).map_err(Error::Io)?;
                BankSession::initialize_bytes(storage, payload, sides)
            }
        };
        self.bank = Some(session.map_err(Error::Bank)?);
        Ok(())
    }

    /// Reads the copy of the transport box that `part` names. `false` when
    /// it cannot be read and `dispensable` says that nothing is lost with it;
    /// otherwise that is an error.
    fn read_box(
        &mut self,
        part: Part,
        dispensable: bool,
        records: &mut [u8],
        tags: &mut [u8],
    ) -> Outcome<bool, F> {
        let read = match self.open_side(FileName::Transport)? {
            Some(mut file) => file.read(part, &mut [records, tags]),
            None => Err(PairError::Checksum),
        };
        match read {
            Ok(()) => Ok(true),
            Err(_) if dispensable => Ok(false),
            Err(error) => Err(side(FileName::Transport)(error)),
        }
    }

    /// Assembles the current save in `staging` as one native body: one read
    /// per file, each into its final place and checked on that pass. Nothing
    /// is written to any file.
    pub fn read(&mut self, staging: &mut [u8]) -> Outcome<Loaded, F> {
        if staging.len() != BLOB_SIZE {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        let record = self.bank()?.record().map_err(Error::Bank)?;
        let current = self.bank()?.read_bank(staging).map_err(Error::Bank)?;
        sections::expand(staging).map_err(|_| Error::Side(FileName::Bank, Problem::Layout))?;
        self.taken = false;

        // The main file holds what Bank cannot do without: the header with
        // the format, the Miles and the flags, and the Pokédex. Its copy
        // for this save must be there and read.
        let Some(mut file) = self.open_side(FileName::Main)? else {
            return Err(Error::Side(FileName::Main, Problem::Damaged));
        };
        let layout = |_| Error::Side(FileName::Main, Problem::Layout);
        let mut parts = sections::main_parts(staging).map_err(layout)?;
        file.read(current.main, &mut parts)
            .map_err(side(FileName::Main))?;
        drop(file);
        sections::validate_header(staging).map_err(layout)?;

        // The transport box: the save's own, or the delivery that waits
        // beside it. A delivery that cannot be read is left alone and not
        // shown, and an empty box that cannot be read is an empty box.
        let load = transport::on_load(&record);
        let (low, high) = staging.split_at_mut(TRANSPORT_TAGS.start);
        let (records, tags) = (&mut low[TRANSPORT_RECORDS], &mut high[..TRANSPORT_SLOTS]);
        let mut loaded = Loaded {
            transport_missing: false,
            delivered: 0,
        };
        if !self.read_box(
            load.source,
            load.delivery || current.held == 0,
            records,
            tags,
        )? {
            records.fill(0);
            tags.fill(0);
            loaded.transport_missing = true;
        } else if load.delivery {
            self.taken = true;
            loaded.delivered = sections::occupied(records);
        }
        Ok(loaded)
    }

    /// Makes the next save durable up to the game write: the main file and
    /// the transport box first, then the journal with the Bank file.
    /// `staging` holds the full native body and is compacted. The native game
    /// writer MUST NOT run if this fails.
    pub fn prepare(
        &mut self,
        staging: &mut [u8],
        game: GameIdentity,
        before: Fingerprint,
        after: Fingerprint,
    ) -> Outcome<(), F> {
        let current = self.bank()?.current().map_err(Error::Bank)?;
        let delivery = self.bank()?.record().map_err(Error::Bank)?.delivery;
        // The record that makes this save current says that nothing waits
        // any more, so a delivery that was not shown is not saved over.
        if delivery.is_some() && !self.taken {
            return Err(Error::Side(FileName::Transport, Problem::Conflict));
        }
        // Test builds: keep the record being written for a while, so that a
        // real power cut lands inside one of its writes.
        #[cfg(feature = "test-build")]
        if self.rehearsal == Rehearsal::TearRecord {
            self.bank()?
                .tear_record(TEAR_RECORD_PASSES)
                .map_err(Error::Bank)?;
        }
        let sides = self.write_sides(
            staging,
            current.main.slot.other(),
            current.next_mover(delivery.is_some()),
        )?;
        // Test builds: the same for the spare copy of the Bank file.
        #[cfg(feature = "test-build")]
        if self.rehearsal == Rehearsal::TearBoxes {
            self.bank()?
                .tear_boxes(&mut staging[..BANK_SIZE], TEAR_BOXES_PASSES)
                .map_err(Error::Bank)?;
        }
        self.bank()?
            .prepare_bytes(&staging[..BANK_SIZE], sides, game, before, after)
            .map_err(Error::Bank)?;
        Ok(())
    }

    /// Resolves a prepared save from the game actually observed. Follow it
    /// with `read`.
    pub fn reconcile(&mut self, fingerprint: Fingerprint) -> Outcome<RecoveryDecision, F> {
        self.bank()?
            .reconcile_game(fingerprint)
            .map_err(Error::Bank)
    }

    /// What the prepared save moved between Bank and the game: everything
    /// the Bank held before it, in its boxes and in the transport box it
    /// showed, against everything it holds after it. `staging` is used for
    /// one file at a time and holds nothing of use afterwards; the two
    /// lists are working room. Nothing is written.
    pub fn pending_moves(
        &mut self,
        staging: &mut [u8],
        before: &mut [Identity; HELD],
        after: &mut [Identity; HELD],
    ) -> Outcome<Moved, F> {
        let record = self.bank()?.record().map_err(Error::Bank)?;
        let Phase::Prepared(pending) = record.phase else {
            return Err(Error::Side(FileName::Bank, Problem::NotReady));
        };
        if staging.len() != BLOB_SIZE {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        let layout = |_| Error::Side(FileName::Bank, Problem::Layout);
        let mut counts = [0; 2];
        for (side, list) in [&mut *before, &mut *after].into_iter().enumerate() {
            self.bank()?
                .read_pending(side == 1, staging)
                .map_err(Error::Bank)?;
            counts[side] =
                sections::box_identities(&staging[..BANK_SIZE], list, 0).map_err(layout)?;
        }
        // The transport box: before, what a load showed (its own box, or the
        // delivery it took); after, the box the save wrote.
        let shown = transport::on_load(&record);
        let (records, tags) = staging[..TRANSPORT_SIZE].split_at_mut(TRANSPORT_SLOTS * RECORD_SIZE);
        for (part, empty, list, count) in [
            (
                shown.source,
                !shown.delivery && pending.before.held == 0,
                &mut *before,
                0,
            ),
            (pending.after.mover, pending.after.held == 0, &mut *after, 1),
        ] {
            if self.read_box(part, empty, records, tags)? {
                counts[count] = sections::identities(records, list, counts[count]);
            }
        }
        Ok(moved::moved(
            &mut before[..counts[0]],
            &mut after[..counts[1]],
        ))
    }

    /// Resolves a prepared save toward the side `decision` names, for a
    /// game whose save is neither of its two images (`moved::choose`). The
    /// journal's own rule is not loosened: this names the image the game is
    /// taken to have been saved from. Follow it with `read`, as `reconcile`.
    pub fn reconcile_as(&mut self, decision: RecoveryDecision) -> Outcome<RecoveryDecision, F> {
        let Phase::Prepared(pending) = self.phase()? else {
            return Err(Error::Side(FileName::Bank, Problem::NotReady));
        };
        self.reconcile(match decision {
            RecoveryDecision::KeepBefore => pending.before_fingerprint,
            RecoveryDecision::CommitAfter => pending.after_fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::testing::*;
    use offline_core::{
        native_blob::{MILES_OFFSET, REWARD_DATE_OFFSET},
        sections::{BANK_RANGES, DEX},
    };
    const MILES: core::ops::Range<usize> = MILES_OFFSET..MILES_OFFSET + 4;
    const REWARD_DATE: core::ops::Range<usize> = REWARD_DATE_OFFSET..REWARD_DATE_OFFSET + 6;
    use std::{boxed::Box, collections::BTreeSet, format, vec, vec::Vec};

    /// Opens after a reboot, resolving a pending save from the observed game.
    fn recovered(disk: &Shared, game: Fingerprint) -> (Vec<u8>, Loaded) {
        let mut files = BankFiles::new(disk.reboot());
        if let Phase::Prepared(_) = files.open().unwrap().unwrap() {
            files.reconcile(game).unwrap();
        }
        load(&mut files)
    }

    const CONTAINERS: [FileName; 3] = [FileName::Bank, FileName::Main, FileName::Transport];

    #[test]
    fn file_names_and_sizes_are_fixed() {
        let files: Vec<_> = CONTAINERS
            .iter()
            .flat_map(|file| file.units())
            .map(|unit| (unit.path, unit.len))
            .collect();
        assert_eq!(
            files,
            [
                ("/journal.bin", 140),
                ("/journal.alt.bin", 140),
                ("/bank.bin", 729_800),
                ("/bank.alt.bin", 729_800),
                ("/main.bin", 30_466),
                ("/main.alt.bin", 30_466),
                ("/mover.bin", 6_990),
                ("/mover.alt.bin", 6_990),
            ]
        );
        assert_eq!(CONTAINERS.map(FileName::size), [1_459_880, 60_932, 13_980]);
        // The three data files are the native body and nothing else.
        assert_eq!((SNAPSHOT + MAIN + MOVER) as usize, BLOB_SIZE);
        let lengths: Vec<u64> = FileName::Bank.units().iter().map(|unit| unit.len).collect();
        assert_eq!(lengths, session::BANK_LAYOUT.units().map(|unit| unit.1));
    }

    #[test]
    fn every_file_name_fits_the_console_limit() {
        let mut seen = BTreeSet::new();
        for unit in CONTAINERS.iter().flat_map(|file| file.units()) {
            let name = unit.path.strip_prefix('/').unwrap();
            assert!(unit.path.is_ascii() && !name.contains('/'));
            assert!(name.len() <= UnitFile::MAX_NAME, "{}", unit.path);
            assert!(seen.insert(unit.path), "{} twice", unit.path);
        }
        assert_eq!(UnitFile::MAX_NAME, 16);
    }

    #[test]
    fn new_bank_writes_three_containers_and_loads_back_every_byte() {
        let original = body(0x31, 0x44, 0);
        let disk = fresh(&original);
        assert_eq!(disk.0.borrow().files.len(), 3);
        // Each file is its ranges of the body, unchanged and in order, with
        // nothing before, between or behind them. The second copies are
        // not written yet.
        let stored = disk.0.borrow().files.clone();
        let cut = |ranges: &[core::ops::Range<usize>]| -> Vec<u8> {
            let parts = ranges.iter().map(|range| &original[range.clone()]);
            parts.flatten().copied().collect()
        };
        for (file, skip, ranges) in [
            (FileName::Bank, 2 * RECORD as usize, &BANK_RANGES[..]),
            (FileName::Main, 0, &MAIN_RANGES[..]),
            (
                FileName::Transport,
                0,
                &[TRANSPORT_RECORDS, TRANSPORT_TAGS][..],
            ),
        ] {
            let (first, second) = stored[&(file as u8)][skip..].split_at(cut(ranges).len());
            assert!(first == cut(ranges), "{file:?}");
            assert!(second.len() == first.len() && second.iter().all(|byte| *byte == 0));
        }
        let (loaded_body, loaded) = load(&mut opened(&disk));
        assert!(loaded_body == original);
        assert_eq!(
            loaded,
            Loaded {
                transport_missing: false,
                delivered: 0,
            }
        );
    }

    #[test]
    fn miles_balance_and_reward_date_are_stored_in_the_main_file() {
        let date = [0xea, 0x07, 10, 6, 18, 10];
        let mut with_balance = body(0x31, 0x44, 0);
        with_balance[MILES].copy_from_slice(&999u32.to_le_bytes());
        with_balance[REWARD_DATE].copy_from_slice(&date);
        let disk = fresh(&with_balance);
        let mut files = opened(&disk);
        let mut staging = load(&mut files).0;
        // A new Bank keeps the native fields as its body has them.
        assert_eq!(&staging[MILES], &999u32.to_le_bytes());
        assert_eq!(&staging[REWARD_DATE], &date);

        staging[MILES].copy_from_slice(&777u32.to_le_bytes());
        staging[REWARD_DATE.start + 3] = 7;
        files.prepare(&mut staging, GAME, BEFORE, AFTER).unwrap();
        // A save that is undone leaves the balance and the date of before.
        let before = recovered(&disk, BEFORE).0;
        assert_eq!(&before[MILES], &999u32.to_le_bytes());
        assert_eq!(before[REWARD_DATE.start + 3], 6);
        // A save that is committed has those of the save.
        files.reconcile(AFTER).unwrap();
        let after = load(&mut files).0;
        assert_eq!(&after[MILES], &777u32.to_le_bytes());
        assert_eq!(after[REWARD_DATE.start + 3], 7);
    }

    #[test]
    fn a_cut_at_every_write_of_a_save_leaves_one_consistent_save() {
        every_cut_of_a_save(false, 0, 0);
    }
    #[test]
    fn on_the_console_a_cut_at_every_write_of_a_save_leaves_one_consistent_save() {
        every_cut_of_a_save(true, 0, 0);
    }
    /// A save that takes a delivery writes its box over the empty one in
    /// use, so that the delivery outlasts it: whole until the save counts.
    #[test]
    fn a_cut_at_every_write_of_a_save_that_takes_a_delivery_keeps_or_takes_it_whole() {
        for console in [false, true] {
            every_cut_of_a_save(console, 0, 6);
        }
    }
    /// The test build's two rehearsals that write for a while: a cut at
    /// every one of their writes too leaves the old save.
    #[cfg(feature = "test-build")]
    #[test]
    fn a_cut_at_every_write_of_a_rehearsed_save_leaves_one_consistent_save() {
        for rehearsal in [Rehearsal::TearRecord, Rehearsal::TearBoxes] {
            for console in [false, true] {
                every_cut_of_a_save(console, rehearsal as u8, 0);
            }
        }
    }
    /// `rehearsal` is 0 for a save as in a release. With `delivered` not 0
    /// the box of the old save is empty and that many Pokémon wait for it.
    #[cfg_attr(not(feature = "test-build"), allow(unused_variables))]
    fn every_cut_of_a_save(console: bool, rehearsal: u8, delivered: usize) {
        let old = body(0x31, 0x44, if delivered == 0 { 2 } else { 0 });
        let new = body(0x52, 0x66, 1);
        // Establish a committed save that has all three pieces.
        let disk = fresh(&body(0, 0, 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut old.clone(), GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile([9; 32]).unwrap();
        load(&mut files);
        drop(files);
        if delivered != 0 {
            deliver(&disk, delivered);
        }
        let base = disk.reboot().like(console);

        // The next session changes Miles, boxes, Pokédex and box.
        let save = |disk: &Shared| {
            let mut files = opened(disk);
            load(&mut files);
            #[cfg(feature = "test-build")]
            files.rehearse(match rehearsal {
                3 => Rehearsal::TearRecord,
                4 => Rehearsal::TearBoxes,
                _ => Rehearsal::None,
            });
            files.prepare(&mut new.clone(), GAME, BEFORE, AFTER)
        };
        let counting = base.cut_after(usize::MAX);
        save(&counting).unwrap();
        let total = counting.0.borrow().operations;
        assert!(total > 10);
        // A rehearsal writes far more often than a save does.
        #[cfg(feature = "test-build")]
        assert_eq!(total > 50, rehearsal != 0);

        // What Bank shows of the old save: its box, or the delivery.
        let old_state = body(0x31, 0x44, if delivered == 0 { 2 } else { delivered });
        let new_state = new.clone();
        let third = body(0x73, 0x21, 1);
        let check = |disk: &Shared, game, expected: &Vec<u8>, context: &str| {
            let (bytes, loaded) = recovered(disk, game);
            assert!(bytes == *expected, "{context}: body differs");
            assert!(!loaded.transport_missing, "{context}");
            let waiting = if game == BEFORE { delivered } else { 0 };
            assert_eq!(loaded.delivered as usize, waiting, "{context}");
            // Whatever the cut left behind, the next save goes through.
            let mut files = BankFiles::new(disk.reboot());
            if let Phase::Prepared(_) = files.open().unwrap().unwrap() {
                files.reconcile(game).unwrap();
            }
            load(&mut files);
            files
                .prepare(&mut third.clone(), GAME, [5; 32], [6; 32])
                .unwrap_or_else(|error| panic!("{context}: the next save: {error:?}"));
            files.reconcile([6; 32]).unwrap();
            let bytes = load(&mut opened(&files.files_mut().reboot())).0;
            assert!(bytes == third, "{context}: the next save differs");
        };
        for cut in 0..=total {
            let disk = base.cut_after(cut);
            assert_eq!(save(&disk).is_ok(), cut == total, "cut {cut}");
            // The game was not written: everything is the old save.
            check(&disk, BEFORE, &old_state, &format!("prepare cut {cut}"));
        }

        // Prepared and the game saved: publication is cut at every write.
        let prepared = base.copy();
        save(&prepared).unwrap();
        check(&prepared, BEFORE, &old_state, "game write failed");
        let finish = |disk: &Shared| {
            let mut files = opened(disk);
            files.reconcile(AFTER)?;
            let mut staging = vec![0; BLOB_SIZE];
            files.read(&mut staging).map(|_| ())
        };
        let counting = prepared.cut_after(usize::MAX);
        finish(&counting).unwrap();
        let total = counting.0.borrow().operations;
        for cut in 0..=total {
            let disk = prepared.cut_after(cut);
            let _ = finish(&disk);
            check(&disk, AFTER, &new_state, &format!("publish cut {cut}"));
        }
    }

    #[test]
    fn pokedex_of_two_games_survives_saves_and_a_rollback() {
        // Y's region marked 0x19, then Ultra Sun adds 0x76 beside it.
        let mut y = body(0x31, 0, 0);
        y[DEX.start + 0x6A8..DEX.start + 0xD48].fill(0x19);
        let mut both = y.clone();
        both[DEX.start + 0x4FE0..DEX.start + 0x5F58].fill(0x76);
        let disk = fresh(&body(0x31, 0, 0));
        let mut files = opened(&disk);
        load(&mut files);
        files.prepare(&mut y.clone(), GAME, BEFORE, AFTER).unwrap();
        files.reconcile(AFTER).unwrap();
        assert!(load(&mut files).0 == y);
        // A live import alone publishes nothing.
        assert!(load(&mut opened(&disk.reboot())).0 == y);
        files
            .prepare(&mut both.clone(), GAME, [3; 32], [4; 32])
            .unwrap();
        assert!(recovered(&disk, [3; 32]).0 == y);
        assert!(recovered(&disk, [4; 32]).0 == both);
    }

    #[test]
    fn a_missing_transport_file_loads_as_empty_and_is_created_by_the_next_save() {
        let original = body(0x31, 0x44, 0);
        let disk = fresh(&original);
        disk.0
            .borrow_mut()
            .files
            .remove(&(FileName::Transport as u8));
        let mut files = opened(&disk);
        let mut staging = vec![0xEE; BLOB_SIZE];
        let loaded = files.read(&mut staging).unwrap();
        assert_eq!(
            loaded,
            Loaded {
                transport_missing: true,
                delivered: 0,
            }
        );
        // The box is zero for the main thread to fill with blank records.
        assert!(staging[TRANSPORT_RECORDS].iter().all(|byte| *byte == 0));
        assert_eq!(
            staging[..TRANSPORT_RECORDS.start],
            original[..TRANSPORT_RECORDS.start]
        );
        files
            .prepare(&mut original.clone(), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(AFTER).unwrap();
        let bytes = load(&mut files).0;
        assert!(bytes == original);
        assert_eq!(disk.0.borrow().files.len(), 3);
    }

    #[test]
    fn the_main_file_is_needed_and_never_replaced_by_defaults() {
        let disk = fresh(&body(0x31, 0x44, 0));
        let refused = Err(Error::Side(FileName::Main, Problem::Damaged));
        // No main file at all.
        let missing = disk.copy();
        missing.0.borrow_mut().files.remove(&(FileName::Main as u8));
        // Its payload damaged.
        let damaged = disk.copy();
        damaged
            .0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Main as u8))
            .unwrap()[100] ^= 1;
        // A main file from some other Bank is not this save's.
        let foreign = disk.copy();
        let other = fresh(&body(0x77, 0x44, 0));
        let bytes = other.0.borrow().files[&(FileName::Main as u8)].clone();
        foreign
            .0
            .borrow_mut()
            .files
            .insert(FileName::Main as u8, bytes);
        for (name, disk) in [
            ("missing", missing),
            ("damaged", damaged),
            ("foreign", foreign),
        ] {
            let before = disk.0.borrow().files.clone();
            let mut files = opened(&disk);
            assert_eq!(files.read(&mut vec![0; BLOB_SIZE]), refused, "{name}");
            // Nothing was written, and nothing can be saved over it.
            assert!(disk.0.borrow().files == before, "{name}");
        }
        // A header that is not the format the patch knows.
        let mut other_format = body(0x31, 0x44, 0);
        other_format[0x15C] = 9;
        let disk = fresh(&other_format);
        assert_eq!(
            opened(&disk).read(&mut vec![0; BLOB_SIZE]),
            Err(Error::Side(FileName::Main, Problem::Layout))
        );
    }

    #[test]
    fn a_delivery_is_taken_once_and_what_stays_in_the_box_is_kept() {
        let disk = fresh(&body(0x31, 0x44, 0));
        load(&mut opened(&disk));
        deliver(&disk, 12);
        assert!(!transport_ok(&disk));

        // Bank shows the twelve Pokémon; blank records fill the rest.
        let mut files = opened(&disk);
        let (shown, loaded) = load(&mut files);
        assert_eq!(loaded.delivered, 12);
        assert!(shown == body(0x31, 0x44, 12));
        // Leaving without saving changes nothing; they are shown again.
        assert!(load(&mut opened(&disk.reboot())).0 == body(0x31, 0x44, 12));

        // Eight are moved into boxes, four stay in the transport box.
        let after = body(0x52, 0x44, 4);
        files
            .prepare(&mut after.clone(), GAME, BEFORE, AFTER)
            .unwrap();
        // Rolled back: the delivery is whole again.
        let (bytes, loaded) = recovered(&disk, BEFORE);
        assert_eq!(loaded.delivered, 12);
        assert!(bytes == body(0x31, 0x44, 12));
        // Published: four remain and the delivery is gone, with the one
        // record that makes the save current, wherever that is cut.
        for (cut, console) in [0, 1, 2, 3, 4, usize::MAX]
            .into_iter()
            .flat_map(|cut| [(cut, false), (cut, true)])
        {
            let published = disk.like(console).cut_after(cut);
            let mut files = opened(&published);
            let _ = files.reconcile(AFTER);
            let (bytes, loaded) = recovered(&published, AFTER);
            assert_eq!(loaded.delivered, 0, "cut {cut}");
            assert!(bytes == after, "cut {cut}");
            // Pokémon are still in the box: Transporter must wait.
            assert!(!transport_ok(&published.reboot()), "cut {cut}");
        }
    }

    #[test]
    fn a_first_start_cut_at_every_write_is_finished_by_the_next_one() {
        every_cut_of_a_first_start(false);
    }
    #[test]
    fn on_the_console_a_first_start_cut_at_every_write_is_finished_by_the_next_one() {
        every_cut_of_a_first_start(true);
    }
    fn every_cut_of_a_first_start(console: bool) {
        let original = body(0x31, 0x44, 0);
        let create = |disk: &Shared, body: &[u8]| {
            let mut files = BankFiles::new(disk.clone());
            match files.open()? {
                Some(_) => Ok(false),
                None => files.initialize(&mut body.to_vec()).map(|_| true),
            }
        };
        let empty = Shared::default().like(console);
        let counting = empty.cut_after(usize::MAX);
        assert_eq!(create(&counting, &original), Ok(true));
        let total = counting.0.borrow().operations;
        assert!(total > 15);

        let mut finished_later = 0;
        let mut deliveries = 0;
        for cut in 0..=total {
            let disk = empty.cut_after(cut);
            assert_eq!(create(&disk, &original).is_ok(), cut == total, "cut {cut}");
            let disk = disk.reboot();
            // Once the first journal record is there the Bank exists, and
            // Transporter may deliver into it; those Pokémon have left
            // their game.
            let delivered = transport_ok(&disk) && try_deliver(&disk, 5).is_some();
            deliveries += usize::from(delivered);
            // The next start: no Bank yet means it is created now, with the
            // body of that start.
            let second = body(0x32, 0x45, 0);
            let created = create(&disk, &second)
                .unwrap_or_else(|error| panic!("start after cut {cut} cannot finish: {error:?}"));
            finished_later += usize::from(created);
            let expected = if created { &second } else { &original };
            let (bytes, loaded) = load(&mut opened(&disk.reboot()));
            assert!(!loaded.transport_missing, "cut {cut}");
            let shown = if delivered { 5 } else { 0 };
            assert_eq!(loaded.delivered, shown, "cut {cut}");
            let mut with_box = expected.clone();
            let source = body(0, 0, shown as usize);
            with_box[TRANSPORT_RECORDS].copy_from_slice(&source[TRANSPORT_RECORDS]);
            with_box[TRANSPORT_TAGS].copy_from_slice(&source[TRANSPORT_TAGS]);
            assert!(bytes == with_box, "cut {cut}: body differs");
        }
        // Both happened: starts that had to finish the creation, and
        // deliveries into a Bank whose creation was cut at its very end.
        assert!(
            finished_later > 15 && deliveries > 0,
            "{finished_later} {deliveries}"
        );
    }

    /// An occupied record of its own identity; `id` must stay below 0x1000.
    fn pokemon(id: u32) -> [u8; RECORD_SIZE] {
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
    /// A body whose boxes hold exactly `boxed`, each at (box, slot), and
    /// whose transport box holds `transport` Pokémon.
    fn bank(fill: u8, boxed: &[(usize, usize, [u8; RECORD_SIZE])], transport: usize) -> Vec<u8> {
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
        bytes
    }

    #[test]
    fn a_save_in_progress_is_settled_by_what_it_moved_when_the_game_cannot_say() {
        let old = bank(
            0x31,
            &[(0, 0, pokemon(1)), (0, 1, pokemon(2)), (99, 29, pokemon(3))],
            0,
        );
        let disk = fresh(&bank(0, &[], 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut old.clone(), GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile([9; 32]).unwrap();
        assert!(load(&mut files).0 == old);
        let base = disk.reboot();

        // Each session: the boxes it leaves, what it moved, and the snapshot
        // that cannot lose a Pokémon.
        let keep = RecoveryDecision::KeepBefore;
        let commit = RecoveryDecision::CommitAfter;
        let sessions = [
            (
                "deposits only",
                vec![
                    (0, 0, pokemon(1)),
                    (0, 1, pokemon(2)),
                    (99, 29, pokemon(3)),
                    (5, 5, pokemon(4)),
                    (5, 6, pokemon(5)),
                ],
                (2, 0),
                commit,
            ),
            ("withdrawals only", vec![(0, 0, pokemon(1))], (0, 2), keep),
            (
                "both ways",
                vec![(0, 0, pokemon(1)), (0, 1, pokemon(2)), (3, 3, pokemon(9))],
                (1, 1),
                commit,
            ),
            (
                "only rearranged in Bank",
                vec![(7, 7, pokemon(3)), (7, 8, pokemon(1)), (50, 0, pokemon(2))],
                (0, 0),
                keep,
            ),
        ];
        for (name, boxes, expected, decision) in sessions {
            let new = bank(0x52, &boxes, 0);
            let prepared = base.copy();
            let mut files = opened(&prepared);
            load(&mut files);
            files
                .prepare(&mut new.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            let operations = prepared.0.borrow().operations;

            // The next start: the game shows some save of its own.
            let mut files = BankFiles::new(prepared.reboot());
            assert!(matches!(files.open(), Ok(Some(Phase::Prepared(_)))));
            let mut staging = vec![0xEE; BLOB_SIZE];
            let (mut before, mut after) = (Box::new([[0; 3]; HELD]), Box::new([[0; 3]; HELD]));
            let moved = files
                .pending_moves(&mut staging, &mut before, &mut after)
                .unwrap();
            assert_eq!((moved.deposited, moved.withdrawn), expected, "{name}");
            // Looking wrote nothing, and the save is still in progress.
            assert!(matches!(files.phase(), Ok(Phase::Prepared(_))));
            assert_eq!(moved::choose(moved), decision, "{name}");
            // Without the game the same answer, except for the session
            // that moved Pokémon both ways: that one waits for its game.
            assert_eq!(
                moved::choose_unseen(moved),
                (name != "both ways").then_some(decision),
                "{name}"
            );
            assert_eq!(files.reconcile_as(decision), Ok(decision), "{name}");
            let (bytes, loaded) = load(&mut files);
            let state = if decision == commit { &new } else { &old };
            assert!(bytes == *state, "{name}: body differs");
            assert!(!loaded.transport_missing, "{name}");
            // It stays settled, and the transport box is free again.
            let (again, _) = load(&mut opened(&files.files_mut().reboot()));
            assert!(again == *state, "{name}: after a restart");
            assert!(transport_ok(files.files_mut()), "{name}");
            assert!(operations > 0);
        }
    }

    #[test]
    fn a_taken_delivery_and_a_release_are_counted_for_what_they_are() {
        let old = bank(0x31, &[(0, 0, pokemon(1)), (0, 1, pokemon(2))], 0);
        let disk = fresh(&bank(0, &[], 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut old.clone(), GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile([9; 32]).unwrap();
        let base = disk.reboot();
        let count = |disk: &Shared| {
            let mut files = BankFiles::new(disk.reboot());
            files.open().unwrap();
            let mut staging = vec![0; BLOB_SIZE];
            let (mut before, mut after) = (Box::new([[0; 3]; HELD]), Box::new([[0; 3]; HELD]));
            let moved = files
                .pending_moves(&mut staging, &mut before, &mut after)
                .unwrap();
            (moved.deposited, moved.withdrawn)
        };

        // Transporter delivered five; the session moved three of them into
        // boxes and left two in the transport box. Nothing came from or went
        // to the game.
        let delivered = base.copy();
        deliver(&delivered, 5);
        let mut files = opened(&delivered);
        assert_eq!(load(&mut files).1.delivered, 5);
        let source = body(0, 0, 5);
        let from_box = |slot: usize| -> [u8; RECORD_SIZE] {
            let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
            source[at..at + RECORD_SIZE].try_into().unwrap()
        };
        // `body` keeps the first Pokémon in the transport box, so the two
        // that stay are the first two; the other three go into boxes.
        let moved_in = bank(
            0x52,
            &[
                (0, 0, pokemon(1)),
                (0, 1, pokemon(2)),
                (4, 0, from_box(2)),
                (4, 1, from_box(3)),
                (4, 2, from_box(4)),
            ],
            2,
        );
        files
            .prepare(&mut moved_in.clone(), GAME, BEFORE, AFTER)
            .unwrap();
        assert_eq!(count(&delivered), (0, 0));

        // The same with one of the Bank's own Pokémon deposited beside it.
        let delivered = base.copy();
        deliver(&delivered, 5);
        let mut files = opened(&delivered);
        load(&mut files);
        let mut with_deposit = moved_in.clone();
        let at = 0x17C + 9 * 0x1B56;
        with_deposit[at..at + RECORD_SIZE].copy_from_slice(&pokemon(7));
        files
            .prepare(&mut with_deposit, GAME, BEFORE, AFTER)
            .unwrap();
        assert_eq!(count(&delivered), (1, 0));

        // A release leaves Bank like a withdrawal: the old boxes keep it.
        let released = base.copy();
        let mut files = opened(&released);
        load(&mut files);
        files
            .prepare(
                &mut bank(0x52, &[(0, 0, pokemon(1))], 0),
                GAME,
                BEFORE,
                AFTER,
            )
            .unwrap();
        assert_eq!(count(&released), (0, 1));

        // Without a save in progress there is nothing to compare.
        let mut files = opened(&base);
        let (mut before, mut after) = (Box::new([[0; 3]; HELD]), Box::new([[0; 3]; HELD]));
        assert_eq!(
            files.pending_moves(&mut vec![0; BLOB_SIZE], &mut before, &mut after),
            Err(Error::Side(FileName::Bank, Problem::NotReady))
        );
        assert_eq!(
            files.reconcile_as(RecoveryDecision::KeepBefore),
            Err(Error::Side(FileName::Bank, Problem::NotReady))
        );
    }

    #[test]
    fn a_bank_file_that_held_a_bank_is_never_created_anew() {
        let disk = fresh(&body(0x31, 0x44, 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut body(0x52, 0x44, 0), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(AFTER).unwrap();
        drop(files);
        let intact = disk.copy();
        // Both journal records are lost.
        disk.0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Bank as u8))
            .unwrap()[..2 * offline_core::METADATA_SIZE]
            .fill(0);
        let lost = disk.reboot();
        let before = lost.0.borrow().files.clone();
        let mut files = BankFiles::new(lost.clone());
        assert!(matches!(files.open(), Err(Error::Bank(_))));
        assert!(matches!(
            files.initialize(&mut body(0, 0, 0)),
            Err(Error::Bank(_))
        ));
        // Nothing was written beside it either.
        assert!(lost.0.borrow().files == before);
        // Nor is a Bank that is there ever created over.
        let mut files = BankFiles::new(intact);
        assert_eq!(
            files.initialize(&mut body(0, 0, 0)),
            Err(Error::Side(FileName::Bank, Problem::NotReady))
        );
    }

    #[test]
    fn an_emptied_box_accepts_the_next_delivery_and_never_repeats_the_old_one() {
        let disk = fresh(&body(0x31, 0x44, 0));
        load(&mut opened(&disk));
        deliver(&disk, 3);
        let mut files = opened(&disk);
        load(&mut files);
        // All three are moved out before saving.
        let emptied = body(0x52, 0x44, 0);
        files
            .prepare(&mut emptied.clone(), GAME, BEFORE, AFTER)
            .unwrap();
        // Power is lost right after the game was saved, before any cleanup.
        let (bytes, loaded) = recovered(&disk, AFTER);
        assert_eq!(loaded.delivered, 0);
        assert!(bytes == emptied);

        // While the save is in progress Transporter waits. The record that
        // makes it current also says that nothing waits any more: the box
        // is free at once, and loads show it empty.
        let disk = disk.reboot();
        assert!(!transport_ok(&disk));
        let mut files = opened(&disk);
        files.reconcile(AFTER).unwrap();
        for _ in 0..3 {
            let (bytes, loaded) = load(&mut files);
            assert_eq!(loaded.delivered, 0);
            assert!(bytes == emptied);
        }
        assert!(transport_ok(&disk));
        deliver(&disk, 30);
        // The next session: the one before has closed its files.
        drop(files);
        let (bytes, loaded) = load(&mut opened(&disk));
        assert_eq!(loaded.delivered, 30);
        assert!(bytes == body(0x52, 0x44, 30));
    }

    #[test]
    fn loading_never_writes_any_file_in_any_transport_state() {
        // Each state is produced, then loaded repeatedly on a write-counting disk.
        let waiting = fresh(&body(0x31, 0x44, 0));
        deliver(&waiting, 12);

        let rolled_back = waiting.copy();
        let mut files = opened(&rolled_back);
        load(&mut files);
        files
            .prepare(&mut body(0x52, 0x44, 4), GAME, BEFORE, AFTER)
            .unwrap();

        drop(files);
        let in_progress = rolled_back.copy();
        let published = rolled_back.copy();
        opened(&published).reconcile(AFTER).unwrap();
        opened(&rolled_back).reconcile(BEFORE).unwrap();

        for (name, disk, delivered, pokemon) in [
            ("delivery waiting", &waiting, 12, 12),
            (
                "save rolled back, its box left behind",
                &rolled_back,
                12,
                12,
            ),
            ("save published", &published, 0, 4),
        ] {
            let disk = disk.reboot();
            let before = disk.0.borrow().files.clone();
            for _ in 0..3 {
                let (bytes, loaded) = load(&mut opened(&disk));
                assert_eq!(loaded.delivered, delivered, "{name}");
                assert_eq!(sections::transport_count(&bytes), Ok(pokemon), "{name}");
            }
            assert_eq!(disk.0.borrow().operations, 0, "{name}: a load wrote");
            assert!(disk.0.borrow().files == before, "{name}: a file changed");
        }
        // In each of them Pokémon are in the way, or the save is not
        // settled: Transporter waits.
        for disk in [&waiting, &rolled_back, &in_progress, &published] {
            assert!(!transport_ok(disk));
        }
    }

    #[test]
    fn nothing_is_delivered_before_a_bank_exists() {
        // Not even into files that a creation which was cut left behind.
        let disk = Shared::default();
        assert!(try_deliver(&disk, 2).is_none());
        drop(disk.clone().create(FileName::Transport).unwrap());
        drop(disk.clone().create(FileName::Bank).unwrap());
        assert!(!transport_ok(&disk));
        assert!(try_deliver(&disk, 2).is_none());
        let mut files = BankFiles::new(disk.clone());
        assert_eq!(files.open(), Ok(None));
        files.initialize(&mut body(0x31, 0x44, 0)).unwrap();
        let (bytes, loaded) = load(&mut opened(&disk.reboot()));
        assert_eq!(loaded.delivered, 0);
        assert!(bytes == body(0x31, 0x44, 0));
        // From then on there is a box to deliver into.
        assert!(transport_ok(&disk));
    }

    #[test]
    fn a_damaged_delivery_is_not_shown_and_not_saved_over() {
        let disk = fresh(&body(0x31, 0x44, 0));
        load(&mut opened(&disk));
        let delivery = deliver(&disk, 5);
        let at = delivery.slot.index() as usize * TRANSPORT_SIZE;
        disk.0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Transport as u8))
            .unwrap()[at + 3] ^= 1;
        let damaged = disk.0.borrow().files.clone();
        let mut files = opened(&disk);
        let (bytes, loaded) = load(&mut files);
        assert_eq!((loaded.delivered, loaded.transport_missing), (0, true));
        assert!(bytes == body(0x31, 0x44, 0));
        // A save would say that nothing waits any more: it is refused, and
        // the delivery stays where it is.
        assert_eq!(
            files.prepare(&mut body(0x52, 0x44, 0), GAME, BEFORE, AFTER),
            Err(Error::Side(FileName::Transport, Problem::Conflict))
        );
        assert!(disk.0.borrow().files == damaged);
        assert!(!transport_ok(&disk));
    }

    #[test]
    fn a_transport_box_that_holds_pokemon_is_needed() {
        let disk = fresh(&body(0x31, 0x44, 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut body(0x31, 0x44, 3), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(AFTER).unwrap();
        let Phase::Clean(current) = files.phase().unwrap() else {
            unreachable!()
        };
        assert_eq!(current.held, 3);
        drop(files);
        let missing = disk.copy();
        missing
            .0
            .borrow_mut()
            .files
            .remove(&(FileName::Transport as u8));
        let damaged = disk.copy();
        damaged
            .0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Transport as u8))
            .unwrap()[current.mover.slot.index() as usize * TRANSPORT_SIZE + 9] ^= 1;
        for (name, disk) in [("missing", missing), ("damaged", damaged)] {
            let before = disk.0.borrow().files.clone();
            assert_eq!(
                opened(&disk).read(&mut vec![0; BLOB_SIZE]),
                Err(Error::Side(FileName::Transport, Problem::Damaged)),
                "{name}"
            );
            assert!(disk.0.borrow().files == before, "{name}");
        }
        // The other copy is not this save's: damage there changes nothing.
        let spare = disk.copy();
        spare
            .0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Transport as u8))
            .unwrap()[current.mover.slot.other().index() as usize * TRANSPORT_SIZE + 9] ^= 1;
        assert!(load(&mut opened(&spare)).0 == body(0x31, 0x44, 3));
    }

    #[test]
    fn a_cut_at_every_write_of_a_delivery_leaves_none_or_the_whole_one() {
        every_cut_of_a_delivery(false);
    }
    #[test]
    fn on_the_console_a_cut_at_every_write_of_a_delivery_leaves_none_or_the_whole_one() {
        every_cut_of_a_delivery(true);
    }
    fn every_cut_of_a_delivery(console: bool) {
        let base = fresh(&body(0x31, 0x44, 0)).like(console);
        let counting = base.cut_after(usize::MAX);
        deliver(&counting, 7);
        // The records and the tags of the box and their sync, then each
        // journal record and its sync.
        let total = counting.0.borrow().operations;
        assert_eq!(total, 7);
        let mut whole = 0;
        for cut in 0..=total {
            let disk = base.cut_after(cut);
            let done = try_deliver(&disk, 7).is_some();
            assert!(done || cut < total, "cut {cut}");
            let disk = disk.reboot();
            // Transporter's check is exactly where a delivery would refuse.
            assert_eq!(
                transport_ok(&disk),
                try_deliver(&disk.copy(), 3).is_some(),
                "cut {cut}"
            );
            // Bank shows no delivery or all of it, and writes nothing.
            let (bytes, loaded) = load(&mut opened(&disk));
            let shown = loaded.delivered as usize;
            assert!(shown == 0 || shown == 7, "cut {cut}: {shown}");
            // What Transporter was told is what Bank shows: "done" from the
            // first durable journal record on, also when the second one
            // was cut. Only the simple card can keep a record whose sync
            // was cut.
            assert!(!done || shown == 7, "cut {cut}");
            assert!(done || shown == 0 || !console, "cut {cut}");
            assert!(bytes == body(0x31, 0x44, shown), "cut {cut}: body differs");
            assert_eq!(disk.0.borrow().operations, 0, "cut {cut}: a load wrote");
            whole += usize::from(shown == 7);
            // Whatever the cut left, Bank's next save goes through, and
            // after it Transporter delivers again.
            let mut files = opened(&disk);
            load(&mut files);
            let emptied = body(0x52, 0x44, 0);
            files
                .prepare(&mut emptied.clone(), GAME, BEFORE, AFTER)
                .unwrap_or_else(|error| panic!("cut {cut}: the next save: {error:?}"));
            files.reconcile(AFTER).unwrap();
            assert!(load(&mut files).0 == emptied, "cut {cut}");
            drop(files);
            deliver(&disk, 2);
            let (bytes, loaded) = load(&mut opened(&disk.reboot()));
            assert_eq!(loaded.delivered, 2, "cut {cut}");
            assert!(bytes == body(0x52, 0x44, 2), "cut {cut}");
        }
        // The delivery exists from the first journal record on: once it
        // is written, and on the console once it is synced.
        assert_eq!(whole, if console { 3 } else { 4 });
    }

    /// What the end of a session does: the Bank file is given back and
    /// closed. Nothing of Bank's is open afterwards.
    fn end_session(files: &mut BankFiles<Shared>, disk: &Shared) {
        drop(files.close());
        assert_eq!(disk.0.borrow().open, 0);
    }

    #[test]
    fn each_session_of_one_run_loads_what_the_one_before_saved() {
        let first = body(0x31, 0x44, 0);
        let second = body(0x52, 0x66, 0);
        let third = body(0x73, 0x11, 0);
        let disk = fresh(&first);
        // One worker for the whole run: the same object opens every session.
        let mut files = BankFiles::new(disk.clone());
        let mut expected = first;
        for next in [second, third] {
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
            let bytes = load(&mut files).0;
            assert!(bytes == expected);
            files
                .prepare(&mut next.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            files.reconcile(AFTER).unwrap();
            end_session(&mut files, &disk);
            expected = next;
        }
        assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
        let bytes = load(&mut files).0;
        assert!(bytes == expected);
        // A session that only looks is closed the same way.
        end_session(&mut files, &disk);
        assert!(!files.is_open());
    }

    #[test]
    fn a_bank_that_rests_between_jobs_holds_nothing_open_and_keeps_the_same_bytes() {
        let old = body(0x31, 0x44, 0);
        let new = body(0x52, 0x66, 0);
        let mut stored = Vec::new();
        for rests in [false, true] {
            let disk = fresh(&old);
            let mut files = BankFiles::new(disk.clone());
            // What the worker does after every job.
            let between_jobs = |files: &mut BankFiles<Shared>| {
                if rests {
                    files.rest().unwrap();
                    assert_eq!(disk.0.borrow().open, 0);
                }
            };
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
            between_jobs(&mut files);
            assert!(load(&mut files).0 == old);
            between_jobs(&mut files);
            files
                .prepare(&mut new.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            between_jobs(&mut files);
            assert!(matches!(files.phase(), Ok(Phase::Prepared(_))));
            files.reconcile(AFTER).unwrap();
            assert!(load(&mut files).0 == new);
            between_jobs(&mut files);
            // A session that rests is closed as any other.
            end_session(&mut files, &disk);
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
            assert!(load(&mut files).0 == new);
            end_session(&mut files, &disk);
            stored.push(disk.0.borrow().files.clone());
        }
        assert!(stored[0] == stored[1]);
    }

    #[test]
    fn a_session_that_failed_is_closed_all_the_same() {
        let old = body(0x31, 0x44, 0);
        let base = fresh(&old);
        let counting = base.cut_after(usize::MAX);
        let mut files = opened(&counting);
        load(&mut files);
        files
            .prepare(&mut body(0x52, 0x66, 0), GAME, BEFORE, AFTER)
            .unwrap();
        let total = counting.0.borrow().operations;
        drop(files);
        for cut in 0..total {
            let disk = base.cut_after(cut);
            let mut files = BankFiles::new(disk.clone());
            let failed = files.open().is_err()
                || files.read(&mut vec![0; BLOB_SIZE]).is_err()
                || files
                    .prepare(&mut body(0x52, 0x66, 0), GAME, BEFORE, AFTER)
                    .is_err();
            assert!(failed, "cut {cut}");
            end_session(&mut files, &disk);
        }
    }

    #[test]
    fn a_save_in_progress_is_settled_after_closing_as_after_a_restart() {
        let old = body(0x31, 0x44, 0);
        let new = body(0x52, 0x66, 0);
        for (game, kept) in [(BEFORE, &old), (AFTER, &new)] {
            let disk = fresh(&old);
            let mut files = opened(&disk);
            load(&mut files);
            files
                .prepare(&mut new.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            // The session ends before the game answered.
            end_session(&mut files, &disk);
            assert!(matches!(files.open(), Ok(Some(Phase::Prepared(_)))));
            files.reconcile(game).unwrap();
            let (bytes, _) = load(&mut files);
            assert!(&bytes == kept);
            // A restart at the same point comes to the same Bank.
            let restarted = fresh(&old);
            let mut first = opened(&restarted);
            load(&mut first);
            first
                .prepare(&mut new.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            drop(first);
            assert!(recovered(&restarted, game).0 == bytes);
        }
    }

    #[test]
    fn a_save_that_waits_for_its_game_writes_nothing_and_is_settled_once_it_is_there() {
        // A session that deposited one Pokémon and withdrew another.
        let old = bank(0x31, &[(0, 0, pokemon(1)), (0, 1, pokemon(2))], 0);
        let new = bank(0x52, &[(0, 0, pokemon(1)), (3, 3, pokemon(9))], 0);
        for (game, kept) in [(BEFORE, &old), (AFTER, &new)] {
            let disk = fresh(&bank(0, &[], 0));
            let mut files = BankFiles::new(disk.clone());
            files.open().unwrap();
            load(&mut files);
            files
                .prepare(&mut old.clone(), GAME, [8; 32], [9; 32])
                .unwrap();
            files.reconcile([9; 32]).unwrap();
            assert!(load(&mut files).0 == old);
            files
                .prepare(&mut new.clone(), GAME, BEFORE, AFTER)
                .unwrap();
            end_session(&mut files, &disk);

            // START without the game, as often as it is pressed: the save
            // waits, nothing is written, and the session ends like any.
            for _ in 0..3 {
                assert!(matches!(files.open(), Ok(Some(Phase::Prepared(_)))));
                let written = disk.0.borrow().operations;
                let (mut before, mut after) = (Box::new([[0; 3]; HELD]), Box::new([[0; 3]; HELD]));
                let moved = files
                    .pending_moves(&mut vec![0; BLOB_SIZE], &mut before, &mut after)
                    .unwrap();
                assert_eq!((moved.deposited, moved.withdrawn), (1, 1));
                assert_eq!(moved::choose_unseen(moved), None);
                assert_eq!(disk.0.borrow().operations, written);
                assert!(matches!(files.phase(), Ok(Phase::Prepared(_))));
                end_session(&mut files, &disk);
            }

            // START with the game there, in the same run: its save decides.
            assert!(matches!(files.open(), Ok(Some(Phase::Prepared(_)))));
            files.reconcile(game).unwrap();
            let (bytes, _) = load(&mut files);
            assert!(&bytes == kept);
            end_session(&mut files, &disk);
            assert!(matches!(files.open(), Ok(Some(Phase::Clean(_)))));
        }
    }

    /// The files of the current journal version, by name and length. A
    /// change here is a new version of the stored files: raise
    /// offline-core's version to the next whole number, and give the
    /// migration a folder for the files as they were (docs/building.md).
    #[test]
    fn the_files_are_those_of_this_journal_version() {
        assert_eq!(offline_core::SCHEMA_VERSION, 1);
        let files: Vec<(&str, u64)> = [FileName::Bank, FileName::Main, FileName::Transport]
            .into_iter()
            .flat_map(FileName::units)
            .map(|unit| (unit.path, unit.len))
            .collect();
        assert_eq!(
            files,
            [
                ("/journal.bin", 140),
                ("/journal.alt.bin", 140),
                ("/bank.bin", 729_800),
                ("/bank.alt.bin", 729_800),
                ("/main.bin", 30_466),
                ("/main.alt.bin", 30_466),
                ("/mover.bin", 6_990),
                ("/mover.alt.bin", 6_990),
            ]
        );
    }
}
