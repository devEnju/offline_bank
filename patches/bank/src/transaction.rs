//! Identifies a pending game transaction by its two complete file images.
//!
//! No unverified trainer-ID field is used. The binding is stable for one pending
//! transaction, not across a save's entire lifetime. Recovery must independently
//! read the selected title and complete file hash before using the binding.
//! Platform secure-value validation is a separate required step.
//!
//! A save of the title that is neither image was written since: by the game
//! played on, by a new game started on it, or it is another copy. The three
//! are not told apart; the worker settles each by what the transaction moved.

use offline_core::{Fingerprint, GameIdentity, PendingTransfer, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Before,
    After,
    /// Neither image.
    Other,
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
) -> Result<GameIdentity, Error> {
    if title_id == 0 {
        return Err(Error::InvalidTitle);
    }
    if before == after {
        return Err(Error::IdenticalImages);
    }
    let mut hash = Sha256::new();
    // This fixed-length input cannot overflow SHA-256's length field.
    hash.update(b"Bank offline game transaction v1\0").unwrap();
    hash.update(&title_id.to_le_bytes()).unwrap();
    hash.update(&before).unwrap();
    hash.update(&after).unwrap();
    Ok(GameIdentity {
        title_id,
        save_identity: hash.finalize(),
    })
}

/// Classifies actual file bytes; it neither repairs secure values nor commits
/// the Bank. An exact image match alone is insufficient for final reconciliation.
pub fn match_pending_image(
    pending: &PendingTransfer,
    selected_title: u64,
    actual: Fingerprint,
) -> Result<Match, Error> {
    if pending.game
        != identity(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
        )?
    {
        return Err(Error::InvalidBinding);
    }
    if selected_title != pending.game.title_id {
        return Err(Error::DifferentTitle);
    }
    if actual == pending.before_fingerprint {
        Ok(Match::Before)
    } else if actual == pending.after_fingerprint {
        Ok(Match::After)
    } else {
        Ok(Match::Other)
    }
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
            game: identity(0x0004000000055d00, [1; 32], [2; 32]).unwrap(),
            before_fingerprint: [1; 32],
            after_fingerprint: [2; 32],
        }
    }
    #[test]
    fn full_image_and_title_are_required_before_adopting_journal_identity() {
        let pending = pending();
        assert_eq!(
            match_pending_image(&pending, pending.game.title_id, [1; 32]),
            Ok(Match::Before)
        );
        assert_eq!(
            match_pending_image(&pending, pending.game.title_id, [2; 32]),
            Ok(Match::After)
        );
        assert_eq!(
            match_pending_image(&pending, pending.game.title_id + 0x100, [2; 32]),
            Err(Error::DifferentTitle)
        );
        let mut partial = [2; 32];
        partial[31] ^= 1;
        assert_eq!(
            match_pending_image(&pending, pending.game.title_id, partial),
            Ok(Match::Other)
        );
        let mut damaged = pending;
        damaged.game.save_identity[0] ^= 1;
        assert_eq!(
            match_pending_image(&damaged, pending.game.title_id, [2; 32]),
            Err(Error::InvalidBinding)
        );
    }
    #[test]
    fn binding_distinguishes_title_direction_and_each_file_image() {
        let first = identity(1, [1; 32], [2; 32]).unwrap();
        assert_ne!(first, identity(2, [1; 32], [2; 32]).unwrap());
        assert_ne!(first, identity(1, [2; 32], [1; 32]).unwrap());
        assert_ne!(first, identity(1, [1; 32], [3; 32]).unwrap());
        assert_eq!(identity(0, [1; 32], [2; 32]), Err(Error::InvalidTitle));
        assert_eq!(identity(1, [1; 32], [1; 32]), Err(Error::IdenticalImages));
    }
}
