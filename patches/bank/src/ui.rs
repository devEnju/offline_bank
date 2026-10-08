//! Dialogs and loading texts through the native Bank UI.
//!
//! Every text is the original's own, in the language of the screens. The
//! error dialog is the first sentence of the original's message for a failed
//! save with two code numbers on the dialog's third line; nothing else is
//! written here.

use core::mem::transmute;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPointer,
    MissingBuffer,
    InsufficientCapacity,
}
fn pointer(value: u32) -> Result<*mut u8, Error> {
    if value == 0 || value & 3 != 0 {
        Err(Error::InvalidPointer)
    } else {
        Ok(value as *mut u8)
    }
}
unsafe fn word(base: *mut u8, offset: usize) -> u32 {
    unsafe { base.add(offset).cast::<u32>().read() }
}

/// Native message 0xB, which the original shows after a failed save
/// (task 0x16, 002a9118): "The server did not receive the data. ..."
pub const FAILURE_NOTICE: u32 = 0xb;
/// A control code in the original's texts: this unit, the number of units
/// that follow, then the code and its arguments.
const CONTROL: u16 = 0x10;
/// The control code between two pages: wait for a button, then clear.
const NEW_PAGE: u16 = 0xbe01;
const LINE_FEED: u16 = 0x0a;
/// Two numbers of eight digits with a space between them.
const CODES: usize = 17;
/// The dialog holds three lines; the numbers are always on the last.
const CODE_LINE: usize = 3;

/// The line of numbers under the failure message's first sentence.
fn codes(fault: u32, native_result: u32) -> [u16; CODES] {
    let mut out = [0; CODES];
    let mut at = 0;
    for (index, value) in [fault, native_result].into_iter().enumerate() {
        if index == 1 {
            out[at] = u16::from(b' ');
            at += 1;
        }
        for shift in (0..8).rev() {
            let digit = ((value >> (shift * 4)) & 15) as u8;
            out[at] = u16::from(if digit < 10 {
                b'0' + digit
            } else {
                b'A' + digit - 10
            });
            at += 1;
        }
    }
    out
}

/// Where the first sentence of the failure message ends ("the data was not
/// received"; what follows is advice about the Game Card): at its first
/// page break. One language has the whole message on one page, the
/// sentence on its first line; there it ends at the first line feed.
fn first_sentence_end(text: &[u16]) -> usize {
    let mut line = None;
    let mut at = 0;
    while at < text.len() {
        if text[at] != CONTROL {
            if text[at] == LINE_FEED && line.is_none() {
                line = Some(at);
            }
            at += 1;
            continue;
        }
        if at + 2 >= text.len() {
            break;
        }
        if text[at + 2] == NEW_PAGE {
            return at;
        }
        at += 2 + usize::from(text[at + 1]);
    }
    line.unwrap_or(text.len())
}

/// Shows the original's failure message through 001d6650, as its task 0x16
/// does, then ends it after its first sentence, which has one or two lines
/// in every language of the original, and puts the two code numbers on the
/// third line, in the UI's own string object. The string is handed to the
/// dialog again (001d643c, the call 001d6650 ends its text set-up with).
/// Without room for the numbers the message stays as it is.
/// # Safety
/// `ui` is the live initialized native task UI at task+3c (tasks9/7) or +38
/// (task10). Its context and allocated string object must remain live until
/// poll returns true. No other writer may modify that dialog while it is shown.
pub unsafe fn show(ui: *mut u8, fault: u32, native_result: u32) -> Result<(), Error> {
    unsafe { notice(ui, FAILURE_NOTICE)? };
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let text = pointer(unsafe { word(ui, 0x94) })?;
    let data = pointer(unsafe { word(text, 4) })?.cast::<u16>();
    let capacity = unsafe { text.add(8).cast::<u16>().read() } as usize;
    let len = unsafe { text.add(0xa).cast::<u16>().read() } as usize;
    if unsafe { text.add(0xc).read() } == 0 {
        return Err(Error::MissingBuffer);
    }
    let codes = codes(fault, native_result);
    if len == 0 || unsafe { data.add(len).read() } != 0 {
        return Ok(());
    }
    unsafe {
        let message = core::slice::from_raw_parts(data, len);
        let mut at = first_sentence_end(message);
        let lines = 1 + message[..at]
            .iter()
            .filter(|&&unit| unit == LINE_FEED)
            .count();
        let feeds = CODE_LINE.saturating_sub(lines).max(1);
        if capacity <= at + feeds + codes.len() {
            return Ok(());
        }
        for _ in 0..feeds {
            data.add(at).write(LINE_FEED);
            at += 1;
        }
        core::ptr::copy_nonoverlapping(codes.as_ptr(), data.add(at), codes.len());
        at += codes.len();
        data.add(at).write(0);
        text.add(0xa).cast::<u16>().write(at as u16);
        let begin: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(0x001d_643cusize);
        begin(context, text);
    }
    Ok(())
}

/// # Safety
/// `ui` and its context must satisfy show's lifetime contract, with the same
/// dialog still active. Only the main task thread may poll it.
pub unsafe fn acknowledged(ui: *mut u8) -> Result<bool, Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let poll: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(0x001d_6600usize) };
    Ok(unsafe { poll(context) } == 4)
}

/// Native message 2, shown by task 9 only while it creates a new Bank.
pub const CREATING_MESSAGE: u32 = 2;
/// Native message 0xE, on screen while task 9 opens an existing Bank (left
/// up by the tasks before it) and while task 0x10 loads it.
pub const BANK_LOADING_MESSAGE: u32 = 0xe;
/// Native notice 0x1A, shown by game selection when no game can be used.
pub const NO_GAME_NOTICE: u32 = 0x1a;

/// Starts the native loading panel, sound, and animation through 001d5b44,
/// then lets 0025da24 load the original localized message into the UI's own
/// string object. No text is written by this patch.
/// # Safety
/// `ui` has show's native lifetime contract. The task must keep returning each
/// frame while the storage worker runs, so status and audio keep updating.
pub unsafe fn loading(ui: *mut u8, message: u32) -> Result<(), Error> {
    pointer(ui as u32)?;
    pointer(unsafe { word(ui, 0x5c) })?;
    pointer(unsafe { word(ui, 0x78) })?;
    pointer(unsafe { word(ui, 0x94) })?;
    unsafe {
        let open_status: unsafe extern "aapcs" fn(*mut u8) = transmute(0x001d_5b44usize);
        let show_message: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(0x0025_da24usize);
        open_status(ui);
        show_message(ui, message);
    }
    Ok(())
}

/// The three operations native tasks perform when loading ends: stop sound
/// 0x50011, hide pane 0x97, and unbind animation 0. The skipped main-menu
/// initializer 002a6e38 did this before; game selection does not.
/// # Safety
/// `ui` has show's native lifetime contract, on the main task thread.
pub unsafe fn stop_loading(ui: *mut u8) -> Result<(), Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    unsafe {
        let stop_sound: unsafe extern "aapcs" fn(u32, u32, u32) = transmute(0x001d_6200usize);
        let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(0x001e_7084usize);
        let unbind: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(0x001e_69acusize);
        stop_sound(0x0005_0011, 0, u32::MAX);
        pane(context, 0, 0x97, 0);
        unbind(context, 0, 0, 0);
    }
    Ok(())
}

/// What native task 9 does on entering its welcome for a new Bank
/// (002ae634): end loading, show the dialog scene through 001d6194, and
/// 002b6114(0x10004, 0x3c, 0), the call before each of that character's
/// speeches. The messages themselves are shown by the task's own states.
/// # Safety
/// `ui` has show's native lifetime contract, on the main task thread.
pub unsafe fn welcome(ui: *mut u8) -> Result<(), Error> {
    unsafe { stop_loading(ui)? };
    unsafe {
        let scene: unsafe extern "aapcs" fn(*mut u8) = transmute(0x001d_6194usize);
        let sound: unsafe extern "aapcs" fn(u32, u32, u32) = transmute(0x002b_6114usize);
        scene(ui);
        sound(0x0001_0004, 0x3c, 0);
    }
    Ok(())
}

/// Shows an original localized notice with acknowledgement through 001d6650,
/// exactly as native tasks call it. Poll `acknowledged` afterwards.
/// # Safety
/// `ui` has show's native lifetime contract, with no other dialog active.
pub unsafe fn notice(ui: *mut u8, message: u32) -> Result<(), Error> {
    pointer(ui as u32)?;
    pointer(unsafe { word(ui, 0x5c) })?;
    pointer(unsafe { word(ui, 0x78) })?;
    pointer(unsafe { word(ui, 0x94) })?;
    let show: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
        unsafe { transmute(0x001d_6650usize) };
    unsafe { show(ui, 0, message, 1) };
    Ok(())
}
