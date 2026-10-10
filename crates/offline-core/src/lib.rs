#![no_std]
#![forbid(unsafe_code)]
//! Local storage snapshots/journal plus bounded views of the reviewed native Bank blob.
//!
//! The storage payload is opaque; `native_blob` exposes only verified layout fields. The runtime must supply a verified Bank codec, an empty
//! Bank initializer, a filesystem adapter, and fingerprints of the complete game
//! save transaction. This crate neither patches Bank nor edits game saves.
//!
//! # Container format (version 1)
//!
//! All integers are little endian. A preallocated file contains two 192-byte
//! metadata records, followed by two snapshot slots. Each snapshot slot contains
//! a 32-byte header and `Layout::capacity()` payload bytes. Unused payload bytes
//! are ignored. Capacity is a caller-supplied, version-pinned configuration.
//!
//! Snapshot header: magic `BKOFSNAP` at 0; u16 version at 8; u16 header size at 10;
//! u32 payload length at 12; u64 generation at 16; u32 payload CRC at 24;
//! u32 CRC of the preceding 28 header bytes at 28.
//!
//! Metadata: magic `BKOFMETA` at 0; u16 version at 8; u16 record size at 10;
//! u8 state (1 clean, 2 prepared) at 12; reserved zero bytes 13..16;
//! u64 sequence at 16; current snapshot reference at 24; next reference at 48;
//! u64 game title ID at 72; 32-byte stable save identity at 80;
//! 32-byte before fingerprint at 112; 32-byte after fingerprint at 144;
//! reserved zero bytes 176..184; u32 slot capacity at 184; u32 CRC of bytes 0..188 at 188.
//! A 24-byte snapshot reference stores slot at 0, three zero bytes, u32 length
//! at 4, u64 generation at 8, u32 payload CRC at 16, four zero bytes.
//! Clean metadata has all bytes 48..184 zero. CRC is CRC-32/ISO-HDLC, intended
//! to detect accidental damage, not malicious edits. Fingerprints are opaque
//! 32-byte values; use a cryptographic digest of the complete relevant save.
//!
//! The Bank runtime stores the Bank regions of the native body as the payload
//! (`sections`). The Pokédex, transport box, and Miles record live in tagged
//! two-slot side files that follow this journal (`sidecar`).
//!
//! # Durable protocol
//!
//! A new snapshot is written only to the inactive slot and synchronized first.
//! Metadata is then written to the older/alternate replica and synchronized,
//! followed by its other replica and another synchronization. Both replicas have
//! the same sequence. The highest valid sequence controls visibility; a newer
//! snapshot by itself is never committed.
//!
//! For a transfer: verify the game's before state, call `prepare_transfer`, and
//! mutate the game ONLY after that call succeeds. Then read the game back and
//! call `reconcile`. A matching before state aborts to the old snapshot; a
//! matching after state commits the new one. Missing, different, or modified
//! saves block recovery here; for a modified save, and for a missing one if
//! the transfer moved Pokémon one way only, the runtime chooses the snapshot
//! itself, from what the transfer moved (`moved`), and names that image to
//! `reconcile`. Startup must reconcile prepared transactions before allowing
//! any Bank or game edits.
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

pub mod game_image;
pub mod moved;
pub mod native_blob;
pub mod rewards;
pub mod sections;
pub mod sidecar;
pub mod transport;

mod checksum;
mod format;
mod hash;
mod store;

pub use checksum::crc32;
pub use format::{
    decide_recovery, Fingerprint, FormatError, GameIdentity, GameObservation, Metadata,
    PendingTransfer, Phase, RecoveryBlock, RecoveryDecision, Slot, SnapshotHeader, SnapshotRef,
    METADATA_SIZE, SCHEMA_VERSION, SNAPSHOT_HEADER_SIZE,
};
pub use hash::{Sha256, Sha256Error};
pub use store::{BankStore, Head, Layout, Reconciled, Storage, StoreError};
