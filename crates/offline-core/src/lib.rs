#![no_std]
#![forbid(unsafe_code)]
//! The journal and the stored files of the offline Bank, plus bounded views of
//! the reviewed native Bank blob.
//!
//! What is stored is opaque here; `native_blob` exposes only verified layout
//! fields. The runtime must supply a verified Bank codec, an empty Bank
//! initializer, a filesystem adapter, and fingerprints of the complete game
//! save transaction. This crate neither patches Bank nor edits game saves.
//!
//! # Stored format (version 1: the major number of this crate's version)
//!
//! The Bank is the native body cut into three files (`sections`): the Bank
//! file with the boxes, the main file, and the transport box. Each exists in
//! two copies, and a copy is its data and nothing else. The journal ties them
//! together: one 140-byte record, kept twice, that names the copy of each
//! file that belongs to the current save and that copy's checksum, a save in
//! progress with the same for it and with what identifies the game it is made
//! with, and a delivery from Transporter that waits (`format::Metadata` has
//! the layout). A copy belongs to a save only if its bytes have the checksum
//! the record names. CRC is CRC-32/ISO-HDLC, intended to detect accidental
//! damage, not malicious edits. Fingerprints are opaque 32-byte values; use a
//! cryptographic digest of the complete relevant save.
//!
//! `store` holds the journal and the two copies of the Bank file as one
//! preallocated container: the two records, then the two copies. The main
//! file and the transport box are a `pair` each. `transport` has the rules by
//! which Transporter hands a box over.
//!
//! # Durable protocol
//!
//! New content is written only to a copy that holds nothing still needed and
//! synchronized first: the copy that is not in use, or, for a save beside a
//! waiting delivery, the empty transport box in use (`transport`). The
//! record is then written to the replica that was not selected and
//! synchronized, followed by the other replica and another synchronization.
//! A completed publication leaves both replicas with the same sequence. The
//! highest valid sequence controls visibility; a newer copy by itself is
//! never current.
//!
//! For a transfer: verify the game's before state, call `prepare_transfer`, and
//! mutate the game ONLY after that call succeeds. Then read the game back and
//! call `reconcile` with its fingerprint. The before image keeps the save
//! before; the after image makes the new one current; any other is refused.
//! For a game whose save is neither image, or that is not there, the runtime
//! chooses the save itself, from what the transfer moved (`moved`), and names
//! that image to `reconcile`. Startup must reconcile prepared transactions
//! before allowing any Bank or game edits.
//!
//! The adapter must implement exact reads/writes and durable `sync`. After ANY
//! I/O error, stop the operation, reopen storage and inspect it before continuing.
//! No method writes the game. One exclusive writer is required. Preallocation,
//! extdata quotas, backup/migration, and power-loss behavior of real 3DS filesystem
//! calls must be validated by the runtime. Fresh storage must be explicitly
//! initialized; corrupt or unsupported storage is never treated as empty.
//!
//! Recovery covers prepared transactions. Clean metadata contains no historical
//! game receipt, so this crate cannot detect independently restored completed
//! Bank/game backups. The runtime must require paired restores or implement a
//! separately verified native secure-value/backup policy. A complete rollback of
//! all local records also needs an external monotonic reference to be detectable.

pub mod files;
pub mod game_image;
pub mod moved;
pub mod native_blob;
pub mod pair;
pub mod rewards;
pub mod sections;
pub mod transport;

mod checksum;
mod format;
mod hash;
mod store;

pub use checksum::{crc32, Crc32};
pub use format::{
    decide_recovery, Fingerprint, FormatError, GameIdentity, Metadata, Part, PendingTransfer,
    Phase, RecoveryDecision, Save, Sides, Slot, GAME_TITLES, METADATA_SIZE, SCHEMA_VERSION,
};
pub use hash::{Sha256, Sha256Error};
pub use store::{may_deliver, BankStore, Head, Journal, Layout, Reconciled, Storage, StoreError};
