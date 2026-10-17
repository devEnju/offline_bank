#![no_std]
//! What the patches of Bank and of Transporter both use: the worker thread
//! that owns the file work, the kernel calls behind it, and the state a
//! patch's hooks share. Nothing here is an address of either program; a
//! patch names its program's (`worker::Program`).

pub mod exclusive;
pub mod worker;

#[cfg(any(test, target_arch = "arm"))]
pub mod kernel;
