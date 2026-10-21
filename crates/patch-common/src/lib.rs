#![no_std]
//! What the patches of Bank and of Transporter both use: the start-up hook,
//! the worker thread that owns the file work, the kernel calls behind it,
//! the SDK's calls on an open file,
//! the state a patch's hooks share, the refusal of HOME and sleep, how long
//! a loading screen stands before its work and stays at least, the version
//! line, the walk over the text of a message, the entries written in
//! assembly, and the table that keeps a payload's entry functions in the
//! link. Nothing here is an address of either program; a patch names its
//! program's (`program::Program`).

pub mod bootstrap;
pub mod exclusive;
pub mod file;
#[cfg(target_arch = "arm")]
pub mod home;
pub mod link;
pub mod message;
pub mod native;
pub mod program;
pub mod spinner;
#[cfg(target_arch = "arm")]
mod stub;
#[cfg(target_arch = "arm")]
pub mod ui;
pub mod worker;

#[cfg(any(test, target_arch = "arm"))]
pub mod kernel;
