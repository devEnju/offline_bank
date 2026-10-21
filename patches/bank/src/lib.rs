#![no_std]
//! The offline patch for Bank 1.5: the hooks that replace the server, on
//! what `bank-common` gives every patch.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! The builder must verify the complete input before emitting any patch.

pub mod dex;
pub mod local_clock;
pub mod native_game;
pub mod navigation;
pub mod panel;
pub mod save_steps;
pub mod storage_worker;
pub mod transaction;

#[cfg(target_arch = "arm")]
pub mod hooks;
