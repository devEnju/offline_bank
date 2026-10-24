//! The worker of the Transporter patch (`patch_common::worker`): the check,
//! the delivery and the answer to the restore question, on Bank's files as
//! the delivery rules reach them. The
//! main thread hands the delivery the transport box in the staging buffer;
//! the worker touches no object of the original.
//!
//! What each answer leads to is decided here, in plain code (`checked`,
//! `delivered`): an original message, or the error screen with Bank's two
//! numbers (`bank_common::fault`) for what is not expected.

use crate::task::RECORDS_LEN;
use bank_common::{fault::Fault, worker::WorkerError};
use offline_core::{
    sections::TRANSPORT_SLOTS,
    transport::{DeliverError, Refusal, SettleError},
    RecoveryDecision,
};
use patch_common::fs_result::lack_of_space;

/// The staging buffer: the transport box's records, then its tags.
pub const STAGING_LEN: usize = RECORDS_LEN + TRANSPORT_SLOTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// May Bank take a delivery?
    Check,
    /// Deliver the transport box in the staging buffer.
    Deliver,
    /// Settle the save in progress Bank left to the restore question, to
    /// the snapshot the answer chose (`transport::settle`).
    Settle(RecoveryDecision),
}
/// What a job found. Which message the screen shows for it is the hooks'
/// (`ui::refusal_message`, `ui::shown`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Bank can take a delivery.
    Ready,
    /// Bank cannot take one now.
    Refused(Refusal),
    /// Bank holds the delivery: it is written, flushed and read back, or
    /// the record announcing it is in force when Bank's journal is read
    /// again (`transport::deliver`).
    Delivered,
    /// Bank holds none, for certain, as the SD card had no room for its
    /// write (`lack_of_space`), and the Pokémon stay in the game.
    NoRoom,
    /// The save in progress is settled as the answer chose.
    Settled,
}

/// The second number of `Fault::Delivery` for a delivery that did not
/// happen: a transport box that is no whole box, or one without Pokémon,
/// which the question task never hands over; and a journal or transport
/// file whose copy did not take the delivery for another reason than the
/// card's (`DeliverError::Write`). A write the card refused is the card's
/// own result.
pub const NOT_ONE_BOX: u32 = 1;
pub const NO_POKEMON: u32 = 2;
pub const NOT_WRITTEN: u32 = 3;
/// Whether Bank holds the delivery cannot be told
/// (`DeliverError::Unconfirmed`): the Pokémon stay in the game, and Bank may
/// hold them as well.
pub const UNCONFIRMED: u32 = 4;
/// Bank's journal no longer allows a delivery that the check at START
/// allowed. Bank holds none.
pub const REFUSED: u32 = 5;

/// The check's reply from what `transport::check` answered. `unreachable`
/// is the result of a file of Bank's that did not open for a reason Bank
/// cannot resolve (`bank_common::fs::Error::bank_resolves`): the error
/// screen, as Bank shows it for its files (`Fault::StorageOpen`). A journal
/// no power cut leaves is Bank's error too (`Fault::StorageInvalid`).
/// Everything else Bank resolves, and the refusal sends the user there.
pub fn check_reply(
    checked: Result<(), Refusal>,
    unreachable: Option<u32>,
) -> Result<Reply, WorkerError> {
    match (checked, unreachable) {
        (Ok(()), _) => Ok(Reply::Ready),
        (Err(Refusal::Damaged), _) => Err(Fault::StorageInvalid.with(0)),
        (Err(_), Some(result)) => Err(Fault::StorageOpen.with(result)),
        (Err(refusal), None) => Ok(Reply::Refused(refusal)),
    }
}

/// The delivery's reply from what `transport::deliver` answered, with the
/// card's results as their numbers, and `unreachable` as for `check_reply`.
/// A write the SD card had no room for is an ending of its own. Everything
/// else is the error screen: another write the SD card refused, a file that
/// did not take it, a journal that no longer allows it, and a delivery Bank
/// may hold. Bank's refusals are START's alone.
pub fn delivery_reply(
    delivered: Result<(), DeliverError<u32>>,
    unreachable: Option<u32>,
) -> Result<Reply, WorkerError> {
    match (delivered, unreachable) {
        (Ok(()), _) => Ok(Reply::Delivered),
        (Err(DeliverError::Unconfirmed), _) => Err(Fault::Delivery.with(UNCONFIRMED)),
        (Err(DeliverError::Refused(Refusal::Damaged)), _) => Err(Fault::StorageInvalid.with(0)),
        (Err(DeliverError::Refused(_)), Some(result)) => Err(Fault::StorageOpen.with(result)),
        (Err(DeliverError::Refused(_)), None) => Err(Fault::Delivery.with(REFUSED)),
        (Err(DeliverError::Io(result)), _) if lack_of_space(result) => Ok(Reply::NoRoom),
        (Err(DeliverError::Io(result)), _) => Err(Fault::Delivery.with(result)),
        (Err(DeliverError::Write), _) => Err(Fault::Delivery.with(NOT_WRITTEN)),
        (Err(DeliverError::Layout), _) => Err(Fault::Delivery.with(NOT_ONE_BOX)),
        (Err(DeliverError::Empty), _) => Err(Fault::Delivery.with(NO_POKEMON)),
    }
}

/// The answer's reply from what `transport::settle` answered, with the
/// card's result as the second number, and `unreachable` as for
/// `check_reply`. Every failure is the error screen of Bank's own failure
/// to settle a save (`Fault::JournalRecovery`), or of Bank's files that do
/// not open or a journal no power cut leaves, as Bank shows them. The save
/// then stays left to the question, for the next START, unless the first
/// record of the answer was written and only its read-back failed
/// (`Journal::settle`): then the answer may hold, and the next START of
/// either program finds which.
pub fn settle_reply(
    settled: Result<(), SettleError<u32>>,
    unreachable: Option<u32>,
) -> Result<Reply, WorkerError> {
    match (settled, unreachable) {
        (Ok(()), _) => Ok(Reply::Settled),
        (Err(SettleError::Refused(Refusal::Damaged)), _) => Err(Fault::StorageInvalid.with(0)),
        (Err(SettleError::Refused(_)), Some(result)) => Err(Fault::StorageOpen.with(result)),
        (Err(SettleError::Io(result)), _) => Err(Fault::JournalRecovery.with(result)),
        (Err(_), _) => Err(Fault::JournalRecovery.with(0)),
    }
}

#[cfg(target_arch = "arm")]
pub(crate) use arm::{TransporterRequests, TransporterWorker};

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::program::Transporter15;
    use bank_common::fs::{self, Visit};
    use offline_core::transport;
    use patch_common::{
        program::Program,
        worker::{Service, Shared, Worker},
    };

    /// Bank's files as the delivery rules reach them, through bank-common
    /// and Transporter's file calls (`bank_common::fs::Visit`): Transporter
    /// neither creates nor replaces a file, and a write to one that is not
    /// there is refused. Bank's next Save and Quit makes it.
    fn bank_files() -> Visit<Transporter15> {
        unsafe { Visit::new(Transporter15::fs_session()) }
    }

    /// What the worker thread runs.
    pub(crate) struct StorageWorker;
    static SHARED: Shared<StorageWorker> = Shared::new([0; STAGING_LEN]);
    /// The worker of the Transporter patch.
    pub(crate) type TransporterWorker = Worker<StorageWorker>;
    /// The worker and the job out, for the kind of job that submitted it.
    pub(crate) type TransporterRequests = patch_common::worker::Requests<StorageWorker, Job>;

    impl Service for StorageWorker {
        type Program = Transporter15;
        type Job = Job;
        type Reply = Reply;
        type Error = WorkerError;
        type Staging = [u8; STAGING_LEN];
        // The worker alone; this patch starts no other thread.
        const THREADS: i64 = 1;
        fn shared() -> &'static Shared<Self> {
            &SHARED
        }
        fn start() -> Self {
            Self
        }
        fn run(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            let mut files = bank_files();
            match job {
                Job::Check => {
                    let checked = transport::check(&mut files);
                    check_reply(checked, files.unreachable().map(fs::Error::diagnostic))
                }
                Job::Deliver => {
                    let (records, tags) = staging.split_at(RECORDS_LEN);
                    let delivered = match transport::deliver(&mut files, records, tags) {
                        Ok(_) => Ok(()),
                        Err(DeliverError::Io(error)) => Err(DeliverError::Io(error.diagnostic())),
                        Err(DeliverError::Layout) => Err(DeliverError::Layout),
                        Err(DeliverError::Empty) => Err(DeliverError::Empty),
                        Err(DeliverError::Refused(refusal)) => Err(DeliverError::Refused(refusal)),
                        Err(DeliverError::Write) => Err(DeliverError::Write),
                        Err(DeliverError::Unconfirmed) => Err(DeliverError::Unconfirmed),
                    };
                    delivery_reply(delivered, files.unreachable().map(fs::Error::diagnostic))
                }
                Job::Settle(decision) => {
                    let settled =
                        transport::settle(&mut files, decision).map_err(|error| match error {
                            SettleError::Io(error) => SettleError::Io(error.diagnostic()),
                            SettleError::Refused(refusal) => SettleError::Refused(refusal),
                            SettleError::Changed => SettleError::Changed,
                            SettleError::Write => SettleError::Write,
                        });
                    settle_reply(settled, files.unreachable().map(fs::Error::diagnostic))
                }
            }
        }
    }
}

/// One frame of a job, as the hooks ask for it: `None` while it runs.
pub type Answer = Option<Result<Reply, WorkerError>>;

/// A reply that is not one the job gives: the job held by the other kind,
/// or none.
const NOT_ITS_REPLY: WorkerError = Fault::NativeObject.with(0);

/// What the check at START leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Not answered yet.
    Pending,
    /// Bank can take a delivery: the search goes on.
    Go,
    /// Bank left its save in progress to the restore question: the search
    /// ends, and the restore task asks it.
    Ask,
    /// The original's message to show instead (`ui`).
    Refused(u32),
    /// The worker cannot be used: the error screen.
    Failed(WorkerError),
}

/// The check's answer.
pub fn checked(answer: Answer) -> Checked {
    match answer {
        None => Checked::Pending,
        Some(Ok(Reply::Ready)) => Checked::Go,
        Some(Ok(Reply::Refused(Refusal::Asked))) => Checked::Ask,
        Some(Ok(Reply::Refused(refusal))) => Checked::Refused(crate::ui::refusal_message(refusal)),
        Some(Ok(_)) => Checked::Failed(NOT_ITS_REPLY),
        Some(Err(error)) => Checked::Failed(error),
    }
}

/// What the delivery leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivered {
    /// Not answered yet.
    Pending,
    /// Bank holds it: the original removes the Pokémon from the game.
    Done,
    /// Bank holds nothing, for certain, for lack of room on the SD card: the
    /// transfer's failure message, in the place of which "The SD card is
    /// full." comes (`ui::shown`).
    NoRoom,
    /// The worker cannot be used, or the delivery failed otherwise or may
    /// have happened: the error screen.
    Failed(WorkerError),
}

/// The delivery's answer: done only for a delivery Bank holds.
pub fn delivered(answer: Answer) -> Delivered {
    match answer {
        None => Delivered::Pending,
        Some(Ok(Reply::Delivered)) => Delivered::Done,
        Some(Ok(Reply::NoRoom)) => Delivered::NoRoom,
        Some(Ok(_)) => Delivered::Failed(NOT_ITS_REPLY),
        Some(Err(error)) => Delivered::Failed(error),
    }
}

/// What the answer to the restore question leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    /// Not answered yet.
    Pending,
    /// Settled: the loading screen ends once it has stood, and the
    /// disconnect task follows.
    Done,
    /// The error screen.
    Failed(WorkerError),
}

/// The answer's reply.
pub fn settled(answer: Answer) -> Settled {
    match answer {
        None => Settled::Pending,
        Some(Ok(Reply::Settled)) => Settled::Done,
        Some(Ok(_)) => Settled::Failed(NOT_ITS_REPLY),
        Some(Err(error)) => Settled::Failed(error),
    }
}

/// The delivery entry's answers, as its stub tests them. Only `DONE` lets
/// the original go on and take the Pokémon out of the game; `PENDING` comes
/// back next frame, which is also how the transfer waits on the error
/// screen.
pub const DELIVER_FAILED: u32 = 0;
pub const DELIVER_DONE: u32 = 1;
pub const DELIVER_PENDING: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use patch_common::worker::{Control, ControlKind};

    /// Every answer a frame of a job can give.
    fn answers() -> [Answer; 8] {
        [
            None,
            Some(Ok(Reply::Ready)),
            Some(Ok(Reply::Refused(Refusal::Occupied))),
            Some(Ok(Reply::Refused(Refusal::NotReady))),
            Some(Ok(Reply::Delivered)),
            Some(Ok(Reply::NoRoom)),
            Some(Err(WorkerError::from(Control::new(ControlKind::Resource)))),
            Some(Err(Fault::Delivery.with(UNCONFIRMED))),
        ]
    }

    #[test]
    fn only_a_delivery_bank_holds_lets_the_pokemon_leave_the_game() {
        for answer in answers() {
            let expected = match answer {
                None => Delivered::Pending,
                Some(Ok(Reply::Delivered)) => Delivered::Done,
                // Nothing is in Bank: the SD card is full.
                Some(Ok(Reply::NoRoom)) => Delivered::NoRoom,
                // Everything else is the error screen, a delivery Bank may
                // hold too: the Pokémon stay in the game.
                Some(Err(error)) => Delivered::Failed(error),
                // A reply of the check is none of the delivery's.
                Some(Ok(_)) => Delivered::Failed(NOT_ITS_REPLY),
            };
            assert_eq!(delivered(answer), expected, "{answer:?}");
        }
    }

    #[test]
    fn the_search_goes_on_only_when_bank_can_take_a_delivery() {
        for answer in answers() {
            let result = checked(answer);
            match answer {
                None => assert_eq!(result, Checked::Pending),
                Some(Ok(Reply::Ready)) => assert_eq!(result, Checked::Go),
                Some(Ok(Reply::Refused(Refusal::Occupied))) => {
                    assert_eq!(result, Checked::Refused(crate::ui::MESSAGE_BOX_NOT_EMPTY))
                }
                Some(Ok(Reply::Refused(Refusal::NotReady))) => {
                    assert_eq!(result, Checked::Refused(crate::ui::MESSAGE_OPEN_BANK))
                }
                // A worker that fails is the error screen, with its numbers.
                Some(Err(error)) => assert_eq!(result, Checked::Failed(error)),
                Some(Ok(_)) => assert_eq!(result, Checked::Failed(NOT_ITS_REPLY)),
            }
        }
    }

    #[test]
    fn a_worker_that_cannot_be_used_shows_the_numbers_bank_shows_for_it() {
        let error = WorkerError::from(Control::new(ControlKind::Resource));
        assert_eq!(error.fault(), Fault::NativeObject);
        let Checked::Failed(shown) = checked(Some(Err(error))) else {
            panic!("not the error screen");
        };
        assert_eq!(shown.numbers(), error.numbers());
        // A box that is no box is the delivery's own number.
        assert_eq!(
            Fault::Delivery.with(NOT_ONE_BOX).numbers(),
            [0x14, NOT_ONE_BOX]
        );
    }

    #[test]
    fn bank_is_named_only_at_start_and_everything_else_shows_the_numbers() {
        for refusal in [Refusal::Occupied, Refusal::NotReady] {
            // At START the refusal sends the user to Bank.
            assert_eq!(check_reply(Err(refusal), None), Ok(Reply::Refused(refusal)));
            // At the delivery it is an error: START allowed it.
            assert_eq!(
                delivery_reply(Err(DeliverError::Refused(refusal)), None),
                Err(Fault::Delivery.with(REFUSED))
            );
            // A file that did not open for another reason: Bank's number.
            let error = Fault::StorageOpen.with(0xd900_458b);
            assert_eq!(check_reply(Err(refusal), Some(0xd900_458b)), Err(error));
            assert_eq!(
                delivery_reply(Err(DeliverError::Refused(refusal)), Some(0xd900_458b)),
                Err(error)
            );
        }
        // A journal no power cut leaves, as Bank shows it.
        let damaged = Err(Fault::StorageInvalid.with(0));
        assert_eq!(check_reply(Err(Refusal::Damaged), None), damaged);
        assert_eq!(
            delivery_reply(Err(DeliverError::Refused(Refusal::Damaged)), None),
            damaged
        );
        assert_eq!(check_reply(Ok(()), Some(1)), Ok(Reply::Ready));
        // A delivery the card did not take, or one Bank may hold: the
        // delivery's number.
        for (error, second) in [
            (DeliverError::Io(0xd900_458b), 0xd900_458b),
            (DeliverError::Write, NOT_WRITTEN),
            (DeliverError::Layout, NOT_ONE_BOX),
            (DeliverError::Empty, NO_POKEMON),
            (DeliverError::Unconfirmed, UNCONFIRMED),
        ] {
            assert_eq!(
                delivery_reply(Err(error), None),
                Err(Fault::Delivery.with(second))
            );
        }
        // One the card had no room for: said so, without numbers.
        assert_eq!(
            delivery_reply(Err(DeliverError::Io(0xc86044d2)), None),
            Ok(Reply::NoRoom)
        );
        assert_eq!(delivery_reply(Ok(()), None), Ok(Reply::Delivered));
    }

    #[test]
    fn a_save_left_to_the_question_ends_the_search_and_its_answer_settles_it() {
        assert_eq!(
            check_reply(Err(Refusal::Asked), None),
            Ok(Reply::Refused(Refusal::Asked))
        );
        assert_eq!(
            checked(Some(Ok(Reply::Refused(Refusal::Asked)))),
            Checked::Ask
        );
        // Bank's files that do not open are Bank's error, as for the check.
        assert_eq!(
            check_reply(Err(Refusal::Asked), Some(7)),
            Err(Fault::StorageOpen.with(7))
        );
        assert_eq!(settle_reply(Ok(()), None), Ok(Reply::Settled));
        assert_eq!(settled(Some(Ok(Reply::Settled))), Settled::Done);
        assert_eq!(settled(None), Settled::Pending);
        // A reply of another job is none of the answer's.
        assert_eq!(
            settled(Some(Ok(Reply::Ready))),
            Settled::Failed(NOT_ITS_REPLY)
        );
        // Every failure is the error screen; the save stays for the next
        // START.
        for (error, unreachable, expected) in [
            (
                SettleError::Io(0xd900_458b),
                None,
                Fault::JournalRecovery.with(0xd900_458b),
            ),
            (SettleError::Changed, None, Fault::JournalRecovery.with(0)),
            (SettleError::Write, None, Fault::JournalRecovery.with(0)),
            (
                SettleError::Refused(Refusal::Damaged),
                None,
                Fault::StorageInvalid.with(0),
            ),
            (
                SettleError::Refused(Refusal::NotReady),
                Some(7),
                Fault::StorageOpen.with(7),
            ),
            (
                SettleError::Refused(Refusal::NotReady),
                None,
                Fault::JournalRecovery.with(0),
            ),
        ] {
            assert_eq!(settle_reply(Err(error), unreachable), Err(expected));
            assert_eq!(
                settled(Some(settle_reply(Err(error), unreachable))),
                Settled::Failed(expected)
            );
        }
    }

    #[test]
    fn the_staging_buffer_holds_one_transport_box() {
        assert_eq!(STAGING_LEN, 6_990);
    }
}
