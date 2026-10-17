//! The Bank as releases v0.3.0 to v0.4.0 stored it (tag v0.4.0, 42b00b4).
//!
//! Ten files: the journal and the boxes, the Pokédex, the transport box and
//! a rewards record, each twice. The modules below are that release's
//! storage code, frozen: `offline-core`'s and `bank-common`'s as it has them,
//! without their tests and without what only a running Bank needs. They read
//! its files with the code that wrote them, whatever the shared crates
//! become. What writes is kept as that release has it, for the tests, which
//! make a Bank of that release with it. The containers are v0.2.1's too, so
//! `v0_2_1` reads with them as well.
//!
//! Its rewards record means what v0.4.0 reads in it (`rewards`). A v0.3.x
//! Bank, in the same files, is read as v0.4.0 read one.
#![allow(dead_code, unused_imports, clippy::enum_variant_names)]

pub mod bank_files;
pub mod rewards;
pub mod sections;
pub mod session;
pub mod sidecar;
pub mod transport;

mod format;
mod store;

/// What has not changed since and is shared with the current code.
pub use offline_core::{crc32, Storage};
pub mod checksum {
    pub use offline_core::Crc32;
}
pub mod native_blob {
    pub use offline_core::native_blob::BLOB_SIZE;
}

pub use format::{
    decide_recovery, Fingerprint, FormatError, GameIdentity, GameObservation, Metadata,
    PendingTransfer, Phase, RecoveryBlock, RecoveryDecision, Slot, SnapshotHeader, SnapshotRef,
    METADATA_SIZE, SCHEMA_VERSION, SNAPSHOT_HEADER_SIZE,
};
pub use store::{BankStore, Head, Layout, Reconciled, StoreError};

use crate::migrate::{Card, Cause, Earlier, Guarded, Step, Stopped};
use bank_common::bank_files::UnitFile;
use bank_files::{BankFiles, FileName, Files, Loaded};
use offline_core::native_blob::{clear_unused, FRACTION_OFFSET};

/// Its files, the Bank last.
pub(crate) fn files() -> [&'static [UnitFile]; 4] {
    [
        FileName::Dex,
        FileName::Rewards,
        FileName::Transport,
        FileName::Bank,
    ]
    .map(FileName::units)
}

/// Whether such a Bank is there: one of its journal records, which have
/// another size than the current ones. Its other files say nothing: some it
/// can do without, and the rest have the names of current ones.
pub(crate) fn there<C: Card>(card: &mut C) -> Result<bool, C::Error> {
    let records = &FileName::Bank.units()[..2];
    Ok(card.exists(&records[..1])? || card.exists(&records[1..])?)
}

pub(crate) fn read<C: Card>(card: &mut C, body: &mut [u8]) -> Result<(), Stopped<C::Error>> {
    let loaded = settled(Earlier::new(card, FileName::units), body)?;
    into_body(body, loaded.rewards);
    Ok(())
}

/// Gives the body the unused places as a new Bank has them
/// (`native_blob::clear_unused`): nothing the server put into a downloaded
/// Bank, and no day of a redemption. Of the record the remainder goes into
/// the body, as a u16. The balance is already there: v0.4.0's load adds the
/// one v0.3.1 kept in the record (`rewards::balance_beside`).
pub(crate) fn into_body(body: &mut [u8], record: rewards::Record) {
    clear_unused(body);
    body[FRACTION_OFFSET..FRACTION_OFFSET + 2].copy_from_slice(&record.fraction.to_le_bytes());
}

/// Loads the Bank in `files` with this code into `body`, as one native body,
/// and requires it at rest: a save in progress, or a Pokédex or transport
/// box that the release would fill at its next save, are that release's to
/// settle. A waiting delivery is taken, as a load takes it. Nothing is
/// written.
pub(crate) fn settled<E, F>(files: F, body: &mut [u8]) -> Result<Loaded, Stopped<E>>
where
    F: Files,
    F::Storage: Storage<Error = Guarded<E>>,
{
    let read = Step::ReadOld;
    let mut bank = BankFiles::new(files);
    match bank.open().map_err(unread(read))? {
        Some(Phase::Clean(_)) => {}
        Some(Phase::Prepared(_)) => return Err(Stopped::InProgress),
        None => return Err(Stopped::Failed(read, Cause::Damaged)),
    }
    let loaded = bank.read(body).map_err(unread(read))?;
    if loaded.dex_missing || loaded.transport_missing {
        return Err(Stopped::Unsettled);
    }
    Ok(loaded)
}

/// Why this code could not read the Bank.
fn unread<E>(step: Step) -> impl Fn(bank_files::Error<Guarded<E>>) -> Stopped<E> {
    move |error| {
        Stopped::Failed(
            step,
            match error {
                bank_files::Error::Io(Guarded::Io(error))
                | bank_files::Error::Bank(session::Error::Storage(StoreError::Io(Guarded::Io(
                    error,
                )))) => Cause::Io(error),
                _ => Cause::Damaged,
            },
        )
    }
}
