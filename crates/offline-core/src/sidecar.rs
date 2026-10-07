//! Two-slot side files that follow the Bank journal without one of their own.
//!
//! The Bank file is the single source of truth. A side file holds two slots;
//! each slot is tagged with the generation and payload CRC of the Bank
//! snapshot it belongs to. New content is written to the spare slot *before*
//! the Bank journal is prepared. Whichever Bank snapshot the journal then
//! selects, the slot with the matching tag is the valid one.
//!
//! Slot layout, little endian, at `index * slot_len`:
//! 64-byte header: magic (8, per file) at 0; u16 version 1 at 8; u16 header
//! size 64 at 10; u32 payload length at 12; u64 tag generation at 16 (0 = no
//! tag); u32 tag CRC at 24; u32 `aux` at 28; u32 `count` at 32; u32 payload
//! CRC at 36; zero at 40..60; u32 CRC of bytes 0..60 at 60. The payload
//! follows at +64. An all-zero or damaged header is a void slot.

use crate::{crc32, format::zero, SnapshotRef, Storage};

pub const SLOTS: usize = 2;
pub const HEADER_SIZE: usize = 64;
const VERSION: u16 = 1;

/// Identifies one Bank snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag {
    pub generation: u64,
    pub crc: u32,
}
impl Tag {
    pub fn of(snapshot: &SnapshotRef) -> Self {
        Self {
            generation: snapshot.header.generation,
            crc: snapshot.header.payload_crc32,
        }
    }
}

/// A side-file type: its magic and the largest payload one slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub magic: [u8; 8],
    pub capacity: u32,
}
impl Kind {
    pub const fn slot_len(&self) -> u64 {
        HEADER_SIZE as u64 + ((self.capacity as u64 + 3) & !3)
    }
    pub const fn file_len(&self) -> u64 {
        self.slot_len() * SLOTS as u64
    }
}

/// A valid slot header. `aux` and `count` belong to the file type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub tag: Option<Tag>,
    pub aux: u32,
    pub count: u32,
    pub len: u32,
    pub crc: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SidecarError<E> {
    Io(E),
    PayloadTooLarge,
    WrongLength,
    Checksum,
    Readback,
}

pub struct Sidecar<S> {
    storage: S,
    kind: Kind,
}

/// Index of the slot belonging to `tag`, if any.
pub fn matching(slots: &[Option<Slot>; SLOTS], tag: Tag) -> Option<usize> {
    slots
        .iter()
        .position(|slot| slot.is_some_and(|slot| slot.tag == Some(tag)))
}

/// The slot to overwrite for a new snapshot: never the current one.
pub fn spare(slots: &[Option<Slot>; SLOTS], current: Option<Tag>) -> usize {
    match current.and_then(|tag| matching(slots, tag)) {
        Some(index) => 1 - index,
        None => slots.iter().position(Option::is_none).unwrap_or(0),
    }
}

impl<S: Storage> Sidecar<S> {
    pub fn new(storage: S, kind: Kind) -> Self {
        Self { storage, kind }
    }
    pub fn into_inner(self) -> S {
        self.storage
    }
    fn offset(&self, index: usize) -> u64 {
        index as u64 * self.kind.slot_len()
    }

    fn decode(&self, bytes: &[u8; HEADER_SIZE]) -> Option<Slot> {
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if crc32(&bytes[..60]) != word(60)
            || bytes[..8] != self.kind.magic
            || bytes[8..12] != [VERSION as u8, 0, HEADER_SIZE as u8, 0]
            || !zero(&bytes[40..60])
            || word(12) > self.kind.capacity
        {
            return None;
        }
        let generation = u64::from(word(16)) | (u64::from(word(20)) << 32);
        Some(Slot {
            tag: (generation != 0).then_some(Tag {
                generation,
                crc: word(24),
            }),
            aux: word(28),
            count: word(32),
            len: word(12),
            crc: word(36),
        })
    }

    /// Reads both headers. Damaged or empty headers are `None`, never errors.
    pub fn slots(&mut self) -> Result<[Option<Slot>; SLOTS], SidecarError<S::Error>> {
        let mut out = [None; SLOTS];
        for (index, slot) in out.iter_mut().enumerate() {
            let mut bytes = [0; HEADER_SIZE];
            self.storage
                .read(self.offset(index), &mut bytes)
                .map_err(SidecarError::Io)?;
            *slot = self.decode(&bytes);
        }
        Ok(out)
    }

    /// One read of the payload into its final place, checked on the same pass.
    pub fn read(
        &mut self,
        index: usize,
        slot: &Slot,
        out: &mut [u8],
    ) -> Result<(), SidecarError<S::Error>> {
        if out.len() != slot.len as usize {
            return Err(SidecarError::WrongLength);
        }
        self.storage
            .read(self.offset(index) + HEADER_SIZE as u64, out)
            .map_err(SidecarError::Io)?;
        if crc32(out) != slot.crc {
            return Err(SidecarError::Checksum);
        }
        Ok(())
    }

    /// Like `read` for a payload stored as two consecutive parts.
    pub fn read_split(
        &mut self,
        index: usize,
        slot: &Slot,
        first: &mut [u8],
        second: &mut [u8],
    ) -> Result<(), SidecarError<S::Error>> {
        if first.len() + second.len() != slot.len as usize {
            return Err(SidecarError::WrongLength);
        }
        let at = self.offset(index) + HEADER_SIZE as u64;
        self.storage.read(at, first).map_err(SidecarError::Io)?;
        self.storage
            .read(at + first.len() as u64, second)
            .map_err(SidecarError::Io)?;
        let mut crc = crate::checksum::Crc32::new();
        crc.update(first);
        crc.update(second);
        if crc.finish() != slot.crc {
            return Err(SidecarError::Checksum);
        }
        Ok(())
    }

    /// Voids the header, writes the payload parts, then the header, each made
    /// durable in turn, and reads the header back. A cut leaves a void slot.
    pub fn write(
        &mut self,
        index: usize,
        tag: Option<Tag>,
        aux: u32,
        count: u32,
        parts: &[&[u8]],
    ) -> Result<Slot, SidecarError<S::Error>> {
        let len: usize = parts.iter().map(|part| part.len()).sum();
        if len > self.kind.capacity as usize {
            return Err(SidecarError::PayloadTooLarge);
        }
        let mut payload_crc = crate::checksum::Crc32::new();
        for part in parts {
            payload_crc.update(part);
        }
        let slot = Slot {
            tag,
            aux,
            count,
            len: len as u32,
            crc: payload_crc.finish(),
        };
        self.clear(index)?;
        let mut at = self.offset(index) + HEADER_SIZE as u64;
        for part in parts {
            self.storage.write(at, part).map_err(SidecarError::Io)?;
            at += part.len() as u64;
        }
        self.storage.sync().map_err(SidecarError::Io)?;
        let mut bytes = [0; HEADER_SIZE];
        bytes[..8].copy_from_slice(&self.kind.magic);
        bytes[8..12].copy_from_slice(&[VERSION as u8, 0, HEADER_SIZE as u8, 0]);
        bytes[12..16].copy_from_slice(&slot.len.to_le_bytes());
        if let Some(tag) = tag {
            bytes[16..24].copy_from_slice(&tag.generation.to_le_bytes());
            bytes[24..28].copy_from_slice(&tag.crc.to_le_bytes());
        }
        bytes[28..32].copy_from_slice(&aux.to_le_bytes());
        bytes[32..36].copy_from_slice(&count.to_le_bytes());
        bytes[36..40].copy_from_slice(&slot.crc.to_le_bytes());
        let header_crc = crc32(&bytes[..60]);
        bytes[60..].copy_from_slice(&header_crc.to_le_bytes());
        self.storage
            .write(self.offset(index), &bytes)
            .map_err(SidecarError::Io)?;
        self.storage.sync().map_err(SidecarError::Io)?;
        let mut check = [0; HEADER_SIZE];
        self.storage
            .read(self.offset(index), &mut check)
            .map_err(SidecarError::Io)?;
        if check != bytes {
            return Err(SidecarError::Readback);
        }
        Ok(slot)
    }

    /// Makes a slot void. Repeating it is harmless.
    pub fn clear(&mut self, index: usize) -> Result<(), SidecarError<S::Error>> {
        self.storage
            .write(self.offset(index), &[0; HEADER_SIZE])
            .map_err(SidecarError::Io)?;
        self.storage.sync().map_err(SidecarError::Io)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    const KIND: Kind = Kind {
        magic: *b"BKOFTEST",
        capacity: 10,
    };
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
            // A cut lands a prefix of the write.
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
    }
    fn file() -> Sidecar<Memory> {
        Sidecar::new(
            Memory {
                bytes: vec![0; KIND.file_len() as usize],
                budget: None,
            },
            KIND,
        )
    }
    const OLD: Tag = Tag {
        generation: 7,
        crc: 0x1111,
    };
    const NEW: Tag = Tag {
        generation: 8,
        crc: 0x2222,
    };

    #[test]
    fn geometry_is_fixed_and_aligned() {
        assert_eq!((KIND.slot_len(), KIND.file_len()), (76, 152));
        assert_eq!(
            Kind {
                magic: [0; 8],
                capacity: 16
            }
            .slot_len(),
            80
        );
    }

    #[test]
    fn empty_file_has_no_slots_and_any_slot_is_spare() {
        let mut file = file();
        let slots = file.slots().unwrap();
        assert_eq!(slots, [None, None]);
        assert_eq!(matching(&slots, OLD), None);
        assert_eq!(spare(&slots, Some(OLD)), 0);
        assert_eq!(spare(&slots, None), 0);
    }

    #[test]
    fn round_trip_keeps_tag_fields_and_split_payload() {
        let mut file = file();
        let written = file.write(1, Some(OLD), 42, 3, &[b"abc", b"defg"]).unwrap();
        let slots = file.slots().unwrap();
        assert_eq!(slots, [None, Some(written)]);
        assert_eq!((written.aux, written.count, written.len), (42, 3, 7));
        let mut out = [0; 7];
        file.read(1, &written, &mut out).unwrap();
        assert_eq!(&out, b"abcdefg");
        let (mut first, mut second) = ([0; 3], [0; 4]);
        file.read_split(1, &written, &mut first, &mut second)
            .unwrap();
        assert_eq!((&first, &second), (b"abc", b"defg"));
        assert_eq!(
            file.read(1, &written, &mut [0; 6]),
            Err(SidecarError::WrongLength)
        );
        assert_eq!(
            file.write(0, None, 0, 0, &[&[0; 11]]),
            Err(SidecarError::PayloadTooLarge)
        );
        // An untagged slot is valid but matches no snapshot.
        let untagged = file.write(0, None, 9, 1, &[b"x"]).unwrap();
        assert_eq!(untagged.tag, None);
        assert_eq!(matching(&file.slots().unwrap(), OLD), Some(1));
    }

    #[test]
    fn the_current_slot_is_never_the_spare_one() {
        let mut file = file();
        file.write(0, Some(OLD), 0, 0, &[b"old"]).unwrap();
        for _ in 0..4 {
            // Repeated attempts for the next snapshot reuse the other slot.
            let slots = file.slots().unwrap();
            assert_eq!(spare(&slots, Some(OLD)), 1);
            file.write(1, Some(NEW), 0, 0, &[b"new"]).unwrap();
        }
        let slots = file.slots().unwrap();
        assert_eq!(
            (matching(&slots, OLD), matching(&slots, NEW)),
            (Some(0), Some(1))
        );
        // After the Bank committed NEW, the old slot becomes the spare.
        assert_eq!(spare(&slots, Some(NEW)), 0);
    }

    #[test]
    fn damage_voids_a_slot_or_fails_its_read_but_never_the_other() {
        let mut file = file();
        let kept = file.write(0, Some(OLD), 0, 0, &[b"keep"]).unwrap();
        let other = file.write(1, Some(NEW), 0, 0, &[b"lose"]).unwrap();
        let slot = KIND.slot_len() as usize;
        for at in 0..HEADER_SIZE {
            let mut damaged = Sidecar::new(
                Memory {
                    bytes: file.storage.bytes.clone(),
                    budget: None,
                },
                KIND,
            );
            damaged.storage.bytes[slot + at] ^= 1;
            assert_eq!(damaged.slots().unwrap(), [Some(kept), None], "byte {at}");
        }
        file.storage.bytes[slot + HEADER_SIZE] ^= 1;
        assert_eq!(file.slots().unwrap(), [Some(kept), Some(other)]);
        assert_eq!(
            file.read(1, &other, &mut [0; 4]),
            Err(SidecarError::Checksum)
        );
        let mut out = [0; 4];
        file.read(0, &kept, &mut out).unwrap();
        assert_eq!(&out, b"keep");
    }

    #[test]
    fn a_cut_at_any_byte_leaves_the_current_slot_and_no_false_match() {
        let mut base = file();
        let kept = base.write(0, Some(OLD), 0, 0, &[b"keep"]).unwrap();
        let total = HEADER_SIZE + 4 + HEADER_SIZE;
        for cut in 0..=total {
            let mut file = Sidecar::new(
                Memory {
                    bytes: base.storage.bytes.clone(),
                    budget: Some(cut),
                },
                KIND,
            );
            let result = file.write(1, Some(NEW), 0, 0, &[b"next"]);
            assert_eq!(result.is_ok(), cut == total, "cut {cut}");
            file.storage.budget = None;
            let slots = file.slots().unwrap();
            assert_eq!(slots[0], Some(kept), "cut {cut}");
            if let Some(index) = matching(&slots, NEW) {
                let mut out = [0; 4];
                file.read(index, &slots[index].unwrap(), &mut out).unwrap();
                assert_eq!(&out, b"next", "cut {cut}");
            }
        }
    }

    #[test]
    fn another_files_magic_is_not_accepted() {
        let mut file = file();
        file.write(0, Some(OLD), 0, 0, &[b"data"]).unwrap();
        let mut other = Sidecar::new(
            file.into_inner(),
            Kind {
                magic: *b"BKOFELSE",
                ..KIND
            },
        );
        assert_eq!(other.slots().unwrap(), [None, None]);
    }
}
