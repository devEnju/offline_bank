#![no_std]
//! The migration patch for Bank 1.5: converts the Bank that v0.2.1 stored
//! into the files of v0.3.0, shows the result as two numbers, and does
//! nothing else. It never opens the Bank, and it has none of the offline
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

#[cfg(target_arch = "arm")]
pub mod runtime;
