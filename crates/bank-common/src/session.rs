//! The journaled Bank file: one validated payload per snapshot.
//!
//! The payload is the Bank regions of the native body (`offline_core::sections`).
//! A load reads it once into a staging buffer and checks it on that pass. Any
//! storage error poisons the session; close and reopen before retrying. A
//! prepared transfer must be reconciled before any Bank data is displayed.
//! The side files that follow this journal are handled by `bank_files`.

use offline_core::{
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

    /// Test builds only: see `BankStore::republish`.
    #[cfg(feature = "test-tear-record")]
    pub fn tear_record(&mut self, passes: u32) -> Result<(), Error<S::Error>> {
        self.ensure_usable()?;
        for _ in 0..passes {
            let result = self.store.republish(&self.head);
            self.head = self.storage_result(result)?;
        }
        Ok(())
    }

    /// Test builds only: see `BankStore::rewrite_spare`.
    #[cfg(feature = "test-tear-boxes")]
    pub fn tear_boxes(&mut self, payload: &mut [u8], passes: u32) -> Result<(), Error<S::Error>> {
        self.ensure_usable()?;
        let result = self.store.rewrite_spare(&self.head, payload, passes);
        self.storage_result(result)
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

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{cell::Cell, rc::Rc, vec, vec::Vec};

    struct Memory {
        bytes: Vec<u8>,
        fail_payload_read: Rc<Cell<bool>>,
        reads: Rc<Cell<usize>>,
        payload_reads: Rc<Cell<usize>>,
    }
    impl Memory {
        fn with(bytes: Vec<u8>) -> Self {
            Self {
                bytes,
                fail_payload_read: Rc::new(Cell::new(false)),
                reads: Rc::new(Cell::new(0)),
                payload_reads: Rc::new(Cell::new(0)),
            }
        }
    }
    impl Storage for Memory {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            self.reads.set(self.reads.get() + 1);
            if out.len() == BANK_SIZE {
                self.payload_reads.set(self.payload_reads.get() + 1);
                if self.fail_payload_read.get() {
                    out[..7].fill(0xDE);
                    return Err(());
                }
            }
            out.copy_from_slice(&self.bytes[at as usize..at as usize + out.len()]);
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            self.bytes[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn recreate(&mut self, at: u64, length: u64) -> Result<(), ()> {
            self.bytes[at as usize..(at + length) as usize].fill(0);
            Ok(())
        }
        fn sync(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }
    fn payload(marker: u8) -> Vec<u8> {
        let mut bytes = vec![0; BANK_SIZE];
        bytes[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        bytes[BANK_SIZE - 1] = marker;
        bytes
    }
    fn storage(payload: &[u8]) -> Memory {
        let memory = Memory::with(vec![0; BANK_LAYOUT.file_len() as usize]);
        let mut store = BankStore::new(memory, BANK_LAYOUT);
        store.initialize_new(payload).unwrap();
        store.into_inner()
    }
    const GAME: GameIdentity = GameIdentity {
        title_id: 0x0004000000055E00,
        save_identity: [7; 32],
    };

    #[test]
    fn bank_file_has_the_documented_size() {
        assert_eq!(BANK_SIZE, 730_442);
        assert_eq!(BANK_LAYOUT.file_len(), 1_461_332);
    }

    #[test]
    fn a_load_reads_the_payload_exactly_once_and_still_checks_it() {
        let memory = storage(&payload(0x71));
        let (reads, payload_reads) = (memory.reads.clone(), memory.payload_reads.clone());
        let mut bytes = memory.bytes.clone();
        reads.set(0);
        let mut session = BankSession::open_existing(memory).unwrap();
        // Opening touches only the journal and the 32-byte snapshot header.
        assert!(reads.get() <= 4);
        assert_eq!(payload_reads.get(), 0);
        let mut staging = vec![0; BANK_SIZE];
        let tag = session.read_bank(&mut staging).unwrap();
        assert_eq!(payload_reads.get(), 1);
        assert_eq!(staging, payload(0x71));
        assert_eq!(tag, session.tag().unwrap());

        // A corrupted payload byte anywhere is caught by that one pass.
        let at = BANK_LAYOUT.snapshot_offset(offline_core::Slot::A) as usize + 32;
        for offset in [0, 0x15C, BANK_SIZE / 2, BANK_SIZE - 1] {
            bytes[at + offset] ^= 0x10;
            let mut session = BankSession::open_existing(Memory::with(bytes.clone())).unwrap();
            assert_eq!(
                session.read_bank(&mut staging),
                Err(Error::Storage(StoreError::PayloadChecksum))
            );
            assert_eq!(session.phase(), Err(Error::Poisoned));
            bytes[at + offset] ^= 0x10;
        }
    }

    #[test]
    fn partial_read_poisons_and_blocks_retry_and_saving() {
        let memory = storage(&payload(1));
        let fail = memory.fail_payload_read.clone();
        let reads = memory.reads.clone();
        let mut session = BankSession::open_existing(memory).unwrap();
        let mut staging = vec![0; BANK_SIZE];
        assert_eq!(
            session.prepare_bytes(&payload(2), GAME, [1; 32], [2; 32]),
            Err(Error::BankNotLoaded)
        );
        fail.set(true);
        assert!(matches!(
            session.read_bank(&mut staging),
            Err(Error::Storage(StoreError::Io(())))
        ));
        let count = reads.get();
        fail.set(false);
        assert_eq!(session.read_bank(&mut staging), Err(Error::Poisoned));
        assert_eq!(reads.get(), count);
        assert_eq!(
            session.prepare_bytes(&payload(2), GAME, [1; 32], [2; 32]),
            Err(Error::Poisoned)
        );
    }

    #[test]
    fn checked_container_with_bad_payload_layout_is_refused() {
        let mut invalid = payload(1);
        invalid[0x15C] = 99;
        let mut session = BankSession::open_existing(storage(&invalid)).unwrap();
        assert!(matches!(
            session.read_bank(&mut vec![0; BANK_SIZE]),
            Err(Error::Payload(_))
        ));
        assert_eq!(session.phase(), Err(Error::Poisoned));
        // Full native bodies and other sizes are not Bank payloads.
        for size in [BANK_SIZE - 1, offline_core::native_blob::BLOB_SIZE] {
            assert!(matches!(
                BankSession::initialize_bytes(
                    Memory::with(vec![0; BANK_LAYOUT.file_len() as usize]),
                    &vec![0; size]
                ),
                Err(Error::Payload(_))
            ));
        }
    }

    #[test]
    fn pending_bank_stays_hidden_until_verified_recovery() {
        let mut session = BankSession::open_existing(storage(&payload(1))).unwrap();
        let mut staging = vec![0; BANK_SIZE];
        let before = session.read_bank(&mut staging).unwrap();
        let after = session
            .prepare_bytes(&payload(2), GAME, [1; 32], [2; 32])
            .unwrap();
        assert_eq!(after.generation, before.generation + 1);
        assert_eq!(after.crc, offline_core::crc32(&payload(2)));
        staging.fill(0xA9);
        assert_eq!(
            session.read_bank(&mut staging),
            Err(Error::UnresolvedTransfer)
        );
        assert!(staging.iter().all(|byte| *byte == 0xA9));
        let bytes = session.into_storage().bytes;

        // Rolled back: the old tag and payload are current again.
        let mut rolled = BankSession::open_existing(Memory::with(bytes.clone())).unwrap();
        assert_eq!(
            rolled
                .reconcile_game(GameObservation::Present {
                    game: GAME,
                    fingerprint: [1; 32]
                })
                .unwrap(),
            RecoveryDecision::KeepBefore
        );
        assert_eq!(rolled.read_bank(&mut staging), Ok(before));
        assert_eq!(staging, payload(1));

        // Committed: the prepared tag and payload become current.
        let mut committed = BankSession::open_existing(Memory::with(bytes)).unwrap();
        assert_eq!(
            committed
                .reconcile_game(GameObservation::Present {
                    game: GAME,
                    fingerprint: [2; 32]
                })
                .unwrap(),
            RecoveryDecision::CommitAfter
        );
        assert_eq!(committed.read_bank(&mut staging), Ok(after));
        assert_eq!(staging, payload(2));
    }
}
