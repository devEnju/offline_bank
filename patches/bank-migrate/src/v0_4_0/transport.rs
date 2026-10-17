//! Frozen: v0.4.0's `transport.rs`. Only the migration reads it.
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

use super::{
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
