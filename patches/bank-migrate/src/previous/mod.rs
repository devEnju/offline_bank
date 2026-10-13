//! The storage code of the previous release, frozen.
//!
//! These files are `offline-core`'s and `bank-common`'s as that release has
//! them, without their tests and without what only a running Bank needs.
//! They are here so that the migration reads the previous release's files
//! with the code that wrote them, whatever the shared crates become. Nothing
//! outside this crate uses them, and they go when the migration goes.

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
