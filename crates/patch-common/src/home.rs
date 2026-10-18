//! HOME and sleep, refused while a patch reads or writes files and let
//! through on every screen that waits for the user (docs/internals.md,
//! "HOME button and sleep").

use crate::program::Program;
use core::mem::transmute;

/// Sets or clears bit 1 of the original's activity mask through the
/// original functions. While the mask is not zero the original's main loop
/// refuses HOME and sleep, as it did during its own transfers.
///
/// The mask only refuses new HOME presses. One accepted just before it was
/// set would still be carried out, so setting the mask also takes that press
/// back: accepting it did nothing but set the one byte cleared here. From
/// then on the original refuses every press itself, and the HOME Menu, and
/// closing the program from it, never meet a file being read or written.
pub fn refuse<P: Program>(refuse: bool) {
    let address = if refuse {
        P::ACTIVITY_SET
    } else {
        P::ACTIVITY_CLEAR
    };
    let mark: unsafe extern "aapcs" fn(u32) = unsafe { transmute(address) };
    unsafe { mark(1) };
    if refuse {
        unsafe { (P::HOME_ACCEPTED as *mut u8).write_volatile(0) };
    }
}
