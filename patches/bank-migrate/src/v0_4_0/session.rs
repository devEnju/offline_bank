//! Frozen: v0.4.0's `session.rs`. Only the migration reads it.
//! The journaled Bank file: one validated payload per snapshot.
//!
//! The payload is the Bank regions of the native body (`offline_core::sections`).
//! A load reads it once into a staging buffer and checks it on that pass. Any
//! storage error poisons the session; close and reopen before retrying. A
//! prepared transfer must be reconciled before any Bank data is displayed.
//! The side files that follow this journal are handled by `bank_files`.

use super::{
    sections::{self, SectionError, BANK_SIZE},
    sidecar::Tag,
    BankStore, Fingerprint, GameIdentity, GameObservation, Head, Layout, Phase, RecoveryDecision,
    Storage, StoreError,
};

pub const BANK_LAYOUT: Layout = Layout::new(BANK_SIZE as u32);

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Storage(StoreError<E>),
    Payload(SectionError),
    Poisoned,
    UnresolvedTransfer,
    BankNotLoaded,
    ScratchTooSmall,
}

pub struct BankSession<S: Storage> {
    store: BankStore<S>,
    head: Head,
    poisoned: bool,
    loaded: bool,
}

impl<S: Storage> BankSession<S> {
    /// Opens only existing, valid storage. Missing/zero/corrupt data never
    /// triggers initialization here; `never_held_a_bank` tells the caller
    /// when the file is an unfinished initialization. Prepared state is
    /// retained for recovery.
    /// Only the journal and snapshot headers are read; `read_bank` verifies
    /// the payload on its single pass.
    pub fn open_existing(storage: S) -> Result<Self, Error<S::Error>> {
        let mut store = BankStore::new(storage, BANK_LAYOUT);
        let head = store.inspect_quick().map_err(Error::Storage)?;
        Ok(Self {
            store,
            head,
            poisoned: false,
            loaded: false,
        })
    }

    /// Initializes an exclusively created, zeroed file with a validated Bank
    /// payload. This must never be a fallback after a failed existing load.
    pub fn initialize_bytes(storage: S, payload: &[u8]) -> Result<Self, Error<S::Error>> {
        sections::validate_bank(payload).map_err(Error::Payload)?;
        let mut store = BankStore::new(storage, BANK_LAYOUT);
        let head = store.initialize_new(payload).map_err(Error::Storage)?;
        Ok(Self {
            store,
            head,
            poisoned: false,
            loaded: true,
        })
    }

    /// Finishes the initialization of an existing file that never held a
    /// Bank: zero-filled, or cut before its first journal record. Refused
    /// by the store for any file that has a journal record or a later
    /// snapshot.
    pub fn reinitialize_bytes(storage: S, payload: &[u8]) -> Result<Self, Error<S::Error>> {
        sections::validate_bank(payload).map_err(Error::Payload)?;
        let mut store = BankStore::new(storage, BANK_LAYOUT);
        let head = store.reinitialize(payload).map_err(Error::Storage)?;
        Ok(Self {
            store,
            head,
            poisoned: false,
            loaded: true,
        })
    }

    /// True for the error of a file in which no Bank was ever current.
    pub fn never_held_a_bank(error: &Error<S::Error>) -> bool {
        matches!(
            error,
            Error::Storage(StoreError::Uninitialized | StoreError::NeverPublished)
        )
    }

    /// The committed snapshot, when no transfer is pending.
    pub fn tag(&self) -> Result<Tag, Error<S::Error>> {
        self.ensure_usable()?;
        match self.head.phase() {
            Phase::Clean(current) => Ok(Tag::of(&current)),
            Phase::Prepared(_) => Err(Error::UnresolvedTransfer),
        }
    }

    /// Reads the payload into `scratch[..BANK_SIZE]` in one pass, checks its
    /// CRC and layout, and returns the snapshot it belongs to.
    pub fn read_bank(&mut self, scratch: &mut [u8]) -> Result<Tag, Error<S::Error>> {
        let tag = self.tag()?;
        let Some(scratch) = scratch.get_mut(..BANK_SIZE) else {
            return Err(Error::ScratchTooSmall);
        };
        let read = self.store.read_current(&self.head, scratch);
        let length = self.storage_result(read)?;
        if let Err(error) = sections::validate_bank(&scratch[..length]) {
            self.poisoned = true;
            return Err(Error::Payload(error));
        }
        self.loaded = true;
        Ok(tag)
    }

    /// Reads one of the two payloads of a prepared transfer into
    /// `scratch[..BANK_SIZE]`, checked like `read_bank`, for comparing them.
    /// Neither becomes the loaded Bank by this.
    pub fn read_pending(&mut self, after: bool, scratch: &mut [u8]) -> Result<(), Error<S::Error>> {
        self.ensure_usable()?;
        let Some(scratch) = scratch.get_mut(..BANK_SIZE) else {
            return Err(Error::ScratchTooSmall);
        };
        let read = self.store.read_pending(&self.head, after, scratch);
        let length = self.storage_result(read)?;
        sections::validate_bank(&scratch[..length]).map_err(Error::Payload)
    }

    /// Returns success only after the new payload and both prepare records are
    /// durable. The native game writer MUST NOT run if this returns an error.
    /// Fingerprints must describe the verified complete before/after game
    /// write. Returns the tag of the prepared snapshot.
    pub fn prepare_bytes(
        &mut self,
        payload: &[u8],
        game: GameIdentity,
        before: Fingerprint,
        after: Fingerprint,
    ) -> Result<Tag, Error<S::Error>> {
        self.ensure_loaded()?;
        sections::validate_bank(payload).map_err(Error::Payload)?;
        let result = self
            .store
            .prepare_transfer(&self.head, payload, game, before, after);
        self.head = self.storage_result(result)?;
        self.loaded = false;
        match self.head.phase() {
            Phase::Prepared(pending) => Ok(Tag::of(&pending.after)),
            Phase::Clean(current) => Ok(Tag::of(&current)),
        }
    }

    pub fn phase(&self) -> Result<Phase, Error<S::Error>> {
        self.ensure_usable()?;
        Ok(self.head.phase())
    }

    /// `observed` must come from identifying and reading the actual game save,
    /// after its native writer/commit has completed or during startup recovery.
    /// Call read_bank afterwards, including when recovery chooses the old Bank.
    pub fn reconcile_game(
        &mut self,
        observed: GameObservation,
    ) -> Result<RecoveryDecision, Error<S::Error>> {
        self.ensure_usable()?;
        let result = self.store.reconcile(&self.head, observed);
        let reconciled = self.storage_result(result)?;
        self.head = reconciled.head;
        self.loaded = false;
        Ok(reconciled.decision)
    }

    pub fn into_storage(self) -> S {
        self.store.into_inner()
    }

    fn ensure_usable(&self) -> Result<(), Error<S::Error>> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }
    fn ensure_loaded(&self) -> Result<(), Error<S::Error>> {
        self.tag()?;
        if self.loaded {
            Ok(())
        } else {
            Err(Error::BankNotLoaded)
        }
    }
    fn storage_result<T>(
        &mut self,
        result: Result<T, StoreError<S::Error>>,
    ) -> Result<T, Error<S::Error>> {
        result.map_err(|error| {
            self.poisoned = true;
            Error::Storage(error)
        })
    }
}
