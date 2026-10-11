//! Dialogs and loading texts through the native Bank UI.
//!
//! Every text is the original's own, in the language of the screens. The
//! error dialog is the first sentence of the original's message for a failed
//! save with two code numbers on the dialog's third line, and the same
//! numbers go under the start screen's line about the HOME Menu; nothing
//! else is written here.

use core::mem::transmute;
use offline_core::numbers::{self, Rest};

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
/// Native message 0x4D, the start screen's own line: "Press HOME to return
/// to the HOME Menu". One line in every language of the original.
pub const HOME_NOTICE: u32 = 0x4d;

/// Shows an original message through 001d6650, as its task 0x16 does for
/// the failure message, and puts the two code numbers on the third line of
/// its first part, in the UI's own string object. What followed that part is
/// dropped: the failure message ends after its first sentence, which has one
/// or two lines in every language of the original. The string is handed to
/// the dialog again (001d643c, the call 001d6650 ends its text set-up with).
/// Without room for the numbers the message stays as it is.
///
/// With `wait` the dialog ends when its last page is acknowledged. Without,
/// it stays, as the original's message 0x1C does (002a926c): nothing ends it.
/// # Safety
/// `ui` is the live initialized native task UI at task+3c (tasks9/7) or +38
/// (task10). Its context and allocated string object must remain live until
/// poll returns true. No other writer may modify that dialog while it is shown.
pub unsafe fn show(
    ui: *mut u8,
    message: u32,
    wait: bool,
    fault: u32,
    native_result: u32,
) -> Result<(), Error> {
    unsafe { display(ui, message, wait)? };
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let text = pointer(unsafe { word(ui, 0x94) })?;
    let data = pointer(unsafe { word(text, 4) })?.cast::<u16>();
    let capacity = unsafe { text.add(8).cast::<u16>().read() } as usize;
    let len = unsafe { text.add(0xa).cast::<u16>().read() } as usize;
    if unsafe { text.add(0xc).read() } == 0 {
        return Err(Error::MissingBuffer);
    }
    if len == 0 || len >= capacity || unsafe { data.add(len).read() } != 0 {
        return Ok(());
    }
    unsafe {
        let room = core::slice::from_raw_parts_mut(data, capacity);
        let Some(len) = numbers::put(room, len, fault, native_result, Rest::Dropped) else {
            return Ok(());
        };
        room[len] = 0;
        text.add(0xa).cast::<u16>().write(len as u16);
        let begin: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(0x001d_643cusize);
        begin(context, text);
    }
    Ok(())
}

/// One frame of a dialog (001d6600). True once a waiting dialog was
/// acknowledged; a dialog that does not wait is only kept alive by this.
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
/// Native message 0xA: "You previously used a different copy of this game.
/// Please try again using {name}'s game (Trainer ID No. {number})."
const OTHER_COPY_NOTICE: u32 = 0xa;

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

/// Shows an original localized message through 001d6650, exactly as native
/// tasks call it.
unsafe fn display(ui: *mut u8, message: u32, wait: bool) -> Result<(), Error> {
    pointer(ui as u32)?;
    pointer(unsafe { word(ui, 0x5c) })?;
    pointer(unsafe { word(ui, 0x78) })?;
    pointer(unsafe { word(ui, 0x94) })?;
    let show: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
        unsafe { transmute(0x001d_6650usize) };
    unsafe { show(ui, 0, message, u32::from(wait)) };
    Ok(())
}

/// Shows an original localized notice with acknowledgement. Poll
/// `acknowledged` afterwards.
/// # Safety
/// `ui` has show's native lifetime contract, with no other dialog active.
pub unsafe fn notice(ui: *mut u8, message: u32) -> Result<(), Error> {
    unsafe { display(ui, message, true) }
}

/// Shows the original's message for another copy of a game, filled the way
/// its own task fills it (002a8c24..002a8ca0): a string object from the
/// trainer's name (001dfd90, on `heap`, the task's own), the name as the
/// message's value 0 (002b67d8), the number as its value 1 with `digits`
/// digits (001e6c0c), the message, and the string object's end (002297a4).
/// The original gives 002b67d8 a fourth value from its server's reply, of 0
/// to 2, for the grammar of a text; this message refers to it in none of
/// its languages, and 0 is given. Poll `acknowledged` afterwards.
/// # Safety
/// `ui` has show's native lifetime contract, with no other dialog active;
/// `heap` is the live heap of the task that owns `ui`.
pub unsafe fn other_copy(
    ui: *mut u8,
    heap: *mut u8,
    name: &[u16; 12],
    number: u32,
    digits: u32,
) -> Result<(), Error> {
    pointer(ui as u32)?;
    pointer(heap as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    pointer(unsafe { word(ui, 0x78) })?;
    pointer(unsafe { word(ui, 0x94) })?;
    // The original's source is zero-terminated: 12 units and the end mark.
    let mut source = [0u16; 13];
    source[..12].copy_from_slice(name);
    // The string object: vtable, buffer, room, length, flag (16 bytes).
    let mut string = [0u32; 4];
    unsafe {
        let create: unsafe extern "aapcs" fn(*mut u32, *const u16, *mut u8) =
            transmute(0x001d_fd90usize);
        let set_name: unsafe extern "aapcs" fn(*mut u8, u32, *mut u32, u32) =
            transmute(0x002b_67d8usize);
        let set_number: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32, u32) =
            transmute(0x001e_6c0cusize);
        let destroy: unsafe extern "aapcs" fn(*mut u32) = transmute(0x0022_97a4usize);
        create(string.as_mut_ptr(), source.as_ptr(), heap);
        set_name(context, 0, string.as_mut_ptr(), 0);
        set_number(context, 1, number, digits, 2, 1);
        let shown = display(ui, OTHER_COPY_NOTICE, true);
        destroy(string.as_mut_ptr());
        shown
    }
}
