//! Frozen: v0.4.0's `bank_files.rs`. Only the migration reads it.
//! The stored files and the order in which they are read and written.
//!
//! The Bank container carries the journal and decides which save is current.
//! The Pokédex, transport box, and rewards containers each hold two slots
//! tagged with a Bank snapshot (`offline_core::sidecar`). A save writes every
//! side container's spare slot first, then prepares the Bank journal; a load
//! uses the slots whose tag matches the Bank's current snapshot. So all four
//! always describe the same save, without a second journal.
//!
//! Every unit of a container is a file of its own (`UnitFile`): the two
//! journal records, the two snapshot slots, and the two slots of each side
//! container. An interrupted write can make the file it went to unreadable,
//! and that file is then never one that holds anything still needed. The
//! two files of a pair take turns; `.alt` names the alternate of the two, not
//! an older copy.
//!
//! This module is generic over storage so the whole sequence is host-tested.
//! In memory the main thread only ever sees the one native body.

use super::session::{self, BankSession};
use super::{
    native_blob::BLOB_SIZE,
    rewards::{self, Record},
    sections::{
        self, BANK_SIZE, DEX, DEX_FILE, DEX_SIZE, MILES, TRANSPORT_RECORDS, TRANSPORT_SIZE,
        TRANSPORT_TAGS,
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
pub use bank_common::bank_files::UnitFile;

pub const RECORD: u64 = super::METADATA_SIZE as u64;
pub const SNAPSHOT: u64 = session::BANK_LAYOUT.snapshot_len();
/// In the order of the container's layout: the journal records, then the
/// snapshot slots.
static BANK_UNITS: [UnitFile; 4] = [
    UnitFile::new("/journal.bin", RECORD),
    UnitFile::new("/journal.alt.bin", RECORD),
    UnitFile::new("/bank.bin", SNAPSHOT),
    UnitFile::new("/bank.alt.bin", SNAPSHOT),
];
static DEX_UNITS: [UnitFile; 2] = [
    UnitFile::new("/dex.bin", DEX_FILE.slot_len()),
    UnitFile::new("/dex.alt.bin", DEX_FILE.slot_len()),
];
static TRANSPORT_UNITS: [UnitFile; 2] = [
    UnitFile::new("/mover.bin", transport::KIND.slot_len()),
    UnitFile::new("/mover.alt.bin", transport::KIND.slot_len()),
];
static REWARDS_UNITS: [UnitFile; 2] = [
    UnitFile::new("/rewards.bin", rewards::FILE.slot_len()),
    UnitFile::new("/rewards.alt.bin", rewards::FILE.slot_len()),
];

impl FileName {
    /// The files of the container, in the order of its layout.
    pub fn units(self) -> &'static [UnitFile] {
        match self {
            Self::Bank => &BANK_UNITS,
            Self::Dex => &DEX_UNITS,
            Self::Transport => &TRANSPORT_UNITS,
            Self::Rewards => &REWARDS_UNITS,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Problem {
    /// The slot for the current Bank snapshot is unreadable.
    Damaged = 1,
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
    pub rewards: Record,
    /// Not of that release: the record's bytes as read, for a migration that
    /// reads them with the meaning an earlier release gave them.
    pub record: Option<[u8; rewards::RECORD_SIZE]>,
    /// No Pokédex for this save (no file, a damaged one, or one of another
    /// save): the staging region is zero and must be filled with the native
    /// defaults by the main thread.
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
        stored: Record,
    ) -> Outcome<Tag, F> {
        if staging.len() != BLOB_SIZE {
            return Err(Error::Side(FileName::Bank, Problem::Layout));
        }
        let layout = |_| Error::Side(FileName::Bank, Problem::Layout);
        let next = Tag {
            generation,
            crc: sections::bank_crc(staging).map_err(layout)?,
        };

        // The Pokédex and the Miles record never block the Bank, so a slot
        // of theirs that cannot be read is void whichever one it is.
        let mut dex = self.ensure_side(FileName::Dex, DEX_FILE)?;
        let slots = dex.slots_or_void();
        dex.write(spare(&slots, current), Some(next), 0, 0, &[&staging[DEX]])
            .map_err(side(FileName::Dex))?;
        drop(dex);

        let mut file = self.ensure_side(FileName::Rewards, rewards::FILE)?;
        let slots = file.slots_or_void();
        file.write(
            spare(&slots, current),
            Some(next),
            0,
            0,
            &[&rewards::encode(stored)],
        )
        .map_err(side(FileName::Rewards))?;
        drop(file);

        // The transport box can hold Pokémon: a slot that cannot be read
        // is void only beside the one seen to be current, or while no Bank
        // exists yet.
        let mut file = self.ensure_side(FileName::Transport, transport::KIND)?;
        let slots = match current {
            Some(current) => file
                .slots_beside(current)
                .map_err(side(FileName::Transport))?,
            None => file.slots_or_void(),
        };
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
        self.write_sides(staging, None, 1, Record::NONE)?;
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
            rewards: Record::NONE,
            record: None,
            dex_missing: true,
            transport_missing: true,
            delivered: 0,
        };
        self.adopted = None;

        // A Pokédex that is damaged or belongs to another save never blocks
        // the Bank: it starts empty, as without the file, and each game
        // writes its part again at its next Save and Quit.
        if let Some(mut file) = self.open_side(FileName::Dex, DEX_FILE)? {
            let slots = file.slots_or_void();
            let found = matching(&slots, tag).and_then(|index| {
                let slot = slots[index].filter(|slot| slot.len as usize == DEX_SIZE);
                slot.map(|slot| (index, slot))
            });
            if let Some((index, slot)) = found {
                match file.read(index, &slot, &mut staging[DEX]) {
                    Ok(()) => loaded.dex_missing = false,
                    Err(SidecarError::Io(error)) => return Err(Error::Io(error)),
                    Err(_) => staging[DEX].fill(0),
                }
            }
        }

        // A missing or unreadable Miles record never blocks the Bank.
        if let Some(mut file) = self.open_side(FileName::Rewards, rewards::FILE)? {
            let slots = file.slots_or_void();
            if let Some(index) = matching(&slots, tag) {
                let slot = slots[index].filter(|slot| slot.len as usize == rewards::RECORD_SIZE);
                let mut bytes = [0; rewards::RECORD_SIZE];
                if let Some(slot) = slot {
                    match file.read(index, &slot, &mut bytes) {
                        Ok(()) => {
                            loaded.record = Some(bytes);
                            loaded.rewards = rewards::decode(&bytes).unwrap_or(Record::NONE);
                            // The balance v0.3.1 kept in this record.
                            let body = u32::from_le_bytes([
                                staging[MILES.start],
                                staging[MILES.start + 1],
                                staging[MILES.start + 2],
                                staging[MILES.start + 3],
                            ]);
                            if let Some(balance) = rewards::balance_beside(body, &bytes) {
                                staging[MILES].copy_from_slice(&balance.to_le_bytes());
                            }
                        }
                        Err(SidecarError::Io(error)) => return Err(Error::Io(error)),
                        Err(_) => {}
                    }
                }
            }
        }

        if let Some(mut file) = self.open_side(FileName::Transport, transport::KIND)? {
            let slots = file.slots_beside(tag).map_err(side(FileName::Transport))?;
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
            let slots = file.slots_beside(tag).map_err(side(FileName::Transport))?;
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
        stored: Record,
        game: GameIdentity,
        before: Fingerprint,
        after: Fingerprint,
    ) -> Outcome<(), F> {
        let current = self.bank()?.tag().map_err(Error::Bank)?;
        // Test builds: keep the record being written for a while, so that a
        // real power cut lands inside one of its writes.
        let next = self.write_sides(staging, Some(current), current.generation + 1, stored)?;
        // Test builds: the same for the spare snapshot slot.
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
