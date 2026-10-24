//! The restore question, frame by frame: what the restore entry does with
//! the original's restore task (`00243AF4`) for a save in progress Bank left
//! to it (docs/internals.md, "Tasks and routing"). The entry
//! (`hooks::transporter_restore`) only applies it.

use offline_core::RecoveryDecision;

/// The task's states the entry looks at: its first, the one that shows its
/// question (`00243E94`), and the one that waits for the answer and takes
/// the buttons out (`00243F68`).
pub const START: u32 = 0;
pub const ASK: u32 = 0xb;
pub const ANSWERING: u32 = 0xe;

/// Where the question stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restore {
    /// Asked, with the answer once it is given.
    Asking(Option<RecoveryDecision>),
    /// Answered: the save is settled behind the loading panel.
    Settling(RecoveryDecision),
    /// Settled: the panel stands its time.
    Settled,
}

/// What one frame of the task does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The task's first frame: on to the question (`ASK`), with the
    /// question's number and the answer cleared, then the original update.
    Begin,
    /// The original update: the question, its buttons, the wait for the
    /// answer, and the buttons taken out.
    Original,
    /// The buttons are gone: the loading panel goes up, and the original
    /// update is not run again.
    Answered,
    /// The worker settles the save as the answer chose.
    Work(RecoveryDecision),
    /// Settled: the task ends once the panel has stood its time.
    Done,
}

impl Restore {
    /// One frame in the task's state `at`, with `answer`, the byte its
    /// buttons' listener writes.
    pub fn frame(&mut self, at: u32, answer: u8) -> Frame {
        if at == START {
            *self = Self::Asking(None);
            return Frame::Begin;
        }
        match *self {
            Self::Asking(Some(decision)) if at != ANSWERING => {
                *self = Self::Settling(decision);
                Frame::Answered
            }
            Self::Asking(chosen) => {
                let given = if at == ANSWERING {
                    answered(answer)
                } else {
                    None
                };
                *self = Self::Asking(chosen.or(given));
                Frame::Original
            }
            Self::Settling(decision) => Frame::Work(decision),
            Self::Settled => Frame::Done,
        }
    }
}

/// The answer, the byte the buttons' listener writes (`task + 0x90`, read
/// at `00243F68`): 1 Yes, 2 No; 0 while none is given. Yes restores the
/// data: Bank goes back to the save before. No keeps the save after.
pub fn answered(answer: u8) -> Option<RecoveryDecision> {
    match answer {
        1 => Some(RecoveryDecision::KeepBefore),
        2 => Some(RecoveryDecision::CommitAfter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_restores_the_bank_before_and_no_keeps_the_one_after() {
        assert_eq!(answered(1), Some(RecoveryDecision::KeepBefore));
        assert_eq!(answered(2), Some(RecoveryDecision::CommitAfter));
        for none in [0, 3, 0xff] {
            assert_eq!(answered(none), None);
        }
    }

    #[test]
    fn the_first_question_is_asked_and_its_answer_settles_once_the_buttons_are_gone() {
        for (answer, decision, after) in [
            // After a Yes the original goes on to its second question,
            // after a No to its end: neither is run.
            (1, RecoveryDecision::KeepBefore, ASK),
            (2, RecoveryDecision::CommitAfter, 0x10),
        ] {
            let mut restore = Restore::Settled;
            assert_eq!(restore.frame(START, 0xaa), Frame::Begin);
            assert_eq!(restore, Restore::Asking(None));
            // The question, its buttons, the wait.
            for at in [ASK, 0xc, 0xd, ANSWERING, ANSWERING] {
                assert_eq!(restore.frame(at, 0), Frame::Original);
            }
            assert_eq!(restore, Restore::Asking(None));
            // Answered: the original takes the buttons out, over frames.
            assert_eq!(restore.frame(ANSWERING, answer), Frame::Original);
            assert_eq!(restore, Restore::Asking(Some(decision)));
            // The listener's byte cleared meanwhile: the answer stays.
            assert_eq!(restore.frame(ANSWERING, 0), Frame::Original);
            // The buttons are gone.
            assert_eq!(restore.frame(after, 0), Frame::Answered);
            assert_eq!(restore, Restore::Settling(decision));
            assert_eq!(restore.frame(after, 0), Frame::Work(decision));
            restore = Restore::Settled;
            assert_eq!(restore.frame(after, 0), Frame::Done);
        }
    }

    #[test]
    fn nothing_but_an_answer_in_the_wait_for_it_counts() {
        let mut restore = Restore::Asking(None);
        for at in [ASK, 0xc, 0xd] {
            assert_eq!(restore.frame(at, 1), Frame::Original);
        }
        assert_eq!(restore.frame(ANSWERING, 7), Frame::Original);
        assert_eq!(restore, Restore::Asking(None));
    }
}
