#![no_std]
//! The migration patch for Bank 1.5: converts a Bank that an earlier
//! release stored, in either of the two earlier ways, into the current
//! files, shows the result as two numbers, and does nothing else. It holds
//! the only code that reads those files (`v0_2_1`, `v0_4_0`). It never opens
//! the Bank, and it has none of the offline patch's hooks beyond the way
//! from the start screen to "open Bank".
//!
//! Nothing depends on this crate. Every release below v1.0.0 ships it;
//! then it is deleted with the places docs/building.md names.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.

pub mod migrate;
pub mod navigation;
pub mod source;
mod target;
#[cfg(test)]
mod testing;
mod v0_2_1;
mod v0_4_0;

#[cfg(target_arch = "arm")]
pub mod runtime;
