//! Rules for the transport box shared with a patched Poké Transporter.
//!
//! The transport file is a `pair`: two copies of the native transport box,
//! 30 stored records (`E8` bytes each) followed by their 30 format-tag
//! bytes. The journal says which copy is Bank's box and how many Pokémon it
//! holds, and whether the other copy holds a delivery.
//!
//! - **Transporter** delivers (`deliver`): it writes the complete transport
//!   box the original Transporter filled, all 30 records and tags, into the
//!   copy that is not in use, makes it durable and reads it back, and only
//!   then announces it in the journal. Until the journal says so there is no
//!   delivery. It may deliver only while `check` allows it, which it asks
//!   before a session begins and once more when it delivers.
//! - **Bank** shows the copy the journal names. If a delivery waits, the
//!   box in use is empty, and the delivery becomes the box (`on_load`).
//! - On Save and Quit Bank writes the box into the copy `Save::next_mover`
//!   gives: the one that is not in use, or, while a delivery waits there,
//!   over the empty box in use, so that the delivery outlasts a save that is
//!   undone. The journal record that makes the save current also says that
//!   nothing waits any more: a power cut can neither duplicate nor lose a
//!   delivery, and nothing has to be tidied up afterwards.

use crate::{
    pair::{Pair, PairError},
    sections::{occupied, RECORD_SIZE, TRANSPORT_SIZE, TRANSPORT_SLOTS},
    store::{may_deliver, Head, Journal, StoreError},
    Metadata, Part, Phase, Slot, Storage,
};

/// Length of one copy of the transport file.
pub const LEN: u32 = TRANSPORT_SIZE as u32;

/// What Bank shows as the transport box when it loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// The copy whose bytes become the transport box.
    pub source: Part,
    /// `source` is a delivery, which is complete and is shown unchanged.
    pub delivery: bool,
}

/// The box of the save in force, or the delivery that waits beside it.
pub fn on_load(metadata: &Metadata) -> Load {
    let current = metadata.phase.current();
    match metadata.delivery {
        Some(crc) => Load {
            source: Part {
                slot: current.mover.slot.other(),
                crc,
            },
            delivery: true,
        },
        None => Load {
            source: current.mover,
            delivery: false,
        },
    }
}

/// Bank's files as Transporter reaches them. Each call opens the two files
/// of one pair as one range, or answers `None` if they are not there or
/// cannot be opened. The functions here never hold both pairs at once.
pub trait Files {
    type Storage: Storage;
    /// The two journal records.
    fn journal(&mut self) -> Option<Self::Storage>;
    /// The two copies of the transport box.
    fn mover(&mut self) -> Option<Self::Storage>;
}

/// Why Transporter cannot deliver now. Opening Bank shows or solves each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Pokémon are in the way: in the transport box, or in a delivery that
    /// waits.
    Occupied,
    /// No Bank exists, a save of it is in progress, or a file that would be
    /// written cannot be read: Transporter replaces no file of Bank's, and
    /// Bank's next save does.
    NotReady,
}

/// The record a delivery would follow, and the transport file, open.
fn ready<F: Files>(files: &mut F) -> Result<(Head, Pair<F::Storage>), Refusal> {
    let head = {
        let mut journal = Journal::new(files.journal().ok_or(Refusal::NotReady)?);
        let Ok(Some(head)) = journal.load() else {
            return Err(Refusal::NotReady);
        };
        if !journal.readable() {
            return Err(Refusal::NotReady);
        }
        head
    };
    let Phase::Clean(current) = head.phase() else {
        return Err(Refusal::NotReady);
    };
    if !may_deliver(head.metadata()) {
        return Err(Refusal::Occupied);
    }
    let mut file = Pair::new(files.mover().ok_or(Refusal::NotReady)?, LEN);
    if file.checksum(current.mover.slot.other()).is_err() {
        return Err(Refusal::NotReady);
    }
    Ok((head, file))
}

/// Transporter's question before a session begins, and the first thing a
/// delivery asks again: may a box be handed over now? Nothing is written.
pub fn check<F: Files>(files: &mut F) -> Result<(), Refusal> {
    ready(files).map(|_| ())
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliverError<E> {
    Io(E),
    /// The records or tags are not one complete transport box.
    Layout,
    /// The box holds no Pokémon.
    Empty,
    /// `check` does not allow it.
    Refused(Refusal),
    /// The journal or the transport file cannot be used.
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub slot: Slot,
    pub count: u32,
    pub crc: u32,
}

/// Transporter's whole part of the hand-over: checks the rule, writes the
/// box into the copy Bank is not using, then announces it. `records` and
/// `tags` are the native transport box, unchanged. A cut leaves no delivery
/// at all, or the complete one.
pub fn deliver<F: Files>(
    files: &mut F,
    records: &[u8],
    tags: &[u8],
) -> Result<Delivery, DeliverError<<F::Storage as Storage>::Error>> {
    if records.len() != TRANSPORT_SLOTS * RECORD_SIZE || tags.len() != TRANSPORT_SLOTS {
        return Err(DeliverError::Layout);
    }
    let count = occupied(records);
    if count == 0 {
        return Err(DeliverError::Empty);
    }
    let (head, mut file) = ready(files).map_err(DeliverError::Refused)?;
    let slot = head.phase().current().mover.slot.other();
    let crc = file
        .write(slot, &[records, tags])
        .map_err(|error| match error {
            PairError::Io(error) => DeliverError::Io(error),
            _ => DeliverError::Write,
        })?;
    drop(file);
    let journal = files.journal();
    let mut journal = Journal::new(journal.ok_or(DeliverError::Refused(Refusal::NotReady))?);
    journal.announce(&head, crc).map_err(|error| match error {
        StoreError::Io(error) => DeliverError::Io(error),
        StoreError::DeliveryRefused | StoreError::StaleHead => {
            DeliverError::Refused(Refusal::NotReady)
        }
        _ => DeliverError::Write,
    })?;
    Ok(Delivery { slot, count, crc })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::{GameIdentity, PendingTransfer, Save, METADATA_SIZE};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        vec,
        vec::Vec,
    };

    type Bytes = Rc<RefCell<Vec<u8>>>;
    /// The two pairs of files, and how many of them are open.
    struct Card {
        journal: Option<Bytes>,
        mover: Option<Bytes>,
        open: Rc<Cell<usize>>,
    }
    struct Open(Bytes, Rc<Cell<usize>>);
    impl Card {
        fn open(&self, file: &Option<Bytes>) -> Option<Open> {
            let file = file.clone()?;
            self.open.set(self.open.get() + 1);
            assert_eq!(self.open.get(), 1, "both pairs open at once");
            Some(Open(file, self.open.clone()))
        }
        fn bytes(&self) -> (Vec<u8>, Vec<u8>) {
            let of = |file: &Option<Bytes>| file.as_ref().unwrap().borrow().clone();
            (of(&self.journal), of(&self.mover))
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            self.1.set(self.1.get() - 1);
        }
    }
    impl Files for Card {
        type Storage = Open;
        fn journal(&mut self) -> Option<Open> {
            self.open(&self.journal)
        }
        fn mover(&mut self) -> Option<Open> {
            self.open(&self.mover)
        }
    }
    impl Storage for Open {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            let bytes = self.0.borrow();
            let from = bytes.get(at as usize..at as usize + out.len()).ok_or(())?;
            out.copy_from_slice(from);
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            self.0.borrow_mut()[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn sync(&mut self) -> Result<(), ()> {
            Ok(())
        }
        /// Transporter replaces no file.
        fn recreate(&mut self, _: u64, _: u64) -> Result<(), ()> {
            Err(())
        }
    }

    /// A stored record whose species word decrypts to `species`.
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
    /// A transport box with `pokemon` in its first slots.
    fn a_box(pokemon: usize) -> (Vec<u8>, Vec<u8>) {
        let records = (0..TRANSPORT_SLOTS)
            .flat_map(|slot| record(if slot < pokemon { 25 } else { 0 }))
            .collect();
        (records, vec![2; TRANSPORT_SLOTS])
    }

    fn save(mover: Slot, held: u8) -> Save {
        let part = |slot| Part { slot, crc: 7 };
        Save {
            bank: part(Slot::A),
            main: part(Slot::A),
            mover: part(mover),
            held,
        }
    }
    fn clean(mover: Slot, held: u8, delivery: Option<u32>) -> Metadata {
        Metadata {
            sequence: 4,
            phase: Phase::Clean(save(mover, held)),
            delivery,
        }
    }
    fn in_progress() -> Metadata {
        let part = |slot| Part { slot, crc: 9 };
        Metadata {
            sequence: 4,
            phase: Phase::Prepared(PendingTransfer {
                before: save(Slot::A, 0),
                after: Save {
                    bank: part(Slot::B),
                    main: part(Slot::B),
                    mover: part(Slot::B),
                    held: 0,
                },
                game: GameIdentity {
                    title_id: 0x0004_0000_0005_5e00,
                    save_identity: [7; 32],
                },
                before_fingerprint: [1; 32],
                after_fingerprint: [2; 32],
            }),
            delivery: None,
        }
    }
    /// A card whose journal holds `metadata` in both records, or nothing.
    fn card(metadata: Option<Metadata>) -> Card {
        let mut journal = vec![0; 2 * METADATA_SIZE];
        if let Some(metadata) = metadata {
            let bytes = metadata.encode().unwrap();
            journal[..METADATA_SIZE].copy_from_slice(&bytes);
            journal[METADATA_SIZE..].copy_from_slice(&bytes);
        }
        Card {
            journal: Some(Rc::new(RefCell::new(journal))),
            mover: Some(Rc::new(RefCell::new(vec![0xAA; 2 * TRANSPORT_SIZE]))),
            open: Rc::new(Cell::new(0)),
        }
    }
    fn record_in_force(card: &mut Card) -> Metadata {
        let mut journal = Journal::new(card.journal().unwrap());
        *journal.load().unwrap().unwrap().metadata()
    }

    #[test]
    fn bank_shows_its_own_box_or_the_delivery_that_waits_beside_it() {
        for mover in [Slot::A, Slot::B] {
            let own = Part {
                slot: mover,
                crc: 7,
            };
            assert_eq!(
                on_load(&clean(mover, 3, None)),
                Load {
                    source: own,
                    delivery: false
                }
            );
            let waiting = Part {
                slot: mover.other(),
                crc: 0x1234,
            };
            assert_eq!(
                on_load(&clean(mover, 0, Some(0x1234))),
                Load {
                    source: waiting,
                    delivery: true
                }
            );
        }
        // While a save is in progress the save before it is in force.
        assert_eq!(on_load(&in_progress()).source.slot, Slot::A);
    }

    #[test]
    fn the_check_allows_an_empty_settled_box_and_says_why_not_otherwise() {
        assert_eq!(check(&mut card(Some(clean(Slot::A, 0, None)))), Ok(()));
        assert_eq!(check(&mut card(Some(clean(Slot::B, 0, None)))), Ok(()));
        for (metadata, refusal) in [
            // No Bank yet.
            (None, Refusal::NotReady),
            (Some(in_progress()), Refusal::NotReady),
            (Some(clean(Slot::A, 1, None)), Refusal::Occupied),
            (Some(clean(Slot::A, 30, None)), Refusal::Occupied),
            (Some(clean(Slot::A, 0, Some(5))), Refusal::Occupied),
        ] {
            let mut card = card(metadata);
            let before = card.bytes();
            assert_eq!(check(&mut card), Err(refusal));
            // Where the check refuses the delivery does, and writes nothing.
            let (records, tags) = a_box(3);
            assert_eq!(
                deliver(&mut card, &records, &tags),
                Err(DeliverError::Refused(refusal))
            );
            assert!(card.bytes() == before);
        }
    }

    #[test]
    fn files_that_are_missing_or_would_not_take_a_write_are_for_bank_to_repair() {
        let whole = || card(Some(clean(Slot::A, 0, None)));
        let mut no_journal = whole();
        no_journal.journal = None;
        let mut no_box = whole();
        no_box.mover = None;
        // A record that cannot be read cannot be written either, and
        // Transporter does not replace it; the same for the copy of the box
        // it would write.
        let short_journal = whole();
        short_journal.journal.as_ref().unwrap().borrow_mut().pop();
        let short_box = whole();
        short_box.mover.as_ref().unwrap().borrow_mut().pop();
        for mut card in [no_journal, no_box, short_journal, short_box] {
            assert_eq!(check(&mut card), Err(Refusal::NotReady));
        }
        // A record that reads but is torn is written over like any other.
        let mut torn = whole();
        torn.journal.as_ref().unwrap().borrow_mut()[METADATA_SIZE + 9] ^= 1;
        assert_eq!(check(&mut torn), Ok(()));
        let (records, tags) = a_box(1);
        deliver(&mut torn, &records, &tags).unwrap();
        // The copy of the box that is in use is never read by Transporter.
        let mut current_short = card(Some(clean(Slot::B, 0, None)));
        current_short.mover.as_ref().unwrap().borrow_mut().pop();
        assert_eq!(check(&mut current_short), Ok(()));
    }

    #[test]
    fn a_delivery_goes_into_the_copy_not_in_use_and_is_announced_last() {
        for current in [Slot::A, Slot::B] {
            let mut card = card(Some(clean(current, 0, None)));
            let (records, tags) = a_box(12);
            let done = deliver(&mut card, &records, &tags).unwrap();
            let whole = [&records[..], &tags[..]].concat();
            assert_eq!(
                done,
                Delivery {
                    slot: current.other(),
                    count: 12,
                    crc: crate::crc32(&whole)
                }
            );
            // The copy is the native box and nothing else; Bank's own box
            // is not touched.
            let (journal, mover) = card.bytes();
            let at = done.slot.index() as usize * TRANSPORT_SIZE;
            assert!(mover[at..at + TRANSPORT_SIZE] == whole);
            let own = current.index() as usize * TRANSPORT_SIZE;
            assert!(mover[own..own + TRANSPORT_SIZE]
                .iter()
                .all(|byte| *byte == 0xAA));
            // Both records say so, with the next sequence number and
            // nothing else changed.
            assert!(journal[..METADATA_SIZE] == journal[METADATA_SIZE..]);
            let announced = record_in_force(&mut card);
            assert_eq!(
                announced,
                Metadata {
                    sequence: 5,
                    delivery: Some(done.crc),
                    ..clean(current, 0, None)
                }
            );
            let waiting = Part {
                slot: done.slot,
                crc: done.crc,
            };
            assert_eq!(on_load(&announced).source, waiting);
            // Now Pokémon are in the way.
            assert_eq!(check(&mut card), Err(Refusal::Occupied));
            assert_eq!(
                deliver(&mut card, &records, &tags),
                Err(DeliverError::Refused(Refusal::Occupied))
            );
            assert!(card.bytes() == (journal, mover));
        }
    }

    #[test]
    fn only_a_whole_box_that_holds_pokemon_is_delivered() {
        let mut card = card(Some(clean(Slot::A, 0, None)));
        let before = card.bytes();
        let (records, tags) = a_box(2);
        let (blank, _) = a_box(0);
        assert_eq!(deliver(&mut card, &blank, &tags), Err(DeliverError::Empty));
        for (records, tags) in [
            (&records[1..], &tags[..]),
            (&records[..], &tags[1..]),
            (&records[..RECORD_SIZE], &tags[..1]),
        ] {
            assert_eq!(deliver(&mut card, records, tags), Err(DeliverError::Layout));
        }
        assert!(card.bytes() == before);
    }
}
