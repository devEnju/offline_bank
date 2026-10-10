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

use offline_core::{Fingerprint, GameIdentity, PendingTransfer, Sha256};

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
) -> Result<GameIdentity, Error> {
    if title_id == 0 {
        return Err(Error::InvalidTitle);
    }
    if before == after {
        return Err(Error::IdenticalImages);
    }
    let mut hash = Sha256::new();
    // This fixed-length input cannot overflow SHA-256's length field.
    hash.update(b"Bank offline game transaction v2\0").unwrap();
    hash.update(&title_id.to_le_bytes()).unwrap();
    hash.update(&before).unwrap();
    hash.update(&after).unwrap();
    hash.update(&[medium as u8]).unwrap();
    Ok(GameIdentity {
        title_id,
        save_identity: hash.finalize(),
    })
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
    let bound = |medium| {
        identity(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            medium,
        )
    };
    let place = if pending.game == bound(found)? {
        Found::SameKind
    } else if pending.game == bound(found.other())? {
        Found::StandIn
    } else {
        return Err(Error::InvalidBinding);
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
    use super::*;
    use offline_core::{Slot, SnapshotHeader, SnapshotRef};
    const TITLE: u64 = 0x0004000000055d00;
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
            game: identity(TITLE, [1; 32], [2; 32], medium).unwrap(),
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
        let mut damaged = pending(Medium::Cartridge);
        damaged.game.save_identity[0] ^= 1;
        // The binding earlier builds wrote, without a kind of copy: a save
        // they left in progress is theirs to finish.
        let mut earlier = pending(Medium::Cartridge);
        let mut hash = Sha256::new();
        hash.update(b"Bank offline game transaction v1\0").unwrap();
        hash.update(&TITLE.to_le_bytes()).unwrap();
        hash.update(&[1; 32]).unwrap();
        hash.update(&[2; 32]).unwrap();
        earlier.game.save_identity = hash.finalize();
        let mut other = [2; 32];
        other[31] ^= 1;
        for pending in [damaged, earlier] {
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
        let first = identity(1, [1; 32], [2; 32], cartridge).unwrap();
        assert_ne!(first, identity(2, [1; 32], [2; 32], cartridge).unwrap());
        assert_ne!(first, identity(1, [2; 32], [1; 32], cartridge).unwrap());
        assert_ne!(first, identity(1, [1; 32], [3; 32], cartridge).unwrap());
        assert_ne!(
            first,
            identity(1, [1; 32], [2; 32], Medium::Installed).unwrap()
        );
        assert_eq!(
            identity(0, [1; 32], [2; 32], cartridge),
            Err(Error::InvalidTitle)
        );
        assert_eq!(
            identity(1, [1; 32], [1; 32], cartridge),
            Err(Error::IdenticalImages)
        );
    }
}
