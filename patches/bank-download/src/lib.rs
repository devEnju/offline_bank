#![no_std]
//! The download patch for Bank 1.5: lets the original sign in to its server
//! and load the Bank, writes what it loaded as the files of the offline
//! patch, has the original leave without saving, and shows the result as two
//! numbers. It never shows the boxes and never saves to the server.
//!
//! Everything that talks to the server is the original's own code; this
//! crate only decides where the session turns off the original's way
//! (`navigation`) and writes the files (`download`).
//!
//! It is of use while the service runs. Nothing depends on this crate; when
//! the service has ended, the crate, its builder profile
//! (`bank15/download.rs`) and its script can be deleted.
//!
//! Addresses in this crate belong only to code SHA-256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.

pub mod download;
pub mod navigation;

#[cfg(target_arch = "arm")]
pub mod runtime;
