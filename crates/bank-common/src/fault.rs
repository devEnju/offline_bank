//! The first of the two numbers on the error screen of Bank and of
//! Transporter: what could not go on. Every patch of either program shows
//! the same number for the same thing; the second number says why
//! (docs/bank.md, "Troubleshooting").

use crate::{bank_files, fs, session, worker::WorkerError};
use offline_core::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Fault {
    None = 0,
    /// A job was asked for while one was out, or collected without one.
    Reentrant = 1,
    /// An object of the original is not what a hook expects, or the worker
    /// could not be used.
    NativeObject = 2,
    StorageOpen = 3,
    StorageCreate = 4,
    StorageInvalid = 5,
    Clock = 6,
    StorageLoad = 9,
    GameRead = 10,
    GameChanged = 11,
    GamePrepare = 12,
    GameWrite = 13,
    GameSecureValue = 14,
    JournalPrepare = 15,
    JournalRecovery = 16,
    RewardState = 18,
    RewardClaim = 19,
    /// Transporter: the transport box handed to the delivery is no whole
    /// box that holds Pokémon (`transporter_payload::storage_worker`).
    Delivery = 20,
    /// Test builds: a Save and Quit stopped on purpose by the chosen
    /// rehearsal (`rehearsal`). Its second number is the stop point: 1
    /// after the journal is prepared and before the game is written, 2
    /// after the game is written and before the journal is resolved.
    #[cfg(feature = "test-build")]
    TestStop = 0x7e57,
}

impl Fault {
    /// A job that failed for `native`, the second number.
    pub const fn with(self, native: u32) -> WorkerError {
        WorkerError::operation(self, native)
    }
    /// A file operation that failed: the console's result, or a code of the
    /// file layer (`fs::Error::diagnostic`).
    pub fn io(self, error: fs::Error) -> WorkerError {
        self.with(error.diagnostic())
    }
    /// Bank's files: an operation that failed as `io`, a file that is not
    /// what the journal says as `7A00ffpp`, with `ff` the file (0 Bank,
    /// 1 main, 2 transport) and `pp` the `bank_files::Problem`, and anything
    /// else of the journal as 0.
    pub fn files(self, error: bank_files::Error<fs::Error>) -> WorkerError {
        match error {
            bank_files::Error::Io(error)
            | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(error))) => {
                self.io(error)
            }
            bank_files::Error::Bank(_) => self.with(0),
            bank_files::Error::Side(file, problem) => {
                self.with(0x7a00_0000 | (file as u32) << 8 | problem as u32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank_files::{FileName, Problem};

    #[test]
    fn the_numbers_of_a_failed_file_operation() {
        let native = fs::Error::Native {
            operation: fs::Operation::Read,
            code: 0xd900_458b,
        };
        for error in [
            bank_files::Error::Io(native),
            bank_files::Error::Bank(session::Error::Storage(StoreError::Io(native))),
        ] {
            let error = Fault::StorageLoad.files(error);
            assert_eq!(
                (error.fault(), error.native_result()),
                (Fault::StorageLoad, 0xd900_458b)
            );
        }
        let error = Fault::StorageLoad.files(bank_files::Error::Bank(session::Error::Poisoned));
        assert_eq!(error.native_result(), 0);
        let error = Fault::StorageLoad.files(bank_files::Error::Side(
            FileName::Transport,
            Problem::Conflict,
        ));
        assert_eq!(error.native_result(), 0x7a00_0203);
    }
}
