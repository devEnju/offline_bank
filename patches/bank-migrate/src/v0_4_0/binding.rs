//! Frozen: how the releases of this way bound a save in progress to its
//! game, from their `patches/bank/src/transaction.rs`. v0.3.0 wrote a whole
//! SHA-256 ("v2"); v0.3.1 to v0.4.0 the trainer's name and ID and the first
//! four bytes of one ("v3"). v0.4.0, the last release of this way, reads only
//! the second, so the migration writes a save in progress of v0.3.0 in it
//! (`super::Way::ready_for_its_release`). Only the migration uses this.

use super::{Fingerprint, GameIdentity, PendingTransfer};
use offline_core::Sha256;

/// The kind of copy the save was made with, as both releases number it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    Cartridge = 0,
    Installed = 1,
}
const MEDIA: [Medium; 2] = [Medium::Cartridge, Medium::Installed];

/// Trainer name units in a v3 binding.
pub const NAME: usize = 12;
const HASH_AT: usize = NAME * 2 + 4;

/// v0.3.0's `identity`. `None` where it refuses one.
pub fn v2(
    title_id: u64,
    before: Fingerprint,
    after: Fingerprint,
    medium: Medium,
) -> Option<GameIdentity> {
    if title_id == 0 || before == after {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"Bank offline game transaction v2\0").ok()?;
    hash.update(&title_id.to_le_bytes()).ok()?;
    hash.update(&before).ok()?;
    hash.update(&after).ok()?;
    hash.update(&[medium as u8]).ok()?;
    Some(GameIdentity {
        title_id,
        save_identity: hash.finalize(),
    })
}

/// v0.4.0's `identity`. `None` where it refuses one.
pub fn v3(
    title_id: u64,
    before: Fingerprint,
    after: Fingerprint,
    medium: Medium,
    name: [u16; NAME],
    id: u32,
) -> Option<GameIdentity> {
    if title_id == 0 || before == after {
        return None;
    }
    let mut out = [0; 32];
    for (index, unit) in name.into_iter().enumerate() {
        out[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    out[NAME * 2..HASH_AT].copy_from_slice(&id.to_le_bytes());
    let mut hash = Sha256::new();
    hash.update(b"Bank offline game transaction v3\0").ok()?;
    hash.update(&title_id.to_le_bytes()).ok()?;
    hash.update(&before).ok()?;
    hash.update(&after).ok()?;
    hash.update(&[medium as u8]).ok()?;
    hash.update(&out[..HASH_AT]).ok()?;
    let hash = hash.finalize();
    out[HASH_AT..].copy_from_slice(&hash[..32 - HASH_AT]);
    Some(GameIdentity {
        title_id,
        save_identity: out,
    })
}

/// The trainer a v3 binding names: v0.4.0's `Trainer::of`.
pub fn trainer(game: &GameIdentity) -> ([u16; NAME], u32) {
    let bytes = &game.save_identity;
    let mut name = [0; NAME];
    for (index, unit) in name.iter_mut().enumerate() {
        *unit = u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]);
    }
    let id = u32::from_le_bytes([
        bytes[NAME * 2],
        bytes[NAME * 2 + 1],
        bytes[NAME * 2 + 2],
        bytes[NAME * 2 + 3],
    ]);
    (name, id)
}

/// v0.4.0's `bound`: the kind of copy a v3 binding names, if it is one.
pub fn bound(pending: &PendingTransfer) -> Option<Medium> {
    let (name, id) = trainer(&pending.game);
    MEDIA.into_iter().find(|&medium| {
        v3(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            medium,
            name,
            id,
        ) == Some(pending.game)
    })
}

/// The kind of copy a v2 binding names, if it is one: the one whose hash it
/// is, as v0.3.0's `match_pending_image` tried both.
pub fn bound_v2(pending: &PendingTransfer) -> Option<Medium> {
    MEDIA.into_iter().find(|&medium| {
        v2(
            pending.game.title_id,
            pending.before_fingerprint,
            pending.after_fingerprint,
            medium,
        ) == Some(pending.game)
    })
}

/// The same save in progress bound as v0.4.0 binds one, if v0.3.0 bound it:
/// for the kind of copy the v2 hash names, with no trainer, which v0.3.0 did
/// not record. `None` for every other binding.
pub fn as_v3(pending: &PendingTransfer) -> Option<GameIdentity> {
    if bound(pending).is_some() {
        return None;
    }
    let medium = bound_v2(pending)?;
    v3(
        pending.game.title_id,
        pending.before_fingerprint,
        pending.after_fingerprint,
        medium,
        [0; NAME],
        0,
    )
}
