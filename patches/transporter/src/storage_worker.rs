//! The worker of the Transporter patch (`patch_common::worker`): the check
//! and the delivery, on Bank's files as the delivery rules reach them. The
//! main thread hands the delivery the transport box in the staging buffer;
//! the worker touches no object of the original.

use crate::layout::RECORDS_LEN;
use offline_core::sections::TRANSPORT_SLOTS;
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
/// The check's answer is the message to show, or 0 for none; the
/// delivery's is 1 when it is written.
pub type Reply = u32;

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
pub use arm::{DeliveryWorker, Files};

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::{fs::FS_SESSION, layout::refusal_message, program::Transporter15};
    use bank_common::fs::Visit;
    use offline_core::transport;
    use patch_common::worker::{Service, Shared, Worker};

    /// Bank's files as the delivery rules reach them, through bank-common
    /// and Transporter's file calls (`bank_common::fs::Visit`): Transporter
    /// neither creates nor replaces a file, and a write to one that is not
    /// there is refused. Bank's next Save and Quit makes it.
    fn bank_files() -> Visit<Transporter15> {
        unsafe { Visit::new(FS_SESSION.read()) }
    }

    /// What the worker thread runs.
    pub struct Files;
    static SHARED: Shared<Files> = Shared::new([0; STAGING_LEN]);
    /// The worker of the Transporter patch.
    pub type DeliveryWorker = Worker<Files>;

    impl Service for Files {
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
                Job::Check => transport::check(&mut bank_files())
                    .err()
                    .map_or(0, refusal_message),
                Job::Deliver => {
                    let (records, tags) = staging.split_at(RECORDS_LEN);
                    u32::from(transport::deliver(&mut bank_files(), records, tags).is_ok())
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
