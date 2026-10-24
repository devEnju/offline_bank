use crate::{
    checksum::Crc32, decide_recovery, format::zero, Fingerprint, FormatError, GameIdentity,
    Metadata, Part, PendingTransfer, Phase, RecoveryDecision, Save, Sides, Slot, METADATA_SIZE,
};

/// Exact I/O on one preallocated container. A successful sync must make all
/// preceding writes durable. Writes must never resize the file or silently short-write.
///
/// A container is made of units that are written independently: a journal
/// record, a copy of a file. On the console an interrupted write leaves the
/// whole file it went to unreadable, not just the bytes it was writing, so
/// every unit is a file of its own there, and a unit that cannot be read is
/// one that an interrupted write left behind.
pub trait Storage {
    type Error;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn sync(&mut self) -> Result<(), Self::Error>;
    /// Discards the unit that is exactly `offset..offset + length`, whatever
    /// state it is in, so that it reads as zeros and can be written again.
    /// Only ever asked for a unit that holds nothing that is still needed.
    fn recreate(&mut self, offset: u64, length: u64) -> Result<(), Self::Error>;
}

/// The Bank container: the two journal records, then the two copies of the
/// Bank file. A copy is the payload and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    capacity: u32,
}

impl Layout {
    pub const fn new(capacity: u32) -> Self {
        Self { capacity }
    }
    pub const fn file_len(self) -> u64 {
        (METADATA_SIZE as u64 * 2) + 2 * self.snapshot_len()
    }
    pub fn metadata_offset(self, slot: Slot) -> u64 {
        record_offset(slot)
    }
    pub fn snapshot_offset(self, slot: Slot) -> u64 {
        METADATA_SIZE as u64 * 2 + slot.index() * self.snapshot_len()
    }
    /// Length of one copy of the Bank file.
    pub const fn snapshot_len(self) -> u64 {
        self.capacity as u64
    }
    /// The four units of the container as (offset, length), in file order:
    /// the two journal records, then the two copies.
    pub fn units(self) -> [(u64, u64); 4] {
        [
            (self.metadata_offset(Slot::A), METADATA_SIZE as u64),
            (self.metadata_offset(Slot::B), METADATA_SIZE as u64),
            (self.snapshot_offset(Slot::A), self.snapshot_len()),
            (self.snapshot_offset(Slot::B), self.snapshot_len()),
        ]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError<E> {
    Io(E),
    Format(FormatError),
    /// Only returned for a completely zero-filled, correctly sized container.
    Uninitialized,
    /// An initialization was begun and cut before its journal record became
    /// durable: no journal record is valid and at least one is still empty.
    /// No Bank was ever current in this container.
    NeverPublished,
    AlreadyContainsData,
    NoValidMetadata,
    ConflictingMetadata,
    PayloadChecksum,
    PendingRecovery,
    NoPendingTransfer,
    /// The game's save is neither image of the save in progress.
    NeitherImage,
    StaleHead,
    CounterExhausted,
    BufferTooSmall {
        needed: u32,
    },
    /// Transporter may not deliver: a save is in progress, the transport box
    /// holds Pokémon, or a delivery waits already.
    DeliveryRefused,
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

enum Record {
    Empty,
    Damaged,
    Valid(Metadata),
}

/// What the two journal records say together.
enum Records {
    Current(Head),
    /// Both are zero: nothing was ever written to them.
    BothEmpty,
    /// None is valid and one is still zero: a first publication was cut.
    OneEmpty,
    /// None is valid and none is empty.
    Unusable,
}

/// The two records are the first units of the Bank container, and the whole
/// of the container a reader of the journal alone is given.
fn record_offset(slot: Slot) -> u64 {
    slot.index() * METADATA_SIZE as u64
}

fn read_record<S: Storage>(storage: &mut S, slot: Slot) -> Result<Record, StoreError<S::Error>> {
    let mut bytes = [0; METADATA_SIZE];
    // A record that cannot be read is one an interrupted publication
    // left behind, as a torn one is; the other replica decides.
    if storage.read(record_offset(slot), &mut bytes).is_err() {
        return Ok(Record::Damaged);
    }
    if zero(&bytes) {
        return Ok(Record::Empty);
    }
    match Metadata::decode(&bytes) {
        Ok(metadata) => Ok(Record::Valid(metadata)),
        // A torn publication may leave the other replica as the current one.
        Err(FormatError::BadChecksum) => Ok(Record::Damaged),
        // A checksummed unknown or malformed schema is not a torn write.
        Err(error) => Err(StoreError::Format(error)),
    }
}

fn records<S: Storage>(storage: &mut S) -> Result<Records, StoreError<S::Error>> {
    let a = read_record(storage, Slot::A)?;
    let b = read_record(storage, Slot::B)?;
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
        (Record::Empty, Record::Empty) => return Ok(Records::BothEmpty),
        // An initialization writes one record after the other into empty
        // units: until the first one is durable at least one of them is
        // still empty. Two unreadable records say nothing of the kind and
        // are never taken for it.
        (Record::Empty, _) | (_, Record::Empty) => return Ok(Records::OneEmpty),
        _ => return Ok(Records::Unusable),
    };
    Ok(Records::Current(Head {
        metadata,
        preferred,
    }))
}

/// Writes `metadata` to both replicas, the one that was not selected first,
/// each made durable and read back before the other is touched.
fn publish<S: Storage>(
    storage: &mut S,
    previously_selected: Slot,
    metadata: Metadata,
) -> Result<Head, StoreError<S::Error>> {
    let bytes = metadata.encode().map_err(StoreError::Format)?;
    for slot in [previously_selected.other(), previously_selected] {
        put_replica(storage, slot, &bytes)?;
    }
    Ok(Head {
        metadata,
        preferred: Slot::A,
    })
}

/// Writes one replica. The other replica is complete while this one is
/// written, so one that an interrupted write left unusable is discarded and
/// written once more.
fn put_replica<S: Storage>(
    storage: &mut S,
    slot: Slot,
    bytes: &[u8; METADATA_SIZE],
) -> Result<(), StoreError<S::Error>> {
    let offset = record_offset(slot);
    if put_record(storage, offset, bytes).is_err() {
        storage
            .recreate(offset, METADATA_SIZE as u64)
            .map_err(StoreError::Io)?;
        put_record(storage, offset, bytes)?;
    }
    Ok(())
}

/// Transporter's write of `metadata`: the replica that was not selected
/// first, and the record is in force once that one is durable and read
/// back; the other is then written too. If that one cannot be written, the
/// journal is as after a power cut between the two, and Bank's next write
/// of the journal writes it. An error means that the first replica was not
/// written and read back; only if its read-back alone failed can the record
/// be in force all the same.
fn hand_over<S: Storage>(
    storage: &mut S,
    selected: Slot,
    metadata: Metadata,
) -> Result<Head, StoreError<S::Error>> {
    let bytes = metadata.encode().map_err(StoreError::Format)?;
    let first = selected.other();
    put_replica(storage, first, &bytes)?;
    let _ = put_replica(storage, selected, &bytes);
    Ok(Head {
        metadata,
        preferred: first,
    })
}

/// Whether both replicas can be read, valid or not.
fn both_readable<S: Storage>(storage: &mut S) -> bool {
    let mut bytes = [0; METADATA_SIZE];
    [Slot::A, Slot::B]
        .into_iter()
        .all(|slot| storage.read(record_offset(slot), &mut bytes).is_ok())
}

fn put_record<S: Storage>(
    storage: &mut S,
    offset: u64,
    bytes: &[u8; METADATA_SIZE],
) -> Result<(), StoreError<S::Error>> {
    storage.write(offset, bytes).map_err(StoreError::Io)?;
    storage.sync().map_err(StoreError::Io)?;
    let mut check = [0; METADATA_SIZE];
    storage.read(offset, &mut check).map_err(StoreError::Io)?;
    if check != *bytes {
        return Err(StoreError::Format(FormatError::BadChecksum));
    }
    Ok(())
}

fn next_sequence<E>(head: &Head) -> Result<u32, StoreError<E>> {
    head.metadata
        .sequence
        .checked_add(1)
        .ok_or(StoreError::CounterExhausted)
}

/// The journal alone, for a reader or writer that has nothing to do with the
/// Bank's other files: Transporter. Its storage is the two records.
pub struct Journal<S> {
    storage: S,
}

impl<S: Storage> Journal<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }
    pub fn into_inner(self) -> S {
        self.storage
    }

    /// The record in force. `None` while no Bank exists: no record was ever
    /// published. Two records of which neither is valid nor empty are an
    /// error, never "no Bank".
    pub fn load(&mut self) -> Result<Option<Head>, StoreError<S::Error>> {
        match records(&mut self.storage)? {
            Records::Current(head) => Ok(Some(head)),
            Records::BothEmpty | Records::OneEmpty => Ok(None),
            Records::Unusable => Err(StoreError::NoValidMetadata),
        }
    }

    /// Whether both records can be read, valid or not. One that an
    /// interrupted write left unreadable cannot be written either until it
    /// is replaced, which only Bank does, with its next save.
    pub fn readable(&mut self) -> bool {
        both_readable(&mut self.storage)
    }

    /// Publishes a delivery whose bytes, with the checksum `crc`, are durable
    /// in the transport file's copy that is not in use. The delivery exists
    /// from the moment the first replica is durable, and that is what this
    /// answers for (`hand_over`). Only if the first replica's read-back alone
    /// fails can a delivery be there after an error, which leaves the
    /// Pokémon in the game as well.
    pub fn announce(&mut self, head: &Head, crc: u32) -> Result<Head, StoreError<S::Error>> {
        // The record in force, and the replica that holds it.
        let selected = match self.load()? {
            Some(now) if now.metadata == head.metadata => now.preferred,
            _ => return Err(StoreError::StaleHead),
        };
        if !may_deliver(&head.metadata) {
            return Err(StoreError::DeliveryRefused);
        }
        let metadata = announcing(head, crc)?;
        hand_over(&mut self.storage, selected, metadata)
    }

    /// After `announce` with `head` and `crc` failed: whether the delivery
    /// is there all the same, by the record in force now, as Bank will read
    /// it. `true`: it announces the delivery. `false`: it is still `head`'s
    /// own, and nothing was announced. Any other record, or records that do
    /// not read, are an error: whether Bank holds the delivery cannot be told.
    /// Both records must read: with one unreadable the other is in force
    /// now, but the unreadable one may hold the announcement and be in force
    /// once it reads again.
    pub fn announced(&mut self, head: &Head, crc: u32) -> Result<bool, StoreError<S::Error>> {
        if !self.readable() {
            return Err(StoreError::NoValidMetadata);
        }
        let now = self.load()?.ok_or(StoreError::StaleHead)?.metadata;
        if now == head.metadata {
            Ok(false)
        } else if now == announcing(head, crc)? {
            Ok(true)
        } else {
            Err(StoreError::StaleHead)
        }
    }

    /// Settles the save in progress that `head` left to Transporter's
    /// question (`Metadata::ask`), to the snapshot of `decision`, as Bank's
    /// reconciliation settles one (`settled`). Only the journal is written,
    /// and the answer holds from the moment the first replica is durable
    /// (`hand_over`): only if that one's read-back alone fails can it hold
    /// after an error.
    pub fn settle(
        &mut self,
        head: &Head,
        decision: RecoveryDecision,
    ) -> Result<Head, StoreError<S::Error>> {
        let selected = match self.load()? {
            Some(now) if now.metadata == head.metadata => now.preferred,
            _ => return Err(StoreError::StaleHead),
        };
        if !head.metadata.ask {
            return Err(StoreError::NoPendingTransfer);
        }
        let metadata = settled(head, decision)?;
        hand_over(&mut self.storage, selected, metadata)
    }
}

/// The record that settles the save in progress of `head` to the snapshot of
/// `decision`, for Bank's reconciliation and Transporter's question alike. A
/// save that becomes current has taken the delivery that waited, and the
/// same record says so: the delivery cannot be taken twice. A save that is
/// undone leaves it waiting.
fn settled<E>(head: &Head, decision: RecoveryDecision) -> Result<Metadata, StoreError<E>> {
    let Phase::Prepared(pending) = head.phase() else {
        return Err(StoreError::NoPendingTransfer);
    };
    let (selected, delivery) = match decision {
        RecoveryDecision::KeepBefore => (pending.before, head.metadata.delivery),
        RecoveryDecision::CommitAfter => (pending.after, None),
    };
    Ok(Metadata {
        sequence: next_sequence(head)?,
        phase: Phase::Clean(selected),
        delivery,
        ask: false,
    })
}

/// The record that announces a delivery with checksum `crc` after `head`.
fn announcing<E>(head: &Head, crc: u32) -> Result<Metadata, StoreError<E>> {
    Ok(Metadata {
        sequence: next_sequence(head)?,
        delivery: Some(crc),
        ..head.metadata
    })
}

/// Transporter's rule: no save is in progress, the transport box is empty,
/// and nothing waits already.
pub fn may_deliver(metadata: &Metadata) -> bool {
    matches!(metadata.phase, Phase::Clean(current) if current.held == 0)
        && metadata.delivery.is_none()
}

/// Does not cache a current head. Each mutation verifies its input head against
/// durable metadata; callers must still ensure exclusive access to storage.
pub struct BankStore<S> {
    storage: S,
    layout: Layout,
}

impl<S: Storage> BankStore<S> {
    pub fn new(storage: S, layout: Layout) -> Self {
        Self { storage, layout }
    }
    pub fn into_inner(self) -> S {
        self.storage
    }
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// The record in force, from both journal records. The copies of the
    /// Bank file it names are checked by the single pass of `read_current`
    /// or `read_pending`, so a load reads every payload byte once.
    pub fn inspect(&mut self) -> Result<Head, StoreError<S::Error>> {
        match records(&mut self.storage)? {
            Records::Current(head) => Ok(head),
            Records::BothEmpty if self.zero_filled() => Err(StoreError::Uninitialized),
            // An initialization writes one record after the other into
            // empty units: until the first one is durable at least one of
            // them is still empty.
            Records::BothEmpty | Records::OneEmpty if self.only_a_first_copy() => {
                Err(StoreError::NeverPublished)
            }
            _ => Err(StoreError::NoValidMetadata),
        }
    }

    /// Explicitly initializes a newly created, completely zero-filled file.
    /// The caller supplies the native Bank representation of an empty payload
    /// and what it has written beside it, into the first copy of each file.
    /// An interrupted initialization is reported as `NeverPublished` and is
    /// finished by `reinitialize`, never here.
    pub fn initialize_new(
        &mut self,
        payload: &[u8],
        sides: Sides,
    ) -> Result<Head, StoreError<S::Error>> {
        if !self.zero_filled() {
            return Err(StoreError::AlreadyContainsData);
        }
        let bank = self.write_snapshot(Slot::A, payload)?;
        let first = |crc| Part { slot: Slot::A, crc };
        publish(
            &mut self.storage,
            Slot::B,
            Metadata {
                sequence: 1,
                phase: Phase::Clean(Save {
                    bank,
                    main: first(sides.main),
                    mover: first(sides.mover),
                    held: sides.held,
                }),
                delivery: None,
                ask: false,
            },
        )
    }

    /// Finishes an initialization that was cut before its journal record
    /// became durable (`NeverPublished`), or initializes a zero-filled
    /// container. Anything else is refused: a container that ever held a
    /// current Bank has a journal record, and that state is not touched here.
    pub fn reinitialize(
        &mut self,
        payload: &[u8],
        sides: Sides,
    ) -> Result<Head, StoreError<S::Error>> {
        match self.inspect() {
            Err(StoreError::Uninitialized) => return self.initialize_new(payload, sides),
            Err(StoreError::NeverPublished) => {}
            Ok(_) => return Err(StoreError::AlreadyContainsData),
            Err(error) => return Err(error),
        }
        // The journal records first: a cut here leaves the same state, to
        // be finished next time.
        for (offset, length) in self.layout.units() {
            self.storage
                .recreate(offset, length)
                .map_err(StoreError::Io)?;
        }
        self.initialize_new(payload, sides)
    }

    /// Durably prepares both metadata replicas before returning success.
    /// The caller must have written the main file and the transport box of
    /// this save, with the checksums in `sides`, into the copies the save
    /// before leaves free (`Save::next_mover`); it must verify the before
    /// game state immediately before this call and must not mutate the game
    /// if this call returns an error.
    pub fn prepare_transfer(
        &mut self,
        head: &Head,
        payload: &[u8],
        sides: Sides,
        game: GameIdentity,
        before_fingerprint: Fingerprint,
        after_fingerprint: Fingerprint,
    ) -> Result<Head, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let current = match head.phase() {
            Phase::Clean(current) => current,
            Phase::Prepared(_) => return Err(StoreError::PendingRecovery),
        };
        if before_fingerprint == after_fingerprint {
            return Err(StoreError::Format(FormatError::IndistinguishableGameStates));
        }
        let sequence = next_sequence(&head)?;
        let after = Save {
            bank: Part {
                slot: current.bank.slot.other(),
                crc: crate::crc32(payload),
            },
            main: Part {
                slot: current.main.slot.other(),
                crc: sides.main,
            },
            mover: Part {
                slot: current.next_mover(head.metadata.delivery.is_some()),
                crc: sides.mover,
            },
            held: sides.held,
        };
        let metadata = Metadata {
            sequence,
            phase: Phase::Prepared(PendingTransfer {
                before: current,
                after,
                game,
                before_fingerprint,
                after_fingerprint,
            }),
            ..head.metadata
        };
        // Refuse what cannot be recorded before anything is written.
        metadata.encode().map_err(StoreError::Format)?;
        self.write_snapshot(after.bank.slot, payload)?;
        publish(&mut self.storage, head.preferred, metadata)
    }

    /// Resolves a prepared transfer by the fingerprint of the game's actual
    /// save, which is one of its two images or is refused without writes
    /// (`settled`).
    pub fn reconcile(
        &mut self,
        head: &Head,
        fingerprint: Fingerprint,
    ) -> Result<Reconciled, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let pending = match head.phase() {
            Phase::Prepared(pending) => pending,
            Phase::Clean(_) => return Err(StoreError::NoPendingTransfer),
        };
        let decision = decide_recovery(&pending, fingerprint).ok_or(StoreError::NeitherImage)?;
        let metadata = settled(&head, decision)?;
        let head = publish(&mut self.storage, head.preferred, metadata)?;
        Ok(Reconciled { head, decision })
    }

    /// Leaves the save in progress to Transporter's question, or takes it
    /// back (`Metadata::ask`): the same record with the next sequence number
    /// and `ask`. Nothing is written when the record says so already, unless
    /// it leaves the save to the question while one replica does not read:
    /// Transporter reads the question only from a journal whose replicas
    /// both read, and replaces none, so the record is written again, which
    /// replaces that replica (`put_replica`).
    pub fn ask(&mut self, head: &Head, ask: bool) -> Result<Head, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        if !matches!(head.phase(), Phase::Prepared(_)) {
            return Err(StoreError::NoPendingTransfer);
        }
        if head.metadata.ask == ask && (!ask || both_readable(&mut self.storage)) {
            return Ok(head);
        }
        let sequence = next_sequence(&head)?;
        publish(
            &mut self.storage,
            head.preferred,
            Metadata {
                sequence,
                ask,
                ..head.metadata
            },
        )
    }

    /// Test builds only (docs/building.md): publishes the clean record again,
    /// with the next sequence number and nothing else changed. A journal
    /// write that changes nothing the Bank shows, so that a real power cut
    /// can be made to land inside one.
    #[cfg(feature = "tear-test")]
    pub fn republish(&mut self, head: &Head) -> Result<Head, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        if !matches!(head.phase(), Phase::Clean(_)) {
            return Err(StoreError::PendingRecovery);
        }
        let sequence = next_sequence(&head)?;
        publish(
            &mut self.storage,
            head.preferred,
            Metadata {
                sequence,
                ..head.metadata
            },
        )
    }

    /// Test builds only: writes `payload` into the spare copy of the Bank
    /// file `passes` times, inverted every other time, without publishing it.
    /// The copy in use is never touched.
    #[cfg(feature = "tear-test")]
    pub fn rewrite_spare(
        &mut self,
        head: &Head,
        payload: &mut [u8],
        passes: u32,
    ) -> Result<(), StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let Phase::Clean(current) = head.phase() else {
            return Err(StoreError::PendingRecovery);
        };
        for _ in 0..passes {
            for byte in payload.iter_mut() {
                *byte = !*byte;
            }
            self.write_snapshot(current.bank.slot.other(), payload)?;
        }
        Ok(())
    }

    /// Reads the Bank file of the current save into `out` and checks it.
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
        self.read_snapshot(current.bank, out)
    }

    /// Reads the Bank file of one side of a save in progress.
    pub fn read_pending(
        &mut self,
        head: &Head,
        after: bool,
        out: &mut [u8],
    ) -> Result<usize, StoreError<S::Error>> {
        let head = self.check_head(head)?;
        let save = match head.phase() {
            Phase::Prepared(pending) if after => pending.after,
            Phase::Prepared(pending) => pending.before,
            Phase::Clean(_) => return Err(StoreError::NoPendingTransfer),
        };
        self.read_snapshot(save.bank, out)
    }

    fn read_snapshot(&mut self, part: Part, out: &mut [u8]) -> Result<usize, StoreError<S::Error>> {
        let length = self.layout.capacity as usize;
        if out.len() < length {
            return Err(StoreError::BufferTooSmall {
                needed: self.layout.capacity,
            });
        }
        self.storage
            .read(self.layout.snapshot_offset(part.slot), &mut out[..length])
            .map_err(StoreError::Io)?;
        if crate::crc32(&out[..length]) != part.crc {
            return Err(StoreError::PayloadChecksum);
        }
        Ok(length)
    }

    fn check_head(&mut self, expected: &Head) -> Result<Head, StoreError<S::Error>> {
        let actual = self.inspect()?;
        if actual.metadata != expected.metadata {
            return Err(StoreError::StaleHead);
        }
        Ok(actual)
    }

    /// Reads a copy of the Bank file and compares it with its checksum.
    fn validate(&mut self, expected: Part) -> Result<(), StoreError<S::Error>> {
        let mut crc = Crc32::new();
        let mut buffer = [0; 512];
        let mut remaining = self.layout.snapshot_len();
        let mut at = self.layout.snapshot_offset(expected.slot);
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            self.storage
                .read(at, &mut buffer[..count])
                .map_err(StoreError::Io)?;
            crc.update(&buffer[..count]);
            at += count as u64;
            remaining -= count as u64;
        }
        if crc.finish() != expected.crc {
            return Err(StoreError::PayloadChecksum);
        }
        Ok(())
    }

    fn write_snapshot(&mut self, slot: Slot, payload: &[u8]) -> Result<Part, StoreError<S::Error>> {
        if payload.len() as u64 != self.layout.snapshot_len() {
            return Err(StoreError::Format(FormatError::PayloadSize));
        }
        let part = Part {
            slot,
            crc: crate::crc32(payload),
        };
        // The copy is the spare one. An interrupted write may have left it
        // unreadable, and then it may also refuse to be written: it is
        // discarded and written once more.
        if self.put_snapshot(part, payload).is_err() {
            self.storage
                .recreate(
                    self.layout.snapshot_offset(slot),
                    self.layout.snapshot_len(),
                )
                .map_err(StoreError::Io)?;
            self.put_snapshot(part, payload)?;
        }
        Ok(part)
    }

    fn put_snapshot(&mut self, part: Part, payload: &[u8]) -> Result<(), StoreError<S::Error>> {
        self.storage
            .write(self.layout.snapshot_offset(part.slot), payload)
            .map_err(StoreError::Io)?;
        self.storage.sync().map_err(StoreError::Io)?;
        // Read back before any journal can make this copy relevant.
        self.validate(part)
    }

    /// True when the Bank file holds nothing but what an initialization
    /// writes: its second copy, which only a later save writes, is empty.
    /// A copy that cannot be read is one a cut initialization left behind.
    fn only_a_first_copy(&mut self) -> bool {
        let start = self.layout.snapshot_offset(Slot::B);
        self.reads_zero(start, start + self.layout.snapshot_len())
            .unwrap_or(true)
    }

    /// Whether every byte reads as zero. A part that cannot be read is not.
    fn zero_filled(&mut self) -> bool {
        self.reads_zero(0, self.layout.file_len()).unwrap_or(false)
    }

    fn reads_zero(&mut self, mut at: u64, end: u64) -> Result<bool, S::Error> {
        let mut buffer = [0; 512];
        while at < end {
            let count = (end - at).min(buffer.len() as u64) as usize;
            self.storage.read(at, &mut buffer[..count])?;
            if !zero(&buffer[..count]) {
                return Ok(false);
            }
            at += count as u64;
        }
        Ok(true)
    }
}
