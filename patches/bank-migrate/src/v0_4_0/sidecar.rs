//! Frozen: v0.4.0's `sidecar.rs`. Only the migration reads it.
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

use super::{crc32, format::zero, SnapshotRef, Storage};

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
    /// A slot that cannot be read is an error: it may be the one in use.
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

    /// Like `slots`, for the owner of the file, who knows which snapshot is
    /// current. An interrupted write leaves the slot it went to unreadable,
    /// and that is never the slot in use: a slot that cannot be read is
    /// `None` when the other one is seen to belong to `current`. Otherwise it
    /// may be the one in use, and the error stands.
    pub fn slots_beside(
        &mut self,
        current: Tag,
    ) -> Result<[Option<Slot>; SLOTS], SidecarError<S::Error>> {
        let mut out = [None; SLOTS];
        let mut failed = None;
        for (index, slot) in out.iter_mut().enumerate() {
            let mut bytes = [0; HEADER_SIZE];
            match self.storage.read(self.offset(index), &mut bytes) {
                Ok(()) => *slot = self.decode(&bytes),
                Err(error) => failed = Some(error),
            }
        }
        match failed {
            Some(error) if matching(&out, current).is_none() => Err(SidecarError::Io(error)),
            _ => Ok(out),
        }
    }

    /// Like `slots` where nothing in the file is needed as it is: every
    /// slot that cannot be read is `None`.
    pub fn slots_or_void(&mut self) -> [Option<Slot>; SLOTS] {
        let mut out = [None; SLOTS];
        for (index, slot) in out.iter_mut().enumerate() {
            let mut bytes = [0; HEADER_SIZE];
            if self.storage.read(self.offset(index), &mut bytes).is_ok() {
                *slot = self.decode(&bytes);
            }
        }
        out
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
        let mut crc = super::checksum::Crc32::new();
        crc.update(first);
        crc.update(second);
        if crc.finish() != slot.crc {
            return Err(SidecarError::Checksum);
        }
        Ok(())
    }

    /// Voids the header, writes the payload parts, then the header, each made
    /// durable in turn, and reads the header back. A cut leaves a void slot.
    /// A slot that an earlier cut left unusable is discarded and written once
    /// more; it is never the slot in use.
    pub fn write(
        &mut self,
        index: usize,
        tag: Option<Tag>,
        aux: u32,
        count: u32,
        parts: &[&[u8]],
    ) -> Result<Slot, SidecarError<S::Error>> {
        match self.put(index, tag, aux, count, parts) {
            Err(SidecarError::Io(_) | SidecarError::Readback) => {
                self.discard(index)?;
                self.put(index, tag, aux, count, parts)
            }
            result => result,
        }
    }

    fn discard(&mut self, index: usize) -> Result<(), SidecarError<S::Error>> {
        self.storage
            .recreate(self.offset(index), self.kind.slot_len())
            .map_err(SidecarError::Io)
    }

    fn put(
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
        let mut payload_crc = super::checksum::Crc32::new();
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
        self.void(index)?;
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

    /// Makes a slot void. Repeating it is harmless. A slot that cannot be
    /// written is discarded, which leaves it void as well.
    pub fn clear(&mut self, index: usize) -> Result<(), SidecarError<S::Error>> {
        match self.void(index) {
            Err(_) => self.discard(index),
            result => result,
        }
    }

    fn void(&mut self, index: usize) -> Result<(), SidecarError<S::Error>> {
        self.storage
            .write(self.offset(index), &[0; HEADER_SIZE])
            .map_err(SidecarError::Io)?;
        self.storage.sync().map_err(SidecarError::Io)
    }
}
