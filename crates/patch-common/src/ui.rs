//! The version line on the title screen, which every patch extends by its
//! name ("Ver. 1.5-offline"), with the original's own calls.

use crate::{native::follow, program::Program};
use core::mem::transmute;

/// The second of the context's two work strings (`[context + 4]`), which
/// the original fills and hands to a pane: the string object, its text, and
/// its room in units.
unsafe fn work_string(context: *mut u8) -> Option<(*mut u8, *mut u16, usize)> {
    unsafe {
        let strings = follow(context, 4)?;
        let text = follow(strings, 4)?;
        let data = follow(text, 4)?.cast::<u16>();
        if text.add(0xc).read() == 0 {
            return None;
        }
        Some((text, data, usize::from(text.add(8).cast::<u16>().read())))
    }
}

/// Ends the work string at `len` and gives it to pane `pane` of the layout
/// of `group` (`[[context + 0x1C] + 8 * group]`), with the original's two
/// calls.
unsafe fn hand_over<P: Program>(
    context: *mut u8,
    group: u32,
    pane: u32,
    (text, data, len): (*mut u8, *mut u16, usize),
) {
    unsafe {
        data.add(len).write(0);
        text.add(0xa).cast::<u16>().write(len as u16);
        let Some(layouts) = follow(context, 0x1c) else {
            return;
        };
        let find: unsafe extern "aapcs" fn(u32, u32) -> *mut u8 = transmute(P::FIND_PANE);
        let set: unsafe extern "aapcs" fn(*mut u8, *mut u8, i32) = transmute(P::PANE_TEXT);
        let found = find(layouts.add(group as usize * 8).cast::<u32>().read(), pane);
        if !found.is_null() {
            set(found, text, -1);
        }
    }
}

/// Adds `suffix`, plain Latin letters, to the text the original has just
/// put into the work string and into pane `pane` of the layout of `group`,
/// and gives the pane the longer text. Without room for it the pane keeps
/// the original's text.
/// # Safety
/// `context` is the live context the original has just used for that pane,
/// on the main thread.
pub unsafe fn add_suffix<P: Program>(context: *mut u8, group: u32, pane: u32, suffix: &[u8]) {
    unsafe {
        let Some((text, data, room)) = work_string(context) else {
            return;
        };
        let mut len = 0;
        while len < room && data.add(len).read() != 0 {
            len += 1;
        }
        if len == 0 || len + suffix.len() >= room {
            return;
        }
        for (index, &unit) in suffix.iter().enumerate() {
            data.add(len + index).write(u16::from(unit));
        }
        hand_over::<P>(context, group, pane, (text, data, len + suffix.len()));
    }
}

/// Writes the title screen's version line with `suffix` behind it, "Ver.
/// 1.5" + suffix, so that the screen shows which patch runs. Stands in for
/// the title screen's one call of `P::PANE_MESSAGE` for its version line:
/// the original runs first, then the suffix is added.
/// # Safety
/// The arguments are those of the replaced call, on the main thread.
pub unsafe fn version<P: Program>(
    context: *mut u8,
    group: u32,
    pane: u32,
    message: u32,
    suffix: &[u8],
) {
    unsafe {
        let original: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(P::PANE_MESSAGE);
        original(context, group, pane, message);
        add_suffix::<P>(context, group, pane, suffix);
    }
}
