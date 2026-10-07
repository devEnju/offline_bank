use crate::crc32;

pub const SCHEMA_VERSION: u16 = 1;
pub const SNAPSHOT_HEADER_SIZE: usize = 32;
pub const METADATA_SIZE: usize = 192;
const SNAPSHOT_MAGIC: &[u8; 8] = b"BKOFSNAP";
const METADATA_MAGIC: &[u8; 8] = b"BKOFMETA";

pub type Fingerprint = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u16),
    BadHeaderSize,
    BadChecksum,
    NonzeroReserved,
    InvalidState,
    InvalidReference,
    InvalidGeneration,
    InvalidSequence,
    InvalidGameIdentity,
    IndistinguishableGameStates,
    PayloadTooLarge,
    CapacityMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
    pub(crate) fn index(self) -> u64 {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
    fn byte(self) -> u8 {
        self.index() as u8
    }
    fn parse(byte: u8) -> Result<Self, FormatError> {
        match byte {
            0 => Ok(Self::A),
            1 => Ok(Self::B),
            _ => Err(FormatError::InvalidReference),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub generation: u64,
    pub payload_len: u32,
    pub payload_crc32: u32,
}

impl SnapshotHeader {
    pub fn encode(self) -> Result<[u8; SNAPSHOT_HEADER_SIZE], FormatError> {
        if self.generation == 0 {
            return Err(FormatError::InvalidGeneration);
        }
        let mut out = [0; SNAPSHOT_HEADER_SIZE];
        out[..8].copy_from_slice(SNAPSHOT_MAGIC);
        put16(&mut out, 8, SCHEMA_VERSION);
        put16(&mut out, 10, SNAPSHOT_HEADER_SIZE as u16);
        put32(&mut out, 12, self.payload_len);
        put64(&mut out, 16, self.generation);
        put32(&mut out, 24, self.payload_crc32);
        let checksum = crc32(&out[..28]);
        put32(&mut out, 28, checksum);
        Ok(out)
    }

    pub fn decode(bytes: &[u8], capacity: u32) -> Result<Self, FormatError> {
        if bytes.len() != SNAPSHOT_HEADER_SIZE {
            return Err(FormatError::Truncated);
        }
        validate_prefix(bytes, SNAPSHOT_MAGIC, SNAPSHOT_HEADER_SIZE)?;
        let header = Self {
            generation: get64(bytes, 16),
            payload_len: get32(bytes, 12),
            payload_crc32: get32(bytes, 24),
        };
        if header.generation == 0 {
            return Err(FormatError::InvalidGeneration);
        }
        if header.payload_len > capacity {
            return Err(FormatError::PayloadTooLarge);
        }
        Ok(header)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotRef {
    pub slot: Slot,
    pub header: SnapshotHeader,
}

impl SnapshotRef {
    fn encode(self, out: &mut [u8]) -> Result<(), FormatError> {
        if self.header.generation == 0 {
            return Err(FormatError::InvalidGeneration);
        }
        out.fill(0);
        out[0] = self.slot.byte();
        put32(out, 4, self.header.payload_len);
        put64(out, 8, self.header.generation);
        put32(out, 16, self.header.payload_crc32);
        Ok(())
    }

    fn decode(bytes: &[u8], capacity: u32) -> Result<Self, FormatError> {
        if !zero(&bytes[1..4]) || !zero(&bytes[20..24]) {
            return Err(FormatError::NonzeroReserved);
        }
        let header = SnapshotHeader {
            payload_len: get32(bytes, 4),
            generation: get64(bytes, 8),
            payload_crc32: get32(bytes, 16),
        };
        if header.generation == 0 {
            return Err(FormatError::InvalidGeneration);
        }
        if header.payload_len > capacity {
            return Err(FormatError::PayloadTooLarge);
        }
        Ok(Self {
            slot: Slot::parse(bytes[0])?,
            header,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameIdentity {
    pub title_id: u64,
    /// Identity of this particular save transaction, not merely its game title.
    /// The runtime may bind the complete before/after images, provided recovery
    /// independently verifies the selected title and an exact image match.
    pub save_identity: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTransfer {
    pub before: SnapshotRef,
    pub after: SnapshotRef,
    pub game: GameIdentity,
    pub before_fingerprint: Fingerprint,
    pub after_fingerprint: Fingerprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Clean(SnapshotRef),
    Prepared(PendingTransfer),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub sequence: u64,
    pub capacity: u32,
    pub phase: Phase,
}

impl Metadata {
    pub fn encode(self) -> Result<[u8; METADATA_SIZE], FormatError> {
        if self.sequence == 0 {
            return Err(FormatError::InvalidSequence);
        }
        let too_large = match self.phase {
            Phase::Clean(current) => current.header.payload_len > self.capacity,
            Phase::Prepared(pending) => {
                pending.before.header.payload_len > self.capacity
                    || pending.after.header.payload_len > self.capacity
            }
        };
        if too_large {
            return Err(FormatError::PayloadTooLarge);
        }
        let mut out = [0; METADATA_SIZE];
        out[..8].copy_from_slice(METADATA_MAGIC);
        put16(&mut out, 8, SCHEMA_VERSION);
        put16(&mut out, 10, METADATA_SIZE as u16);
        put64(&mut out, 16, self.sequence);
        put32(&mut out, 184, self.capacity);
        match self.phase {
            Phase::Clean(current) => {
                out[12] = 1;
                current.encode(&mut out[24..48])?;
            }
            Phase::Prepared(pending) => {
                validate_pending(pending)?;
                out[12] = 2;
                pending.before.encode(&mut out[24..48])?;
                pending.after.encode(&mut out[48..72])?;
                put64(&mut out, 72, pending.game.title_id);
                out[80..112].copy_from_slice(&pending.game.save_identity);
                out[112..144].copy_from_slice(&pending.before_fingerprint);
                out[144..176].copy_from_slice(&pending.after_fingerprint);
            }
        }
        let checksum = crc32(&out[..188]);
        put32(&mut out, 188, checksum);
        Ok(out)
    }

    pub fn decode(bytes: &[u8], capacity: u32) -> Result<Self, FormatError> {
        if bytes.len() != METADATA_SIZE {
            return Err(FormatError::Truncated);
        }
        validate_prefix(bytes, METADATA_MAGIC, METADATA_SIZE)?;
        if !zero(&bytes[13..16]) || !zero(&bytes[176..184]) {
            return Err(FormatError::NonzeroReserved);
        }
        if get32(bytes, 184) != capacity {
            return Err(FormatError::CapacityMismatch);
        }
        let sequence = get64(bytes, 16);
        if sequence == 0 {
            return Err(FormatError::InvalidSequence);
        }
        let current = SnapshotRef::decode(&bytes[24..48], capacity)?;
        let phase = match bytes[12] {
            1 => {
                if !zero(&bytes[48..184]) {
                    return Err(FormatError::NonzeroReserved);
                }
                Phase::Clean(current)
            }
            2 => {
                let pending = PendingTransfer {
                    before: current,
                    after: SnapshotRef::decode(&bytes[48..72], capacity)?,
                    game: GameIdentity {
                        title_id: get64(bytes, 72),
                        save_identity: array32(&bytes[80..112]),
                    },
                    before_fingerprint: array32(&bytes[112..144]),
                    after_fingerprint: array32(&bytes[144..176]),
                };
                validate_pending(pending)?;
                Phase::Prepared(pending)
            }
            _ => return Err(FormatError::InvalidState),
        };
        Ok(Self {
            sequence,
            capacity,
            phase,
        })
    }
}

fn validate_pending(p: PendingTransfer) -> Result<(), FormatError> {
    if p.before.slot == p.after.slot
        || p.before.header.generation.checked_add(1) != Some(p.after.header.generation)
    {
        return Err(FormatError::InvalidReference);
    }
    if p.game.title_id == 0 {
        return Err(FormatError::InvalidGameIdentity);
    }
    if p.before_fingerprint == p.after_fingerprint {
        return Err(FormatError::IndistinguishableGameStates);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameObservation {
    Missing,
    Present {
        game: GameIdentity,
        fingerprint: Fingerprint,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryBlock {
    MissingGame,
    DifferentGame,
    ModifiedGame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryDecision {
    KeepBefore,
    CommitAfter,
    Blocked(RecoveryBlock),
}

/// Decides from evidence only; never edits storage or the game.
pub fn decide_recovery(pending: &PendingTransfer, observed: GameObservation) -> RecoveryDecision {
    match observed {
        GameObservation::Missing => RecoveryDecision::Blocked(RecoveryBlock::MissingGame),
        GameObservation::Present { game, .. } if game != pending.game => {
            RecoveryDecision::Blocked(RecoveryBlock::DifferentGame)
        }
        GameObservation::Present { fingerprint, .. }
            if fingerprint == pending.before_fingerprint =>
        {
            RecoveryDecision::KeepBefore
        }
        GameObservation::Present { fingerprint, .. }
            if fingerprint == pending.after_fingerprint =>
        {
            RecoveryDecision::CommitAfter
        }
        GameObservation::Present { .. } => RecoveryDecision::Blocked(RecoveryBlock::ModifiedGame),
    }
}

fn validate_prefix(bytes: &[u8], magic: &[u8; 8], size: usize) -> Result<(), FormatError> {
    // Check integrity before trusting a version possibly damaged by a torn write.
    if crc32(&bytes[..size - 4]) != get32(bytes, size - 4) {
        return Err(FormatError::BadChecksum);
    }
    if &bytes[..8] != magic {
        return Err(FormatError::BadMagic);
    }
    let version = get16(bytes, 8);
    if version != SCHEMA_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if get16(bytes, 10) as usize != size {
        return Err(FormatError::BadHeaderSize);
    }
    Ok(())
}
pub(crate) fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}
fn array32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(bytes);
    out
}
fn get16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn get64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}
fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
