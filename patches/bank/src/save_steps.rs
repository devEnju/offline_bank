//! Save and Quit on the main thread (`runtime` carries this out): the order
//! of its steps, the worker's reply each one waits for, and when a save may
//! be in progress in the Bank's journal. The worker's side is
//! `storage_worker`.

use crate::storage_worker::Reply;
use bank_common::fault::Fault;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `Job::Detect` is out: is the game's Game Card there? Nothing of the
    /// save is done before its answer.
    Detect,
    /// The Game Card is there and the counts and Miles are settled: the game
    /// and the Bank are staged on this frame, and `Job::Prepare` is handed
    /// over.
    Stage,
    /// `Job::Prepare` is out: the journal is prepared and the game checked.
    Prepare,
    /// The original writes the game.
    Write,
    /// `Job::Finalize` is out: the game is read back and the save resolved.
    Finalize,
    /// Save and Quit has ended, saved or not, and the original's message for
    /// that ending is up.
    Ended { saved: bool },
}

impl Step {
    /// The step that `reply` leads to. Only `Detect`, `Prepare` and
    /// `Finalize` wait for the worker. A missing Game Card is answered
    /// before anything is written, and only a cartridge can be missing; any
    /// other reply is not the one the step waits for.
    pub fn after(self, reply: Reply, cartridge: bool) -> Result<Self, Fault> {
        match (self, reply) {
            (Self::Detect | Self::Prepare, Reply::CardRemoved) if cartridge => {
                Ok(Self::Ended { saved: false })
            }
            (Self::Detect | Self::Prepare, Reply::CardRemoved) => Err(Fault::GameRead),
            (Self::Detect, Reply::Detected) => Ok(Self::Stage),
            (Self::Prepare, Reply::ReadyToWrite) => Ok(Self::Write),
            (Self::Finalize, Reply::Saved) => Ok(Self::Ended { saved: true }),
            _ => Err(Fault::JournalRecovery),
        }
    }
    /// Whether a save may be in progress in the journal: from the moment
    /// `Job::Prepare` is handed over, since a failed preparation may already
    /// have published one, until the worker has resolved it. A step that
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

    const STEPS: [Step; 7] = [
        Step::Detect,
        Step::Stage,
        Step::Prepare,
        Step::Write,
        Step::Finalize,
        Step::Ended { saved: false },
        Step::Ended { saved: true },
    ];

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
        std::vec![
            Reply::Missing,
            Reply::Opened {
                phase: Phase::Clean(save),
                loaded: None,
            },
            Reply::BankReady { loaded: None },
            Reply::Saved,
            Reply::Inspected(evidence),
            Reply::Detected,
            Reply::CardRemoved,
            Reply::ReadyToWrite,
            Reply::Waiting {
                title: 1,
                trainer: Trainer {
                    name: [0; crate::transaction::NAME],
                    id: 0,
                },
            },
            Reply::Closed,
        ]
    }

    #[test]
    fn a_save_goes_from_the_game_card_to_its_message() {
        let step = Step::Detect.after(Reply::Detected, true).unwrap();
        assert_eq!(step, Step::Stage);
        // The hook hands `Job::Prepare` over from `Stage`, and starts the
        // original's writer, whose end hands `Job::Finalize` over.
        let step = Step::Prepare.after(Reply::ReadyToWrite, true).unwrap();
        assert_eq!(step, Step::Write);
        let step = Step::Finalize.after(Reply::Saved, true).unwrap();
        assert_eq!(step, Step::Ended { saved: true });
    }

    #[test]
    fn a_missing_game_card_ends_the_save_only_before_anything_is_written() {
        for step in [Step::Detect, Step::Prepare] {
            assert_eq!(
                step.after(Reply::CardRemoved, true),
                Ok(Step::Ended { saved: false })
            );
            // An installed copy has no card to take out.
            assert_eq!(step.after(Reply::CardRemoved, false), Err(Fault::GameRead));
        }
        // Once the game is written, a missing card is an error of the save.
        assert_eq!(
            Step::Finalize.after(Reply::CardRemoved, true),
            Err(Fault::JournalRecovery)
        );
    }

    #[test]
    fn every_other_reply_is_one_no_step_waits_for() {
        let expected = [
            (Step::Detect, Reply::Detected),
            (Step::Detect, Reply::CardRemoved),
            (Step::Prepare, Reply::ReadyToWrite),
            (Step::Prepare, Reply::CardRemoved),
            (Step::Finalize, Reply::Saved),
        ];
        for step in STEPS {
            for reply in replies() {
                for cartridge in [false, true] {
                    let after = step.after(reply, cartridge);
                    if expected.contains(&(step, reply)) {
                        assert_ne!(after, Err(Fault::JournalRecovery), "{step:?} {reply:?}");
                    } else {
                        assert_eq!(after, Err(Fault::JournalRecovery), "{step:?} {reply:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_save_is_in_progress_from_its_preparation_until_it_is_resolved() {
        for step in STEPS {
            assert_eq!(
                step.pending(),
                matches!(step, Step::Prepare | Step::Write | Step::Finalize),
                "{step:?}"
            );
        }
        // Both endings leave none: nothing was written, or it was resolved.
        assert!(!Step::Prepare
            .after(Reply::CardRemoved, true)
            .unwrap()
            .pending());
        assert!(!Step::Finalize.after(Reply::Saved, true).unwrap().pending());
    }
}
