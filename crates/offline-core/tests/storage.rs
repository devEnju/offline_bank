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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    PowerCut,
    OutOfBounds,
}

impl Memory {
    fn new(layout: Layout) -> Self {
        Self {
            bytes: vec![0; layout.file_len() as usize],
            durable: None,
            budget: None,
            operations: 0,
        }
    }
    fn tick(&mut self) -> Result<(), Failure> {
        if let Some(budget) = &mut self.budget {
            if *budget == 0 {
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
        self
    }
}

impl Storage for Memory {
    type Error = Failure;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Failure> {
        let start = usize::try_from(offset).map_err(|_| Failure::OutOfBounds)?;
        let end = start.checked_add(bytes.len()).ok_or(Failure::OutOfBounds)?;
        let source = self.bytes.get(start..end).ok_or(Failure::OutOfBounds)?;
        bytes.copy_from_slice(source);
        Ok(())
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Failure> {
        let start = usize::try_from(offset).map_err(|_| Failure::OutOfBounds)?;
        let end = start.checked_add(bytes.len()).ok_or(Failure::OutOfBounds)?;
        if end > self.bytes.len() {
            return Err(Failure::OutOfBounds);
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
        if let Some(durable) = &mut self.durable {
            durable.clone_from(&self.bytes);
        }
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
    assert_eq!(store.inspect(), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.initialize_new(b""),
        Err(StoreError::AlreadyContainsData)
    );

    let mut memory = Memory::new(layout);
    memory.bytes.pop();
    let mut store = BankStore::new(memory, layout);
    assert_eq!(store.inspect(), Err(StoreError::Io(Failure::OutOfBounds)));
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
fn every_initialization_boundary_is_valid_or_requires_explicit_repair() {
    let layout = Layout::new(CAPACITY);
    let mut full = BankStore::new(Memory::new(layout), layout);
    full.initialize_new(OLD).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(Memory::new(layout).cut_after(cut), layout);
        let _ = store.initialize_new(OLD);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        match recovered.inspect() {
            Ok(head) => assert_eq!(payload(&mut recovered, &head), OLD),
            Err(StoreError::Uninitialized) => assert_eq!(cut, 0),
            Err(StoreError::NoValidMetadata) => {
                assert_eq!(
                    recovered.initialize_new(OLD),
                    Err(StoreError::AlreadyContainsData)
                );
            }
            other => panic!("initialization cut {cut}: {other:?}"),
        }
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
