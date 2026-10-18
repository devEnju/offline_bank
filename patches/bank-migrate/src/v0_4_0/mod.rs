//! The Bank as releases v0.3.0 to v0.4.0 stored it (tag v0.4.0, 42b00b4).
//!
//! Ten files: the journal and the boxes, the Pokédex, the transport box and
//! a rewards record, each twice. The modules below are that release's
//! storage code, frozen: `offline-core`'s and `bank-common`'s as it has them,
//! without their tests and without what only a running Bank needs. They read
//! its files with the code that wrote them, whatever the shared crates
//! become. What writes is kept as that release has it, for the tests, which
//! make a Bank of that release with it. The containers are v0.2.1's too, so
//! `v0_2_1` reads with them as well (`Containers`, `settled`).
//!
//! Its rewards record means what v0.4.0 reads in it (`rewards`). A v0.3.x
//! Bank, in the same files, is read as v0.4.0 read one. A save in progress
//! that v0.3.0 began is bound in a form v0.4.0 does not read (`binding`);
//! the migration writes it in v0.4.0's, so that v0.4.0 settles it.
#![allow(dead_code, unused_imports, clippy::enum_variant_names)]

pub mod bank_files;
pub mod binding;
pub mod rewards;
pub mod sections;
pub mod session;
pub mod sidecar;
pub mod transport;

mod format;
mod store;

/// What has not changed since and is shared with the current code.
pub use offline_core::{crc32, Storage};
pub mod checksum {
    pub use offline_core::Crc32;
}
pub mod native_blob {
    pub use offline_core::native_blob::BLOB_SIZE;
}

pub use format::{
    decide_recovery, Fingerprint, FormatError, GameIdentity, GameObservation, Metadata,
    PendingTransfer, Phase, RecoveryBlock, RecoveryDecision, Slot, SnapshotHeader, SnapshotRef,
    METADATA_SIZE, SCHEMA_VERSION, SNAPSHOT_HEADER_SIZE,
};
pub use store::{BankStore, Head, Layout, Reconciled, StoreError};

use crate::{
    migrate::{Card, Cause, Guarded, ReadOnly, Step, Stopped},
    source,
    target::{self, Miles},
};
use bank_common::bank_files::UnitFile;
use bank_files::{BankFiles, FileName, Files, Loaded};

/// Its containers, the Bank last.
const CONTAINERS: [FileName; 4] = [
    FileName::Dex,
    FileName::Rewards,
    FileName::Transport,
    FileName::Bank,
];

pub(crate) struct Way;
impl source::Way for Way {
    const TAG: &'static [u8] = b"v0.4.0";
    /// Its `/journal.bin`, a record of another length.
    const REFUSED: u32 = 0xc0;

    /// One of its journal records, which have another size than the current
    /// ones. Its other files say nothing: some it can do without, and the
    /// rest have the names of current ones.
    fn there<C: Card>(card: &mut C) -> Result<bool, C::Error> {
        let records = &FileName::Bank.units()[..2];
        Ok(card.exists(&records[..1])? || card.exists(&records[1..])?)
    }

    fn read<C: Card>(card: &mut C, body: &mut [u8]) -> Result<(), Stopped<C::Error>> {
        let loaded = settled(Containers::new(card, FileName::units), body)?;
        into_body(body, loaded.rewards);
        Ok(())
    }

    fn remove<C: Card>(card: &mut C) -> Result<(), C::Error> {
        for file in CONTAINERS {
            card.remove(file.units())?;
        }
        Ok(())
    }

    /// A save in progress that v0.3.0 began, whose binding v0.4.0 does not
    /// read, is published once more with the binding v0.4.0 gives it
    /// (`binding::as_v3`), the way v0.4.0 publishes a record
    /// (`BankStore::rebind`). Its snapshots and side files stay as they are.
    fn ready_for_its_release<C: Card>(card: &mut C) -> Result<(), C::Error> {
        let Some(storage) = card.open(FileName::Bank.units())? else {
            return Ok(());
        };
        let mut store = BankStore::new(storage, session::BANK_LAYOUT);
        let head = match store.inspect() {
            Ok(head) => head,
            Err(StoreError::Io(error)) => return Err(error),
            Err(_) => return Ok(()),
        };
        let Phase::Prepared(pending) = head.phase() else {
            return Ok(());
        };
        let Some(game) = binding::as_v3(&pending) else {
            return Ok(());
        };
        match store.rebind(&head, game) {
            Err(StoreError::Io(error)) => Err(error),
            _ => Ok(()),
        }
    }
}

/// The balance and the date stay as the body has them: v0.4.0's load adds
/// the balance v0.3.1 kept in the record (`rewards::balance_beside`). Of the
/// record the remainder goes into the body; the day of a redemption is not
/// kept.
pub(crate) fn into_body(body: &mut [u8], record: rewards::Record) {
    let miles = Miles {
        remainder: record.fraction,
        ..Miles::in_body(body)
    };
    target::into_body(body, miles);
}

/// The containers as this code knows them, each made of the files `units`
/// gives it, only read.
pub(crate) struct Containers<'a, C: Card> {
    card: &'a mut C,
    units: fn(FileName) -> &'static [UnitFile],
}
impl<'a, C: Card> Containers<'a, C> {
    pub(crate) fn new(card: &'a mut C, units: fn(FileName) -> &'static [UnitFile]) -> Self {
        Self { card, units }
    }
}
impl<C: Card> Files for Containers<'_, C> {
    type Storage = ReadOnly<C::Storage>;
    fn open(&mut self, file: FileName) -> Result<Option<Self::Storage>, Guarded<C::Error>> {
        let found = self.card.open((self.units)(file)).map_err(Guarded::Io)?;
        Ok(found.map(ReadOnly))
    }
    fn create(&mut self, _: FileName) -> Result<Self::Storage, Guarded<C::Error>> {
        Err(Guarded::ReadOnly)
    }
}

/// Loads the Bank in `files` with this code into `body`, as one native body,
/// and requires it at rest: a save in progress, or a Pokédex or transport
/// box that the release would fill at its next save, are that release's to
/// settle. A waiting delivery is taken, as a load takes it. Nothing is
/// written.
pub(crate) fn settled<E, F>(files: F, body: &mut [u8]) -> Result<Loaded, Stopped<E>>
where
    F: Files,
    F::Storage: Storage<Error = Guarded<E>>,
{
    let read = Step::ReadOld;
    let mut bank = BankFiles::new(files);
    match bank.open().map_err(unread(read))? {
        Some(Phase::Clean(_)) => {}
        Some(Phase::Prepared(_)) => return Err(Stopped::InProgress),
        None => return Err(Stopped::Failed(read, Cause::Damaged)),
    }
    let loaded = bank.read(body).map_err(unread(read))?;
    if loaded.dex_missing || loaded.transport_missing {
        return Err(Stopped::Unsettled);
    }
    Ok(loaded)
}

/// Why this code could not read the Bank.
fn unread<E>(step: Step) -> impl Fn(bank_files::Error<Guarded<E>>) -> Stopped<E> {
    move |error| {
        Stopped::Failed(
            step,
            match error {
                bank_files::Error::Io(Guarded::Io(error))
                | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(Guarded::Io(
                    error,
                )))) => Cause::Io(error),
                _ => Cause::Damaged,
            },
        )
    }
}

/// Banks of this way for the tests, made with this code.
#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use super::*;
    use crate::{
        migrate::Done,
        testing::{convert, current, only_the_target_files, Fixtures, Open, Sd},
    };
    use bank_common::testing::body;
    use offline_core::{
        native_blob::{self as current_blob, BLOB_SIZE, FRACTION_OFFSET},
        sections::{TRANSPORT_RECORDS, TRANSPORT_TAGS},
    };
    use rewards::Record;
    use std::{vec, vec::Vec};

    pub(crate) const FIXTURES: Fixtures = Fixtures {
        used,
        in_progress,
        unsettled,
    };

    /// The card as v0.4.0 uses it.
    pub(crate) struct Before(pub(crate) Sd);
    impl Files for Before {
        type Storage = Open;
        fn open(&mut self, file: FileName) -> Result<Option<Open>, ()> {
            self.0.open(file.units())
        }
        fn create(&mut self, file: FileName) -> Result<Open, ()> {
            self.0.create(file.units())
        }
    }

    pub(crate) const GAME: GameIdentity = GameIdentity {
        title_id: 0x0004_0000_0005_5e00,
        save_identity: [7; 32],
    };
    pub(crate) fn kept() -> Record {
        Record {
            fraction: 321,
            redeemed: Some((2026, 10, 6)),
        }
    }
    /// `body` with the record where the current release keeps it.
    pub(crate) fn with_record(mut body: Vec<u8>, record: Record) -> Vec<u8> {
        into_body(&mut body, record);
        body
    }
    /// A Bank of v0.4.0 on `card`, created and never saved.
    pub(crate) fn created(card: &Sd, first: &[u8]) {
        let mut bank = BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(None)));
        bank.initialize(&mut first.to_vec()).unwrap();
    }
    /// One Save and Quit of v0.4.0, up to the game's write.
    pub(crate) fn prepared(card: &Sd, saved: &[u8], record: Record) -> BankFiles<Before> {
        let mut bank = BankFiles::new(Before(card.clone()));
        assert!(matches!(bank.open(), Ok(Some(Phase::Clean(_)))));
        bank.read(&mut vec![0; BLOB_SIZE]).unwrap();
        bank.prepare(&mut saved.to_vec(), record, GAME, [1; 32], [2; 32])
            .unwrap();
        bank
    }
    /// The body `used` saves.
    pub(crate) fn saved() -> Vec<u8> {
        body(0x52, 0x66, 3)
    }
    /// A card with a Bank v0.4.0 created and saved once with `saved()`, and
    /// the body the current release has to load from it.
    pub(crate) fn used(console: bool) -> (Sd, Vec<u8>) {
        let card = Sd::default();
        card.0.borrow_mut().console = console;
        created(&card, &body(0x31, 0x44, 0));
        let mut bank = prepared(&card, &saved(), kept());
        bank.reconcile(GameObservation::Present {
            game: GAME,
            fingerprint: [2; 32],
        })
        .unwrap();
        bank.tidy_transport().unwrap();
        drop(bank);
        (card.restarted(None), with_record(saved(), kept()))
    }
    fn in_progress() -> Sd {
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        drop(prepared(&card, &body(0x52, 0x66, 1), kept()));
        card
    }
    /// Without a Pokédex, and without a transport box, for the current save:
    /// v0.4.0 writes them at its next Save and Quit.
    fn unsettled() -> Vec<Sd> {
        [FileName::Dex, FileName::Transport]
            .into_iter()
            .map(|container| {
                let (card, _) = used(false);
                for unit in container.units() {
                    card.0.borrow_mut().files.remove(unit.path);
                }
                card
            })
            .collect()
    }
    /// Puts `bytes` into the rewards slot of the current save on `card`.
    pub(crate) fn write_record(card: &Sd, bytes: [u8; 16]) {
        let mut file = sidecar::Sidecar::new(
            card.clone()
                .open(FileName::Rewards.units())
                .unwrap()
                .unwrap(),
            rewards::FILE,
        );
        let slots = file.slots().unwrap();
        // The slot of the current save: the later of the two.
        let generation = |index: &usize| {
            let tag = slots[*index].and_then(|slot| slot.tag);
            tag.map(|tag| tag.generation)
        };
        let index = (0..slots.len()).max_by_key(generation).unwrap();
        let slot = slots[index].unwrap();
        file.write(index, slot.tag, slot.aux, slot.count, &[&bytes])
            .unwrap();
    }

    #[test]
    fn its_record_goes_into_the_body_without_the_day_of_a_redemption() {
        let (card, body) = used(false);
        // Ten files: every one of its containers twice.
        assert_eq!(card.files().len(), 10);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(current(&card).unwrap() == body);
        let view = current_blob::NativeBlobView::parse_layout(&body).unwrap();
        let now = offline_core::rewards::Record {
            fraction: 321,
            redeemed: None,
        };
        assert_eq!(view.rewards(), now);
        // The unused places are as in a new Bank, but for the remainder.
        for range in current_blob::UNUSED {
            for at in range {
                let fraction = FRACTION_OFFSET..FRACTION_OFFSET + 2;
                assert!(fraction.contains(&at) || body[at] == 0, "{at:#x}");
            }
        }
    }

    #[test]
    fn a_bank_that_was_never_saved_is_converted_too() {
        let card = Sd::default();
        let first = body(0x31, 0x44, 2);
        created(&card, &first);
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_target_files(&card));
        // Its rewards record is "none": no remainder, never redeemed.
        assert!(current(&card).unwrap() == with_record(first, Record::NONE));
    }

    #[test]
    fn a_waiting_delivery_is_taken_into_the_transport_box() {
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        let delivered = body(0, 0, 4);
        let mut file = sidecar::Sidecar::new(
            card.clone()
                .open(FileName::Transport.units())
                .unwrap()
                .unwrap(),
            transport::KIND,
        );
        transport::deliver(
            &mut file,
            &delivered[TRANSPORT_RECORDS],
            &delivered[TRANSPORT_TAGS],
        )
        .unwrap();
        drop(file);
        assert_eq!(convert(&card), Ok(Done::Converted));
        let mut expected = with_record(body(0x31, 0x44, 0), Record::NONE);
        expected[TRANSPORT_RECORDS].copy_from_slice(&delivered[TRANSPORT_RECORDS]);
        expected[TRANSPORT_TAGS].copy_from_slice(&delivered[TRANSPORT_TAGS]);
        assert!(current(&card).unwrap() == expected);
    }

    #[test]
    fn a_damaged_bank_is_left_as_it_is() {
        let (card, _) = used(false);
        // Both journal records gone: nothing says which boxes are current.
        for unit in &FileName::Bank.units()[..2] {
            card.0
                .borrow_mut()
                .files
                .get_mut(unit.path)
                .unwrap()
                .fill(0xa5);
        }
        let files = card.files();
        assert_eq!(
            convert(&card),
            Err(Stopped::Failed(Step::ReadOld, Cause::Damaged))
        );
        assert!(card.files() == files);
    }

    #[test]
    fn a_balance_that_v0_3_1_kept_in_its_record_comes_along() {
        // v0.3.1 wrote zero into the body and the balance into the first
        // four bytes of its record; the release after it reads both.
        let card = Sd::default();
        created(&card, &body(0x31, 0x44, 0));
        let mut saved = saved();
        saved[0x170..0x174].fill(0);
        let mut bank = prepared(&card, &saved, kept());
        bank.reconcile(GameObservation::Present {
            game: GAME,
            fingerprint: [2; 32],
        })
        .unwrap();
        bank.tidy_transport().unwrap();
        drop(bank);
        let mut earlier = rewards::encode(kept());
        earlier[..4].copy_from_slice(&1234u32.to_le_bytes());
        write_record(&card, earlier);
        let card = card.restarted(None);
        assert_eq!(convert(&card), Ok(Done::Converted));
        let mut expected = with_record(saved, kept());
        expected[0x170..0x174].copy_from_slice(&1234u32.to_le_bytes());
        assert!(current(&card).unwrap() == expected);
        // A record of that release itself adds nothing.
        assert_eq!(rewards::balance_beside(7, &[0; 16]), None);
    }

    #[test]
    fn a_bank_without_rewards_files_is_converted_as_that_release_reads_it() {
        let (card, body) = used(false);
        for unit in FileName::Rewards.units() {
            card.0.borrow_mut().files.remove(unit.path);
        }
        assert_eq!(convert(&card), Ok(Done::Converted));
        assert!(only_the_target_files(&card));
        // No remainder, never redeemed.
        assert!(current(&card).unwrap() == with_record(body, Record::NONE));
    }

    /// The save in progress on `card` and the head it is in.
    fn journal(card: &Sd) -> (Head, PendingTransfer) {
        let storage = card.clone().open(FileName::Bank.units()).unwrap().unwrap();
        let head = BankStore::new(storage, session::BANK_LAYOUT)
            .inspect()
            .unwrap();
        let Phase::Prepared(pending) = head.phase() else {
            panic!("no save in progress");
        };
        (head, pending)
    }
    /// The save in progress on `card` bound by `game`, written the way the
    /// release writes its records.
    fn bound_by(card: &Sd, game: GameIdentity) {
        let storage = card.clone().open(FileName::Bank.units()).unwrap().unwrap();
        let mut store = BankStore::new(storage, session::BANK_LAYOUT);
        let head = store.inspect().unwrap();
        store.rebind(&head, game).unwrap();
    }
    /// v0.3.0's binding, written out here from its `transaction.rs`: a whole
    /// SHA-256 over the title, both fingerprints and the kind of copy.
    fn of_v0_3_0(pending: &PendingTransfer, medium: u8) -> GameIdentity {
        let mut hash = offline_core::Sha256::new();
        hash.update(b"Bank offline game transaction v2\0").unwrap();
        hash.update(&pending.game.title_id.to_le_bytes()).unwrap();
        hash.update(&pending.before_fingerprint).unwrap();
        hash.update(&pending.after_fingerprint).unwrap();
        hash.update(&[medium]).unwrap();
        GameIdentity {
            title_id: pending.game.title_id,
            save_identity: hash.finalize(),
        }
    }
    /// A card with a save in progress that v0.3.0 began on `medium`.
    fn begun_by_v0_3_0(medium: u8) -> Sd {
        let card = in_progress();
        let (_, pending) = journal(&card);
        bound_by(&card, of_v0_3_0(&pending, medium));
        card.restarted(None)
    }
    /// Everything on `card` but the two journal records.
    fn all_but_the_journal(card: &Sd) -> std::collections::BTreeMap<&'static str, Vec<u8>> {
        let mut files = card.files();
        for unit in &FileName::Bank.units()[..2] {
            files.remove(unit.path);
        }
        files
    }

    #[test]
    fn a_save_in_progress_of_v0_3_0_is_bound_as_v0_4_0_binds_one() {
        for (medium, kind) in [
            (0, binding::Medium::Cartridge),
            (1, binding::Medium::Installed),
        ] {
            let card = begun_by_v0_3_0(medium);
            let (_, before) = journal(&card);
            assert_eq!(binding::bound(&before), None, "v0.4.0 does not read it");
            let others = all_but_the_journal(&card);
            assert_eq!(
                crate::testing::report(&card),
                crate::migrate::Report {
                    source: Some(crate::source::Source::V0_4_0),
                    result: Err(Stopped::InProgress)
                }
            );
            // The same save in progress, bound as v0.4.0 binds one, for the
            // kind of copy it was made with and without a trainer.
            let (_, after) = journal(&card);
            assert_eq!(binding::bound(&after), Some(kind));
            assert_eq!(binding::trainer(&after.game), ([0; binding::NAME], 0));
            assert_eq!(
                after,
                PendingTransfer {
                    game: after.game,
                    ..before
                }
            );
            assert!(all_but_the_journal(&card) == others);
            // Once is enough: the next run finds it as v0.4.0 reads it.
            let files = card.files();
            assert_eq!(crate::testing::convert(&card), Err(Stopped::InProgress));
            assert!(card.files() == files);
        }
    }

    #[test]
    fn a_save_in_progress_v0_4_0_reads_or_does_not_know_is_left_as_it_is() {
        // Bound as v0.3.1 to v0.4.0 bind one.
        let card = in_progress();
        let (_, pending) = journal(&card);
        let v3 = binding::v3(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            binding::Medium::Cartridge,
            [0x41; binding::NAME],
            12345,
        )
        .unwrap();
        bound_by(&card, v3);
        // And the fixture's own, which is neither form.
        for card in [card.restarted(None), in_progress()] {
            let files = card.files();
            assert_eq!(crate::testing::convert(&card), Err(Stopped::InProgress));
            assert!(card.files() == files);
        }
    }

    #[test]
    fn a_cut_while_the_record_is_written_leaves_the_one_before() {
        for console in [false, true] {
            let card = begun_by_v0_3_0(1);
            card.0.borrow_mut().console = console;
            let (_, before) = journal(&card);
            let whole = card.restarted(Some(usize::MAX));
            let _ = crate::testing::convert(&whole);
            let total = whole.0.borrow().operations;
            let mut rewritten = 0;
            for cut in 0..total {
                let cut_card = card.restarted(Some(cut));
                let _ = crate::testing::convert(&cut_card);
                let after = cut_card.restarted(None);
                // Always one whole record: the one before, or the new one.
                let (_, now) = journal(&after);
                if binding::bound(&now).is_some() {
                    rewritten += 1;
                } else {
                    assert_eq!(now, before, "cut {cut}");
                }
                // The next start gets it to v0.4.0's form either way.
                assert_eq!(crate::testing::convert(&after), Err(Stopped::InProgress));
                let (_, last) = journal(&after);
                assert_eq!(
                    binding::bound(&last),
                    Some(binding::Medium::Installed),
                    "cut {cut}"
                );
            }
            // Some cuts fell after the first record was whole.
            assert!(rewritten > 0);
        }
    }
}
