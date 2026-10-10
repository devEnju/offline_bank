#![no_std]
//! The offline patch for Bank 1.5: the hooks that replace the server, on
//! what `bank-common` gives every patch.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! The builder must verify the complete input before emitting any patch.

pub mod local_clock;
#[cfg(feature = "migrate")]
pub mod migrate;
pub mod native_bank;
pub mod navigation;

pub mod native_game;

#[cfg(target_arch = "arm")]
pub mod runtime;

pub mod transaction;

pub mod dex;
pub mod storage_worker;
