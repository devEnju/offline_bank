use offline_core::*;

const CAPACITY: u32 = 96;
/// A copy of the Bank file is exactly as long as its place in the container.
const OLD: &[u8] = &[b'o'; CAPACITY as usize];
const NEW: &[u8] = &[b'n'; CAPACITY as usize];
const SIDES: Sides = Sides {
    main: 0x1111_1111,
    mover: 0x2222_2222,
    held: 0,
};
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
    store.initialize_new(OLD, SIDES).unwrap();
    store.into_inner().reboot()
}

/// The record in force, and a check of every copy of the Bank file it names.
fn inspect_all(store: &mut BankStore<Memory>) -> Result<Head, StoreError<Failure>> {
    let head = store.inspect()?;
    let mut out = [0; CAPACITY as usize];
    match head.phase() {
        Phase::Clean(_) => {
            store.read_current(&head, &mut out)?;
        }
        Phase::Prepared(_) => {
            for after in [false, true] {
                store.read_pending(&head, after, &mut out)?;
            }
        }
    }
    Ok(head)
}

/// A whole save of `payload`: prepared, then made current with the image
/// the game was written with.
fn save(
    store: &mut BankStore<Memory>,
    head: &Head,
    payload: &[u8],
) -> Result<Head, StoreError<Failure>> {
    let prepared = store.prepare_transfer(head, payload, SIDES, GAME, BEFORE, AFTER)?;
    Ok(store.reconcile(&prepared, AFTER)?.head)
}

fn prepare(memory: Memory) -> (BankStore<Memory>, Head) {
    let mut store = BankStore::new(memory, Layout::new(CAPACITY));
    let head = inspect_all(&mut store).unwrap();
    let head = store
        .prepare_transfer(&head, NEW, SIDES, GAME, BEFORE, AFTER)
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
fn the_journal_record_is_explicit_little_endian_and_checks_every_byte() {
    let part = |slot, crc| Part { slot, crc };
    let before = Save {
        bank: part(Slot::A, 0x0102_0304),
        main: part(Slot::B, 0x1112_1314),
        mover: part(Slot::A, 0x2122_2324),
        held: 0,
    };
    let after = Save {
        bank: part(Slot::B, 0x3132_3334),
        main: part(Slot::A, 0x4142_4344),
        // A delivery waits beside the empty box, so the box is written over.
        mover: part(Slot::A, 0x5152_5354),
        held: 5,
    };
    let record = Metadata {
        sequence: 0x0a0b_0c0d,
        phase: Phase::Prepared(PendingTransfer {
            before,
            after,
            game: GAME,
            before_fingerprint: BEFORE,
            after_fingerprint: AFTER,
        }),
        delivery: Some(0x6162_6364),
        ask: false,
    };
    let encoded = record.encode().unwrap();
    assert_eq!(encoded.len(), 140);
    assert_eq!(&encoded[..4], b"BKJ\x01");
    assert_eq!(&encoded[4..8], &[0x0d, 0x0c, 0x0b, 0x0a]);
    // In progress, a delivery waits, game 3 (Omega Ruby).
    assert_eq!(encoded[8], 0x33);
    // Copies: current bank A, main B, mover A; next bank B, main A, mover A.
    assert_eq!(encoded[9], 0b00_001_010);
    assert_eq!(&encoded[10..12], &[0, 5]);
    assert_eq!(&encoded[12..16], &[4, 3, 2, 1]);
    assert_eq!(&encoded[16..20], &[0x14, 0x13, 0x12, 0x11]);
    assert_eq!(&encoded[20..24], &[0x24, 0x23, 0x22, 0x21]);
    assert_eq!(&encoded[24..28], &[0x34, 0x33, 0x32, 0x31]);
    assert_eq!(&encoded[36..40], &[0x64, 0x63, 0x62, 0x61]);
    assert_eq!(&encoded[40..72], &GAME.save_identity);
    assert_eq!(&encoded[72..104], &BEFORE);
    assert_eq!(&encoded[104..136], &AFTER);
    assert_eq!(Metadata::decode(&encoded), Ok(record));
    // The same record left to Transporter's question: bit 2 of byte 8, and
    // nothing else of the record changes.
    let asked = Metadata {
        ask: true,
        ..record
    };
    let asking = asked.encode().unwrap();
    assert_eq!(asking[8], 0x37);
    assert_eq!(&asking[..8], &encoded[..8]);
    assert_eq!(&asking[9..136], &encoded[9..136]);
    assert_eq!(Metadata::decode(&asking), Ok(asked));
    for at in 0..encoded.len() {
        let mut corrupt = encoded;
        corrupt[at] ^= 1;
        assert!(Metadata::decode(&corrupt).is_err(), "undetected byte {at}");
    }
    for end in 0..encoded.len() {
        assert_eq!(
            Metadata::decode(&encoded[..end]),
            Err(FormatError::Truncated)
        );
    }
    // Without a save in progress everything that belongs to one is zero.
    let clean = Metadata {
        sequence: 1,
        phase: Phase::Clean(before),
        delivery: None,
        ask: false,
    };
    let encoded = clean.encode().unwrap();
    assert_eq!((encoded[8], encoded[9]), (0, 0b010));
    // Only a save in progress is left to Transporter's question.
    assert_eq!(
        Metadata { ask: true, ..clean }.encode(),
        Err(FormatError::InvalidReference)
    );
    assert!(encoded[24..136].iter().all(|byte| *byte == 0));
    assert_eq!(Metadata::decode(&encoded), Ok(clean));
}

#[test]
fn a_record_that_breaks_the_rules_of_a_save_is_refused_whoever_wrote_it() {
    let part = |slot| Part { slot, crc: 7 };
    let current = Save {
        bank: part(Slot::A),
        main: part(Slot::A),
        mover: part(Slot::A),
        held: 0,
    };
    let pending = |after: Save| {
        Phase::Prepared(PendingTransfer {
            before: current,
            after,
            game: GAME,
            before_fingerprint: BEFORE,
            after_fingerprint: AFTER,
        })
    };
    let next = Save {
        bank: part(Slot::B),
        main: part(Slot::B),
        mover: part(Slot::B),
        held: 3,
    };
    let record = |phase, delivery| Metadata {
        sequence: 1,
        phase,
        delivery,
        ask: false,
    };
    assert!(record(pending(next), None).encode().is_ok());
    let invalid = Err(FormatError::InvalidReference);
    // A save never writes a copy that is in use.
    for after in [
        Save {
            bank: part(Slot::A),
            ..next
        },
        Save {
            main: part(Slot::A),
            ..next
        },
        Save {
            mover: part(Slot::A),
            ..next
        },
    ] {
        assert_eq!(record(pending(after), None).encode(), invalid);
    }
    // Only an empty box is written over, and only to keep a delivery.
    let over = Save {
        mover: part(Slot::A),
        ..next
    };
    assert!(record(pending(over), Some(9)).encode().is_ok());
    assert_eq!(record(pending(next), Some(9)).encode(), invalid);
    // A delivery never waits beside a box that holds Pokémon, and a box
    // never holds more than thirty.
    let held = |held| Phase::Clean(Save { held, ..current });
    assert_eq!(record(held(1), Some(9)).encode(), invalid);
    assert!(record(held(30), None).encode().is_ok());
    assert_eq!(record(held(31), None).encode(), invalid);
    assert_eq!(
        record(Phase::Clean(current), None).encode().map(|_| ()),
        Ok(())
    );
    // The game is one of the eight, and the record counts from 1.
    let other = PendingTransfer {
        before: current,
        after: next,
        game: GameIdentity {
            title_id: 0x0004_0000_0005_5d01,
            ..GAME
        },
        before_fingerprint: BEFORE,
        after_fingerprint: AFTER,
    };
    assert_eq!(
        record(Phase::Prepared(other), None).encode(),
        Err(FormatError::InvalidGameIdentity)
    );
    assert_eq!(
        Metadata {
            sequence: 0,
            ..record(Phase::Clean(current), None)
        }
        .encode(),
        Err(FormatError::InvalidSequence)
    );
}

#[test]
fn fresh_storage_requires_explicit_initialization() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(Memory::new(layout), layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::Uninitialized));
    let head = store.initialize_new(OLD, SIDES).unwrap();
    assert_eq!(payload(&mut store, &head), OLD);
    assert_eq!(
        store.initialize_new(OLD, SIDES),
        Err(StoreError::AlreadyContainsData)
    );
}

#[test]
fn nonzero_or_truncated_uninitialized_storage_is_not_empty() {
    let layout = Layout::new(CAPACITY);
    let mut memory = Memory::new(layout);
    memory.bytes[layout.snapshot_offset(Slot::A) as usize] = 1;
    let mut store = BankStore::new(memory, layout);
    // No journal record and a first copy begun: an initialization that was cut.
    assert_eq!(inspect_all(&mut store), Err(StoreError::NeverPublished));
    assert_eq!(
        store.initialize_new(OLD, SIDES),
        Err(StoreError::AlreadyContainsData)
    );

    // Only a save writes the second copy: with anything in it this was a
    // Bank once, and it is not initialized again.
    let mut memory = Memory::new(layout);
    *memory.bytes.last_mut().unwrap() = 1;
    let mut store = BankStore::new(memory, layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.reinitialize(OLD, SIDES),
        Err(StoreError::NoValidMetadata)
    );

    // A part that cannot be read is not zero either.
    let mut memory = Memory::new(layout);
    memory.bytes.pop();
    let mut store = BankStore::new(memory, layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::NeverPublished));
}

#[test]
fn transfer_blocks_unresolved_reads_and_further_changes() {
    let (mut store, head) = prepare(baseline());
    assert_eq!(
        store.read_current(&head, &mut [0; 96]),
        Err(StoreError::PendingRecovery)
    );
    assert_eq!(
        store.prepare_transfer(&head, OLD, SIDES, GAME, AFTER, BEFORE),
        Err(StoreError::PendingRecovery)
    );
    let result = store.reconcile(&head, AFTER).unwrap();
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
        let result = store.reconcile(&head, fingerprint).unwrap();
        assert_eq!(result.decision, decision);
        assert_eq!(payload(&mut store, &result.head), expected);
    }
}

#[test]
fn a_game_save_that_is_neither_image_is_refused_without_writes() {
    let (store, head) = prepare(baseline());
    let before = store.into_inner();
    let mut store = BankStore::new(before.clone(), Layout::new(CAPACITY));
    assert_eq!(
        store.reconcile(&head, [3; 32]),
        Err(StoreError::NeitherImage)
    );
    assert_eq!(store.into_inner().bytes, before.bytes);
}
#[test]
fn payload_capacity_and_ambiguous_game_states_fail_before_writes() {
    for (bytes, before, after, expected) in [
        (&[9; 97][..], BEFORE, AFTER, FormatError::PayloadSize),
        (&[9; 95][..], BEFORE, AFTER, FormatError::PayloadSize),
        (
            NEW,
            BEFORE,
            BEFORE,
            FormatError::IndistinguishableGameStates,
        ),
    ] {
        let initial = baseline();
        let mut store = BankStore::new(initial.clone(), Layout::new(CAPACITY));
        let head = inspect_all(&mut store).unwrap();
        assert_eq!(
            store.prepare_transfer(&head, bytes, SIDES, GAME, before, after),
            Err(StoreError::Format(expected))
        );
        assert_eq!(store.into_inner().bytes, initial.bytes);
    }
}

#[test]
fn a_container_of_another_capacity_does_not_pass_for_this_one() {
    // The record holds no length: a copy is as long as its place, and its
    // checksum is over all of it.
    let mut store = BankStore::new(baseline(), Layout::new(CAPACITY - 1));
    assert_eq!(inspect_all(&mut store), Err(StoreError::PayloadChecksum));
}

#[test]
fn unknown_schema_does_not_fall_back_to_an_older_replica() {
    let layout = Layout::new(CAPACITY);
    let mut memory = baseline();
    let start = layout.metadata_offset(Slot::B) as usize;
    memory.bytes[start + 3] = 2;
    let checksum = crc32(&memory.bytes[start..start + 136]);
    memory.bytes[start + 136..start + 140].copy_from_slice(&checksum.to_le_bytes());
    let mut store = BankStore::new(memory, layout);
    assert_eq!(
        inspect_all(&mut store),
        Err(StoreError::Format(FormatError::UnsupportedVersion(2)))
    );
}

#[test]
fn snapshot_corruption_is_an_error_without_old_snapshot_fallback() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(baseline(), layout);
    let head = inspect_all(&mut store).unwrap();
    let head = save(&mut store, &head, NEW).unwrap();
    let snapshot = match head.phase() {
        Phase::Clean(s) => s,
        _ => unreachable!(),
    };
    let mut memory = store.into_inner();
    memory.bytes[layout.snapshot_offset(snapshot.bank.slot) as usize] ^= 1;
    let mut store = BankStore::new(memory, layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::PayloadChecksum));
}

#[test]
fn mirrored_journal_survives_one_damaged_metadata_record() {
    let layout = Layout::new(CAPACITY);
    for slot in [Slot::A, Slot::B] {
        let (store, _) = prepare(baseline());
        let mut memory = store.into_inner().reboot();
        memory.bytes[layout.metadata_offset(slot) as usize + 80] ^= 1;
        let mut store = BankStore::new(memory, layout);
        let head = inspect_all(&mut store).unwrap();
        let result = store.reconcile(&head, AFTER).unwrap();
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
        phase: Phase::Clean(pending.before),
        delivery: None,
        ask: false,
    };
    let start = layout.metadata_offset(Slot::B) as usize;
    memory.bytes[start..start + METADATA_SIZE].copy_from_slice(&conflicting.encode().unwrap());
    assert_eq!(
        inspect_all(&mut BankStore::new(memory, layout)),
        Err(StoreError::ConflictingMetadata)
    );
}

/// The save in progress of `prepare`, left to Transporter's question.
fn asked(memory: Memory) -> (BankStore<Memory>, Head) {
    let (mut store, head) = prepare(memory);
    let head = store.ask(&head, true).unwrap();
    (store, head)
}

#[test]
fn a_save_in_progress_left_to_transporters_question_keeps_everything_else() {
    let (mut store, prepared) = prepare(baseline());
    let head = store.ask(&prepared, true).unwrap();
    assert!(head.metadata().ask);
    assert_eq!(head.phase(), prepared.phase());
    assert_eq!(head.metadata().sequence, prepared.metadata().sequence + 1);
    let mut store = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
    assert_eq!(inspect_all(&mut store).unwrap().metadata(), head.metadata());
    // Asked again: nothing is written. Taken back: the record before, with
    // the next sequence number.
    let before = store.storage_mut().bytes.clone();
    assert_eq!(store.ask(&head, true), Ok(head));
    assert_eq!(store.storage_mut().bytes, before);
    let back = store.ask(&head, false).unwrap();
    assert!(!back.metadata().ask);
    assert_eq!(back.phase(), prepared.phase());
    // A Bank at rest has nothing to leave to it.
    let mut store = BankStore::new(baseline(), Layout::new(CAPACITY));
    let head = inspect_all(&mut store).unwrap();
    assert_eq!(store.ask(&head, true), Err(StoreError::NoPendingTransfer));
}

#[test]
fn transporters_answer_settles_the_save_as_banks_reconciliation_does() {
    for (decision, expected) in [
        (RecoveryDecision::KeepBefore, OLD),
        (RecoveryDecision::CommitAfter, NEW),
    ] {
        let (store, head) = asked(baseline());
        let mut journal = Journal::new(store.into_inner());
        let settled = journal.settle(&head, decision).unwrap();
        assert!(!settled.metadata().ask);
        let mut store = BankStore::new(journal.into_inner().reboot(), Layout::new(CAPACITY));
        let head = inspect_all(&mut store).unwrap();
        assert_eq!(head.metadata(), settled.metadata());
        assert_eq!(payload(&mut store, &head), expected);
        // The same record as Bank's own reconciliation writes.
        let (mut bank, prepared) = prepare(baseline());
        let image = match decision {
            RecoveryDecision::KeepBefore => BEFORE,
            RecoveryDecision::CommitAfter => AFTER,
        };
        let reconciled = bank.reconcile(&prepared, image).unwrap().head;
        assert_eq!(reconciled.phase(), settled.phase());
        assert_eq!(reconciled.metadata().delivery, settled.metadata().delivery);
    }
}

#[test]
fn transporter_settles_only_a_save_left_to_it_and_only_as_it_read_it() {
    // A save in progress Bank settles itself.
    let (store, head) = prepare(baseline());
    let before = store.into_inner();
    let mut journal = Journal::new(before.clone());
    assert_eq!(
        journal.settle(&head, RecoveryDecision::KeepBefore),
        Err(StoreError::NoPendingTransfer)
    );
    assert_eq!(journal.into_inner().bytes, before.bytes);
    // A record that changed since it was read.
    let (store, head) = asked(baseline());
    let mut store = BankStore::new(store.into_inner(), Layout::new(CAPACITY));
    store.ask(&head, false).unwrap();
    let before = store.into_inner();
    let mut journal = Journal::new(before.clone());
    assert_eq!(
        journal.settle(&head, RecoveryDecision::CommitAfter),
        Err(StoreError::StaleHead)
    );
    assert_eq!(journal.into_inner().bytes, before.bytes);
}

#[test]
fn a_cut_while_asking_or_settling_leaves_the_record_before_or_after() {
    let (store, prepared) = prepare(baseline());
    let base = store.into_inner().reboot();
    let mut complete = BankStore::new(base.clone(), Layout::new(CAPACITY));
    complete.ask(&prepared, true).unwrap();
    let total = complete.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let _ = store.ask(&prepared, true);
        let mut store = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
        let head = inspect_all(&mut store).unwrap_or_else(|err| panic!("ask cut {cut}: {err:?}"));
        assert_eq!(head.phase(), prepared.phase(), "ask cut {cut}");
    }
    for decision in [RecoveryDecision::KeepBefore, RecoveryDecision::CommitAfter] {
        let (store, head) = asked(baseline());
        let base = store.into_inner().reboot();
        let mut complete = Journal::new(base.clone());
        let settled = complete.settle(&head, decision).unwrap();
        let total = complete.into_inner().operations;
        for cut in 0..=total {
            let mut journal = Journal::new(base.clone().cut_after(cut));
            let _ = journal.settle(&head, decision);
            let mut store = BankStore::new(journal.into_inner().reboot(), Layout::new(CAPACITY));
            let now =
                inspect_all(&mut store).unwrap_or_else(|err| panic!("settle cut {cut}: {err:?}"));
            assert!(
                now.metadata() == head.metadata() || now.phase() == settled.phase(),
                "settle cut {cut}"
            );
        }
    }
}

#[test]
fn every_prepare_write_boundary_recovers_old_when_game_was_not_changed() {
    let base = baseline();
    let (complete, _) = prepare(base.clone());
    let total = complete.into_inner().operations;
    assert!(total > 300);
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let head = inspect_all(&mut store).unwrap();
        let result = store.prepare_transfer(&head, NEW, SIDES, GAME, BEFORE, AFTER);
        if cut < total {
            assert!(result.is_err(), "cut {cut} unexpectedly succeeded");
        }
        let memory = store.into_inner().reboot();
        let mut recovered = BankStore::new(memory, Layout::new(CAPACITY));
        let head =
            inspect_all(&mut recovered).unwrap_or_else(|err| panic!("prepare cut {cut}: {err:?}"));
        let head = if matches!(head.phase(), Phase::Prepared(_)) {
            recovered.reconcile(&head, BEFORE).unwrap().head
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
    let head = inspect_all(&mut full).unwrap();
    full.reconcile(&head, AFTER).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), Layout::new(CAPACITY));
        let head = inspect_all(&mut store).unwrap();
        let _ = store.reconcile(&head, AFTER);
        let mut recovered = BankStore::new(store.into_inner().reboot(), Layout::new(CAPACITY));
        let head =
            inspect_all(&mut recovered).unwrap_or_else(|err| panic!("finish cut {cut}: {err:?}"));
        let head = if matches!(head.phase(), Phase::Prepared(_)) {
            recovered.reconcile(&head, AFTER).unwrap().head
        } else {
            head
        };
        assert_eq!(payload(&mut recovered, &head), NEW, "finish cut {cut}");
    }
}

#[test]
fn every_initialization_boundary_is_valid_or_can_be_finished() {
    let layout = Layout::new(CAPACITY);
    let mut full = BankStore::new(Memory::new(layout), layout);
    full.initialize_new(OLD, SIDES).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(Memory::new(layout).cut_after(cut), layout);
        let _ = store.initialize_new(OLD, SIDES);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        match inspect_all(&mut recovered) {
            Ok(head) => {
                assert_eq!(payload(&mut recovered, &head), OLD);
                // A Bank is current: it is never initialized again.
                assert_eq!(
                    recovered.reinitialize(NEW, SIDES),
                    Err(StoreError::AlreadyContainsData)
                );
            }
            Err(StoreError::Uninitialized) => assert_eq!(cut, 0),
            Err(StoreError::NeverPublished) => {
                // Only the explicit continuation finishes it, with whatever
                // the caller supplies now.
                assert_eq!(
                    recovered.initialize_new(OLD, SIDES),
                    Err(StoreError::AlreadyContainsData)
                );
                let head = recovered
                    .reinitialize(NEW, SIDES)
                    .unwrap_or_else(|err| panic!("finish after cut {cut}: {err:?}"));
                assert_eq!(payload(&mut recovered, &head), NEW);
                let mut reopened = BankStore::new(recovered.into_inner().reboot(), layout);
                let head = inspect_all(&mut reopened).unwrap();
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
    assert!(store.initialize_new(OLD, SIDES).is_err());
    let unfinished = store.into_inner().reboot();
    let mut full = BankStore::new(unfinished.clone(), layout);
    assert_eq!(inspect_all(&mut full), Err(StoreError::NeverPublished));
    full.reinitialize(NEW, SIDES).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(unfinished.clone().cut_after(cut), layout);
        let _ = store.reinitialize(NEW, SIDES);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = match inspect_all(&mut recovered) {
            Ok(head) => head,
            Err(StoreError::Uninitialized | StoreError::NeverPublished) => recovered
                .reinitialize(NEW, SIDES)
                .unwrap_or_else(|err| panic!("second finish after cut {cut}: {err:?}")),
            other => panic!("finish cut {cut}: {other:?}"),
        };
        assert_eq!(payload(&mut recovered, &head), NEW, "finish cut {cut}");
    }
}

#[test]
fn a_bank_that_was_ever_saved_is_never_taken_for_an_unfinished_one() {
    let layout = Layout::new(CAPACITY);
    // One save after initialization: the second copy holds a Bank.
    let mut store = BankStore::new(baseline(), layout);
    let head = inspect_all(&mut store).unwrap();
    save(&mut store, &head, NEW).unwrap();
    let saved = store.into_inner();
    // Both journal records lost, in every way a record can be lost.
    for (a, b) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
        let mut memory = saved.clone();
        for (slot, damage) in [(Slot::A, a), (Slot::B, b)] {
            let at = layout.metadata_offset(slot) as usize;
            if damage == 0 {
                memory.bytes[at..at + METADATA_SIZE].fill(0);
            } else {
                memory.bytes[at + 100] ^= 1;
            }
        }
        let mut store = BankStore::new(memory, layout);
        assert_eq!(
            inspect_all(&mut store),
            Err(StoreError::NoValidMetadata),
            "{a}{b}"
        );
        assert_eq!(
            store.reinitialize(OLD, SIDES),
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
    let head = inspect_all(&mut store).unwrap();
    let head = save(&mut store, &head, NEW).unwrap();
    save(&mut store, &head, &[b't'; CAPACITY as usize]).unwrap();
    let mut memory = store.into_inner();
    // Simulate loss of current metadata, leaving only an obsolete reference.
    memory.bytes[..METADATA_SIZE].copy_from_slice(&original.bytes[..METADATA_SIZE]);
    memory.bytes[METADATA_SIZE + 80] ^= 1;
    let mut reopened = BankStore::new(memory, layout);
    // The copy that record names holds another save by now.
    assert_eq!(inspect_all(&mut reopened), Err(StoreError::PayloadChecksum));
}

#[test]
fn both_journal_replicas_corrupt_never_become_an_empty_or_old_bank() {
    let layout = Layout::new(CAPACITY);
    let (store, _) = prepare(baseline());
    let mut memory = store.into_inner();
    for slot in [Slot::A, Slot::B] {
        memory.bytes[layout.metadata_offset(slot) as usize + 100] ^= 1;
    }
    let mut store = BankStore::new(memory, layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.initialize_new(OLD, SIDES),
        Err(StoreError::AlreadyContainsData)
    );
}

#[test]
fn bounded_read_and_sequence_overflow_do_not_modify_storage() {
    let layout = Layout::new(CAPACITY);
    let original = baseline();
    let mut store = BankStore::new(original.clone(), layout);
    let head = inspect_all(&mut store).unwrap();
    assert_eq!(
        store.read_current(&head, &mut [0; 1]),
        Err(StoreError::BufferTooSmall { needed: CAPACITY })
    );
    let exhausted = Metadata {
        sequence: u32::MAX,
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
    let head = inspect_all(&mut store).unwrap();
    assert_eq!(
        store.prepare_transfer(&head, NEW, SIDES, GAME, BEFORE, AFTER),
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
        let head = inspect_all(&mut store).unwrap();
        let _ = store.prepare_transfer(&head, NEW, SIDES, GAME, BEFORE, AFTER);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = inspect_all(&mut recovered).unwrap();
        let head = match head.phase() {
            Phase::Prepared(_) => recovered.reconcile(&head, BEFORE).unwrap().head,
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
    let head = inspect_all(&mut full).unwrap();
    full.reconcile(&head, AFTER).unwrap();
    let finish_steps = full.into_inner().operations;
    for cut in 0..=finish_steps {
        let mut store = BankStore::new(prepared.clone().cut_after(cut), layout);
        let head = inspect_all(&mut store).unwrap();
        let _ = store.reconcile(&head, AFTER);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = inspect_all(&mut recovered).unwrap();
        let head = match head.phase() {
            Phase::Prepared(_) => recovered.reconcile(&head, AFTER).unwrap().head,
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
    let head = inspect_all(&mut store).unwrap_or_else(|err| panic!("{context}: {err:?}"));
    let head = match head.phase() {
        Phase::Prepared(pending) => {
            let seen = if expected == OLD { BEFORE } else { AFTER };
            assert!(pending.before_fingerprint == BEFORE);
            store.reconcile(&head, seen).unwrap().head
        }
        Phase::Clean(_) => head,
    };
    assert_eq!(payload(&mut store, &head), expected, "{context}");
    let third: &[u8] = &[b't'; CAPACITY as usize];
    let head = store
        .prepare_transfer(&head, third, SIDES, GAME, [3; 32], [4; 32])
        .unwrap_or_else(|err| panic!("{context}: next save: {err:?}"));
    let head = store
        .reconcile(&head, [4; 32])
        .unwrap_or_else(|err| panic!("{context}: next finish: {err:?}"))
        .head;
    assert_eq!(payload(&mut store, &head), third, "{context}");
    let mut reopened = BankStore::new(store.into_inner().reboot(), layout);
    let head = inspect_all(&mut reopened).unwrap();
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
        let head = inspect_all(&mut store).unwrap();
        let result = store.prepare_transfer(&head, NEW, SIDES, GAME, BEFORE, AFTER);
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
    let head = inspect_all(&mut full).unwrap();
    full.reconcile(&head, AFTER).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(base.clone().cut_after(cut), layout);
        let head = inspect_all(&mut store).unwrap();
        let _ = store.reconcile(&head, AFTER);
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
    full.initialize_new(OLD, SIDES).unwrap();
    let total = full.into_inner().operations;
    for cut in 0..=total {
        let mut store = BankStore::new(Memory::new(layout).console().cut_after(cut), layout);
        let _ = store.initialize_new(OLD, SIDES);
        let mut recovered = BankStore::new(store.into_inner().reboot(), layout);
        let head = match inspect_all(&mut recovered) {
            Ok(head) => head,
            Err(StoreError::Uninitialized | StoreError::NeverPublished) => {
                // Finishing it can be cut as well, at every step.
                let unfinished = recovered.into_inner();
                let mut whole = BankStore::new(unfinished.clone(), layout);
                whole.reinitialize(OLD, SIDES).unwrap();
                let steps = whole.into_inner().operations;
                for second in 0..steps {
                    let mut store = BankStore::new(unfinished.clone().cut_after(second), layout);
                    let _ = store.reinitialize(OLD, SIDES);
                    let mut again = BankStore::new(store.into_inner().reboot(), layout);
                    let head = match inspect_all(&mut again) {
                        Ok(head) => head,
                        Err(StoreError::Uninitialized | StoreError::NeverPublished) => again
                            .reinitialize(OLD, SIDES)
                            .unwrap_or_else(|err| panic!("cuts {cut}, {second}: {err:?}")),
                        other => panic!("cuts {cut}, {second}: {other:?}"),
                    };
                    assert_eq!(payload(&mut again, &head), OLD, "cuts {cut}, {second}");
                }
                recovered = BankStore::new(unfinished, layout);
                recovered
                    .reinitialize(OLD, SIDES)
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
    assert_eq!(inspect_all(&mut store), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.reinitialize(NEW, SIDES),
        Err(StoreError::NoValidMetadata)
    );
    assert_eq!(store.into_inner().bytes, before);
    // Everything unreadable is no evidence of anything.
    let mut memory = baseline().console();
    memory.unreadable = [true; 4];
    let mut store = BankStore::new(memory, layout);
    assert_eq!(inspect_all(&mut store), Err(StoreError::NoValidMetadata));
    assert_eq!(
        store.reinitialize(NEW, SIDES),
        Err(StoreError::NoValidMetadata)
    );
}

#[test]
fn an_unreadable_spare_snapshot_is_replaced_and_an_unreadable_current_one_is_an_error() {
    let layout = Layout::new(CAPACITY);
    let mut store = BankStore::new(baseline().console(), layout);
    let head = inspect_all(&mut store).unwrap();
    let Phase::Clean(current) = head.phase() else {
        unreachable!()
    };
    let (used, spare) = match current.bank.slot {
        Slot::A => (2, 3),
        Slot::B => (3, 2),
    };
    let mut memory = store.into_inner();
    memory.unreadable[spare] = true;
    save_again(memory.clone(), OLD, "spare slot");
    memory.unreadable[spare] = false;
    memory.unreadable[used] = true;
    let mut store = BankStore::new(memory, layout);
    assert_eq!(
        inspect_all(&mut store),
        Err(StoreError::Io(Failure::Unreadable))
    );
}

#[test]
fn a_delivery_is_announced_into_a_settled_empty_box_and_ends_with_the_save_that_takes_it() {
    let layout = Layout::new(CAPACITY);
    // Before a Bank exists there is no record to announce anything in.
    assert_eq!(Journal::new(Memory::new(layout)).load(), Ok(None));

    let mut journal = Journal::new(baseline());
    let head = journal.load().unwrap().unwrap();
    let announced = journal.announce(&head, 0xd0d0).unwrap();
    assert_eq!(
        *announced.metadata(),
        Metadata {
            sequence: head.metadata().sequence + 1,
            delivery: Some(0xd0d0),
            ..*head.metadata()
        }
    );
    // Not from a record that is no longer in force, and not a second one.
    assert_eq!(journal.announce(&head, 1), Err(StoreError::StaleHead));
    assert_eq!(
        journal.announce(&announced, 1),
        Err(StoreError::DeliveryRefused)
    );
    let waiting = journal.into_inner().reboot();

    // A save writes its box over the empty one in use, beside the delivery.
    let (store, head) = prepare(waiting);
    let Phase::Prepared(pending) = head.phase() else {
        unreachable!()
    };
    assert_eq!(pending.after.mover.slot, pending.before.mover.slot);
    assert_eq!(head.metadata().delivery, Some(0xd0d0));
    // While it is in progress nothing is announced.
    let mut journal = Journal::new(store.into_inner());
    let in_progress = journal.load().unwrap().unwrap();
    assert_eq!(
        journal.announce(&in_progress, 2),
        Err(StoreError::DeliveryRefused)
    );
    let prepared = journal.into_inner().reboot();
    // Undone, the delivery still waits; current, it is taken, by the same
    // record.
    for (seen, delivery) in [(BEFORE, Some(0xd0d0)), (AFTER, None)] {
        let mut store = BankStore::new(prepared.clone(), layout);
        let head = inspect_all(&mut store).unwrap();
        let settled = store.reconcile(&head, seen).unwrap().head;
        assert_eq!(settled.metadata().delivery, delivery);
        let mut reopened = BankStore::new(store.into_inner().reboot(), layout);
        assert_eq!(
            inspect_all(&mut reopened).unwrap().metadata().delivery,
            delivery
        );
    }

    // A box that holds Pokémon takes no delivery.
    let mut store = BankStore::new(Memory::new(layout), layout);
    let held = Sides { held: 2, ..SIDES };
    store.initialize_new(OLD, held).unwrap();
    let mut journal = Journal::new(store.into_inner());
    let head = journal.load().unwrap().unwrap();
    assert!(!may_deliver(head.metadata()));
    assert_eq!(journal.announce(&head, 3), Err(StoreError::DeliveryRefused));
}
