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
    use crate::{
        fs::{terminated, Archive, File, OPEN_READ_WRITE},
        layout::refusal_message,
    };
    use offline_core::{files, transport, Storage, METADATA_SIZE};
    use patch_common::worker::{Service, Shared, Worker};

    /// A pair of files in Bank's extdata, open for reading and writing and
    /// addressed as one range: the two journal records, or the two copies
    /// of the transport box. Bank keeps each in a file of its own, because
    /// the console leaves a file that was being written unreadable when the
    /// power fails.
    ///
    /// A file that is missing or cannot be opened makes every read of it
    /// fail, and so the check and the delivery refuse: Transporter neither
    /// creates nor replaces a file. Bank's next Save and Quit does.
    struct Pair {
        files: [Option<File>; 2],
        written: [bool; 2],
        /// Length of each of the two files.
        len: u64,
    }

    /// Bank's files as the delivery rules reach them (`transport::Files`):
    /// one pair at a time, so that never more than two files are open.
    struct BankFiles;
    impl transport::Files for BankFiles {
        type Storage = Pair;
        fn journal(&mut self) -> Option<Pair> {
            unsafe { Pair::open(files::JOURNAL, METADATA_SIZE as u64) }
        }
        fn mover(&mut self) -> Option<Pair> {
            unsafe { Pair::open(files::MOVER, u64::from(transport::LEN)) }
        }
    }
    impl Pair {
        /// `None` when Bank's extdata or both files do not exist, access is
        /// refused, or a file is not exactly `len` bytes, the size Bank
        /// creates.
        unsafe fn open(names: [&str; 2], len: u64) -> Option<Self> {
            // Whatever was opened is closed again if this ends early.
            let mut pair = Self {
                files: [None, None],
                written: [false; 2],
                len,
            };
            for (file, name) in pair.files.iter_mut().zip(names) {
                let (path, length) = terminated(name);
                *file =
                    unsafe { File::open(Archive::BankExtdata, &path[..length], OPEN_READ_WRITE) };
                // One of another size is not a file of this layout.
                if file.as_ref().is_some_and(|file| file.size() != Some(len)) {
                    return None;
                }
            }
            pair.files.iter().any(Option::is_some).then_some(pair)
        }
        /// The file that holds `offset..offset + length`, and where in it.
        fn locate(&self, offset: u64, length: usize) -> Result<(usize, &File, u64), ()> {
            let index = (offset / self.len) as usize;
            let within = offset % self.len;
            if index >= self.files.len() || within + length as u64 > self.len {
                return Err(());
            }
            Ok((index, self.files[index].as_ref().ok_or(())?, within))
        }
    }
    impl Storage for Pair {
        type Error = ();
        fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), ()> {
            let (_, file, within) = self.locate(offset, bytes.len())?;
            file.read(within, bytes.as_mut_ptr(), bytes.len() as u32)
                .then_some(())
                .ok_or(())
        }
        fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), ()> {
            let (index, file, within) = self.locate(offset, bytes.len())?;
            let written = file.write(within, bytes.as_ptr(), bytes.len() as u32, 0);
            self.written[index] = true;
            written.then_some(()).ok_or(())
        }
        /// Only a file that was written is flushed: the one that holds
        /// Bank's own box is never sent a write of any kind.
        fn sync(&mut self) -> Result<(), ()> {
            for (file, written) in self.files.iter().zip(&mut self.written) {
                if let (Some(file), true) = (file, *written) {
                    if !file.flush() {
                        return Err(());
                    }
                    *written = false;
                }
            }
            Ok(())
        }
        /// Transporter replaces no file of Bank's.
        fn recreate(&mut self, _: u64, _: u64) -> Result<(), ()> {
            Err(())
        }
    }

    /// What the worker thread runs.
    pub struct Files;
    static SHARED: Shared<Files> = Shared::new([0; STAGING_LEN]);
    /// The worker of the Transporter patch.
    pub type DeliveryWorker = Worker<Files>;

    impl Service for Files {
        type Program = crate::program::Transporter15;
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
                Job::Check => transport::check(&mut BankFiles)
                    .err()
                    .map_or(0, refusal_message),
                Job::Deliver => {
                    let (records, tags) = staging.split_at(RECORDS_LEN);
                    u32::from(transport::deliver(&mut BankFiles, records, tags).is_ok())
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
