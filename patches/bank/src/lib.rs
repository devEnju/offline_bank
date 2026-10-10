#![no_std]
//! Native Bank 1.5 adapters and offline runtime hooks.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! The builder must verify the complete input before emitting any patch.

pub mod bank_files;
pub mod fs;
pub mod local_clock;
#[cfg(feature = "migrate")]
pub mod migrate;
pub mod native_bank;
pub mod navigation;
pub mod session;

pub mod native_game;

#[cfg(target_arch = "arm")]
pub mod runtime;

pub mod bootstrap;

pub mod transaction;

#[cfg(target_arch = "arm")]
pub mod ui;

pub mod dex;
pub mod storage_worker;
pub mod worker;

#[cfg(any(test, target_arch = "arm"))]
mod kernel;
