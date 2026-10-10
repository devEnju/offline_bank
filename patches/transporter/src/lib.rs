#![no_std]
//! Poké Transporter 1.5 hooks that hand the transport box to the offline Bank.
//!
//! Addresses in this crate belong only to code SHA-256
//! 001c20ada74016507c969bb44a0a50f8edf803ec06a3fc46834263ba8df0fd2f
//! (title 00040000000C9C00, TMD version 5200). The builder verifies the
//! complete input before emitting any patch.
//!
//! The original Transporter converts every Pokémon itself and stores it in a
//! Bank data object of the same class and layout Bank uses, records and
//! format tags included; it used to upload that object. The hooks copy the
//! transport box of that object into the transport-box files of Bank's extdata
//! through `offline_core::transport::deliver`. Nothing is converted here.
//!
//! `cart` presents Black/White saves from the SD card to the original's
//! cartridge code, beside a Gen 5 cartridge (`sdsave` holds the rules).
//!
//! The original language screen chooses which language's games are listed
//! (`language`); it no longer changes the language of the screens. Its list
//! ends in a Back button that `lytpatch` makes of an unused entry.

pub mod gen5;
pub mod language;
pub mod layout;
pub mod lytpatch;
pub mod navigation;
pub mod sdsave;

pub mod bootstrap;

#[cfg(target_arch = "arm")]
pub mod cart;
#[cfg(target_arch = "arm")]
pub mod hooks;
