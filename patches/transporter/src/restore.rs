//! The restore question, frame by frame: what the restore entry does with
//! the original's restore task (`00243AF4`) for a save in progress Bank left
//! to it (docs/internals.md, "Tasks and routing"). The entry
//! (`hooks::transporter_restore`) only applies it.
//!
//! The original asks two questions, message `[0028E00C + 4 * [task +
//! 0x8F]]`: 0x1A "You can try restoring your data, but you may lose the
//! Pokémon currently being transported. Would you still like to try
//! restoring your data?" and 0x1B "This is your final warning. The Pokémon
//! you are trying to send could be lost forever. Are you sure you want to
//! try to restore your data?". Its state 0xE waits for the answer, takes
//! the buttons out, and after a Yes counts `+0x8F` up: on 1 it asks the
//! second question (state 0xB), on 2 it starts its panel and restores
//! (state 7); after a No it ends (state 0x10, outcome 3).
//!
//! Here the first answer chooses (Yes the save before, No the save after),
//! the second question follows either answer, and only its Yes settles the
//! save as the first chose. A No to it writes nothing: the original's end.
//!
//! The buttons are the shared UI's (its key handler `0024AF20`): `ui +
//! 0x84` is the one selected, A presses it, B presses No (button ID 3)
//! whatever is selected, and state 0xD selects No as they come in. On the
//! first question Yes is selected instead and B presses nothing, since that
//! answer only chooses; the second keeps the original's No and B.

use offline_core::RecoveryDecision;

/// The task's states the entry looks at: its first, the one that shows its
/// question (`00243E94`), and the one that waits for the answer and takes
/// the buttons out (`00243F68`).
pub const START: u32 = 0;
pub const ASK: u32 = 0xb;
/// The state that brings the buttons in (`00243EE4`); in its last frame it
/// selects No and goes on to `ANSWERING`.
pub const BUTTONS: u32 = 0xd;
pub const ANSWERING: u32 = 0xe;
/// The question `+0x8F` selects: 0x1A first, 0x1B second.
pub const FIRST: u8 = 0;
pub const SECOND: u8 = 1;

/// Where the questions stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restore {
    /// The first question, with its answer once it is given.
    First(Option<RecoveryDecision>),
    /// The second, with the first's answer and its own once it is given:
    /// `true` Yes, `false` No.
    Second(RecoveryDecision, Option<bool>),
    /// Confirmed: the save is settled behind the loading panel.
    Settling(RecoveryDecision),
    /// Settled: the panel stands its time.
    Settled,
    /// Not confirmed: the original ends the task, and nothing is written.
    Declined,
}

/// What one frame of the task does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The task's first frame: on to the first question (`ASK`, `FIRST`),
    /// with the answer cleared, then the original update.
    Begin,
    /// The original update: a question, its buttons, the wait for the
    /// answer, the buttons taken out, and its end after a declined one.
    Original,
    /// The first question's buttons are gone: on to the second (`ASK`,
    /// `SECOND`), with the answer cleared, then the original update.
    Again,
    /// The second question waits: its question number held at `FIRST`, so
    /// that the original goes on to `ASK` after a Yes as after the first,
    /// and starts neither its panel nor its restore; then the original.
    Hold,
    /// Confirmed and the buttons are gone: the loading panel goes up, and
    /// the original update is not run again.
    Answered,
    /// The worker settles the save as the first answer chose.
    Work(RecoveryDecision),
    /// Settled: the task ends once the panel has stood its time.
    Done,
    /// The first question's buttons come in: the original update, and once
    /// it has gone on to `ANSWERING`, Yes selected.
    Buttons,
}

impl Restore {
    /// Whether B presses No, as the original has it: on every question but
    /// the first, whose answer only chooses.
    pub fn b_presses_no(&self) -> bool {
        !matches!(self, Self::First(_))
    }

    /// One frame in the task's state `at`, with `answer`, the byte its
    /// buttons' listener writes.
    pub fn frame(&mut self, at: u32, answer: u8) -> Frame {
        if at == START {
            *self = Self::First(None);
            return Frame::Begin;
        }
        match *self {
            Self::First(Some(decision)) if at != ANSWERING => {
                *self = Self::Second(decision, None);
                Frame::Again
            }
            Self::First(_) if at == BUTTONS => Frame::Buttons,
            Self::First(chosen) => {
                if at == ANSWERING {
                    *self = Self::First(chosen.or(answered(answer)));
                }
                Frame::Original
            }
            Self::Second(decision, Some(true)) if at != ANSWERING => {
                *self = Self::Settling(decision);
                Frame::Answered
            }
            Self::Second(_, Some(false)) if at != ANSWERING => {
                *self = Self::Declined;
                Frame::Original
            }
            Self::Second(decision, given) if at == ANSWERING => {
                *self = Self::Second(decision, given.or(confirmed(answer)));
                Frame::Hold
            }
            Self::Second(..) | Self::Declined => Frame::Original,
            Self::Settling(decision) => Frame::Work(decision),
            Self::Settled => Frame::Done,
        }
    }
}

/// The first answer, the byte the buttons' listener writes (`task + 0x90`,
/// read at `00243F68`): 1 Yes, 2 No; 0 while none is given. Yes restores
/// the data: Bank goes back to the save before. No keeps the save after.
pub fn answered(answer: u8) -> Option<RecoveryDecision> {
    match answer {
        1 => Some(RecoveryDecision::KeepBefore),
        2 => Some(RecoveryDecision::CommitAfter),
        _ => None,
    }
}

/// The second answer, the same byte: Yes applies the first.
pub fn confirmed(answer: u8) -> Option<bool> {
    match answer {
        1 => Some(true),
        2 => Some(false),
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
        assert_eq!(confirmed(1), Some(true));
        assert_eq!(confirmed(2), Some(false));
        for none in [0, 3, 0xff] {
            assert_eq!(answered(none), None);
            assert_eq!(confirmed(none), None);
        }
    }

    /// One question from its state `ASK`: its buttons, the wait, the
    /// answer, the buttons taken out; `wait` is what each frame of the wait
    /// does. The first question's buttons come in with Yes selected.
    fn ask(restore: &mut Restore, answer: u8, wait: Frame) {
        let first = matches!(restore, Restore::First(_));
        assert_eq!(restore.frame(ASK, 0), Frame::Original);
        assert_eq!(restore.frame(0xc, 0), Frame::Original);
        let buttons = if first {
            Frame::Buttons
        } else {
            Frame::Original
        };
        assert_eq!(restore.frame(BUTTONS, 0), buttons);
        assert_eq!(restore.frame(BUTTONS, 0), buttons);
        // B presses No on every question but the first.
        assert_eq!(restore.b_presses_no(), !first);
        assert_eq!(restore.frame(ANSWERING, 0), wait);
        // Answered: the original takes the buttons out, over frames; the
        // listener's byte cleared meanwhile, the answer stays.
        assert_eq!(restore.frame(ANSWERING, answer), wait);
        assert_eq!(restore.frame(ANSWERING, 0), wait);
    }

    #[test]
    fn the_first_answer_chooses_and_only_a_yes_to_the_second_settles() {
        for (first, decision) in [
            (1, RecoveryDecision::KeepBefore),
            (2, RecoveryDecision::CommitAfter),
        ] {
            // After a Yes the original goes on to its second question,
            // after a No to its end: both go on to the second.
            let after_first = if first == 1 { ASK } else { 0x10 };
            for (second, settles) in [(1, true), (2, false)] {
                let mut restore = Restore::Settled;
                assert_eq!(restore.frame(START, 0xaa), Frame::Begin);
                assert_eq!(restore, Restore::First(None));
                ask(&mut restore, first, Frame::Original);
                assert_eq!(restore, Restore::First(Some(decision)));
                assert_eq!(restore.frame(after_first, 0), Frame::Again);
                assert_eq!(restore, Restore::Second(decision, None));
                ask(&mut restore, second, Frame::Hold);
                if settles {
                    // With its number held, the original goes on to `ASK`.
                    assert_eq!(restore.frame(ASK, 0), Frame::Answered);
                    assert_eq!(restore, Restore::Settling(decision));
                    assert_eq!(restore.frame(ASK, 0), Frame::Work(decision));
                    restore = Restore::Settled;
                    assert_eq!(restore.frame(ASK, 0), Frame::Done);
                } else {
                    // Its end, state 0x10, is the original's.
                    assert_eq!(restore.frame(0x10, 0), Frame::Original);
                    assert_eq!(restore, Restore::Declined);
                    assert_eq!(restore.frame(0x10, 0), Frame::Original);
                }
            }
        }
    }

    #[test]
    fn b_presses_no_but_on_the_first_question() {
        assert!(!Restore::First(None).b_presses_no());
        assert!(!Restore::First(Some(RecoveryDecision::KeepBefore)).b_presses_no());
        for other in [
            Restore::Second(RecoveryDecision::CommitAfter, None),
            Restore::Settling(RecoveryDecision::KeepBefore),
            Restore::Settled,
            Restore::Declined,
        ] {
            assert!(other.b_presses_no(), "{other:?}");
        }
    }

    #[test]
    fn nothing_but_an_answer_in_the_wait_for_it_counts() {
        let mut restore = Restore::First(None);
        for at in [ASK, 0xc] {
            assert_eq!(restore.frame(at, 1), Frame::Original);
        }
        assert_eq!(restore.frame(BUTTONS, 1), Frame::Buttons);
        assert_eq!(restore.frame(ANSWERING, 7), Frame::Original);
        assert_eq!(restore, Restore::First(None));
        let mut restore = Restore::Second(RecoveryDecision::KeepBefore, None);
        for at in [ASK, 0xc, 0xd] {
            assert_eq!(restore.frame(at, 1), Frame::Original);
        }
        assert_eq!(restore.frame(ANSWERING, 7), Frame::Hold);
        assert_eq!(restore, Restore::Second(RecoveryDecision::KeepBefore, None));
    }
}
