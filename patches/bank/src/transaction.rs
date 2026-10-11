//! Identifies a pending game transaction by its two complete file images and
//! the kind of copy of the game it was made with.
//!
//! No unverified trainer-ID field is used. The binding is stable for one pending
//! transaction, not across a save's entire lifetime. Recovery must independently
//! read the selected title and complete file hash before using the binding.
//! Platform secure-value validation is a separate required step.
//!
//! Bank holds one copy of each title: the cartridge if one is inserted,
//! otherwise the installed copy. When the copy a transaction was made with is
//! gone, the other kind can stand in for it under the same title, with a save
//! that has nothing to do with the transaction. The binding covers the kind
//! of copy, so that recovery can tell such a stand-in from the copy it is
//! waiting for. Nothing else is stored for this.
//!
//! A save of the same kind of copy that is neither image was written since:
//! by the game played on, by a new game started on it, or it is another
//! cartridge. These are not told apart; the worker settles each by what the
//! transaction moved.
//!
//! The 32 bytes of the binding also say whose game it is, for the one case in
//! which a transaction has to wait for its game and Bank names it with the
//! original's message: the trainer's name (24 bytes, 12 characters), the
//! trainer ID as the original shows it (4 bytes), and the first 4 bytes of a
//! hash over the title, both images, the kind of copy, the name and the ID.
//! Name and ID are for that message only; nothing is matched by them.

use offline_core::{Fingerprint, GameIdentity, PendingTransfer, Sha256};

/// The characters of a trainer's name. The games allow no longer one; the
/// original keeps it in 13 units, the last being its end mark.
pub const NAME: usize = 12;
const NAME_BYTES: usize = NAME * 2;
const ID_BYTES: usize = 4;
/// Where the hash begins, and how much of it there is.
const HASH_AT: usize = NAME_BYTES + ID_BYTES;

/// Whose game a transaction was made with, as the original's message for
/// another copy of a game says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trainer {
    /// Unused places are zero.
    pub name: [u16; NAME],
    /// The trainer ID as the original shows it.
    pub id: u32,
}
impl Trainer {
    /// `name` and `id` are what the original reads from the game: the name
    /// with its end mark and the game's whole 32-bit ID. The original
    /// (001e20d0) shows the low 16 bits of it for X, Y, Omega Ruby and Alpha
    /// Sapphire, and its last six decimal digits for the later games.
    pub fn of_game(name: &[u16; NAME + 1], id: u32, gen7: bool) -> Self {
        let mut out = [0; NAME];
        for (unit, &read) in out.iter_mut().zip(name) {
            if read == 0 {
                break;
            }
            *unit = read;
        }
        Self {
            name: out,
            id: if gen7 { id % 1_000_000 } else { id & 0xffff },
        }
    }
    /// The digits the original shows the ID with.
    pub const fn digits(gen7: bool) -> u32 {
        if gen7 {
            6
        } else {
            5
        }
    }
    /// The trainer a binding names. Only a binding that `match_pending_image`
    /// accepted is one of this form.
    pub fn of(game: &GameIdentity) -> Self {
        let bytes = &game.save_identity;
        let mut name = [0; NAME];
        for (index, unit) in name.iter_mut().enumerate() {
            *unit = u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]);
        }
        let id = [
            bytes[NAME_BYTES],
            bytes[NAME_BYTES + 1],
            bytes[NAME_BYTES + 2],
            bytes[NAME_BYTES + 3],
        ];
        Self {
            name,
            id: u32::from_le_bytes(id),
        }
    }
}

/// The kind of copy of a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    Cartridge,
    Installed,
}
impl Medium {
    pub const fn of(cartridge: bool) -> Self {
        if cartridge {
            Self::Cartridge
        } else {
            Self::Installed
        }
    }
    #[cfg(test)]
    const fn other(self) -> Self {
        match self {
            Self::Cartridge => Self::Installed,
            Self::Installed => Self::Cartridge,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Before,
    After,
    /// Neither image.
    Other(Found),
}
/// What a save that is neither image was found on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// The kind of copy the transaction was made with.
    SameKind,
    /// The other kind: a stand-in for the copy of the transaction.
    StandIn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidTitle,
    IdenticalImages,
    InvalidBinding,
    DifferentTitle,
}

pub fn identity(
    title_id: u64,
    before: Fingerprint,
    after: Fingerprint,
    medium: Medium,
    trainer: Trainer,
) -> Result<GameIdentity, Error> {
    if title_id == 0 {
        return Err(Error::InvalidTitle);
    }
    if before == after {
        return Err(Error::IdenticalImages);
    }
    let mut out = [0; 32];
    for (index, unit) in trainer.name.into_iter().enumerate() {
        out[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    out[NAME_BYTES..HASH_AT].copy_from_slice(&trainer.id.to_le_bytes());
    let mut hash = Sha256::new();
    // This fixed-length input cannot overflow SHA-256's length field.
    hash.update(b"Bank offline game transaction v3\0").unwrap();
    hash.update(&title_id.to_le_bytes()).unwrap();
    hash.update(&before).unwrap();
    hash.update(&after).unwrap();
    hash.update(&[medium as u8]).unwrap();
    hash.update(&out[..HASH_AT]).unwrap();
    let hash = hash.finalize();
    out[HASH_AT..].copy_from_slice(&hash[..32 - HASH_AT]);
    Ok(GameIdentity {
        title_id,
        save_identity: out,
    })
}

/// The kind of copy a transaction was made with, if its binding is one of
/// this form. A binding of another form, as other versions of the patch
/// wrote, does not come out of its own first 28 bytes again.
pub fn bound(pending: &PendingTransfer) -> Result<Medium, Error> {
    for medium in [Medium::Cartridge, Medium::Installed] {
        let made = identity(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            medium,
            Trainer::of(&pending.game),
        )?;
        if pending.game == made {
            return Ok(medium);
        }
    }
    Err(Error::InvalidBinding)
}

/// Classifies actual file bytes; it neither repairs secure values nor commits
/// the Bank. An exact image match alone is insufficient for final reconciliation.
/// `found` is the kind of copy the file was read from. An exact image is
/// accepted from either kind: it is that save, wherever it now lies.
pub fn match_pending_image(
    pending: &PendingTransfer,
    selected_title: u64,
    actual: Fingerprint,
    found: Medium,
) -> Result<Match, Error> {
    let place = if bound(pending)? == found {
        Found::SameKind
    } else {
        Found::StandIn
    };
    if selected_title != pending.game.title_id {
        return Err(Error::DifferentTitle);
    }
    if actual == pending.before_fingerprint {
        Ok(Match::Before)
    } else if actual == pending.after_fingerprint {
        Ok(Match::After)
    } else {
        Ok(Match::Other(place))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use offline_core::{Slot, SnapshotHeader, SnapshotRef};
    const TITLE: u64 = 0x0004000000055d00;
    fn named(name: &str, id: u32) -> Trainer {
        let mut units = [0; NAME + 1];
        for (unit, read) in units.iter_mut().zip(name.encode_utf16()) {
            *unit = read;
        }
        Trainer::of_game(&units, id, false)
    }
    fn trainer() -> Trainer {
        named("Serena", 12345)
    }
    /// A transaction made with `medium`.
    fn pending(medium: Medium) -> PendingTransfer {
        let before = SnapshotRef {
            slot: Slot::A,
            header: SnapshotHeader {
                generation: 1,
                payload_len: 1,
                payload_crc32: 1,
            },
        };
        let after = SnapshotRef {
            slot: Slot::B,
            header: SnapshotHeader {
                generation: 2,
                payload_len: 1,
                payload_crc32: 2,
            },
        };
        PendingTransfer {
            before,
            after,
            game: identity(TITLE, [1; 32], [2; 32], medium, trainer()).unwrap(),
            before_fingerprint: [1; 32],
            after_fingerprint: [2; 32],
        }
    }
    const KINDS: [Medium; 2] = [Medium::Cartridge, Medium::Installed];
    #[test]
    fn full_image_and_title_are_required_before_adopting_journal_identity() {
        for made in KINDS {
            let pending = pending(made);
            // An exact image is that save on either kind of copy.
            for found in KINDS {
                assert_eq!(
                    match_pending_image(&pending, TITLE, [1; 32], found),
                    Ok(Match::Before)
                );
                assert_eq!(
                    match_pending_image(&pending, TITLE, [2; 32], found),
                    Ok(Match::After)
                );
                assert_eq!(
                    match_pending_image(&pending, TITLE + 0x100, [2; 32], found),
                    Err(Error::DifferentTitle)
                );
            }
        }
    }
    #[test]
    fn a_save_that_is_neither_image_is_told_by_the_kind_of_copy() {
        let mut other = [2; 32];
        other[31] ^= 1;
        for made in KINDS {
            let pending = pending(made);
            assert_eq!(
                match_pending_image(&pending, TITLE, other, made),
                Ok(Match::Other(Found::SameKind))
            );
            // The copy of the transaction is gone and the other kind of
            // copy of the title was found in its place.
            assert_eq!(
                match_pending_image(&pending, TITLE, other, made.other()),
                Ok(Match::Other(Found::StandIn))
            );
        }
    }
    #[test]
    fn a_binding_that_fits_neither_kind_is_refused() {
        // Every part of the binding is covered: the name, the ID, the hash.
        let damaged = [0, NAME_BYTES, HASH_AT, 31].map(|at| {
            let mut damaged = pending(Medium::Cartridge);
            damaged.game.save_identity[at] ^= 1;
            damaged
        });
        // The bindings other versions of the patch wrote, a whole hash
        // without a kind of copy and then with one: a save they left in
        // progress is theirs to finish.
        let earlier = [
            &b"Bank offline game transaction v1\0"[..],
            b"Bank offline game transaction v2\0",
        ]
        .map(|version| {
            let mut earlier = pending(Medium::Cartridge);
            let mut hash = Sha256::new();
            hash.update(version).unwrap();
            hash.update(&TITLE.to_le_bytes()).unwrap();
            hash.update(&[1; 32]).unwrap();
            hash.update(&[2; 32]).unwrap();
            if version[31] == b'2' {
                hash.update(&[Medium::Cartridge as u8]).unwrap();
            }
            earlier.game.save_identity = hash.finalize();
            earlier
        });
        let mut other = [2; 32];
        other[31] ^= 1;
        for made in KINDS {
            assert_eq!(bound(&pending(made)), Ok(made));
        }
        for pending in damaged.into_iter().chain(earlier) {
            // Also when no game is there to show the kind of copy.
            assert_eq!(bound(&pending), Err(Error::InvalidBinding));
            for found in KINDS {
                for actual in [[1; 32], [2; 32], other] {
                    assert_eq!(
                        match_pending_image(&pending, TITLE, actual, found),
                        Err(Error::InvalidBinding)
                    );
                }
            }
        }
    }
    #[test]
    fn binding_distinguishes_title_direction_each_file_image_and_the_kind_of_copy() {
        let cartridge = Medium::Cartridge;
        let bound = |title, before, after, medium| {
            identity(title, [before; 32], [after; 32], medium, trainer())
        };
        let first = bound(1, 1, 2, cartridge).unwrap();
        // Only the hash can differ here: the name and the ID are the same.
        let hash = |game: GameIdentity| game.save_identity[HASH_AT..].to_vec();
        for other in [
            bound(2, 1, 2, cartridge),
            bound(1, 2, 1, cartridge),
            bound(1, 1, 3, cartridge),
            bound(1, 1, 2, Medium::Installed),
        ] {
            let other = other.unwrap();
            assert_eq!(
                other.save_identity[..HASH_AT],
                first.save_identity[..HASH_AT]
            );
            assert_ne!(hash(other), hash(first));
        }
        // Another trainer is another binding, in its hash as well.
        for other in [named("Calem", 12345), named("Serena", 12346)] {
            let other = identity(1, [1; 32], [2; 32], cartridge, other).unwrap();
            assert_ne!(hash(other), hash(first));
        }
        assert_eq!(bound(0, 1, 2, cartridge), Err(Error::InvalidTitle));
        assert_eq!(bound(1, 1, 1, cartridge), Err(Error::IdenticalImages));
    }
    #[test]
    fn the_binding_names_the_trainer_of_its_game() {
        // The longest name the games allow, a short one, and none.
        for name in ["ABCDEFGHIJKL", "Serena", "ポケモン", ""] {
            let trainer = named(name, 54321);
            for medium in KINDS {
                let game = identity(TITLE, [1; 32], [2; 32], medium, trainer).unwrap();
                let read = Trainer::of(&game);
                assert_eq!(read, trainer);
                let end = read.name.iter().position(|&unit| unit == 0);
                let units = &read.name[..end.unwrap_or(NAME)];
                assert_eq!(std::string::String::from_utf16(units).unwrap(), name);
            }
        }
    }
    #[test]
    fn the_id_is_the_one_the_original_shows() {
        let mut name = [0x41; NAME + 1];
        name[NAME] = 0;
        // X, Y, Omega Ruby, Alpha Sapphire: the low half, five digits.
        let gen6 = Trainer::of_game(&name, 0xabcd_3039, false);
        assert_eq!((gen6.id, Trainer::digits(false)), (12345, 5));
        // The later games: the last six decimal digits of the whole ID.
        let gen7 = Trainer::of_game(&name, 0xabcd_3039, true);
        assert_eq!(0xabcd_3039u32, 2_882_351_161);
        assert_eq!((gen7.id, Trainer::digits(true)), (351_161, 6));
        assert_eq!(Trainer::of_game(&name, 999_999, true).id, 999_999);
        assert_eq!(Trainer::of_game(&name, 1_000_000, true).id, 0);
        // A name ends at its end mark; what follows it is not part of it.
        let mut short = [0; NAME + 1];
        short[..3].copy_from_slice(&[0x41, 0, 0x42]);
        assert_eq!(Trainer::of_game(&short, 1, false).name[..2], [0x41, 0]);
        assert_eq!(gen6.name, [0x41; NAME]);
    }
}
