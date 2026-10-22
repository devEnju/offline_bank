//! The worker of the Transporter patch (`patch_common::worker`): the check
//! and the delivery, on Bank's files as the delivery rules reach them. The
//! main thread hands the delivery the transport box in the staging buffer;
//! the worker touches no object of the original.

use crate::task::RECORDS_LEN;
use offline_core::{sections::TRANSPORT_SLOTS, transport::Refusal};
use patch_common::worker::Control;

/// The staging buffer: the transport box's records, then its tags.
pub const STAGING_LEN: usize = RECORDS_LEN + TRANSPORT_SLOTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// May Bank take a delivery?
    Check,
    /// Deliver the transport box in the staging buffer.
    Deliver,
}
/// What a job found. Which message the screen shows for it is the hooks'
/// (`ui::refusal_message`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Bank can take a delivery.
    Ready,
    /// Bank cannot take one now.
    Refused(Refusal),
    /// The delivery is written, flushed and read back.
    Delivered,
    /// Nothing was delivered.
    NotDelivered,
}

/// A job that has no answer: the worker failed. Transporter shows no
/// numbers, so what failed does not matter: the check refuses and the
/// delivery has not happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failed;
impl From<Control> for Failed {
    fn from(_: Control) -> Self {
        Self
    }
}

#[cfg(target_arch = "arm")]
pub(crate) use arm::TransporterWorker;

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::program::Transporter15;
    use bank_common::fs::Visit;
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

    impl Service for StorageWorker {
        type Program = Transporter15;
        type Job = Job;
        type Reply = Reply;
        type Error = Failed;
        type Staging = [u8; STAGING_LEN];
        // The worker alone; this patch starts no other thread.
        const THREADS: i64 = 1;
        fn shared() -> &'static Shared<Self> {
            &SHARED
        }
        fn start() -> Self {
            Self
        }
        fn run(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, Failed> {
            Ok(match job {
                Job::Check => match transport::check(&mut bank_files()) {
                    Ok(()) => Reply::Ready,
                    Err(refusal) => Reply::Refused(refusal),
                },
                Job::Deliver => {
                    let (records, tags) = staging.split_at(RECORDS_LEN);
                    match transport::deliver(&mut bank_files(), records, tags) {
                        Ok(_) => Reply::Delivered,
                        Err(_) => Reply::NotDelivered,
                    }
                }
            })
        }
    }
}

/// One frame of a job, as the hooks ask for it: `None` while it runs.
pub type Answer = Option<Result<Reply, Failed>>;

/// What the check at START leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Not answered yet.
    Pending,
    /// Bank can take a delivery: the search goes on.
    Go,
    /// The original's message to show instead (`ui`).
    Refused(u32),
}

/// The check's answer. A worker that failed refuses as Bank's files that
/// do not read do.
pub fn checked(answer: Answer) -> Checked {
    match answer {
        None => Checked::Pending,
        Some(Ok(Reply::Ready)) => Checked::Go,
        Some(Ok(Reply::Refused(refusal))) => Checked::Refused(crate::ui::refusal_message(refusal)),
        Some(_) => Checked::Refused(crate::ui::MESSAGE_OPEN_BANK),
    }
}

/// The delivery entry's answers, as its stub tests them. Only `DONE` lets
/// the original go on and take the Pokémon out of the game.
pub const DELIVER_FAILED: u32 = 0;
pub const DELIVER_DONE: u32 = 1;
pub const DELIVER_PENDING: u32 = 2;

/// The delivery's answer: done only for a delivery that is written,
/// flushed and read back.
pub fn delivered(answer: Answer) -> u32 {
    match answer {
        None => DELIVER_PENDING,
        Some(Ok(Reply::Delivered)) => DELIVER_DONE,
        Some(_) => DELIVER_FAILED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every answer a frame of a job can give.
    fn answers() -> [Answer; 7] {
        [
            None,
            Some(Ok(Reply::Ready)),
            Some(Ok(Reply::Refused(Refusal::Occupied))),
            Some(Ok(Reply::Refused(Refusal::NotReady))),
            Some(Ok(Reply::Delivered)),
            Some(Ok(Reply::NotDelivered)),
            Some(Err(Failed)),
        ]
    }

    #[test]
    fn only_a_delivery_that_is_written_lets_the_pokemon_leave_the_game() {
        for answer in answers() {
            let expected = match answer {
                None => DELIVER_PENDING,
                Some(Ok(Reply::Delivered)) => DELIVER_DONE,
                _ => DELIVER_FAILED,
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
                // Every other refusal, and a worker that failed, asks to
                // open Bank.
                _ => assert_eq!(
                    result,
                    Checked::Refused(crate::ui::MESSAGE_OPEN_BANK),
                    "{answer:?}"
                ),
            }
        }
    }

    #[test]
    fn the_staging_buffer_holds_one_transport_box() {
        assert_eq!(STAGING_LEN, 6_990);
    }

    #[test]
    fn every_worker_failure_is_one_answer() {
        use patch_common::worker::ControlKind;
        for kind in [
            ControlKind::Busy,
            ControlKind::StaleJob,
            ControlKind::Resource,
            ControlKind::Stopped,
            ControlKind::NoJob,
        ] {
            assert_eq!(Failed::from(Control::new(kind)), Failed);
        }
    }
}
