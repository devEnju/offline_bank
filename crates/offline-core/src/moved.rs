//! What an interrupted Bank save moved between Bank and the game, and what
//! Bank does with that save when the game no longer shows whether its own
//! save was written.
//!
//! A recovery decides by the game's save: unchanged, the old Bank snapshot
//! stays; exactly the save Bank prepared, the new one is committed. A game
//! that was played and saved in between shows neither. Bank still holds both
//! snapshots, though, and for most sessions one of the two answers cannot
//! lose a Pokémon whichever way the game's save went:
//!
//! - a session that only brought Pokémon into Bank: committing leaves them
//!   in Bank, and at worst in the game as well;
//! - a session that only took Pokémon out of Bank: staying with the old
//!   snapshot leaves them in Bank, and at worst in the game as well.
//!
//! Only a session that did both has no such answer.

use crate::{sections::Identity, RecoveryDecision};

/// How many Pokémon one snapshot holds that the other does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    /// In Bank after the session and not before: they came from the game.
    pub deposited: u32,
    /// In Bank before the session and not after: they went to the game, or
    /// were released.
    pub withdrawn: u32,
}

/// Compares everything Bank held before a session with everything it held
/// after it. Both lists are sorted in place. A Pokémon that only changed its
/// place counts for neither side, and one that is there twice counts twice.
pub fn moved(before: &mut [Identity], after: &mut [Identity]) -> Moved {
    before.sort_unstable();
    after.sort_unstable();
    let mut out = Moved {
        deposited: 0,
        withdrawn: 0,
    };
    let (mut old, mut new) = (0, 0);
    while old < before.len() && new < after.len() {
        match before[old].cmp(&after[new]) {
            core::cmp::Ordering::Equal => {
                old += 1;
                new += 1;
            }
            core::cmp::Ordering::Less => {
                out.withdrawn += 1;
                old += 1;
            }
            core::cmp::Ordering::Greater => {
                out.deposited += 1;
                new += 1;
            }
        }
    }
    out.withdrawn += (before.len() - old) as u32;
    out.deposited += (after.len() - new) as u32;
    out
}

/// The snapshot Bank goes on with when the game's save cannot say.
///
/// One-way sessions take the side that cannot lose a Pokémon. A session
/// that moved none takes the old snapshot, which cannot lose the Miles of a
/// claim. A session that moved Pokémon both ways commits: a player who found
/// the game as expected and played on is the likelier case, and neither
/// answer is free of risk there.
pub fn choose(moved: Moved) -> RecoveryDecision {
    if moved.deposited != 0 {
        RecoveryDecision::CommitAfter
    } else {
        RecoveryDecision::KeepBefore
    }
}

/// The snapshot Bank goes on with when the game is not there to be asked,
/// or another copy of it is. Its save may be either image or a later one,
/// so only the answer that cannot lose is taken: `None` for a session that
/// moved Pokémon both ways, which waits for its game.
pub fn choose_unseen(moved: Moved) -> Option<RecoveryDecision> {
    if moved.deposited != 0 && moved.withdrawn != 0 {
        None
    } else {
        Some(choose(moved))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn list(ids: &[u32]) -> Vec<Identity> {
        ids.iter().map(|&id| [id, id ^ 0xffff, 7]).collect()
    }
    fn count(before: &[u32], after: &[u32]) -> (u32, u32) {
        let moved = moved(&mut list(before), &mut list(after));
        (moved.deposited, moved.withdrawn)
    }

    #[test]
    fn only_what_entered_or_left_bank_is_counted() {
        assert_eq!(count(&[], &[]), (0, 0));
        assert_eq!(count(&[1, 2, 3], &[1, 2, 3]), (0, 0));
        // Rearranged inside Bank: order does not matter.
        assert_eq!(count(&[1, 2, 3], &[3, 1, 2]), (0, 0));
        assert_eq!(count(&[1, 2], &[1, 2, 9, 8]), (2, 0));
        assert_eq!(count(&[1, 2, 9, 8], &[2, 8]), (0, 2));
        assert_eq!(count(&[1, 2, 3], &[2, 3, 4, 5]), (2, 1));
        assert_eq!(count(&[], &[4]), (1, 0));
        assert_eq!(count(&[4], &[]), (0, 1));
    }

    #[test]
    fn a_pokemon_that_is_there_twice_counts_twice() {
        assert_eq!(count(&[5, 5], &[5, 5]), (0, 0));
        assert_eq!(count(&[5], &[5, 5]), (1, 0));
        assert_eq!(count(&[5, 5, 5], &[5]), (0, 2));
    }

    #[test]
    fn every_field_of_an_identity_tells_two_pokemon_apart() {
        let mut before = [[1, 2, 3]];
        for other in [[9, 2, 3], [1, 9, 3], [1, 2, 9]] {
            let found = moved(&mut before, &mut [other]);
            assert_eq!((found.deposited, found.withdrawn), (1, 1));
        }
    }

    #[test]
    fn the_choice_cannot_lose_a_pokemon_in_a_one_way_session() {
        let choice = |deposited, withdrawn| {
            choose(Moved {
                deposited,
                withdrawn,
            })
        };
        // Deposits only: they stay in Bank.
        assert_eq!(choice(1, 0), RecoveryDecision::CommitAfter);
        assert_eq!(choice(900, 0), RecoveryDecision::CommitAfter);
        // Withdrawals or releases only: they stay in Bank.
        assert_eq!(choice(0, 1), RecoveryDecision::KeepBefore);
        assert_eq!(choice(0, 30), RecoveryDecision::KeepBefore);
        // Nothing moved: the old Miles record stays.
        assert_eq!(choice(0, 0), RecoveryDecision::KeepBefore);
        // Both ways: the save is taken to have gone through.
        assert_eq!(choice(3, 2), RecoveryDecision::CommitAfter);
        assert_eq!(choice(1, 500), RecoveryDecision::CommitAfter);
    }

    #[test]
    fn without_the_game_only_a_one_way_session_is_settled() {
        let choice = |deposited, withdrawn| {
            choose_unseen(Moved {
                deposited,
                withdrawn,
            })
        };
        assert_eq!(choice(1, 0), Some(RecoveryDecision::CommitAfter));
        assert_eq!(choice(900, 0), Some(RecoveryDecision::CommitAfter));
        assert_eq!(choice(0, 1), Some(RecoveryDecision::KeepBefore));
        assert_eq!(choice(0, 30), Some(RecoveryDecision::KeepBefore));
        assert_eq!(choice(0, 0), Some(RecoveryDecision::KeepBefore));
        // Both ways: either answer could lose a Pokémon; the game decides.
        assert_eq!(choice(3, 2), None);
        assert_eq!(choice(1, 1), None);
    }
}
