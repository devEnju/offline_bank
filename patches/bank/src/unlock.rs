//! The original's unlock screen (task 0x17), without its server, and the
//! keys that lead to it (docs/internals.md, "The unlock screen").
//!
//! The original reaches it from its check of another copy (task 0x12): with
//! L, A and START held when that check finds no save that holds the session
//! (`002a8b4c`), the check ends with outcome 0x16, for which the router
//! answers task 0x17. The offline patch reaches it from the opening (task 9),
//! when a save in progress that moved Pokémon both ways waits for its game
//! or is left to Transporter's question and the keys were held on START's
//! loading screen; it ends the opening with the same outcome.
//!
//! The task's set-up draws the code it shows (`+0x3c`,
//! `001e2e24(100000000)`). Its states 0 to 2 send the code to the server
//! and wait for the keys that unlock it; 3 ends the loading panel and shows
//! "Please enter the unlock key: [code]" (39:0x22); 4 opens the number
//! keyboard. The keyboard's listener (`002d1550`) keeps the number typed
//! (`+0x40`) and selects 5 for one of the server's keys, 7 ("The unlock key
//! is incorrect.", 39:0x23, then 3 again) for any other, 0xC for Cancel and
//! 0xB when Bank is closed meanwhile. State 5 sends the unlock.

/// The outcome of the original's check for L, A and START held (`002a8bb4`),
/// for which the router answers the unlock screen.
pub const KEYS: u8 = 0x16;
/// The keys, in the original's own pad bits: L, A and START (`002a8b68`,
/// `002a8b80`, `002a8b9c`).
pub const KEY_MASKS: [u32; 3] = [0x100, 0x10, 0x1000];

/// The task's states the entry looks at.
pub const SHOW_CODE: u32 = 3;
pub const ACCEPTED: u32 = 5;
pub const INCORRECT: u32 = 7;
pub const CANCELLED: u32 = 0xc;

/// What a frame of the unlock screen does, by the task's state, the number
/// typed and the code shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The request for the keys: on to showing the code at once, as for a
    /// server that answered none; then the original.
    Skip,
    /// The original's frame.
    Original,
    /// A key was typed: the code shown, or one of the server's, of which
    /// there are none. The unlock is written instead of sent.
    Unlock,
    /// Cancel: the message the opening would have shown instead.
    Back,
}

pub fn frame(state: u32, typed: u32, code: u32) -> Frame {
    match state {
        0..=2 => Frame::Skip,
        ACCEPTED => Frame::Unlock,
        INCORRECT if typed == code => Frame::Unlock,
        CANCELLED => Frame::Back,
        _ => Frame::Original,
    }
}

/// Whether the opening leads to the unlock screen instead of its message:
/// the keys were held, and the save in progress, which waits or is left to
/// Transporter's question, is not unlocked yet.
pub fn offered(keys: bool, unlocked: bool) -> bool {
    keys && !unlocked
}

#[cfg(target_arch = "arm")]
mod pad {
    use core::mem::transmute;

    /// `002354f4()` the pads, `00235424(pads, 0)` the first, and
    /// `001e754c(pad, mask, 0)` whether `mask` is held in it, as the
    /// original's check tests the keys (`002a8b4c`..`002a8bb0`).
    const PADS: usize = 0x0023_54f4;
    const PAD: usize = 0x0023_5424;
    const HELD: usize = 0x001e_754c;

    /// L, A and START are held together now.
    /// # Safety
    /// On the main thread, after the original's start-up.
    pub unsafe fn keys_held() -> bool {
        let pads: unsafe extern "aapcs" fn() -> *mut u8 = unsafe { transmute(PADS) };
        let pad: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 = unsafe { transmute(PAD) };
        let held: unsafe extern "aapcs" fn(*mut u8, u32, u32) -> u32 = unsafe { transmute(HELD) };
        let pads = unsafe { pads() };
        if patch_common::native::pointer(pads as u32).is_none() {
            return false;
        }
        let first = unsafe { pad(pads, 0) };
        if patch_common::native::pointer(first as u32).is_none() {
            return false;
        }
        super::KEY_MASKS
            .iter()
            .all(|&mask| unsafe { held(first, mask, 0) } != 0)
    }
}
#[cfg(target_arch = "arm")]
pub use pad::keys_held;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_sent_and_the_code_shown_unlocks() {
        for state in 0..=2 {
            assert_eq!(frame(state, 0, 1234), Frame::Skip);
        }
        assert_eq!(frame(INCORRECT, 1234, 1234), Frame::Unlock);
        // A key of the server's list, of which offline there are none.
        assert_eq!(frame(ACCEPTED, 99, 1234), Frame::Unlock);
        // Another number: the original's "The unlock key is incorrect.".
        assert_eq!(frame(INCORRECT, 1235, 1234), Frame::Original);
        assert_eq!(frame(CANCELLED, 1234, 1234), Frame::Back);
        // The code, the keyboard, the message, its wait, the outcomes and
        // Bank closed meanwhile are the original's.
        for state in [SHOW_CODE, 4, 6, 8, 9, 0xa, 0xb] {
            assert_eq!(frame(state, 1234, 1234), Frame::Original);
        }
    }

    #[test]
    fn the_keys_lead_there_only_for_a_save_not_unlocked_yet() {
        assert!(offered(true, false));
        assert!(!offered(true, true));
        assert!(!offered(false, false));
        assert!(!offered(false, true));
    }
}
