//! Rules for the transport-box file shared with a patched Poké Transporter.
//!
//! The file is a `sidecar` with two slots. A slot's payload is the native
//! transport box: 30 stored records (`E8` bytes each) followed by their 30
//! format-tag bytes. `count` is the number of Pokémon; `aux` is a delivery id.
//!
//! - **Transporter** writes a *delivery* (`deliver`): a slot with no tag,
//!   holding the complete transport box the original Transporter filled, all
//!   30 records and tags, with `count` 1..=30 occupied positions anywhere in
//!   it and a nonzero id that differs from every `aux` it saw. It may deliver
//!   only when `may_deliver` holds.
//! - **Bank** shows the slot tagged with its current snapshot. If that box is
//!   empty or absent and a delivery exists, the delivery becomes the box.
//! - On Save and Quit Bank writes the box to the other slot, tagged with the
//!   snapshot being written, with `aux` set to the id of the delivery it took.
//!   After the save is published it voids every slot except its own. That
//!   is the only time slots are removed; a load never writes this file.
//!
//! A delivery whose id equals the current slot's `aux` was already taken and
//! is ignored, and removed by the next published save, so a power cut can
//! neither duplicate nor lose a delivery.

use crate::{
    sections::{occupied, RECORD_SIZE, TRANSPORT_SIZE, TRANSPORT_SLOTS},
    sidecar::{matching, Kind, Sidecar, SidecarError, Slot, Tag, SLOTS},
    Storage,
};

pub const KIND: Kind = Kind {
    magic: *b"BKOFTRN1",
    capacity: TRANSPORT_SIZE as u32,
};

/// A slot written by Transporter and not yet voided.
pub fn is_delivery(slot: &Option<Slot>) -> bool {
    slot.is_some_and(|slot| {
        slot.tag.is_none()
            && slot.aux != 0
            && (1..=TRANSPORT_SLOTS as u32).contains(&slot.count)
            && slot.len == TRANSPORT_SIZE as u32
    })
}

/// What Bank does with the file when it loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// Slot whose payload becomes the transport box; `None` means empty box.
    pub source: Option<usize>,
    /// Set when `source` is a delivery: its id and Pokémon count. The
    /// delivered box is complete and is shown unchanged.
    pub adopted: Option<(u32, u32)>,
    /// Slots that are neither the current box nor a waiting delivery. Bank
    /// removes them only when a save is published.
    pub void: [bool; SLOTS],
}

pub fn on_load(slots: &[Option<Slot>; SLOTS], current: Tag) -> Load {
    let own = matching(slots, current);
    let own_slot = own.and_then(|index| slots[index]);
    let mut waiting = (0..SLOTS).find(|&index| Some(index) != own && is_delivery(&slots[index]));
    // Already taken by the save that produced the current snapshot.
    if let (Some(index), Some(own_slot)) = (waiting, own_slot) {
        if slots[index].is_some_and(|slot| slot.aux == own_slot.aux) {
            waiting = None;
        }
    }
    let mut void = [false; SLOTS];
    for (index, flag) in void.iter_mut().enumerate() {
        *flag = slots[index].is_some() && Some(index) != own && Some(index) != waiting;
    }
    let occupied = own_slot.is_some_and(|slot| slot.count != 0);
    match waiting {
        Some(index) if !occupied => Load {
            source: Some(index),
            adopted: slots[index].map(|slot| (slot.aux, slot.count)),
            void,
        },
        _ => Load {
            source: own,
            adopted: None,
            void,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict;

/// The slot Bank writes its box to on Save and Quit, and the `aux` to store.
/// `adopted` is the delivery taken at load, as (slot index, id).
pub fn on_save(
    slots: &[Option<Slot>; SLOTS],
    current: Tag,
    adopted: Option<(usize, u32)>,
) -> Result<(usize, u32), Conflict> {
    let own = matching(slots, current);
    let previous = own
        .and_then(|index| slots[index])
        .map_or(0, |slot| slot.aux);
    match adopted {
        // The delivery must survive until the save is published.
        Some((index, id)) if index < SLOTS => Ok((1 - index, id)),
        Some(_) => Err(Conflict),
        None => {
            let index = own.map_or(0, |index| 1 - index);
            if is_delivery(&slots[index]) && slots[index].is_some_and(|slot| slot.aux != previous) {
                // A delivery that was not taken cannot be overwritten.
                return Err(Conflict);
            }
            Ok((index, previous))
        }
    }
}

/// Transporter's rule: nothing is waiting, the box is empty, and no Bank save
/// is unresolved. With two valid slots the user must open Bank first.
pub fn may_deliver(slots: &[Option<Slot>; SLOTS]) -> bool {
    let valid = slots.iter().flatten().count();
    valid <= 1 && slots.iter().flatten().all(|slot| slot.count == 0)
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliverError<E> {
    Io(E),
    /// The records or tags are not one complete transport box.
    Layout,
    /// No slot of the box holds a Pokémon.
    Empty,
    /// `may_deliver` does not hold: Bank has to be opened and saved first.
    Refused,
    /// The slot could not be written and verified.
    Write,
}

/// What `deliver` wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub index: usize,
    pub id: u32,
    pub count: u32,
}

/// Transporter's whole part of the hand-over: checks the rule, then writes
/// the box as a delivery into the slot Bank is not using. `records` and `tags`
/// are the native transport box, unchanged. A cut leaves no delivery at all.
pub fn deliver<S: Storage>(
    file: &mut Sidecar<S>,
    records: &[u8],
    tags: &[u8],
) -> Result<Delivery, DeliverError<S::Error>> {
    if records.len() != TRANSPORT_SLOTS * RECORD_SIZE || tags.len() != TRANSPORT_SLOTS {
        return Err(DeliverError::Layout);
    }
    let count = occupied(records);
    if count == 0 {
        return Err(DeliverError::Empty);
    }
    let io = |error| match error {
        SidecarError::Io(error) => DeliverError::Io(error),
        _ => DeliverError::Write,
    };
    let slots = file.slots().map_err(io)?;
    if !may_deliver(&slots) {
        return Err(DeliverError::Refused);
    }
    // At most one slot is valid here: Bank's own, empty box.
    let index = usize::from(slots[0].is_some());
    let seen = slots.iter().flatten().map(|slot| slot.aux).max();
    let id = match seen.map_or(1, |aux| aux.wrapping_add(1)) {
        0 => 1,
        id => id,
    };
    file.write(index, None, id, count, &[records, tags])
        .map_err(io)?;
    Ok(Delivery { index, id, count })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    struct Memory {
        bytes: Vec<u8>,
        budget: Option<usize>,
    }
    impl Storage for Memory {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            out.copy_from_slice(&self.bytes[at as usize..at as usize + out.len()]);
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            for (index, byte) in bytes.iter().enumerate() {
                if let Some(budget) = &mut self.budget {
                    if *budget == 0 {
                        return Err(());
                    }
                    *budget -= 1;
                }
                self.bytes[at as usize + index] = *byte;
            }
            Ok(())
        }
        fn sync(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn recreate(&mut self, at: u64, length: u64) -> Result<(), ()> {
            self.bytes[at as usize..(at + length) as usize].fill(0);
            Ok(())
        }
    }
    fn file(bytes: Vec<u8>, budget: Option<usize>) -> Sidecar<Memory> {
        Sidecar::new(Memory { bytes, budget }, KIND)
    }
    fn empty_file() -> Vec<u8> {
        vec![0; KIND.file_len() as usize]
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
    /// A native box with Pokémon in the given positions, blank elsewhere.
    fn native_box(positions: &[usize]) -> (Vec<u8>, Vec<u8>) {
        let mut records = Vec::new();
        let mut tags = vec![0u8; TRANSPORT_SLOTS];
        for (slot, tag) in tags.iter_mut().enumerate() {
            let held = positions.contains(&slot);
            records.extend_from_slice(&record(if held { 100 + slot as u16 } else { 0 }));
            *tag = u8::from(held);
        }
        (records, tags)
    }

    #[test]
    fn delivers_the_whole_box_with_gaps_and_bank_takes_it_unchanged() {
        let (records, tags) = native_box(&[0, 3, 29]);
        let mut file = file(empty_file(), None);
        let done = deliver(&mut file, &records, &tags).unwrap();
        assert_eq!((done.index, done.id, done.count), (0, 1, 3));

        let slots = file.slots().unwrap();
        assert!(is_delivery(&slots[0]) && slots[1].is_none());
        let load = on_load(&slots, T1);
        assert_eq!((load.source, load.adopted), (Some(0), Some((1, 3))));
        let (mut first, mut second) = (vec![0; records.len()], vec![0; tags.len()]);
        file.read_split(0, &slots[0].unwrap(), &mut first, &mut second)
            .unwrap();
        assert!(first == records && second == tags);
        // A second delivery has to wait for Bank.
        assert_eq!(
            deliver(&mut file, &records, &tags),
            Err(DeliverError::Refused)
        );
    }

    #[test]
    fn delivery_uses_the_slot_bank_is_not_using_and_a_new_id() {
        let (records, tags) = native_box(&[5]);
        let (blank, blank_tags) = native_box(&[]);
        for (own, last) in [(0, 0), (1, 7), (0, u32::MAX)] {
            let mut file = file(empty_file(), None);
            file.write(own, Some(T1), last, 0, &[&blank, &blank_tags])
                .unwrap();
            let done = deliver(&mut file, &records, &tags).unwrap();
            assert_eq!(done.index, 1 - own);
            assert!(done.id != 0 && done.id != last, "after {last:#x}");
            let load = on_load(&file.slots().unwrap(), T1);
            assert_eq!(load.adopted, Some((done.id, 1)));
        }
    }

    #[test]
    fn refuses_a_full_box_wrong_sizes_and_an_empty_delivery() {
        let (records, tags) = native_box(&[1, 2]);
        let (blank, blank_tags) = native_box(&[]);
        let mut held = file(empty_file(), None);
        held.write(0, Some(T1), 0, 2, &[&records, &tags]).unwrap();
        let before = held.slots().unwrap();
        assert_eq!(
            deliver(&mut held, &records, &tags),
            Err(DeliverError::Refused)
        );
        assert_eq!(held.slots().unwrap(), before);

        let mut open = file(empty_file(), None);
        assert_eq!(
            deliver(&mut open, &blank, &blank_tags),
            Err(DeliverError::Empty)
        );
        assert_eq!(
            deliver(&mut open, &records[1..], &tags),
            Err(DeliverError::Layout)
        );
        assert_eq!(
            deliver(&mut open, &records, &tags[1..]),
            Err(DeliverError::Layout)
        );
        assert_eq!(open.slots().unwrap(), [None, None]);
    }

    #[test]
    fn a_cut_at_any_byte_leaves_no_delivery_and_bank_keeps_its_box() {
        let (records, tags) = native_box(&[0, 1, 2]);
        let (blank, blank_tags) = native_box(&[]);
        let mut base = file(empty_file(), None);
        let own = base
            .write(1, Some(T1), 4, 0, &[&blank, &blank_tags])
            .unwrap();
        let base = base.into_inner().bytes;
        let total = 64 + TRANSPORT_SIZE + 64;
        for cut in (0..=total).step_by(7).chain([total - 1, total]) {
            let mut attempt = file(base.clone(), Some(cut));
            let result = deliver(&mut attempt, &records, &tags);
            assert_eq!(result.is_ok(), cut == total, "cut {cut}");
            let mut after = file(attempt.into_inner().bytes, None);
            let slots = after.slots().unwrap();
            assert_eq!(slots[1], Some(own), "cut {cut}");
            let load = on_load(&slots, T1);
            if cut == total {
                assert_eq!(load.adopted, Some((5, 3)));
            } else {
                // Nothing half-written is ever shown, and a retry is allowed.
                assert_eq!((load.source, load.adopted), (Some(1), None), "cut {cut}");
                assert!(may_deliver(&slots), "cut {cut}");
            }
        }
    }

    const T1: Tag = Tag {
        generation: 5,
        crc: 0xaaaa,
    };
    const T2: Tag = Tag {
        generation: 6,
        crc: 0xbbbb,
    };
    fn bank(tag: Tag, count: u32, aux: u32) -> Option<Slot> {
        Some(Slot {
            tag: Some(tag),
            aux,
            count,
            len: TRANSPORT_SIZE as u32,
            crc: 1,
        })
    }
    fn delivered(id: u32, count: u32) -> Option<Slot> {
        Some(Slot {
            tag: None,
            aux: id,
            count,
            len: TRANSPORT_SIZE as u32,
            crc: 2,
        })
    }

    #[test]
    fn file_holds_exactly_two_native_transport_boxes() {
        assert_eq!(KIND.capacity, 6_990);
        assert_eq!((KIND.slot_len(), KIND.file_len()), (7_056, 14_112));
    }

    #[test]
    fn missing_or_empty_file_is_an_empty_box_and_open_for_delivery() {
        let load = on_load(&[None, None], T1);
        assert_eq!(
            (load.source, load.adopted, load.void),
            (None, None, [false; 2])
        );
        assert!(may_deliver(&[None, None]));
        assert!(may_deliver(&[bank(T1, 0, 0), None]));
        assert!(may_deliver(&[None, bank(T1, 0, 77)]));
    }

    #[test]
    fn delivery_becomes_the_box_when_the_box_is_empty_or_absent() {
        for own in [None, bank(T1, 0, 0), bank(T1, 0, 3)] {
            let load = on_load(&[own, delivered(4, 12)], T1);
            assert_eq!(load.source, Some(1));
            assert_eq!(load.adopted, Some((4, 12)));
            assert_eq!(load.void, [false, false]);
        }
        // Slot order does not matter.
        let load = on_load(&[delivered(4, 30), bank(T1, 0, 0)], T1);
        assert_eq!((load.source, load.adopted), (Some(0), Some((4, 30))));
    }

    #[test]
    fn transporter_cannot_deliver_onto_pokemon_or_an_unresolved_save() {
        assert!(!may_deliver(&[bank(T1, 3, 0), None]));
        assert!(!may_deliver(&[delivered(4, 1), None]));
        assert!(!may_deliver(&[bank(T1, 0, 0), delivered(4, 1)]));
        // Two valid slots: Transporter cannot tell which one the journal chose.
        assert!(!may_deliver(&[bank(T1, 0, 0), bank(T2, 0, 0)]));
    }

    #[test]
    fn saving_keeps_the_delivery_until_published_then_voids_it() {
        // Loaded: empty own box plus delivery 4. Some Pokémon stay in the box.
        let before = [bank(T1, 0, 0), delivered(4, 12)];
        let load = on_load(&before, T1);
        let (index, aux) = on_save(&before, T1, Some((1, 4))).unwrap();
        assert_eq!((index, aux), (0, 4));
        let during = [bank(T2, 9, 4), delivered(4, 12)];

        // Rolled back: the journal still says T1. Nothing matches; the
        // delivery is taken again, exactly as before.
        let again = on_load(&during, T1);
        assert_eq!((again.source, again.adopted), (load.source, load.adopted));
        assert_eq!(again.void, [true, false]);

        // Published: T2 is current. The delivery was taken and is voided,
        // even when every Pokémon was moved out of the box before saving.
        for left in [9, 0] {
            let after = on_load(&[bank(T2, left, 4), delivered(4, 12)], T2);
            assert_eq!((after.source, after.adopted), (Some(0), None));
            assert_eq!(after.void, [false, true]);
        }
        // Once voided, Transporter may deliver again only into an empty box.
        assert!(!may_deliver(&[bank(T2, 9, 4), None]));
        assert!(may_deliver(&[bank(T2, 0, 4), None]));
        // A new delivery has a new id and is taken normally.
        let next = on_load(&[bank(T2, 0, 4), delivered(5, 2)], T2);
        assert_eq!(next.adopted, Some((5, 2)));
    }

    #[test]
    fn saves_without_a_delivery_alternate_slots_and_keep_the_taken_id() {
        let slots = [bank(T1, 9, 4), None];
        assert_eq!(on_save(&slots, T1, None), Ok((1, 4)));
        let slots = [bank(T1, 9, 4), bank(T2, 8, 4)];
        assert_eq!(on_save(&slots, T2, None), Ok((0, 4)));
        // The stale slot of the previous snapshot is voided after loading.
        assert_eq!(on_load(&slots, T2).void, [true, false]);
        // No file yet.
        assert_eq!(on_save(&[None, None], T1, None), Ok((0, 0)));
    }

    #[test]
    fn a_waiting_delivery_is_never_overwritten() {
        // The box still holds Pokémon, so the delivery was not taken.
        let slots = [bank(T1, 2, 0), delivered(4, 12)];
        let load = on_load(&slots, T1);
        assert_eq!((load.source, load.adopted), (Some(0), None));
        assert_eq!(load.void, [false, false]);
        assert_eq!(on_save(&slots, T1, None), Err(Conflict));
        assert_eq!(on_save(&slots, T1, Some((2, 4))), Err(Conflict));
    }

    #[test]
    fn malformed_deliveries_are_ignored_and_voided() {
        let wrong_length = Some(Slot {
            len: 100,
            ..delivered(4, 3).unwrap()
        });
        for bad in [
            delivered(4, 0),
            delivered(4, 31),
            delivered(0, 3),
            wrong_length,
        ] {
            let load = on_load(&[bank(T1, 0, 0), bad], T1);
            assert_eq!((load.source, load.adopted), (Some(0), None));
            assert_eq!(load.void, [false, true]);
        }
    }
}
