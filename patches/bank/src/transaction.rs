//! Identifies a pending game transaction by its two complete file images, and
//! the copy of the game by its trainer.
//!
//! The binding is stable for one pending transaction, not across a save's
//! entire lifetime. Recovery must independently read the selected title and
//! complete file hash before using the binding. Platform secure-value
//! validation is a separate required step.
//!
//! A save that is neither image was changed since: by the game, if it is
//! still the same copy, or it is another copy of the title. The binding
//! covers the trainer of the save so that the two can be told apart without
//! storing anything more; a binding made without a trainer (by an earlier
//! build, or for a save whose trainer could not be read) cannot tell.

use offline_core::{game_save::Trainer, Fingerprint, GameIdentity, PendingTransfer, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Before,
    After,
    /// Neither image.
    Other(Owner),
}
/// Whose save a save that is neither image is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The trainer the transaction was bound to: the same copy, saved since.
    Same,
    /// Another trainer, or none that can be read: not the copy to decide by.
    Different,
    /// The transaction was bound to no trainer.
    Unrecorded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidTitle,
    IdenticalImages,
    InvalidBinding,
    DifferentTitle,
}

/// The binding of a transaction. With a trainer it also covers the trainer's
/// ID, secret ID and name.
pub fn identity(
    title_id: u64,
    before: Fingerprint,
    after: Fingerprint,
    trainer: Option<&Trainer>,
) -> Result<GameIdentity, Error> {
    if title_id == 0 {
        return Err(Error::InvalidTitle);
    }
    if before == after {
        return Err(Error::IdenticalImages);
    }
    let mut hash = Sha256::new();
    // These fixed-length inputs cannot overflow SHA-256's length field.
    let version: &[u8] = match trainer {
        None => b"Bank offline game transaction v1\0",
        Some(_) => b"Bank offline game transaction v2\0",
    };
    hash.update(version).unwrap();
    hash.update(&title_id.to_le_bytes()).unwrap();
    hash.update(&before).unwrap();
    hash.update(&after).unwrap();
    if let Some(trainer) = trainer {
        hash.update(&trainer.id.to_le_bytes()).unwrap();
        hash.update(&trainer.secret_id.to_le_bytes()).unwrap();
        hash.update(&trainer.name).unwrap();
    }
    Ok(GameIdentity {
        title_id,
        save_identity: hash.finalize(),
    })
}

/// Classifies actual file bytes; it neither repairs secure values nor commits
/// the Bank. An exact image match alone is insufficient for final reconciliation.
/// `trainer` is the trainer read from the actual file, if one could be read.
pub fn match_pending_image(
    pending: &PendingTransfer,
    selected_title: u64,
    actual: Fingerprint,
    trainer: Option<&Trainer>,
) -> Result<Match, Error> {
    let binding = |trainer| {
        identity(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            trainer,
        )
    };
    let unrecorded = pending.game == binding(None)?;
    let same = match trainer {
        Some(trainer) => !unrecorded && pending.game == binding(Some(trainer))?,
        None => false,
    };
    if selected_title != pending.game.title_id {
        return Err(Error::DifferentTitle);
    }
    if actual == pending.before_fingerprint || actual == pending.after_fingerprint {
        // An image holds the trainer it was bound with; a binding that fits
        // neither form is not this transaction's.
        if !unrecorded && !same {
            return Err(Error::InvalidBinding);
        }
        return Ok(if actual == pending.before_fingerprint {
            Match::Before
        } else {
            Match::After
        });
    }
    Ok(Match::Other(if unrecorded {
        Owner::Unrecorded
    } else if same {
        Owner::Same
    } else {
        Owner::Different
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use offline_core::{Slot, SnapshotHeader, SnapshotRef};
    fn pending() -> PendingTransfer {
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
            game: identity(TITLE, [1; 32], [2; 32], None).unwrap(),
            before_fingerprint: [1; 32],
            after_fingerprint: [2; 32],
        }
    }
    const TITLE: u64 = 0x0004000000055d00;
    fn trainer(id: u16) -> Trainer {
        let mut name = [0; 26];
        name[..4].copy_from_slice(&[b'R', 0, b'e', 0]);
        Trainer {
            id,
            secret_id: 4321,
            name,
        }
    }
    /// A transaction bound to `trainer(100)`.
    fn bound() -> PendingTransfer {
        let mut pending = pending();
        pending.game = identity(TITLE, [1; 32], [2; 32], Some(&trainer(100))).unwrap();
        pending
    }
    #[test]
    fn full_image_and_title_are_required_before_adopting_journal_identity() {
        let pending = pending();
        for found in [None, Some(&trainer(100))] {
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
        let mut damaged = pending;
        damaged.game.save_identity[0] ^= 1;
        for found in [None, Some(&trainer(100))] {
            assert_eq!(
                match_pending_image(&damaged, TITLE, [2; 32], found),
                Err(Error::InvalidBinding)
            );
        }
    }
    #[test]
    fn a_save_that_is_neither_image_is_told_by_its_trainer() {
        let mut other = [2; 32];
        other[31] ^= 1;
        // Bound to a trainer: the same copy, another copy, or unreadable.
        let pending = bound();
        assert_eq!(
            match_pending_image(&pending, TITLE, other, Some(&trainer(100))),
            Ok(Match::Other(Owner::Same))
        );
        assert_eq!(
            match_pending_image(&pending, TITLE, other, Some(&trainer(101))),
            Ok(Match::Other(Owner::Different))
        );
        assert_eq!(
            match_pending_image(&pending, TITLE, other, None),
            Ok(Match::Other(Owner::Different))
        );
        // Its images are accepted only with the trainer they were bound to.
        assert_eq!(
            match_pending_image(&pending, TITLE, [1; 32], Some(&trainer(100))),
            Ok(Match::Before)
        );
        assert_eq!(
            match_pending_image(&pending, TITLE, [2; 32], Some(&trainer(100))),
            Ok(Match::After)
        );
        assert_eq!(
            match_pending_image(&pending, TITLE, [2; 32], Some(&trainer(101))),
            Err(Error::InvalidBinding)
        );
        // Bound to no trainer, as by an earlier build: nothing to tell by.
        for found in [None, Some(&trainer(100))] {
            assert_eq!(
                match_pending_image(&self::pending(), TITLE, other, found),
                Ok(Match::Other(Owner::Unrecorded))
            );
        }
        // Another title is refused before anything else is said.
        assert_eq!(
            match_pending_image(&pending, TITLE + 0x100, other, Some(&trainer(100))),
            Err(Error::DifferentTitle)
        );
    }
    #[test]
    fn binding_distinguishes_title_direction_each_file_image_and_the_trainer() {
        let first = identity(1, [1; 32], [2; 32], None).unwrap();
        assert_ne!(first, identity(2, [1; 32], [2; 32], None).unwrap());
        assert_ne!(first, identity(1, [2; 32], [1; 32], None).unwrap());
        assert_ne!(first, identity(1, [1; 32], [3; 32], None).unwrap());
        assert_eq!(
            identity(0, [1; 32], [2; 32], None),
            Err(Error::InvalidTitle)
        );
        assert_eq!(
            identity(1, [1; 32], [1; 32], None),
            Err(Error::IdenticalImages)
        );
        let with = identity(1, [1; 32], [2; 32], Some(&trainer(100))).unwrap();
        assert_ne!(with, first);
        let mut renamed = trainer(100);
        renamed.name[25] = 1;
        let mut secret = trainer(100);
        secret.secret_id += 1;
        for other in [trainer(101), renamed, secret] {
            assert_ne!(with, identity(1, [1; 32], [2; 32], Some(&other)).unwrap());
        }
    }
}
