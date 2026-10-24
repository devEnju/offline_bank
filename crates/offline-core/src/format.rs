use crate::crc32;

/// The journal version, in byte 3 of every record: the major number of this
/// crate's version in its `Cargo.toml`, the version of the stored files.
pub const SCHEMA_VERSION: u8 = major(env!("CARGO_PKG_VERSION_MAJOR"));
const fn major(digits: &str) -> u8 {
    let digits = digits.as_bytes();
    assert!(!digits.is_empty());
    let mut value: u32 = 0;
    let mut at = 0;
    while at < digits.len() {
        assert!(digits[at].is_ascii_digit());
        value = value * 10 + (digits[at] - b'0') as u32;
        assert!(value <= u8::MAX as u32);
        at += 1;
    }
    value as u8
}
pub const METADATA_SIZE: usize = 140;
const METADATA_MAGIC: &[u8; 3] = b"BKJ";
/// Most Pokémon the transport box holds.
const MOST_HELD: u8 = 30;
/// The games a save can be with, in the order of their number in the record
/// (1 to 8): X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, Ultra Moon.
pub const GAME_TITLES: [u64; 8] = [
    0x0004_0000_0005_5d00,
    0x0004_0000_0005_5e00,
    0x0004_0000_0011_c400,
    0x0004_0000_0011_c500,
    0x0004_0000_0016_4800,
    0x0004_0000_0017_5e00,
    0x0004_0000_001b_5000,
    0x0004_0000_001b_5100,
];

pub type Fingerprint = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u8),
    BadChecksum,
    NonzeroReserved,
    InvalidReference,
    InvalidSequence,
    InvalidGameIdentity,
    IndistinguishableGameStates,
    /// A Bank payload is exactly as long as its file.
    PayloadSize,
}

/// One of the two copies every file exists in.
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
    pub fn index(self) -> u64 {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
    fn of(bit: u8) -> Self {
        if bit & 1 == 0 {
            Self::A
        } else {
            Self::B
        }
    }
}

/// One file of a save: which of its two copies, and that copy's checksum. A
/// copy belongs to the save only if its bytes have this checksum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub slot: Slot,
    pub crc: u32,
}

/// A save: the copy of each of the three files that belongs to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Save {
    pub bank: Part,
    pub main: Part,
    pub mover: Part,
    /// Pokémon in the transport box.
    pub held: u8,
}

impl Save {
    /// Where the next save writes the transport box. Normally the copy that
    /// is not in use. While a delivery waits, that copy holds the delivery,
    /// which must outlast the save until it is current; the box in use is
    /// empty then, and the new box goes over it.
    pub fn next_mover(&self, delivery: bool) -> Slot {
        if delivery {
            self.mover.slot
        } else {
            self.mover.slot.other()
        }
    }
}

/// What a save has written beside the Bank file: the checksums of its main
/// file and its transport box, and the Pokémon in that box. Their copies
/// follow from the save before (`Save::next_mover`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    pub main: u32,
    pub mover: u32,
    pub held: u8,
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
    pub before: Save,
    pub after: Save,
    pub game: GameIdentity,
    pub before_fingerprint: Fingerprint,
    pub after_fingerprint: Fingerprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Clean(Save),
    Prepared(PendingTransfer),
}

impl Phase {
    /// The save in force: the one before, while another is in progress.
    pub fn current(&self) -> Save {
        match self {
            Self::Clean(current) => *current,
            Self::Prepared(pending) => pending.before,
        }
    }
}

/// The journal record. Layout, little endian:
///
/// | Bytes | Field |
/// | --- | --- |
/// | 0..4 | `BKJ` and the version |
/// | 4..8 | sequence |
/// | 8 | bit 0 a save is in progress, bit 1 a delivery waits, bit 2 Transporter's question settles the save in progress, bits 4..8 its game (1 to 8) |
/// | 9 | copies: bits 0..3 the current save's bank, main, mover; bits 3..6 the same for the save in progress |
/// | 10, 11 | Pokémon in the transport box: current save, save in progress |
/// | 12..24 | checksums of bank, main, mover: current save |
/// | 24..36 | the same: save in progress |
/// | 36..40 | checksum of the waiting delivery |
/// | 40..72 | identity of the save in progress (trainer name, ID, hash) |
/// | 72..104 | fingerprint before |
/// | 104..136 | fingerprint after |
/// | 136..140 | CRC-32 of bytes 0..136 |
///
/// Without a save in progress everything that belongs to one is zero, and so
/// is the delivery's checksum while none waits. Unused bits are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub sequence: u32,
    pub phase: Phase,
    /// Checksum of a delivery from Transporter that waits in the transport
    /// file's copy that is not in use.
    pub delivery: Option<u32>,
    /// The save in progress is settled by Transporter's question, which
    /// asks the user: Bank found its game on the kind of copy it was made
    /// with, saved on since, and the save moved Pokémon both ways, so that
    /// the game no longer tells which snapshot loses none. Only with a save
    /// in progress.
    pub ask: bool,
}

impl Metadata {
    pub fn encode(self) -> Result<[u8; METADATA_SIZE], FormatError> {
        if self.sequence == 0 {
            return Err(FormatError::InvalidSequence);
        }
        validate(&self.phase, self.delivery, self.ask)?;
        let mut out = [0; METADATA_SIZE];
        out[..3].copy_from_slice(METADATA_MAGIC);
        out[3] = SCHEMA_VERSION;
        put32(&mut out, 4, self.sequence);
        let current = self.phase.current();
        out[9] = copies(&current);
        out[10] = current.held;
        put_crcs(&mut out, 12, &current);
        if let Some(crc) = self.delivery {
            out[8] |= 2;
            put32(&mut out, 36, crc);
        }
        if self.ask {
            out[8] |= 4;
        }
        if let Phase::Prepared(pending) = self.phase {
            out[8] |= 1 | game_number(pending.game.title_id)? << 4;
            out[9] |= copies(&pending.after) << 3;
            out[11] = pending.after.held;
            put_crcs(&mut out, 24, &pending.after);
            out[40..72].copy_from_slice(&pending.game.save_identity);
            out[72..104].copy_from_slice(&pending.before_fingerprint);
            out[104..136].copy_from_slice(&pending.after_fingerprint);
        }
        let checksum = crc32(&out[..136]);
        put32(&mut out, 136, checksum);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() != METADATA_SIZE {
            return Err(FormatError::Truncated);
        }
        // Check integrity before trusting a version possibly damaged by a torn write.
        if crc32(&bytes[..136]) != get32(bytes, 136) {
            return Err(FormatError::BadChecksum);
        }
        if &bytes[..3] != METADATA_MAGIC {
            return Err(FormatError::BadMagic);
        }
        if bytes[3] != SCHEMA_VERSION {
            return Err(FormatError::UnsupportedVersion(bytes[3]));
        }
        let sequence = get32(bytes, 4);
        if sequence == 0 {
            return Err(FormatError::InvalidSequence);
        }
        if bytes[8] & 0x08 != 0 || bytes[9] & 0xc0 != 0 {
            return Err(FormatError::NonzeroReserved);
        }
        let current = save(bytes, bytes[9], bytes[10], 12);
        let delivery = (bytes[8] & 2 != 0).then(|| get32(bytes, 36));
        let ask = bytes[8] & 4 != 0;
        if delivery.is_none() && !zero(&bytes[36..40]) {
            return Err(FormatError::NonzeroReserved);
        }
        let phase = if bytes[8] & 1 == 0 {
            if bytes[8] >> 4 != 0 || bytes[9] >> 3 != 0 || bytes[11] != 0 {
                return Err(FormatError::NonzeroReserved);
            }
            if !zero(&bytes[24..36]) || !zero(&bytes[40..136]) {
                return Err(FormatError::NonzeroReserved);
            }
            Phase::Clean(current)
        } else {
            let title_id = *GAME_TITLES
                .get(usize::from(bytes[8] >> 4).wrapping_sub(1))
                .ok_or(FormatError::InvalidGameIdentity)?;
            Phase::Prepared(PendingTransfer {
                before: current,
                after: save(bytes, bytes[9] >> 3, bytes[11], 24),
                game: GameIdentity {
                    title_id,
                    save_identity: array32(&bytes[40..72]),
                },
                before_fingerprint: array32(&bytes[72..104]),
                after_fingerprint: array32(&bytes[104..136]),
            })
        };
        validate(&phase, delivery, ask)?;
        Ok(Self {
            sequence,
            phase,
            delivery,
            ask,
        })
    }
}

/// The number of a game in the record, 1 to 8.
fn game_number(title_id: u64) -> Result<u8, FormatError> {
    GAME_TITLES
        .iter()
        .position(|title| *title == title_id)
        .map(|index| index as u8 + 1)
        .ok_or(FormatError::InvalidGameIdentity)
}

fn copies(save: &Save) -> u8 {
    save.bank.slot.index() as u8
        | (save.main.slot.index() as u8) << 1
        | (save.mover.slot.index() as u8) << 2
}
fn put_crcs(out: &mut [u8], at: usize, save: &Save) {
    put32(out, at, save.bank.crc);
    put32(out, at + 4, save.main.crc);
    put32(out, at + 8, save.mover.crc);
}
fn save(bytes: &[u8], copies: u8, held: u8, at: usize) -> Save {
    let part = |bit: u8, at: usize| Part {
        slot: Slot::of(copies >> bit),
        crc: get32(bytes, at),
    };
    Save {
        bank: part(0, at),
        main: part(1, at + 4),
        mover: part(2, at + 8),
        held,
    }
}

/// What must hold between the saves of a record, whoever wrote it.
fn validate(phase: &Phase, delivery: Option<u32>, ask: bool) -> Result<(), FormatError> {
    let current = phase.current();
    // Only a save in progress is settled by Transporter's question.
    if ask && !matches!(phase, Phase::Prepared(_)) {
        return Err(FormatError::InvalidReference);
    }
    if current.held > MOST_HELD {
        return Err(FormatError::InvalidReference);
    }
    // Transporter delivers only into an empty box.
    if delivery.is_some() && current.held != 0 {
        return Err(FormatError::InvalidReference);
    }
    let Phase::Prepared(pending) = phase else {
        return Ok(());
    };
    let after = pending.after;
    // A save never writes a copy that is in use; only an empty transport
    // box is written over, to keep a delivery beside it.
    if after.held > MOST_HELD
        || after.bank.slot == current.bank.slot
        || after.main.slot == current.main.slot
        || after.mover.slot != current.next_mover(delivery.is_some())
    {
        return Err(FormatError::InvalidReference);
    }
    game_number(pending.game.title_id)?;
    if pending.before_fingerprint == pending.after_fingerprint {
        return Err(FormatError::IndistinguishableGameStates);
    }
    Ok(())
}

/// Which save of a save in progress becomes current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryDecision {
    KeepBefore,
    CommitAfter,
}

/// The save whose image the game's save is, by its fingerprint; `None` for
/// neither image. Never edits storage or the game.
pub fn decide_recovery(
    pending: &PendingTransfer,
    fingerprint: Fingerprint,
) -> Option<RecoveryDecision> {
    if fingerprint == pending.before_fingerprint {
        Some(RecoveryDecision::KeepBefore)
    } else if fingerprint == pending.after_fingerprint {
        Some(RecoveryDecision::CommitAfter)
    } else {
        None
    }
}

pub(crate) fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}
fn array32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(bytes);
    out
}
fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
