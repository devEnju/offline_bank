//! The four stored files and the order in which they are read and written.
//!
//! `/bank.bin` carries the journal and decides which save is current.
//! The Pokédex, transport box, and rewards files each hold two slots tagged
//! with a Bank snapshot (`offline_core::sidecar`). A save writes every side file's
//! spare slot first, then prepares the Bank journal; a load uses the slots
//! whose tag matches the Bank's current snapshot. So all four always describe
//! the same save, without a second journal.
//!
//! This module is generic over storage so the whole sequence is host-tested.
//! In memory the main thread only ever sees the one native body.

use crate::session::{self, BankSession};
use offline_core::{
    moved::{self, Moved},
    native_blob::BLOB_SIZE,
    rewards::{self, Stored},
    sections::{
        self, Identity, BANK_SIZE, DEX, DEX_FILE, DEX_SIZE, HELD, MILES, RECORD_SIZE,
        TRANSPORT_RECORDS, TRANSPORT_SIZE, TRANSPORT_SLOTS, TRANSPORT_TAGS,
    },
    sidecar::{matching, spare, Kind, Sidecar, SidecarError, Tag},
    transport, Fingerprint, GameIdentity, GameObservation, Phase, RecoveryDecision, Storage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FileName {
    Bank = 0,
    Dex = 1,
    Transport = 2,
    Rewards = 3,
}
impl FileName {
    /// Exact allocation of each file. A file of another size is an error.
    pub fn size(self) -> u64 {
        match self {
            Self::Bank => session::BANK_LAYOUT.file_len(),
            Self::Dex => DEX_FILE.file_len(),
            Self::Transport => transport::KIND.file_len(),
            Self::Rewards => rewards::FILE.file_len(),
        }
    }
    /// Longest file name the 3DS stores in save data and extdata, without the
    /// leading `/`. A longer name makes creating the file fail on the console
    /// (`E0E046C7`), which no PC test can show.
    pub const MAX_NAME: usize = 16;
    /// Logical path inside SD extdata archive `0x00000C9B`.
    pub const fn path(self) -> &'static str {
        let path = self.raw_path();
        assert!(path.len() - 1 <= Self::MAX_NAME);
        path
    }
    const fn raw_path(self) -> &'static str {
        match self {
            Self::Bank => "/bank.bin",
            Self::Dex => "/dex.bin",
            Self::Transport => "/transport.bin",
            Self::Rewards => "/rewards.bin",
        }
    }
}

/// Access to the files by name. `open` returns `None` only when the file is
/// reported absent; `create` makes an exclusively new, zero-filled file.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Problem {
    /// The slot for the current Bank snapshot is unreadable.
    Damaged = 1,
    /// The file exists but holds nothing for the current Bank snapshot.
    NoMatch = 2,
    /// A waiting delivery would be overwritten.
    Conflict = 3,
    /// The staged body or prepared snapshot is not what was computed.
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

/// What a load found beside the Bank regions now in the staging buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub rewards: Stored,
    /// No Pokédex file: the staging region is zero and must be filled with
    /// the native defaults by the main thread.
    pub dex_missing: bool,
    /// No transport box: every slot must be filled with the blank record.
    pub transport_missing: bool,
    /// Nonzero when a Transporter delivery became the box: its Pokémon count.
    /// The delivered box is complete and is shown as it is.
    pub delivered: u32,
}

pub struct BankFiles<F: Files> {
    files: F,
    bank: Option<BankSession<F::Storage>>,
    /// The delivery taken at the last load, as (slot, id).
    adopted: Option<(usize, u32)>,
}

fn side<E>(file: FileName) -> impl Fn(SidecarError<E>) -> Error<E> {
    move |error| match error {
        SidecarError::Io(error) => Error::Io(error),
        _ => Error::Side(file, Problem::Damaged),
    }
}

impl<F: Files> BankFiles<F> {
    pub const fn new(files: F) -> Self {
        Self {
            files,
            bank: None,
            adopted: None,
        }
    }
    pub fn files_mut(&mut self) -> &mut F {
        &mut self.files
    }
    pub fn is_open(&self) -> bool {
        self.bank.is_some()
    }
    /// Releases the Bank file. Side files are never held open.
    pub fn close(&mut self) -> Option<F::Storage> {
        self.adopted = None;
        self.bank.take().map(BankSession::into_storage)
    }

    fn bank(&mut self) -> Outcome<&mut BankSession<F::Storage>, F> {
        self.bank
            .as_mut()
            .ok_or(Error::Side(FileName::Bank, Problem::NotReady))
    }
    fn open_side(&mut self, file: FileName, kind: Kind) -> Outcome<Option<Sidecar<F::Storage>>, F> {
        Ok(self
            .files
            .open(file)
            .map_err(Error::Io)?
            .map(|storage| Sidecar::new(storage, kind)))
    }
    fn ensure_side(&mut self, file: FileName, kind: Kind) -> Outcome<Sidecar<F::Storage>, F> {
        match self.open_side(file, kind)? {
            Some(sidecar) => Ok(sidecar),
            None => Ok(Sidecar::new(
                self.files.create(file).map_err(Error::Io)?,
                kind,
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

    /// Writes the side slots for `next` from the full body in `staging`, then
    /// compacts `staging[..BANK_SIZE]` into the Bank payload.
    fn write_sides(
        &mut self,
        staging: &mut [u8],
        current: Option<Tag>,
        generation: u64,
        stored: Stored,
    ) -> Outcome<Tag, F> {
        if staging.len() != BLOB_SIZE {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        // The balance belongs to the rewards file alone.
        staging[MILES].fill(0);
        let layout = |_| Error::Side(FileName::Bank, Problem::Layout);
        let next = Tag {
            generation,
            crc: sections::bank_crc(staging).map_err(layout)?,
        };

        let mut dex = self.ensure_side(FileName::Dex, DEX_FILE)?;
        let slots = dex.slots().map_err(side(FileName::Dex))?;
        dex.write(spare(&slots, current), Some(next), 0, 0, &[&staging[DEX]])
            .map_err(side(FileName::Dex))?;
        drop(dex);

        let mut file = self.ensure_side(FileName::Rewards, rewards::FILE)?;
        let slots = file.slots().map_err(side(FileName::Rewards))?;
        file.write(
            spare(&slots, current),
            Some(next),
            0,
            0,
            &[&stored.encode()],
        )
        .map_err(side(FileName::Rewards))?;
        drop(file);

        let mut file = self.ensure_side(FileName::Transport, transport::KIND)?;
        let slots = file.slots().map_err(side(FileName::Transport))?;
        let conflict = Error::Side(FileName::Transport, Problem::Conflict);
        let (index, aux) = match current {
            Some(current) => {
                transport::on_save(&slots, current, self.adopted).map_err(|_| conflict)?
            }
            // A brand-new Bank: keep a delivery that arrived before it existed.
            None => (
                (0..slots.len())
                    .find(|&index| !transport::is_delivery(&slots[index]))
                    .ok_or(conflict)?,
                0,
            ),
        };
        let count = sections::transport_count(staging).map_err(layout)?;
        file.write(
            index,
            Some(next),
            aux,
            count,
            &[&staging[TRANSPORT_RECORDS], &staging[TRANSPORT_TAGS]],
        )
        .map_err(side(FileName::Transport))?;
        drop(file);

        sections::compact(staging).map_err(layout)?;
        Ok(next)
    }

    /// Creates a new Bank from the full native body in `staging`. Afterwards
    /// `staging` holds the compact Bank payload, not a body. A Bank file left
    /// by a creation that was cut is finished in place; the store refuses
    /// that for any file in which a Bank was ever current.
    pub fn initialize(&mut self, staging: &mut [u8]) -> Outcome<(), F> {
        if self.bank.is_some() {
            return Err(Error::Side(FileName::Bank, Problem::NotReady));
        }
        self.adopted = None;
        self.write_sides(staging, None, 1, Stored::NONE)?;
        let payload = &staging[..BANK_SIZE];
        let session = match self.files.open(FileName::Bank).map_err(Error::Io)? {
            Some(storage) => BankSession::reinitialize_bytes(storage, payload),
            None => {
                let storage = self.files.create(FileName::Bank).map_err(Error::Io)?;
                BankSession::initialize_bytes(storage, payload)
            }
        };
        self.bank = Some(session.map_err(Error::Bank)?);
        Ok(())
    }

    /// Assembles the current save in `staging` as one native body: one read
    /// per file, each into its final place and checked on that pass. Nothing
    /// is written to any file.
    pub fn read(&mut self, staging: &mut [u8]) -> Outcome<Loaded, F> {
        if staging.len() != BLOB_SIZE {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        let tag = self.bank()?.read_bank(staging).map_err(Error::Bank)?;
        sections::expand(staging).map_err(|_| Error::Side(FileName::Bank, Problem::Layout))?;
        let mut loaded = Loaded {
            rewards: Stored::NONE,
            dex_missing: true,
            transport_missing: true,
            delivered: 0,
        };
        self.adopted = None;

        if let Some(mut file) = self.open_side(FileName::Dex, DEX_FILE)? {
            let slots = file.slots().map_err(side(FileName::Dex))?;
            let index =
                matching(&slots, tag).ok_or(Error::Side(FileName::Dex, Problem::NoMatch))?;
            let slot = slots[index].filter(|slot| slot.len as usize == DEX_SIZE);
            let slot = slot.ok_or(Error::Side(FileName::Dex, Problem::Damaged))?;
            file.read(index, &slot, &mut staging[DEX])
                .map_err(side(FileName::Dex))?;
            loaded.dex_missing = false;
        }

        // A missing or unreadable Miles record never blocks the Bank.
        if let Some(mut file) = self.open_side(FileName::Rewards, rewards::FILE)? {
            let slots = file.slots().map_err(side(FileName::Rewards))?;
            if let Some(index) = matching(&slots, tag) {
                let slot = slots[index].filter(|slot| slot.len as usize == rewards::RECORD_SIZE);
                let mut bytes = [0; rewards::RECORD_SIZE];
                if let Some(slot) = slot {
                    match file.read(index, &slot, &mut bytes) {
                        Ok(()) => loaded.rewards = Stored::decode(&bytes).unwrap_or(Stored::NONE),
                        Err(SidecarError::Io(error)) => return Err(Error::Io(error)),
                        Err(_) => {}
                    }
                }
            }
        }

        if let Some(mut file) = self.open_side(FileName::Transport, transport::KIND)? {
            let slots = file.slots().map_err(side(FileName::Transport))?;
            let plan = transport::on_load(&slots, tag);
            if let Some(index) = plan.source {
                let slot = slots[index].filter(|slot| slot.len as usize == TRANSPORT_SIZE);
                let slot = slot.ok_or(Error::Side(FileName::Transport, Problem::Damaged))?;
                let (low, high) = staging.split_at_mut(TRANSPORT_TAGS.start);
                let read = file.read_split(
                    index,
                    &slot,
                    &mut low[TRANSPORT_RECORDS],
                    &mut high[..TRANSPORT_TAGS.len()],
                );
                match (read, plan.adopted) {
                    (Ok(()), Some((id, count))) => {
                        self.adopted = Some((index, id));
                        loaded.delivered = count;
                        loaded.transport_missing = false;
                    }
                    (Ok(()), None) => loaded.transport_missing = false,
                    (Err(SidecarError::Io(error)), _) => return Err(Error::Io(error)),
                    // A damaged delivery is left alone and not shown.
                    (Err(_), Some(_)) => {
                        low[TRANSPORT_RECORDS].fill(0);
                        high[..TRANSPORT_TAGS.len()].fill(0);
                    }
                    (Err(_), None) => {
                        return Err(Error::Side(FileName::Transport, Problem::Damaged))
                    }
                }
            }
        }
        Ok(loaded)
    }

    /// Removes transport slots that the save just published made obsolete: a
    /// delivery it took and the previous copy of the box. This is the only
    /// place slots are removed, and it belongs to Save and Quit; a load never
    /// writes. Until it has run, Transporter sees two valid slots and waits.
    pub fn tidy_transport(&mut self) -> Outcome<(), F> {
        let tag = self.bank()?.tag().map_err(Error::Bank)?;
        if let Some(mut file) = self.open_side(FileName::Transport, transport::KIND)? {
            let slots = file.slots().map_err(side(FileName::Transport))?;
            let plan = transport::on_load(&slots, tag);
            for (index, void) in plan.void.into_iter().enumerate() {
                if void {
                    file.clear(index).map_err(side(FileName::Transport))?;
                }
            }
        }
        Ok(())
    }

    /// Makes the next save durable up to the game write: side slots first,
    /// then the Bank journal. `staging` holds the full native body and is
    /// compacted. The native game writer MUST NOT run if this fails.
    pub fn prepare(
        &mut self,
        staging: &mut [u8],
        stored: Stored,
        game: GameIdentity,
        before: Fingerprint,
        after: Fingerprint,
    ) -> Outcome<(), F> {
        let current = self.bank()?.tag().map_err(Error::Bank)?;
        let next = self.write_sides(staging, Some(current), current.generation + 1, stored)?;
        let prepared = self
            .bank()?
            .prepare_bytes(&staging[..BANK_SIZE], game, before, after)
            .map_err(Error::Bank)?;
        if prepared != next {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        Ok(())
    }

    /// Resolves a prepared save from the game actually observed. Follow it
    /// with `tidy_transport` and `read`.
    pub fn reconcile(&mut self, observed: GameObservation) -> Outcome<RecoveryDecision, F> {
        self.bank()?.reconcile_game(observed).map_err(Error::Bank)
    }

    /// What the prepared save moved between Bank and the game: everything
    /// the Bank held before it, in its boxes and in the transport box it
    /// showed, against everything it holds after it. `staging` is used for
    /// one snapshot at a time and holds nothing of use afterwards; the two
    /// lists are working room. Nothing is written.
    pub fn pending_moves(
        &mut self,
        staging: &mut [u8],
        before: &mut [Identity; HELD],
        after: &mut [Identity; HELD],
    ) -> Outcome<Moved, F> {
        let Phase::Prepared(pending) = self.phase()? else {
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
        // The transport box: before, what a load showed (its own box, or a
        // delivery it took); after, the box the save wrote.
        if let Some(mut file) = self.open_side(FileName::Transport, transport::KIND)? {
            let slots = file.slots().map_err(side(FileName::Transport))?;
            let shown = transport::on_load(&slots, Tag::of(&pending.before)).source;
            let written = matching(&slots, Tag::of(&pending.after));
            let records = &mut staging[..TRANSPORT_SIZE];
            for (index, list, count) in [(shown, &mut *before, 0), (written, &mut *after, 1)] {
                let Some(index) = index else { continue };
                let slot = slots[index].filter(|slot| slot.len as usize == TRANSPORT_SIZE);
                let slot = slot.ok_or(Error::Side(FileName::Transport, Problem::Damaged))?;
                file.read(index, &slot, records)
                    .map_err(side(FileName::Transport))?;
                counts[count] = sections::identities(
                    &records[..TRANSPORT_SLOTS * RECORD_SIZE],
                    list,
                    counts[count],
                );
            }
        }
        Ok(moved::moved(
            &mut before[..counts[0]],
            &mut after[..counts[1]],
        ))
    }

    /// Resolves a prepared save toward the snapshot `decision` names, for a
    /// game whose save is neither of its two images (`moved::choose`). The
    /// journal's own rule is not loosened: this names the image the game is
    /// taken to have been saved from. Follow it with `tidy_transport` and
    /// `read`, as `reconcile`.
    pub fn reconcile_as(&mut self, decision: RecoveryDecision) -> Outcome<RecoveryDecision, F> {
        let Phase::Prepared(pending) = self.phase()? else {
            return Err(Error::Side(FileName::Bank, Problem::NotReady));
        };
        let fingerprint = match decision {
            RecoveryDecision::KeepBefore => pending.before_fingerprint,
            RecoveryDecision::CommitAfter => pending.after_fingerprint,
            RecoveryDecision::Blocked(_) => {
                return Err(Error::Side(FileName::Bank, Problem::NotReady))
            }
        };
        self.reconcile(GameObservation::Present {
            game: pending.game,
            fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use offline_core::{
        rewards::{enter, settle, Accounting, Date},
        sections::RECORD_SIZE,
    };
    use std::{boxed::Box, cell::RefCell, collections::BTreeMap, format, rc::Rc, vec, vec::Vec};

    #[derive(Default)]
    struct Disk {
        files: BTreeMap<u8, Vec<u8>>,
        /// Storage operations allowed before a simulated power cut.
        budget: Option<usize>,
        operations: usize,
    }
    #[derive(Clone, Default)]
    struct Shared(Rc<RefCell<Disk>>);
    struct Handle(Shared, u8);
    impl Shared {
        fn tick(&self) -> Result<(), ()> {
            let mut disk = self.0.borrow_mut();
            if let Some(budget) = &mut disk.budget {
                if *budget == 0 {
                    return Err(());
                }
                *budget -= 1;
            }
            disk.operations += 1;
            Ok(())
        }
        fn copy(&self) -> Self {
            let disk = self.0.borrow();
            Self(Rc::new(RefCell::new(Disk {
                files: disk.files.clone(),
                budget: None,
                operations: 0,
            })))
        }
        fn cut_after(&self, budget: usize) -> Self {
            let copy = self.copy();
            copy.0.borrow_mut().budget = Some(budget);
            copy
        }
        fn reboot(&self) -> Self {
            self.copy()
        }
    }
    impl Storage for Handle {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            let disk = (self.0).0.borrow();
            let file = &disk.files[&self.1];
            out.copy_from_slice(&file[at as usize..at as usize + out.len()]);
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            // A cut write still lands its first half.
            let cut = self.0.tick().is_err();
            let length = if cut { bytes.len() / 2 } else { bytes.len() };
            let mut disk = (self.0).0.borrow_mut();
            let file = disk.files.get_mut(&self.1).unwrap();
            file[at as usize..at as usize + length].copy_from_slice(&bytes[..length]);
            if cut {
                Err(())
            } else {
                Ok(())
            }
        }
        fn sync(&mut self) -> Result<(), ()> {
            self.0.tick()
        }
    }
    impl Files for Shared {
        type Storage = Handle;
        fn open(&mut self, file: FileName) -> Result<Option<Handle>, ()> {
            let disk = self.0.borrow();
            Ok(disk.files.get(&(file as u8)).map(|bytes| {
                assert_eq!(bytes.len() as u64, file.size());
                Handle(self.clone(), file as u8)
            }))
        }
        fn create(&mut self, file: FileName) -> Result<Handle, ()> {
            self.tick()?;
            let mut disk = self.0.borrow_mut();
            assert!(!disk.files.contains_key(&(file as u8)));
            disk.files.insert(file as u8, vec![0; file.size() as usize]);
            Ok(Handle(self.clone(), file as u8))
        }
    }

    const GAME: GameIdentity = GameIdentity {
        title_id: 0x0004_0000_0005_5e00,
        save_identity: [7; 32],
    };
    const BEFORE: Fingerprint = [1; 32];
    const AFTER: Fingerprint = [2; 32];
    fn observed(fingerprint: Fingerprint) -> GameObservation {
        GameObservation::Present {
            game: GAME,
            fingerprint,
        }
    }

    /// A stored record whose species word decrypts to `species` (block A first,
    /// shuffle value 0).
    fn record(species: u16) -> [u8; RECORD_SIZE] {
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
    fn body(boxes: u8, dex: u8, pokemon: usize) -> Vec<u8> {
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
    fn date(day: u8) -> Date {
        Date::new(2026, 10, day).unwrap()
    }
    fn stored(balance: u32, day: u8, count: u32) -> Stored {
        Stored {
            balance,
            accounting: Some(Accounting::new(date(day), count, 0).unwrap()),
        }
    }
    /// Fills what the main thread fills: defaults for missing regions.
    fn load(files: &mut BankFiles<Shared>) -> (Vec<u8>, Loaded) {
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
    fn fresh(body: &[u8]) -> Shared {
        let disk = Shared::default();
        let mut files = BankFiles::new(disk.clone());
        assert_eq!(files.open(), Ok(None));
        let mut staging = body.to_vec();
        files.initialize(&mut staging).unwrap();
        disk
    }
    fn opened(disk: &Shared) -> BankFiles<Shared> {
        let mut files = BankFiles::new(disk.clone());
        files.open().unwrap().unwrap();
        files
    }
    /// Opens after a reboot, resolving a pending save from the observed game.
    fn recovered(disk: &Shared, game: Fingerprint) -> (Vec<u8>, Loaded) {
        let mut files = BankFiles::new(disk.reboot());
        if let Phase::Prepared(_) = files.open().unwrap().unwrap() {
            files.reconcile(observed(game)).unwrap();
        }
        load(&mut files)
    }

    #[test]
    fn file_names_and_sizes_are_fixed() {
        let sizes: Vec<_> = [
            FileName::Bank,
            FileName::Dex,
            FileName::Transport,
            FileName::Rewards,
        ]
        .map(|file| (file.path(), file.size()))
        .to_vec();
        assert_eq!(
            sizes,
            [
                ("/bank.bin", 1_461_332),
                ("/dex.bin", 59_776),
                ("/transport.bin", 14_112),
                ("/rewards.bin", 160),
            ]
        );
    }

    #[test]
    fn every_file_name_fits_the_console_limit() {
        for file in [
            FileName::Bank,
            FileName::Dex,
            FileName::Transport,
            FileName::Rewards,
        ] {
            let path = file.path();
            let name = path.strip_prefix('/').unwrap();
            assert!(path.is_ascii() && !name.contains('/'));
            assert!(name.len() <= FileName::MAX_NAME, "{path}");
        }
        assert_eq!(FileName::MAX_NAME, 16);
    }

    #[test]
    fn new_bank_writes_four_files_and_loads_back_every_byte() {
        let original = body(0x31, 0x44, 0);
        let disk = fresh(&original);
        assert_eq!(disk.0.borrow().files.len(), 4);
        let (loaded_body, loaded) = load(&mut opened(&disk));
        assert!(loaded_body == original);
        assert_eq!(
            loaded,
            Loaded {
                rewards: Stored::NONE,
                dex_missing: false,
                transport_missing: false,
                delivered: 0,
            }
        );
    }

    #[test]
    fn miles_balance_lives_only_in_the_rewards_file() {
        let mut with_balance = body(0x31, 0x44, 0);
        with_balance[MILES].copy_from_slice(&999u32.to_le_bytes());
        let disk = fresh(&with_balance);
        let mut files = opened(&disk);
        let (mut staging, loaded) = load(&mut files);
        // The Bank file never keeps the native field.
        assert_eq!(&staging[MILES], &[0; 4]);
        assert_eq!(loaded.rewards, Stored::NONE);

        staging[MILES].copy_from_slice(&777u32.to_le_bytes());
        files
            .prepare(&mut staging, stored(321, 6, 45), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(observed(AFTER)).unwrap();
        let (after, loaded) = load(&mut files);
        assert_eq!(&after[MILES], &[0; 4]);
        assert_eq!(loaded.rewards, stored(321, 6, 45));
    }

    #[test]
    fn a_cut_at_every_write_of_a_save_leaves_one_consistent_save() {
        let old = body(0x31, 0x44, 2);
        let new = body(0x52, 0x66, 1);
        let committed = stored(7, 1, 3000);
        // Establish a committed save that has all four pieces.
        let disk = fresh(&body(0, 0, 0));
        let mut files = opened(&disk);
        load(&mut files);
        files
            .prepare(&mut old.clone(), committed, GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile(observed([9; 32])).unwrap();
        load(&mut files);
        let base = disk.reboot();

        // The next session earns Miles, changes boxes, Pokédex and box.
        let entry = enter(committed.accounting, committed.balance, date(3)).unwrap();
        let settled = settle(&entry, entry.balance(), 1234, date(4)).unwrap();
        let next = Stored {
            balance: settled.balance,
            accounting: Some(settled.accounting),
        };
        let save = |disk: &Shared| {
            let mut files = opened(disk);
            load(&mut files);
            files.prepare(&mut new.clone(), next, GAME, BEFORE, AFTER)
        };
        let counting = base.cut_after(usize::MAX);
        save(&counting).unwrap();
        let total = counting.0.borrow().operations;
        assert!(total > 20);

        let old_state = (old.clone(), committed);
        let new_state = (new.clone(), next);
        let check = |disk: &Shared, game, expected: &(Vec<u8>, Stored), context: &str| {
            let (bytes, loaded) = recovered(disk, game);
            assert!(bytes == expected.0, "{context}: body differs");
            assert_eq!(loaded.rewards, expected.1, "{context}");
            assert!(
                !loaded.dex_missing && !loaded.transport_missing,
                "{context}"
            );
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
            files.reconcile(observed(AFTER))?;
            files.tidy_transport()?;
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
        files
            .prepare(&mut y.clone(), Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(observed(AFTER)).unwrap();
        assert!(load(&mut files).0 == y);
        // A live import alone publishes nothing.
        assert!(load(&mut opened(&disk.reboot())).0 == y);
        files
            .prepare(&mut both.clone(), Stored::NONE, GAME, [3; 32], [4; 32])
            .unwrap();
        assert!(recovered(&disk, [3; 32]).0 == y);
        assert!(recovered(&disk, [4; 32]).0 == both);
    }

    #[test]
    fn missing_side_files_load_as_empty_and_are_created_by_the_next_save() {
        let original = body(0x31, 0x44, 0);
        let disk = fresh(&original);
        for file in [FileName::Dex, FileName::Transport, FileName::Rewards] {
            disk.0.borrow_mut().files.remove(&(file as u8));
        }
        let mut files = opened(&disk);
        let mut staging = vec![0xEE; BLOB_SIZE];
        let loaded = files.read(&mut staging).unwrap();
        assert_eq!(
            loaded,
            Loaded {
                rewards: Stored::NONE,
                dex_missing: true,
                transport_missing: true,
                delivered: 0,
            }
        );
        // Missing regions are zero for the main thread to fill with defaults.
        assert!(staging[DEX].iter().all(|byte| *byte == 0));
        assert_eq!(
            staging[..TRANSPORT_RECORDS.start],
            original[..TRANSPORT_RECORDS.start]
        );
        files
            .prepare(&mut original.clone(), stored(5, 6, 1), GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(observed(AFTER)).unwrap();
        let (bytes, loaded) = load(&mut files);
        assert!(bytes == original);
        assert_eq!(loaded.rewards, stored(5, 6, 1));
        assert_eq!(disk.0.borrow().files.len(), 4);
    }

    #[test]
    fn damaged_pokedex_is_an_error_but_damaged_rewards_are_not() {
        let disk = fresh(&body(0x31, 0x44, 0));
        let damaged = disk.copy();
        damaged
            .0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Dex as u8))
            .unwrap()[100] ^= 1;
        assert_eq!(
            opened(&damaged).read(&mut vec![0; BLOB_SIZE]),
            Err(Error::Side(FileName::Dex, Problem::Damaged))
        );
        // A Pokédex file from some other Bank matches no snapshot.
        let foreign = disk.copy();
        let other = fresh(&body(0x77, 0x44, 0));
        let bytes = other.0.borrow().files[&(FileName::Dex as u8)].clone();
        foreign
            .0
            .borrow_mut()
            .files
            .insert(FileName::Dex as u8, bytes);
        assert_eq!(
            opened(&foreign).read(&mut vec![0; BLOB_SIZE]),
            Err(Error::Side(FileName::Dex, Problem::NoMatch))
        );
        let rewards = disk.copy();
        rewards
            .0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Rewards as u8))
            .unwrap()[70] ^= 1;
        assert_eq!(load(&mut opened(&rewards)).1.rewards, Stored::NONE);
    }

    /// Delivers with the very function the Transporter patch runs.
    fn deliver(disk: &Shared, pokemon: usize) -> u32 {
        let storage = disk.clone().open(FileName::Transport).unwrap().unwrap();
        let mut file = Sidecar::new(storage, transport::KIND);
        let source = body(0, 0, pokemon);
        let done = transport::deliver(
            &mut file,
            &source[TRANSPORT_RECORDS],
            &source[TRANSPORT_TAGS],
        )
        .expect("Transporter must be allowed to deliver");
        assert_eq!(done.count, pokemon as u32);
        done.id
    }
    fn transport_ok(disk: &Shared) -> bool {
        let storage = disk.clone().open(FileName::Transport).unwrap().unwrap();
        transport::may_deliver(&Sidecar::new(storage, transport::KIND).slots().unwrap())
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
            .prepare(&mut after.clone(), Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();
        // Rolled back: the delivery is whole again.
        let (bytes, loaded) = recovered(&disk, BEFORE);
        assert_eq!(loaded.delivered, 12);
        assert!(bytes == body(0x31, 0x44, 12));
        // Published: four remain, the delivery is gone, even after a cut
        // between publication and cleanup.
        for cut in [0, 1, 2, usize::MAX] {
            let published = disk.cut_after(cut);
            let mut files = opened(&published);
            let _ = files
                .reconcile(observed(AFTER))
                .and_then(|_| files.tidy_transport());
            let (bytes, loaded) = recovered(&published, AFTER);
            assert_eq!(loaded.delivered, 0, "cut {cut}");
            assert!(bytes == after, "cut {cut}");
            // Pokémon are still in the box: Transporter must wait.
            assert!(!transport_ok(&published.reboot()), "cut {cut}");
        }
    }

    #[test]
    fn a_first_start_cut_at_every_write_is_finished_by_the_next_one() {
        let original = body(0x31, 0x44, 0);
        let create = |disk: &Shared, body: &[u8]| {
            let mut files = BankFiles::new(disk.clone());
            match files.open()? {
                Some(_) => Ok(false),
                None => files.initialize(&mut body.to_vec()).map(|_| true),
            }
        };
        let counting = Shared::default().cut_after(usize::MAX);
        assert_eq!(create(&counting, &original), Ok(true));
        let total = counting.0.borrow().operations;
        assert!(total > 20);

        let mut finished_later = 0;
        let mut deliveries = 0;
        for cut in 0..=total {
            let disk = Shared::default().cut_after(cut);
            assert_eq!(create(&disk, &original).is_ok(), cut == total, "cut {cut}");
            let disk = disk.reboot();
            // Transporter may already find the transport file and deliver
            // into it; those Pokémon have left their game.
            let delivered = disk
                .0
                .borrow()
                .files
                .contains_key(&(FileName::Transport as u8))
                && transport_ok(&disk);
            if delivered {
                deliver(&disk, 5);
                deliveries += 1;
            }
            // The next start: no Bank yet means it is created now, with the
            // body of that start.
            let second = body(0x32, 0x45, 0);
            let created = create(&disk, &second)
                .unwrap_or_else(|error| panic!("start after cut {cut} cannot finish: {error:?}"));
            finished_later += usize::from(created);
            let expected = if created { &second } else { &original };
            let (bytes, loaded) = load(&mut opened(&disk.reboot()));
            assert!(
                !loaded.dex_missing && !loaded.transport_missing,
                "cut {cut}"
            );
            let shown = if delivered { 5 } else { 0 };
            assert_eq!(loaded.delivered, shown, "cut {cut}");
            let mut with_box = expected.clone();
            let source = body(0, 0, shown as usize);
            with_box[TRANSPORT_RECORDS].copy_from_slice(&source[TRANSPORT_RECORDS]);
            with_box[TRANSPORT_TAGS].copy_from_slice(&source[TRANSPORT_TAGS]);
            assert!(bytes == with_box, "cut {cut}: body differs");
        }
        // Both happened: starts that had to finish the creation, and
        // deliveries into a Bank that did not exist yet.
        assert!(finished_later > 20 && deliveries > 0);
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
            .prepare(&mut old.clone(), stored(7, 1, 3), GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile(observed([9; 32])).unwrap();
        files.tidy_transport().unwrap();
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
                .prepare(&mut new.clone(), stored(9, 2, 5), GAME, BEFORE, AFTER)
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
            assert_eq!(files.reconcile_as(decision), Ok(decision), "{name}");
            files.tidy_transport().unwrap();
            let (bytes, loaded) = load(&mut files);
            let (state, rewards) = if decision == commit {
                (&new, stored(9, 2, 5))
            } else {
                (&old, stored(7, 1, 3))
            };
            assert!(bytes == *state, "{name}: body differs");
            assert_eq!(loaded.rewards, rewards, "{name}");
            assert!(!loaded.dex_missing && !loaded.transport_missing, "{name}");
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
            .prepare(&mut old.clone(), Stored::NONE, GAME, [8; 32], [9; 32])
            .unwrap();
        files.reconcile(observed([9; 32])).unwrap();
        files.tidy_transport().unwrap();
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
            .prepare(&mut moved_in.clone(), Stored::NONE, GAME, BEFORE, AFTER)
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
            .prepare(&mut with_deposit, Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();
        assert_eq!(count(&delivered), (1, 0));

        // A release leaves Bank like a withdrawal: the old boxes keep it.
        let released = base.copy();
        let mut files = opened(&released);
        load(&mut files);
        files
            .prepare(
                &mut bank(0x52, &[(0, 0, pokemon(1))], 0),
                Stored::NONE,
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
            .prepare(&mut body(0x52, 0x44, 0), Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();
        files.reconcile(observed(AFTER)).unwrap();
        // Both journal records are lost.
        disk.0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Bank as u8))
            .unwrap()[..2 * offline_core::METADATA_SIZE]
            .fill(0);
        let mut files = BankFiles::new(disk.reboot());
        assert!(matches!(files.open(), Err(Error::Bank(_))));
        assert!(matches!(
            files.initialize(&mut body(0, 0, 0)),
            Err(Error::Bank(_))
        ));
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
            .prepare(&mut emptied.clone(), Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();
        // Power is lost right after the game was saved, before any cleanup.
        let (bytes, loaded) = recovered(&disk, AFTER);
        assert_eq!(loaded.delivered, 0);
        assert!(bytes == emptied);

        // Until the save's clean-up has run, loads never remove the taken
        // delivery themselves, and Transporter keeps waiting.
        let disk = disk.reboot();
        let mut files = opened(&disk);
        files.reconcile(observed(AFTER)).unwrap();
        for _ in 0..3 {
            let (bytes, loaded) = load(&mut files);
            assert_eq!(loaded.delivered, 0);
            assert!(bytes == emptied);
            assert!(!transport_ok(&disk));
        }
        // The next completed save finishes the clean-up.
        files
            .prepare(&mut emptied.clone(), Stored::NONE, GAME, [3; 32], [4; 32])
            .unwrap();
        files.reconcile(observed([4; 32])).unwrap();
        files.tidy_transport().unwrap();
        assert!(load(&mut files).0 == emptied);
        assert!(transport_ok(&disk));
        deliver(&disk, 30);
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
            .prepare(&mut body(0x52, 0x44, 4), Stored::NONE, GAME, BEFORE, AFTER)
            .unwrap();

        let untidy = rolled_back.copy();
        opened(&untidy).reconcile(observed(AFTER)).unwrap();
        opened(&rolled_back).reconcile(observed(BEFORE)).unwrap();

        let tidy = untidy.copy();
        opened(&tidy).tidy_transport().unwrap();

        for (name, disk, delivered, pokemon) in [
            ("delivery waiting", &waiting, 12, 12),
            ("save rolled back, leftover slot", &rolled_back, 12, 12),
            ("save published, delivery not yet removed", &untidy, 0, 4),
            ("save published and cleaned up", &tidy, 0, 4),
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
        // Only the published-and-cleaned-up file has room judged by Transporter,
        // and it still holds four Pokémon, so even there it must wait.
        for disk in [&waiting, &rolled_back, &untidy, &tidy] {
            assert!(!transport_ok(disk));
        }
    }

    #[test]
    fn a_delivery_made_before_the_bank_exists_is_kept() {
        let disk = Shared::default();
        disk.clone().create(FileName::Transport).unwrap();
        deliver(&disk, 2);
        let mut files = BankFiles::new(disk.clone());
        assert_eq!(files.open(), Ok(None));
        files.initialize(&mut body(0x31, 0x44, 0)).unwrap();
        let (bytes, loaded) = load(&mut opened(&disk.reboot()));
        assert_eq!(loaded.delivered, 2);
        assert!(bytes == body(0x31, 0x44, 2));
    }

    #[test]
    fn a_damaged_delivery_is_ignored_and_left_in_place() {
        let disk = fresh(&body(0x31, 0x44, 0));
        load(&mut opened(&disk));
        deliver(&disk, 5);
        let before = disk.0.borrow().files[&(FileName::Transport as u8)].clone();
        let slot = transport::KIND.slot_len() as usize;
        let untagged = if before[16..24] == [0; 8] { 0 } else { slot };
        disk.0
            .borrow_mut()
            .files
            .get_mut(&(FileName::Transport as u8))
            .unwrap()[untagged + 64 + 3] ^= 1;
        let damaged = disk.0.borrow().files[&(FileName::Transport as u8)].clone();
        let (bytes, loaded) = load(&mut opened(&disk));
        assert_eq!((loaded.delivered, loaded.transport_missing), (0, true));
        assert!(bytes == body(0x31, 0x44, 0));
        assert!(disk.0.borrow().files[&(FileName::Transport as u8)] == damaged);
    }
}
