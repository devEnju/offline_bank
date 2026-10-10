#![no_std]
//! What every patch for Bank 1.5 shares: the start-up hook, Bank's files in
//! its extdata and the order they are written in, the worker thread, and the
//! original's task objects and dialogs. Nothing here changes what Bank does;
//! a patch adds that (docs/internals.md, "Patches for Bank").
//!
//! A patch writes Bank's files only through this crate, so every patch
//! leaves files that every other one reads.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! The builder must verify the complete input before emitting any patch.

pub mod bank_files;
pub mod bootstrap;
pub mod fs;
pub mod session;
#[cfg(target_arch = "arm")]
pub mod task;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(target_arch = "arm")]
pub mod ui;
pub mod worker;

#[cfg(any(test, target_arch = "arm"))]
mod kernel;
