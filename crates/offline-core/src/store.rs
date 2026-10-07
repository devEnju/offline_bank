use crate::{
    checksum::Crc32, decide_recovery, format::zero, Fingerprint, FormatError, GameIdentity,
    GameObservation, Metadata, PendingTransfer, Phase, RecoveryBlock, RecoveryDecision, Slot,
    SnapshotHeader, SnapshotRef, METADATA_SIZE, SNAPSHOT_HEADER_SIZE,
};

/// Exact I/O on one preallocated container. A successful sync must make all
/// preceding writes durable. Writes must never resize the file or silently short-write.
pub trait Storage {
    type Error;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn sync(&mut self) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    capacity: u32,
}

impl Layout {
    pub const fn new(capacity: u32) -> Self {
        Self { capacity }
    }
    pub const fn capacity(self) -> u32 {
        self.capacity
    }
    pub fn file_len(self) -> u64 {
        (METADATA_SIZE as u64 * 2) + 2 * (SNAPSHOT_HEADER_SIZE as u64 + u64::from(self.capacity))
    }
    pub fn metadata_offset(self, slot: Slot) -> u64 {
        slot.index() * METADATA_SIZE as u64
    }
    pub fn snapshot_offset(self, slot: Slot) -> u64 {
        METADATA_SIZE as u64 * 2
            + slot.index() * (SNAPSHOT_HEADER_SIZE as u64 + u64::from(self.capacity))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError<E> {
    Io(E),
    Format(FormatError),
    /// Only returned for a completely zero-filled, correctly sized container.
    Uninitialized,
    AlreadyContainsData,
    NoValidMetadata,
    ConflictingMetadata,
    SnapshotMismatch,
    PayloadChecksum,
    PendingRecovery,
    NoPendingTransfer,
    RecoveryBlocked(RecoveryBlock),
    StaleHead,
    CounterExhausted,
    BufferTooSmall {
        needed: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    metadata: Metadata,
    preferred: Slot,
}

impl Head {
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn phase(&self) -> Phase {
        self.metadata.phase
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reconciled {
    pub head: Head,
    pub decision: RecoveryDecision,
}

/// Does not cache a current head. Each mutation verifies its input head against
/// durable metadata; callers must still ensure exclusive access to storage.
pub struct BankStore<S> {
    storage: S,
    layout: Layout,
}

enum Record {
    Empty,
    Damaged,
    Valid(Metadata),
}

impl<S: Storage> BankStore<S> {
    pub fn new(storage: S, layout: Layout) -> Self {
        Self { storage, layout }
    }
    pub fn into_inner(self) -> S {
        self.storage
    }
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Inspects both metadata replicas and validates every snapshot referenced by
    /// the selected state. Invalid newer snapshot bytes alone are never selected.
    pub fn inspect(&mut self) -> Result<Head, StoreError<S::Error>> {
        self.select(true)
    }

    fn select(&mut self, payloads: bool) -> Result<Head, StoreError<S::Error>> {
        let a = self.read_record(Slot::A)?;
        let b = self.read_record(Slot::B)?;
        let (metadata, preferred) = match (a, b) {
            (Record::Valid(a), Record::Valid(b)) => {
                if a.sequence == b.sequence && a != b {
                    return Err(StoreError::ConflictingMetadata);
                }
                if a.sequence >= b.sequence {
                    (a, Slot::A)
                } else {
                    (b, Slot::B)
                }
            }
            (Record::Valid(a), _) => (a, Slot::A),
            (_, Record::Valid(b)) => (b, Slot::B),
            (Record::Empty, Record::Empty) => {
                return Err(if self.zero_filled()? {
                    StoreError::Uninitialized
                } else {
                    StoreError::NoValidMetadata
                });
            }
            _ => return Err(StoreError::NoValidMetadata),
        };
        match metadata.phase {
            Phase::Clean(current) => self.validate(current, payloads)?,
            Phase::Prepared(pending) => {
                self.validate(pending.before, payloads)?;
                self.validate(pending.after, payloads)?;
            }
        }
        Ok(Head {
            metadata,
            preferred,
        })
    }

    /// Like `inspect`, but checks only the journal and each referenced
    /// snapshot's 32-byte header. The payload itself is verified by the single
    /// pass of `read_current`, so a load reads every payload byte once.
    pub fn inspect_quick(&mut self) -> Result<Head, StoreError<S::Error>> {
        self.select(false)
    }

    /// Explicitly initializes a newly created, completely zero-filled file.
    /// The caller supplies the native Bank representation of an empty payload.
    /// Interrupted initialization is reported for explicit repair, never reset.
    pub fn initialize_new(&mut self, payload: &[u8]) -> Result<Head, StoreError<S::Error>> {
        if !self.zero_filled()? {
            return Err(StoreError::AlreadyContainsData);
        }
        let current = self.write_snapshot(Slot::A, 1, payload)?;
        self.publish(
            Slot::B,
            Metadata {
                sequence: 1,
                capacity: self.layout.capacity,
                phase: Phase::Clean(current),
            },
        )
    }

    /// Commits changes involving ONLY local Bank state, such as box names.
    /// This method must never be used for transfers or rewards that edit a game.
    pub fn commit_bank_only(
        &mut self,
        head: &Head,
        payload: &[u8],
    ) -> Result<Head, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let current = match head.phase() {
            Phase::Clean(current) => current,
            Phase::Prepared(_) => return Err(StoreError::PendingRecovery),
        };
        let sequence = head
            .metadata
            .sequence
            .checked_add(1)
            .ok_or(StoreError::CounterExhausted)?;
        let generation = current
            .header
            .generation
            .checked_add(1)
            .ok_or(StoreError::CounterExhausted)?;
        let next = self.write_snapshot(current.slot.other(), generation, payload)?;
        self.publish(
            head.preferred,
            Metadata {
                sequence,
                capacity: self.layout.capacity,
                phase: Phase::Clean(next),
            },
        )
    }

    /// Durably prepares both metadata replicas before returning success.
    /// The caller must verify the before game state immediately before this call
    /// and must not mutate the game if this call returns an error.
    pub fn prepare_transfer(
        &mut self,
        head: &Head,
        payload: &[u8],
        game: GameIdentity,
        before_fingerprint: Fingerprint,
        after_fingerprint: Fingerprint,
    ) -> Result<Head, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let current = match head.phase() {
            Phase::Clean(current) => current,
            Phase::Prepared(_) => return Err(StoreError::PendingRecovery),
        };
        if game.title_id == 0 {
            return Err(StoreError::Format(FormatError::InvalidGameIdentity));
        }
        if before_fingerprint == after_fingerprint {
            return Err(StoreError::Format(FormatError::IndistinguishableGameStates));
        }
        let sequence = head
            .metadata
            .sequence
            .checked_add(1)
            .ok_or(StoreError::CounterExhausted)?;
        let generation = current
            .header
            .generation
            .checked_add(1)
            .ok_or(StoreError::CounterExhausted)?;
        let after = self.write_snapshot(current.slot.other(), generation, payload)?;
        let pending = PendingTransfer {
            before: current,
            after,
            game,
            before_fingerprint,
            after_fingerprint,
        };
        self.publish(
            head.preferred,
            Metadata {
                sequence,
                capacity: self.layout.capacity,
                phase: Phase::Prepared(pending),
            },
        )
    }

    /// Resolves a prepared transfer after reading and identifying the actual game
    /// save. Blocks without writes if the save is missing, different, or modified.
    pub fn reconcile(
        &mut self,
        head: &Head,
        observed: GameObservation,
    ) -> Result<Reconciled, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let pending = match head.phase() {
            Phase::Prepared(pending) => pending,
            Phase::Clean(_) => return Err(StoreError::NoPendingTransfer),
        };
        let decision = decide_recovery(&pending, observed);
        let selected = match decision {
            RecoveryDecision::KeepBefore => pending.before,
            RecoveryDecision::CommitAfter => pending.after,
            RecoveryDecision::Blocked(reason) => return Err(StoreError::RecoveryBlocked(reason)),
        };
        let sequence = head
            .metadata
            .sequence
            .checked_add(1)
            .ok_or(StoreError::CounterExhausted)?;
        let head = self.publish(
            head.preferred,
            Metadata {
                sequence,
                capacity: self.layout.capacity,
                phase: Phase::Clean(selected),
            },
        )?;
        Ok(Reconciled { head, decision })
    }

    /// Reads the visible payload only when there is no unresolved transaction.
    /// The full payload is checksummed again after reading into the caller's buffer.
    pub fn read_current(
        &mut self,
        head: &Head,
        out: &mut [u8],
    ) -> Result<usize, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let current = match head.phase() {
            Phase::Clean(current) => current,
            Phase::Prepared(_) => return Err(StoreError::PendingRecovery),
        };
        let length = current.header.payload_len as usize;
        if out.len() < length {
            return Err(StoreError::BufferTooSmall {
                needed: current.header.payload_len,
            });
        }
        self.storage
            .read(
                self.layout.snapshot_offset(current.slot) + SNAPSHOT_HEADER_SIZE as u64,
                &mut out[..length],
            )
            .map_err(StoreError::Io)?;
        if crate::crc32(&out[..length]) != current.header.payload_crc32 {
            return Err(StoreError::PayloadChecksum);
        }
        Ok(length)
    }

    fn check_head(&mut self, expected: &Head) -> Result<Head, StoreError<S::Error>> {
        let actual = self.inspect_quick()?;
        if actual.metadata != expected.metadata {
            return Err(StoreError::StaleHead);
        }
        Ok(actual)
    }

    fn read_record(&mut self, slot: Slot) -> Result<Record, StoreError<S::Error>> {
        let mut bytes = [0; METADATA_SIZE];
        self.storage
            .read(self.layout.metadata_offset(slot), &mut bytes)
            .map_err(StoreError::Io)?;
        if zero(&bytes) {
            return Ok(Record::Empty);
        }
        match Metadata::decode(&bytes, self.layout.capacity) {
            Ok(metadata) => Ok(Record::Valid(metadata)),
            // A torn publication may leave the other replica as the current one.
            Err(FormatError::BadChecksum) => Ok(Record::Damaged),
            // A checksummed unknown or malformed schema is not a torn write.
            Err(error) => Err(StoreError::Format(error)),
        }
    }

    fn validate(
        &mut self,
        expected: SnapshotRef,
        payload: bool,
    ) -> Result<(), StoreError<S::Error>> {
        if payload {
            return self.validate_snapshot(expected);
        }
        let mut header = [0; SNAPSHOT_HEADER_SIZE];
        self.storage
            .read(self.layout.snapshot_offset(expected.slot), &mut header)
            .map_err(StoreError::Io)?;
        let decoded =
            SnapshotHeader::decode(&header, self.layout.capacity).map_err(StoreError::Format)?;
        if decoded != expected.header {
            return Err(StoreError::SnapshotMismatch);
        }
        Ok(())
    }

    fn validate_snapshot(&mut self, expected: SnapshotRef) -> Result<(), StoreError<S::Error>> {
        let mut header = [0; SNAPSHOT_HEADER_SIZE];
        let offset = self.layout.snapshot_offset(expected.slot);
        self.storage
            .read(offset, &mut header)
            .map_err(StoreError::Io)?;
        let decoded =
            SnapshotHeader::decode(&header, self.layout.capacity).map_err(StoreError::Format)?;
        if decoded != expected.header {
            return Err(StoreError::SnapshotMismatch);
        }
        let mut crc = Crc32::new();
        let mut buffer = [0; 512];
        let mut remaining = u64::from(decoded.payload_len);
        let mut at = offset + SNAPSHOT_HEADER_SIZE as u64;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            self.storage
                .read(at, &mut buffer[..count])
                .map_err(StoreError::Io)?;
            crc.update(&buffer[..count]);
            at += count as u64;
            remaining -= count as u64;
        }
        if crc.finish() != decoded.payload_crc32 {
            return Err(StoreError::PayloadChecksum);
        }
        Ok(())
    }

    fn write_snapshot(
        &mut self,
        slot: Slot,
        generation: u64,
        payload: &[u8],
    ) -> Result<SnapshotRef, StoreError<S::Error>> {
        let payload_len = u32::try_from(payload.len())
            .map_err(|_| StoreError::Format(FormatError::PayloadTooLarge))?;
        if payload_len > self.layout.capacity {
            return Err(StoreError::Format(FormatError::PayloadTooLarge));
        }
        let header = SnapshotHeader {
            generation,
            payload_len,
            payload_crc32: crate::crc32(payload),
        };
        let bytes = header.encode().map_err(StoreError::Format)?;
        let offset = self.layout.snapshot_offset(slot);
        self.storage
            .write(offset + SNAPSHOT_HEADER_SIZE as u64, payload)
            .map_err(StoreError::Io)?;
        self.storage.write(offset, &bytes).map_err(StoreError::Io)?;
        self.storage.sync().map_err(StoreError::Io)?;
        // Read back before any journal can make this snapshot relevant.
        let reference = SnapshotRef { slot, header };
        self.validate_snapshot(reference)?;
        Ok(reference)
    }

    fn publish(
        &mut self,
        previously_selected: Slot,
        metadata: Metadata,
    ) -> Result<Head, StoreError<S::Error>> {
        let bytes = metadata.encode().map_err(StoreError::Format)?;
        for slot in [previously_selected.other(), previously_selected] {
            let offset = self.layout.metadata_offset(slot);
            self.storage.write(offset, &bytes).map_err(StoreError::Io)?;
            self.storage.sync().map_err(StoreError::Io)?;
            let mut check = [0; METADATA_SIZE];
            self.storage
                .read(offset, &mut check)
                .map_err(StoreError::Io)?;
            if check != bytes {
                return Err(StoreError::Format(FormatError::BadChecksum));
            }
        }
        Ok(Head {
            metadata,
            preferred: Slot::A,
        })
    }

    fn zero_filled(&mut self) -> Result<bool, StoreError<S::Error>> {
        let mut at = 0;
        let length = self.layout.file_len();
        let mut buffer = [0; 512];
        while at < length {
            let count = (length - at).min(buffer.len() as u64) as usize;
            self.storage
                .read(at, &mut buffer[..count])
                .map_err(StoreError::Io)?;
            if !zero(&buffer[..count]) {
                return Ok(false);
            }
            at += count as u64;
        }
        Ok(true)
    }
}
