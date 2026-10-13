#![no_std]
//! The migration patch for Bank 1.5: converts the Bank that the previous
//! release stored into the current files, shows the result as two numbers,
//! and does nothing else. It holds the only code that reads the previous
//! release's files (`previous`). It never opens the Bank, and it has none of the offline
//! patch's hooks beyond the way from the start screen to "open Bank".
//!
//! It belongs to that one step between two versions. Nothing depends on
//! this crate; once the step is behind, the crate, its builder profile
//! (`bank15/migrate.rs`) and its script can be deleted.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.

pub mod migrate;
pub mod navigation;
pub mod previous;

#[cfg(target_arch = "arm")]
pub mod runtime;
