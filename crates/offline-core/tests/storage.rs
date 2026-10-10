use offline_core::*;

const CAPACITY: u32 = 96;
const OLD: &[u8] = b"opaque old Bank state";
const NEW: &[u8] = b"opaque after-transfer Bank state";
const GAME: GameIdentity = GameIdentity {
    title_id: 0x0004_0000_0011_c400,
    save_identity: [7; 32],
};
const BEFORE: Fingerprint = [1; 32];
const AFTER: Fingerprint = [2; 32];

#[derive(Clone, Debug)]
struct Memory {
    bytes: Vec<u8>,
    durable: Option<Vec<u8>>,
    budget: Option<usize>,
    operations: usize,
    /// The console's behaviour: every unit is a file of its own, and a cut
    /// leaves each file with writes that were not yet synced unreadable and
    /// unwritable until it is recreated.
    console: bool,
    units: [(u64, u64); 4],
    unsynced: [bool; 4],
    unreadable: [bool; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    PowerCut,
    OutOfBounds,
    Unreadable,
}

impl Memory {
    fn new(layout: Layout) -> Self {
        Self {
            bytes: vec![0; layout.file_len() as usize],
            durable: None,
            budget: None,
            operations: 0,
            console: false,
            units: layout.units(),
            unsynced: [false; 4],
            unreadable: [false; 4],
        }
    }
    fn console(mut self) -> Self {
        self.console = true;
        self
    }
    /// The units a byte range lies in.
    fn touched(&self, start: u64, length: u64) -> impl Iterator<Item = usize> + '_ {
        (0..4).filter(move |&unit| {
            let (at, size) = self.units[unit];
            start < at + size && at < start + length
        })
    }
    fn tick(&mut self) -> Result<(), Failure> {
        if let Some(budget) = &mut self.budget {
            if *budget == 0 {
                if self.console {
                    for unit in 0..4 {
                        self.unreadable[unit] |= self.unsynced[unit];
                    }
                }
                return Err(Failure::PowerCut);
            }
            *budget -= 1;
        }
        self.operations += 1;
        Ok(())
    }
    fn cut_after(mut self, budget: usize) -> Self {
        self.budget = Some(budget);
        self.operations = 0;
        self
    }
    fn buffered(mut self) -> Self {
        self.durable = Some(self.bytes.clone());
        self
    }
    fn reboot(mut self) -> Self {
        if let Some(durable) = &self.durable {
            self.bytes.clone_from(durable);
        }
        self.budget = None;
        self.operations = 0;
        self.unsynced = [false; 4];
        self
    }
}

impl Storage for Memory {
    type Error = Failure;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Failure> {
        let start = usize::try_from(offset).map_err(|_| Failure::OutOfBounds)?;
        let end = start.checked_add(bytes.len()).ok_or(Failure::OutOfBounds)?;
        let source = self.bytes.get(start..end).ok_or(Failure::OutOfBounds)?;
        if self
            .touched(offset, bytes.len() as u64)
            .any(|unit| self.unreadable[unit])
        {
            return Err(Failure::Unreadable);
        }
        bytes.copy_from_slice(source);
        Ok(())
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Failure> {
        let start = usize::try_from(offset).map_err(|_| Failure::OutOfBounds)?;
        let end = start.checked_add(bytes.len()).ok_or(Failure::OutOfBounds)?;
        if end > self.bytes.len() {
            return Err(Failure::OutOfBounds);
        }
        let touched: Vec<usize> = self.touched(offset, bytes.len() as u64).collect();
        if touched.iter().any(|&unit| self.unreadable[unit]) {
            return Err(Failure::Unreadable);
        }
        for unit in touched {
            self.unsynced[unit] = true;
        }
        // Every byte may reach durable media before a flush: a conservative
        // model of interrupted writes, including partially written headers.
        for (index, byte) in bytes.iter().enumerate() {
            self.tick()?;
            self.bytes[start + index] = *byte;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Failure> {
        self.tick()?;
        self.unsynced = [false; 4];
        if let Some(durable) = &mut self.durable {
            durable.clone_from(&self.bytes);
        }
        Ok(())
    }
    fn recreate(&mut self, offset: u64, length: u64) -> Result<(), Failure> {
        let unit = self
            .units
            .iter()
            .position(|&unit| unit == (offset, length))
            .ok_or(Failure::OutOfBounds)?;
        self.tick()?;
        self.bytes[offset as usize..(offset + length) as usize].fill(0);
        self.unsynced[unit] = false;
        self.unreadable[unit] = false;
        Ok(())
    }
}

fn baseline() -> Memory {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(Memory::new(layout), layout);
    store.initialize_new(OLD).unwrap();
    store.into_inner().reboot()
}

fn observation(fingerprint: Fingerprint) -> GameObservation {
    GameObservation::Present {
        game: GAME,
        fingerprint,
    }
}

fn prepare(memory: Memory) -> (BankStore<Memory>, Head) {
    let mut store = BankStore::new(memory, Layout::new(CAPACITY));
    let head = store.inspect().unwrap();
    let head = store
        .prepare_transfer(&head, NEW, GAME, BEFORE, AFTER)
        .unwrap();
    (store, head)
}

fn payload(store: &mut BankStore<Memory>, head: &Head) -> Vec<u8> {
    let mut out = [0; CAPACITY as usize];
    let length = store.read_current(head, &mut out).unwrap();
    out[..length].to_vec()
}

#[test]
fn crc_matches_published_check_vector() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn snapshot_is_explicit_little_endian_and_checks_all_header_bytes() {
    let header = SnapshotHeader {
        generation: 0x0102_0304_0506_0708,
        payload_len: 19,
        payload_crc32: 0x89ab_cdef,
    };
    let encoded = header.encode().unwrap();
    assert_eq!(&encoded[..8], b"BKOFSNAP");
    assert_eq!(&encoded[16..24], &[8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(&encoded[24..28], &[0xef, 0xcd, 0xab, 0x89]);
    assert_eq!(SnapshotHeader::decode(&encoded, 19), Ok(header));
    assert_eq!(
        SnapshotHeader::decode(&encoded, 18),
        Err(FormatError::PayloadTooLarge)
    );
    for at in 0..encoded.len() {
        let mut corrupt = encoded;
        corrupt[at] ^= 1;
        assert!(
            SnapshotHeader::decode(&corrupt, 19).is_err(),
            "undetected byte {at}"
        );
    }
    for end in 0..encoded.len() {
        assert_eq!(
            SnapshotHeader::decode(&encoded[..end], 19),
            Err(FormatError::Truncated)
        );
    }
}

#[test]
fn fresh_storage_requires_explicit_initialization() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(Memory::new(layout), layout);
    assert_eq!(store.inspect(), Err(StoreError::Uninitialized));
    let head = store.initialize_new(OLD).unwrap();
    assert_eq!(payload(&mut store, &head), OLD);
    assert_eq!(
        store.initialize_new(b""),
        Err(StoreError::AlreadyContainsData)
    );
}

#[test]
fn nonzero_or_truncated_uninitialized_storage_is_not_empty() {
    let layout = Layout::new(CAPACITY);
    let mut memory = Memory::new(layout);
    *memory.bytes.last_mut().unwrap() = 1;
    let mut store = BankStore::new(memory, layout);
    // No journal record and no snapshot: an initialization that was cut.
    assert_eq!(store.inspect(), Err(StoreError::NeverPublished));
    assert_eq!(
        store.initialize_new(b""),
        Err(StoreError::AlreadyContainsData)
    );

    // A part that cannot be read is not zero either.
    let mut memory = Memory::new(layout);
    memory.bytes.pop();
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::NeverPublished));
}

#[test]
fn local_commit_and_reopen_preserve_payload() {
    let mut store = BankStore::new(baseline(), Layout::new(CAPACITY));
    let old_head = store.inspect().unwrap();
    let next = store.commit_bank_only(&old_head, NEW).unwrap();
    assert_eq!(payload(&mut store, &next), NEW);
    assert_eq!(
        store.commit_bank_only(&old_head, OLD),
        Err(StoreError::StaleHead)
    );
    let mut store = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
    let reopened = store.inspect().unwrap();
    assert_eq!(reopened.metadata(), next.metadata());
    assert_eq!(payload(&mut store, &reopened), NEW);
}

#[test]
fn transfer_blocks_unresolved_reads_and_further_changes() {
    let (mut store, head) = prepare(baseline());
    assert_eq!(
        store.read_current(&head, &mut [0; 96]),
        Err(StoreError::PendingRecovery)
    );
    assert_eq!(
        store.commit_bank_only(&head, OLD),
        Err(StoreError::PendingRecovery)
    );
    assert_eq!(
        store.prepare_transfer(&head, OLD, GAME, AFTER, BEFORE),
        Err(StoreError::PendingRecovery)
    );
    let result = store.reconcile(&head, observation(AFTER)).unwrap();
    assert_eq!(result.decision, RecoveryDecision::CommitAfter);
    assert_eq!(payload(&mut store, &result.head), NEW);
}

#[test]
fn observed_before_aborts_and_observed_after_commits() {
    for (fingerprint, expected, decision) in [
        (BEFORE, OLD, RecoveryDecision::KeepBefore),
        (AFTER, NEW, RecoveryDecision::CommitAfter),
    ] {
        let (mut store, head) = prepare(baseline());
        let result = store.reconcile(&head, observation(fingerprint)).unwrap();
        assert_eq!(result.decision, decision);
        assert_eq!(payload(&mut store, &result.head), expected);
    }
}

#[test]
fn missing_other_or_modified_game_blocks_without_writes() {
    let observations = [
        (GameObservation::Missing, RecoveryBlock::MissingGame),
        (
            GameObservation::Present {
                game: GameIdentity {
                    title_id: GAME.title_id + 1,
                    ..GAME
                },
                fingerprint: BEFORE,
            },
            RecoveryBlock::DifferentGame,
        ),
        (
            GameObservation::Present {
                game: GameIdentity {
                    save_identity: [8; 32],
                    ..GAME
                },
                fingerprint: BEFORE,
            },
            RecoveryBlock::DifferentGame,
        ),
        (observation([3; 32]), RecoveryBlock::ModifiedGame),
    ];
    for (observed, reason) in observations {
        let (store, head) = prepare(baseline());
        let before = store.into_inner();
        let mut store = BankStore::new(before.clone(), Layout::new(CAPACITY));
        assert_eq!(
            store.reconcile(&head, observed),
            Err(StoreError::RecoveryBlocked(reason))
        );
        assert_eq!(store.into_inner().bytes, before.bytes);
    }
}

#[test]
fn payload_capacity_and_ambiguous_game_states_fail_before_writes() {
    for (bytes, before, after, expected) in [
        (&[9; 97][..], BEFORE, AFTER, FormatError::PayloadTooLarge),
        (
            NEW,
            BEFORE,
            BEFORE,
            FormatError::IndistinguishableGameStates,
        ),
    ] {
        let initial = baseline();
        let mut store = BankStore::new(initial.clone(), Layout::new(CAPACITY));
        let head = store.inspect().unwrap();
        assert_eq!(
            store.prepare_transfer(&head, bytes, GAME, before, after),
            Err(StoreError::Format(expected))
        );
        assert_eq!(store.into_inner().bytes, initial.bytes);
    }
}

#[test]
fn wrong_capacity_is_rejected_even_when_current_snapshot_uses_first_slot() {
    let mut store = BankStore::new(baseline(), Layout::new(CAPACITY - 1));
    assert_eq!(
        store.inspect(),
        Err(StoreError::Format(FormatError::CapacityMismatch))
    );
}

#[test]
fn unknown_schema_does_not_fall_back_to_an_older_replica() {
    let layout = Layout::new(CAPACITY);
    let mut memory = baseline();
    let start = layout.metadata_offset(Slot::B) as usize;
    memory.bytes[start + 8..start + 10].copy_from_slice(&2u16.to_le_bytes());
    let checksum = crc32(&memory.bytes[start..start + 188]);
    memory.bytes[start + 188..start + 192].copy_from_slice(&checksum.to_le_bytes());
    let mut store = BankStore::new(memory, layout);
    assert_eq!(
        store.inspect(),
        Err(StoreError::Format(FormatError::UnsupportedVersion(2)))
    );
}

#[test]
fn snapshot_corruption_is_an_error_without_old_snapshot_fallback() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(baseline(), layout);
    let head = store.inspect().unwrap();
    let head = store.commit_bank_only(&head, NEW).unwrap();
    let snapshot = match head.phase() {
        Phase::Clean(s) => s,
        _ => unreachable!(),
    };
    let mut memory = store.into_inner();
    memory.bytes[layout.snapshot_offset(snapshot.slot) as usize + SNAPSHOT_HEADER_SIZE] ^= 1;
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::PayloadChecksum));
}

#[test]
fn mirrored_journal_survives_one_damaged_metadata_record() {
    let layout = Layout::new(CAPACITY);
    for slot in [Slot::A, Slot::B] {
        let (store, _) = prepare(baseline());
        let mut memory = store.into_inner().reboot();
        memory.bytes[layout.metadata_offset(slot) as usize + 80] ^= 1;
        let mut store = BankStore::new(memory, layout);
        let head = store.inspect().unwrap();
        let result = store.reconcile(&head, observation(AFTER)).unwrap();
        assert_eq!(payload(&mut store, &result.head), NEW);
    }
}

#[test]
fn equal_sequence_disagreement_is_rejected() {
    let layout = Layout::new(CAPACITY);
    let (store, head) = prepare(baseline());
    let mut memory = store.into_inner();
    let pending = match head.phase() {
        Phase::Prepared(p) => p,
        _ => unreachable!(),
    };
    let conflicting = Metadata {
        sequence: head.metadata().sequence,
        capacity: CAPACITY,
        phase: Phase::Clean(pending.before),
    };
    let start = layout.metadata_offset(Slot::B) as usize;
    memory.bytes[start..start + METADATA_SIZE].copy_from_slice(&conflicting.encode().unwrap());
    assert_eq!(
        BankStore::new(memory, layout).inspect(),
        Err(StoreError::ConflictingMetadata)
    );
}

#[test]
fn every_prepare_write_boundary_recovers_old_when_game_was_not_changed() {
    let base = baseline();
    let (complete, _) = prepare(base.clone());
    let total = complete.into_inner().operations;
    assert!(total > 400);
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let head = store.inspect().unwrap();
        let result = store.prepare_transfer(&head, NEW, GAME, BEFORE, AFTER);
        if cut < total {
            assert!(result.is_err(), "cut {cut} unexpectedly succeeded");
        }
        let memory = store.into_inner().reboot();
        let mut recovered = BankStore::new(memory, Layout::new(CAPACITY));
        let head = recovered
            .inspect()
            .unwrap_or_else(|err| panic!("prepare cut {cut}: {err:?}"));
        let head = if matches!(head.phase(), Phase::Prepared(_)) {
            recovered
                .reconcile(&head, observation(BEFORE))
                .unwrap()
                .head
        } else {
            head
        };
        assert_eq!(payload(&mut recovered, &head), OLD, "prepare cut {cut}");
    }
}

#[test]
fn every_finish_write_boundary_recovers_new_after_game_was_saved() {
    let (prepared, _) = prepare(baseline());
    let base = prepared.into_inner().reboot();
    let mut full = BankStore::new(base.clone(), Layout::new(CAPACITY));
    let head = full.inspect().unwrap();
    full.reconcile(&head, observation(AFTER)).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let head = store.inspect().unwrap();
        let _ = store.reconcile(&head, observation(AFTER));
        let mut recovered = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
        let head = recovered
            .inspect()
            .unwrap_or_else(|err| panic!("finish cut {cut}: {err:?}"));
        let head = if matches!(head.phase(), Phase::Prepared(_)) {
            recovered.reconcile(&head, observation(AFTER)).unwrap().head
        } else {
            head
        };
        assert_eq!(payload(&mut recovered, &head), NEW, "finish cut {cut}");
    }
}

#[test]
fn every_local_commit_write_boundary_exposes_a_complete_snapshot() {
    let base = baseline();
    let mut full = BankStore::new(base.clone(), Layout::new(CAPACITY));
    let head = full.inspect().unwrap();
    full.commit_bank_only(&head, NEW).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let head = store.inspect().unwrap();
        let _ = store.commit_bank_only(&head, NEW);
        let mut recovered = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
        let head = recovered
            .inspect()
            .unwrap_or_else(|err| panic!("local cut {cut}: {err:?}"));
        let bytes = payload(&mut recovered, &head);
        assert!(bytes == OLD || bytes == NEW, "local cut {cut}");
    }
}

#[test]
fn every_initialization_boundary_is_valid_or_can_be_finished() {
    let layout = Layout::new(CAPACITY);
    let mut full = BankStore::new(Memory::new(layout), layout);
    full.initialize_new(OLD).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(Memory::new(layout).cut_after(cut), layout);
        let _ = store.initialize_new(OLD);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        match recovered.inspect() {
            Ok(head) => {
                assert_eq!(payload(&mut recovered, &head), OLD);
                // A Bank is current: it is never initialized again.
                assert_eq!(
                    recovered.reinitialize(NEW),
                    Err(StoreError::AlreadyContainsData)
                );
            }
            Err(StoreError::Uninitialized) => assert_eq!(cut, 0),
            Err(StoreError::NeverPublished) => {
                // Only the explicit continuation finishes it, with whatever
                // the caller supplies now.
                assert_eq!(
                    recovered.initialize_new(OLD),
                    Err(StoreError::AlreadyContainsData)
                );
                let head = recovered
                    .reinitialize(NEW)
                    .unwrap_or_else(|err| panic!("finish after cut {cut}: {err:?}"));
                assert_eq!(payload(&mut recovered, &head), NEW);
                let mut reopened = BankStore::new(recovered.into_inner().reboot(), layout);
                let head = reopened.inspect().unwrap();
                assert_eq!(payload(&mut reopened, &head), NEW);
            }
            other => panic!("initialization cut {cut}: {other:?}"),
        }
    }
}

#[test]
fn finishing_an_initialization_can_itself_be_cut_at_every_write() {
    let layout = Layout::new(CAPACITY);
    // Cut in the middle of the first snapshot: no journal record yet.
    let mut store = BankStore::new(Memory::new(layout).cut_after(40), layout);
    assert!(store.initialize_new(OLD).is_err());
    let unfinished = store.into_inner().reboot();
    let mut full = BankStore::new(unfinished.clone(), layout);
    assert_eq!(full.inspect(), Err(StoreError::NeverPublished));
    full.reinitialize(NEW).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(unfinished.clone().cut_after(cut), layout);
        let _ = store.reinitialize(NEW);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = match recovered.inspect() {
            Ok(head) => head,
            Err(StoreError::Uninitialized | StoreError::NeverPublished) => recovered
                .reinitialize(NEW)
                .unwrap_or_else(|err| panic!("second finish after cut {cut}: {err:?}")),
            other => panic!("finish cut {cut}: {other:?}"),
        };
        assert_eq!(payload(&mut recovered, &head), NEW, "finish cut {cut}");
    }
}

#[test]
fn a_bank_that_was_ever_saved_is_never_taken_for_an_unfinished_one() {
    let layout = Layout::new(CAPACITY);
    // One save after initialization: slot B holds the second snapshot.
    let mut store = BankStore::new(baseline(), layout);
    let head = store.inspect().unwrap();
    store.commit_bank_only(&head, NEW).unwrap();
    let saved = store.into_inner();
    // Both journal records lost, in every way a record can be lost.
    for (a, b) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
        let mut memory = saved.clone();
        for (slot, damage) in [(Slot::A, a), (Slot::B, b)] {
            let at = layout.metadata_offset(slot) as usize;
            if damage == 0 {
                memory.bytes[at..at + METADATA_SIZE].fill(0);
            } else {
                memory.bytes[at + 144] ^= 1;
            }
        }
        let mut store = BankStore::new(memory, layout);
        assert_eq!(store.inspect(), Err(StoreError::NoValidMetadata), "{a}{b}");
        assert_eq!(
            store.reinitialize(OLD),
            Err(StoreError::NoValidMetadata),
            "{a}{b}"
        );
    }
}

#[test]
fn old_metadata_cannot_select_a_reused_snapshot_slot() {
    let layout = Layout::new(CAPACITY);
    let original = baseline();
    let mut store = BankStore::new(original.clone(), layout);
    let head = store.inspect().unwrap();
    let head = store.commit_bank_only(&head, NEW).unwrap();
    store.commit_bank_only(&head, b"third generation").unwrap();
    let mut memory = store.into_inner();
    // Simulate loss of current metadata, leaving only an obsolete reference.
    memory.bytes[..METADATA_SIZE].copy_from_slice(&original.bytes[..METADATA_SIZE]);
    memory.bytes[METADATA_SIZE + 80] ^= 1;
    let mut reopened = BankStore::new(memory, layout);
    assert_eq!(reopened.inspect(), Err(StoreError::SnapshotMismatch));
}

#[test]
fn both_journal_replicas_corrupt_never_become_an_empty_or_old_bank() {
    let layout = Layout::new(CAPACITY);
    let (store, _) = prepare(baseline());
    let mut memory = store.into_inner();
    for slot in [Slot::A, Slot::B] {
        memory.bytes[layout.metadata_offset(slot) as usize + 144] ^= 1;
    }
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.initialize_new(OLD),
        Err(StoreError::AlreadyContainsData)
    );
}

#[test]
fn bounded_read_and_sequence_overflow_do_not_modify_storage() {
    let layout = Layout::new(CAPACITY);
    let original = baseline();
    let mut store = BankStore::new(original.clone(), layout);
    let head = store.inspect().unwrap();
    assert_eq!(
        store.read_current(&head, &mut [0; 1]),
        Err(StoreError::BufferTooSmall {
            needed: OLD.len() as u32
        })
    );
    let exhausted = Metadata {
        sequence: u64::MAX,
        ..*head.metadata()
    };
    let record = exhausted.encode().unwrap();
    let mut memory = store.into_inner();
    for slot in [Slot::A, Slot::B] {
        let start = layout.metadata_offset(slot) as usize;
        memory.bytes[start..start + METADATA_SIZE].copy_from_slice(&record);
    }
    let before = memory.bytes.clone();
    let mut store = BankStore::new(memory, layout);
    let head = store.inspect().unwrap();
    assert_eq!(
        store.commit_bank_only(&head, NEW),
        Err(StoreError::CounterExhausted)
    );
    assert_eq!(
        store.prepare_transfer(&head, NEW, GAME, BEFORE, AFTER),
        Err(StoreError::CounterExhausted)
    );
    assert_eq!(store.into_inner().bytes, before);
}

#[test]
fn buffered_storage_loses_unflushed_writes_without_losing_saved_game_transfer() {
    let layout = Layout::new(CAPACITY);
    let base = baseline().buffered();
    let (full, _) = prepare(base.clone());
    let prepare_steps = full.into_inner().operations;
    for cut in 0..=prepare_steps {
        let mut store = BankStore::new(base.clone().cut_after(cut), layout);
        let head = store.inspect().unwrap();
        let _ = store.prepare_transfer(&head, NEW, GAME, BEFORE, AFTER);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = recovered.inspect().unwrap();
        let head = match head.phase() {
            Phase::Prepared(_) => {
                recovered
                    .reconcile(&head, observation(BEFORE))
                    .unwrap()
                    .head
            }
            Phase::Clean(_) => head,
        };
        assert_eq!(
            payload(&mut recovered, &head),
            OLD,
            "buffered prepare {cut}"
        );
    }
    let (prepared, _) = prepare(base);
    let prepared = prepared.into_inner().reboot();
    let mut full = BankStore::new(prepared.clone(), layout);
    let head = full.inspect().unwrap();
    full.reconcile(&head, observation(AFTER)).unwrap();
    let finish_steps = full.into_inner().operations;
    for cut in 0..=finish_steps {
        let mut store = BankStore::new(prepared.clone().cut_after(cut), layout);
        let head = store.inspect().unwrap();
        let _ = store.reconcile(&head, observation(AFTER));
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = recovered.inspect().unwrap();
        let head = match head.phase() {
            Phase::Prepared(_) => recovered.reconcile(&head, observation(AFTER)).unwrap().head,
            Phase::Clean(_) => head,
        };
        assert_eq!(payload(&mut recovered, &head), NEW, "buffered finish {cut}");
    }
}

/// A complete save on top of whatever `memory` holds, as the next session
/// does it: it must go through and be there after a restart.
fn save_again(memory: Memory, expected: &[u8], context: &str) {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(memory, layout);
    let head = store
        .inspect()
        .unwrap_or_else(|err| panic!("{context}: {err:?}"));
    let head = match head.phase() {
        Phase::Prepared(pending) => {
            let seen = if expected == OLD { BEFORE } else { AFTER };
            assert!(pending.before_fingerprint == BEFORE);
            store.reconcile(&head, observation(seen)).unwrap().head
        }
        Phase::Clean(_) => head,
    };
    assert_eq!(payload(&mut store, &head), expected, "{context}");
    let third: &[u8] = b"the save after the cut";
    let head = store
        .prepare_transfer(&head, third, GAME, [3; 32], [4; 32])
        .unwrap_or_else(|err| panic!("{context}: next save: {err:?}"));
    let head = store
        .reconcile(&head, observation([4; 32]))
        .unwrap_or_else(|err| panic!("{context}: next finish: {err:?}"))
        .head;
    assert_eq!(payload(&mut store, &head), third, "{context}");
    let mut reopened = BankStore::new(store.into_inner().reboot(), layout);
    let head = reopened.inspect().unwrap();
    assert_eq!(payload(&mut reopened, &head), third, "{context}");
}

#[test]
fn on_the_console_a_cut_save_leaves_the_old_bank_and_the_next_save_works() {
    let layout = Layout::new(CAPACITY);
    let base = baseline().console();
    let (complete, _) = prepare(base.clone());
    let total = complete.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), layout);
        let head = store.inspect().unwrap();
        let result = store.prepare_transfer(&head, NEW, GAME, BEFORE, AFTER);
        assert_eq!(result.is_ok(), cut == total, "cut {cut}");
        // The game was not written: the old Bank, whatever the cut left.
        save_again(
            store.into_inner().reboot(),
            OLD,
            &format!("prepare cut {cut}"),
        );
    }
}

#[test]
fn on_the_console_a_cut_finish_leaves_the_new_bank_and_the_next_save_works() {
    let layout = Layout::new(CAPACITY);
    let (prepared, _) = prepare(baseline().console());
    let base = prepared.into_inner().reboot();
    let mut full = BankStore::new(base.clone(), layout);
    let head = full.inspect().unwrap();
    full.reconcile(&head, observation(AFTER)).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), layout);
        let head = store.inspect().unwrap();
        let _ = store.reconcile(&head, observation(AFTER));
        save_again(
            store.into_inner().reboot(),
            NEW,
            &format!("finish cut {cut}"),
        );
    }
}

#[test]
fn on_the_console_a_cut_initialization_is_finished_by_the_next_one() {
    let layout = Layout::new(CAPACITY);
    let mut full = BankStore::new(Memory::new(layout).console(), layout);
    full.initialize_new(OLD).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(Memory::new(layout).console().cut_after(cut), layout);
        let _ = store.initialize_new(OLD);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = match recovered.inspect() {
            Ok(head) => head,
            Err(StoreError::Uninitialized | StoreError::NeverPublished) => {
                // Finishing it can be cut as well, at every step.
                let unfinished = recovered.into_inner();
                let mut whole = BankStore::new(unfinished.clone(), layout);
                whole.reinitialize(OLD).unwrap();
                let steps = whole.into_inner().operations;
                for second in 0..steps {
                    let mut store = BankStore::new(unfinished.clone().cut_after(second), layout);
                    let _ = store.reinitialize(OLD);
                    let mut again = BankStore::new(store.into_inner().reboot(), layout);
                    let head = match again.inspect() {
                        Ok(head) => head,
                        Err(StoreError::Uninitialized | StoreError::NeverPublished) => again
                            .reinitialize(OLD)
                            .unwrap_or_else(|err| panic!("cuts {cut}, {second}: {err:?}")),
                        other => panic!("cuts {cut}, {second}: {other:?}"),
                    };
                    assert_eq!(payload(&mut again, &head), OLD, "cuts {cut}, {second}");
                }
                recovered = BankStore::new(unfinished, layout);
                recovered
                    .reinitialize(OLD)
                    .unwrap_or_else(|err| panic!("finish after cut {cut}: {err:?}"))
            }
            other => panic!("initialization cut {cut}: {other:?}"),
        };
        assert_eq!(payload(&mut recovered, &head), OLD, "cut {cut}");
        save_again(recovered.into_inner().reboot(), OLD, &format!("cut {cut}"));
    }
}

#[test]
fn an_unreadable_record_counts_as_damaged_and_two_of_them_are_never_an_empty_bank() {
    let layout = Layout::new(CAPACITY);
    for unit in 0..2 {
        // One record lost to a cut: the other one decides.
        let mut memory = baseline().console();
        memory.unreadable[unit] = true;
        save_again(memory, OLD, &format!("record {unit}"));
    }
    // Both lost: nothing says that this was never a Bank, even when it holds
    // only a first snapshot, and nothing is written.
    let mut memory = baseline().console();
    memory.unreadable[0] = true;
    memory.unreadable[1] = true;
    let before = memory.bytes.clone();
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::NoValidMetadata));
    assert_eq!(store.reinitialize(NEW), Err(StoreError::NoValidMetadata));
    assert_eq!(store.into_inner().bytes, before);
    // Everything unreadable is no evidence of anything.
    let mut memory = baseline().console();
    memory.unreadable = [true; 4];
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::NoValidMetadata));
    assert_eq!(store.reinitialize(NEW), Err(StoreError::NoValidMetadata));
}

#[test]
fn an_unreadable_spare_snapshot_is_replaced_and_an_unreadable_current_one_is_an_error() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(baseline().console(), layout);
    let head = store.inspect().unwrap();
    let Phase::Clean(current) = head.phase() else {
        unreachable!()
    };
    let (used, spare) = match current.slot {
        Slot::A => (2, 3),
        Slot::B => (3, 2),
    };
    let mut memory = store.into_inner();
    memory.unreadable[spare] = true;
    save_again(memory.clone(), OLD, "spare slot");
    memory.unreadable[spare] = false;
    memory.unreadable[used] = true;
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::Io(Failure::Unreadable)));
}
