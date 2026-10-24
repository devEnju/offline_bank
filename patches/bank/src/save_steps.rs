//! Save and Quit on the main thread (`hooks` carries this out): the order
//! of its steps, the worker's reply each one waits for, and when a save may
//! be in progress in the Bank's journal. The worker's side is
//! `storage_worker`.

use crate::storage_worker::{Ending, Reply};
use bank_common::fault::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `Job::Detect` is out: is the game's Game Card there? Nothing of the
    /// save is done before its answer.
    ///
    /// From its answer on, each of the following steps is one frame with one
    /// heavy native call on the main thread, as the original's states are.
    Detect,
    /// The Game Card is there: the game's Pokémon are counted on this frame.
    CountGame,
    /// The game's Pokémon are counted (`None` without a game count): the
    /// Bank's are counted, the Miles settled and the counts recorded on this
    /// frame.
    CountBank { game: Option<u32> },
    /// The counts and Miles are settled: the game is prepared on this frame.
    StageGame,
    /// The game is prepared: the Pokédex is refreshed on this frame.
    StageDex,
    /// The Pokédex is refreshed: the Bank is staged on this frame, and
    /// `Job::Prepare` is handed over.
    StageBank,
    /// `Job::Prepare` is out: the journal is prepared and the game checked.
    Prepare,
    /// The original writes the game.
    Write,
    /// `Job::Finalize` is out: the original's writer has ended, and the save
    /// is settled from what the game holds.
    Finalize,
    /// The save is current: its message waits until the loading screen has
    /// stood for 1.5 s (`patch_common::spinner`).
    Saved,
    /// Save and Quit has saved, and the original's message for it is up.
    Ended,
    /// Save and Quit has ended without saving, and the original's message
    /// for what it leaves is up (`Ending`). It shows at once.
    Stopped(Ending),
}

impl Step {
    /// The step that `reply` leads to. Only `Detect`, `Prepare` and
    /// `Finalize` wait for the worker. A missing Game Card at `Detect` is
    /// one only for a cartridge; any other reply is not the one the step
    /// waits for.
    pub fn after(self, reply: Reply, cartridge: bool) -> Result<Self, Fault> {
        match (self, reply) {
            (Self::Detect, Reply::Detected) => Ok(Self::CountGame),
            (Self::Detect, Reply::Ended(Ending::Retry)) if cartridge => {
                Ok(Self::Stopped(Ending::Retry))
            }
            // An installed copy has no card to take out.
            (Self::Detect, Reply::Ended(_)) => Err(Fault::GameRead),
            (Self::Prepare, Reply::ReadyToWrite) => Ok(Self::Write),
            (Self::Prepare | Self::Finalize, Reply::Ended(ending)) => Ok(Self::Stopped(ending)),
            (Self::Finalize, Reply::Saved) => Ok(Self::Saved),
            _ => Err(Fault::JournalRecovery),
        }
    }
    /// Whether a save may be in progress in the journal: from the moment
    /// `Job::Prepare` is handed over, since a failed preparation may already
    /// have published one, until the worker has settled it. A step that
    /// stops with an error keeps what it says; the next opening settles it.
    pub fn pending(self) -> bool {
        matches!(self, Self::Prepare | Self::Write | Self::Finalize)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::{native_game::SecureValues, transaction::Trainer};
    use offline_core::{Part, Phase, Save, Slot};
    use std::vec::Vec;

    const ENDINGS: [Ending; 2] = [Ending::Retry, Ending::NoSpace];

    fn steps() -> Vec<Step> {
        let mut steps = std::vec![
            Step::Detect,
            Step::CountGame,
            Step::CountBank { game: None },
            Step::CountBank { game: Some(31) },
            Step::StageGame,
            Step::StageDex,
            Step::StageBank,
            Step::Prepare,
            Step::Write,
            Step::Finalize,
            Step::Saved,
            Step::Ended,
        ];
        steps.extend(ENDINGS.map(Step::Stopped));
        steps
    }

    /// One of every reply the worker gives.
    fn replies() -> Vec<Reply> {
        let evidence = crate::storage_worker::GameEvidence {
            title: 1,
            fingerprint: [0; 32],
            secure: SecureValues {
                current: 0,
                previous: 0,
            },
            cartridge: true,
        };
        let part = Part {
            slot: Slot::A,
            crc: 0,
        };
        let save = Save {
            bank: part,
            main: part,
            mover: part,
            held: 0,
        };
        let mut replies = std::vec![
            Reply::Missing,
            Reply::Opened {
                phase: Phase::Clean(save),
                loaded: None,
            },
            Reply::BankReady { loaded: None },
            Reply::Saved,
            Reply::Inspected(evidence),
            Reply::Detected,
            Reply::ReadyToWrite,
            Reply::Waiting {
                title: 1,
                trainer: Trainer {
                    name: [0; crate::transaction::NAME],
                    id: 0,
                },
            },
        ];
        replies.extend(ENDINGS.map(Reply::Ended));
        replies
    }

    #[test]
    fn a_save_goes_from_the_game_card_to_its_message() {
        let step = Step::Detect.after(Reply::Detected, true).unwrap();
        assert_eq!(step, Step::CountGame);
        // The hook goes on by itself, a frame each, through `CountBank`,
        // `StageGame` and `StageDex` to `StageBank`, which hands
        // `Job::Prepare` over, and starts the original's writer, whose end
        // hands `Job::Finalize` over.
        let step = Step::Prepare.after(Reply::ReadyToWrite, true).unwrap();
        assert_eq!(step, Step::Write);
        let step = Step::Finalize.after(Reply::Saved, true).unwrap();
        assert_eq!(step, Step::Saved);
        // Its message follows, once the loading screen has stood long
        // enough.
    }

    #[test]
    fn a_save_that_cannot_finish_ends_at_the_message_of_what_it_leaves() {
        // A Game Card that is gone at the first question: only a cartridge
        // has one to take out.
        assert_eq!(
            Step::Detect.after(Reply::Ended(Ending::Retry), true),
            Ok(Step::Stopped(Ending::Retry))
        );
        for cartridge in [false, true] {
            for ending in ENDINGS {
                if (ending, cartridge) != (Ending::Retry, true) {
                    assert_eq!(
                        Step::Detect.after(Reply::Ended(ending), cartridge),
                        Err(Fault::GameRead)
                    );
                }
                // The preparation and the settling after the write say what
                // they leave, for every kind of copy.
                for step in [Step::Prepare, Step::Finalize] {
                    assert_eq!(
                        step.after(Reply::Ended(ending), cartridge),
                        Ok(Step::Stopped(ending))
                    );
                }
            }
        }
    }

    #[test]
    fn every_other_reply_is_one_no_step_waits_for() {
        let expected = |step: Step, reply: Reply| {
            matches!(
                (step, reply),
                (Step::Detect, Reply::Detected | Reply::Ended(_))
                    | (Step::Prepare, Reply::ReadyToWrite | Reply::Ended(_))
                    | (Step::Finalize, Reply::Saved | Reply::Ended(_))
            )
        };
        for step in steps() {
            for reply in replies() {
                for cartridge in [false, true] {
                    let after = step.after(reply, cartridge);
                    if expected(step, reply) {
                        assert_ne!(after, Err(Fault::JournalRecovery), "{step:?} {reply:?}");
                    } else {
                        assert_eq!(after, Err(Fault::JournalRecovery), "{step:?} {reply:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_save_is_in_progress_from_its_preparation_until_it_is_settled() {
        for step in steps() {
            assert_eq!(
                step.pending(),
                matches!(step, Step::Prepare | Step::Write | Step::Finalize),
                "{step:?}"
            );
        }
        // Every ending leaves none the session still waits for: what the
        // journal holds then is the next opening's to settle.
        for ending in ENDINGS {
            for step in [Step::Prepare, Step::Finalize] {
                assert!(!step.after(Reply::Ended(ending), true).unwrap().pending());
            }
        }
        assert!(!Step::Finalize.after(Reply::Saved, true).unwrap().pending());
    }
}
