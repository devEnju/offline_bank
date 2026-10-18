#![no_std]
//! What the patches of Bank and of Transporter both use: the start-up hook,
//! the worker thread that owns the file work, the kernel calls behind it,
//! the state a patch's hooks share, the refusal of HOME and sleep, and the
//! version line. Nothing here is an address of either program; a patch
//! names its program's (`program::Program`).

pub mod bootstrap;
pub mod exclusive;
#[cfg(target_arch = "arm")]
pub mod home;
pub mod native;
pub mod program;
#[cfg(target_arch = "arm")]
pub mod ui;
pub mod worker;

#[cfg(any(test, target_arch = "arm"))]
pub mod kernel;
