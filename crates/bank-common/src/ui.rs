//! Dialogs and loading texts through the native Bank UI.
//!
//! Every text is the original's own, in the language of the screens. The
//! error dialog is the first sentence of the original's message for a failed
//! save, and as its next page the start screen's line about the HOME Menu
//! with two code numbers on the dialog's third line. A save that did not
//! start for lack of its Game Card shows that message's other part, the
//! advice to check the card. Nothing else is written here.

use crate::numbers;
use core::mem::transmute;
use patch_common::native::pointer as native_pointer;
use patch_common::{
    message,
    native::read_word as word,
    ui::{self as shared, StringError, Text},
};

/// `001d6650(ui, kind, message, wait)`: shows a message of the original.
const SHOW_MESSAGE: usize = 0x001d_6650;
/// `001d643c(context, string)`: hands a changed string to the dialog again,
/// the call `SHOW_MESSAGE` ends its text set-up with.
const DIALOG_TEXT: usize = 0x001d_643c;
/// `001d6600(context)`: one frame of a dialog, 4 once a waiting one was
/// acknowledged.
const DIALOG_POLL: usize = 0x001d_6600;
/// `001d5b44(ui)` starts the loading panel, sound and animation;
/// `0025da24(ui, message)` sets its text.
const LOADING: usize = 0x001d_5b44;
const LOADING_TEXT: usize = 0x0025_da24;
/// The end of loading: `001d6200(sound, 0, -1)` stops a sound,
/// `001e7084(context, group, pane, 0)` hides a pane, `001e69ac(context, 0,
/// 0, 0)` unbinds animation 0.
const STOP_SOUND: usize = 0x001d_6200;
const HIDE_PANE: usize = 0x001e_7084;
const UNBIND_ANIMATION: usize = 0x001e_69ac;
/// `001e9bc0(sound, fade, tag, mode)`: plays a sound once.
const PLAY_SOUND: usize = 0x001e_9bc0;
/// `001d6194(ui)` shows the dialog scene; `002b6114(0x10004, 0x3c, 0)` is
/// the call before each of its character's speeches.
const DIALOG_SCENE: usize = 0x001d_6194;
const SPEECH: usize = 0x002b_6114;
/// The values of a message: `001dfd90(string, text, heap)` makes a string
/// object, `002b67d8(context, value, string, attribute)` gives it to the
/// message, `001e6c0c(context, value, number, digits, 2, 1)` gives a number,
/// `002297a4(string)` ends the string object.
const STRING: usize = 0x001d_fd90;
const VALUE_STRING: usize = 0x002b_67d8;
const VALUE_NUMBER: usize = 0x001e_6c0c;
const STRING_END: usize = 0x0022_97a4;
/// `001e0ef8(ui, group, pane, message)`: a message into a button's pane, as
/// the menus label their buttons.
#[cfg(feature = "test-build")]
const BUTTON_MESSAGE: usize = 0x001e_0ef8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPointer,
    MissingBuffer,
    InvalidMessage,
}
fn pointer(value: u32) -> Result<*mut u8, Error> {
    native_pointer(value).ok_or(Error::InvalidPointer)
}
/// The UI's context (`ui + 0x5c`), once the UI, the context and the UI's
/// words at `+0x78` and `+0x94` (its string object) are pointers: what a
/// message, the loading panel and the sounds need.
unsafe fn ready(ui: *mut u8) -> Result<*mut u8, Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    pointer(unsafe { word(ui, 0x78) })?;
    pointer(unsafe { word(ui, 0x94) })?;
    Ok(context)
}

/// Native message 0xB, which the original shows after a failed save
/// (task 7 state 0x10, task 0x16 at 002a9118): "The server did not receive
/// the data." and then "Please confirm that your Game Card is properly
/// inserted, and then try again."
pub const FAILURE_NOTICE: u32 = 0xb;
/// Native message 0x4D, the start screen's own line: "Press HOME to return
/// to the HOME Menu". One line in every language of the original.
pub const HOME_NOTICE: u32 = 0x4d;

/// The longest line about the HOME Menu with its label and its numbers
/// that is carried from one message to the other. The line has 11 to 42
/// units in the ten languages of the original.
const HOME_ROOM: usize = 128;

/// The dialog's own text, in the UI's string object at `ui + 0x94`
/// (`patch_common::ui::text`). `None` if it is not a text that can be
/// changed in place. The caller uses it before the dialog's string changes
/// again.
unsafe fn text<'a>(ui: *mut u8) -> Result<Option<Text<'a>>, Error> {
    let object = pointer(unsafe { word(ui, 0x94) })?;
    unsafe { shared::text(object) }.map_err(|error| match error {
        StringError::NotPointer => Error::InvalidPointer,
        StringError::NoBuffer => Error::MissingBuffer,
    })
}

/// Ends the dialog's string at `len` (`patch_common::ui::end`) and resets
/// the dialog's parser.
unsafe fn begin(context: *mut u8, text: *mut u8, room: &mut [u16], len: usize) {
    unsafe {
        shared::end(text, room, len);
        let begin: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(DIALOG_TEXT);
        begin(context, text);
    }
}

/// Shows the screen a patch ends on, as one dialog that does not end, the
/// way the original shows its message 0x1C (002a926c): a reason, and after
/// A the line about the HOME Menu, which stays. Without a reason the line
/// is the whole screen.
///
/// The reason is the original's message `reason` up to the end of its first
/// part, unchanged: for the failure message its first sentence. The line
/// about the HOME Menu, message 0x4D, becomes the page after it, with
/// `label`, the version and package, on its second line and the two code
/// numbers on its third.
///
/// Both texts are the original's, loaded by 001d6650 into the UI's own
/// string object: first the line, which is copied out, then the reason. The
/// string is changed in place and handed to the dialog again (001d643c, the
/// call 001d6650 ends its text set-up with). Without room for the label
/// the numbers go alone, without room for the numbers the line stays as it
/// is; without room for the page the reason stays alone.
/// # Safety
/// `ui` is the live initialized native task UI at task+3c (tasks9/7) or +38
/// (task10). Its context and allocated string object must remain live while
/// the dialog is shown. No other writer may modify that dialog meanwhile.
pub unsafe fn stop(
    ui: *mut u8,
    reason: Option<u32>,
    label: &numbers::Label,
    first: u32,
    second: u32,
) -> Result<(), Error> {
    let label = label.units();
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let mut home = [0u16; HOME_ROOM];
    let mut home_len = 0;
    unsafe { display(ui, HOME_NOTICE, false)? };
    if let Some(Text {
        object: text,
        room,
        len,
    }) = unsafe { text(ui)? }
    {
        let Some(_) = reason else {
            let len = numbers::put(room, len, label, first, second).unwrap_or(len);
            unsafe { begin(context, text, room, len) };
            return Ok(());
        };
        if len < HOME_ROOM {
            home[..len].copy_from_slice(&room[..len]);
            home_len = numbers::put(&mut home, len, label, first, second).unwrap_or(len);
        }
    }
    let Some(reason) = reason else {
        return Ok(());
    };
    unsafe { display(ui, reason, false)? };
    let Some(Text {
        object: text,
        room,
        len,
    }) = (unsafe { text(ui)? })
    else {
        return Ok(());
    };
    let mut len = message::cut(&room[..len]);
    if home_len != 0 {
        len = numbers::append_page(room, len, &home[..home_len]).unwrap_or(len);
    }
    unsafe { begin(context, text, room, len) };
    Ok(())
}

/// One frame of a dialog (001d6600). True once a waiting dialog was
/// acknowledged; a dialog that does not wait is only kept alive by this.
/// # Safety
/// `ui` and its context must satisfy stop's lifetime contract, with the same
/// dialog still active. Only the main task thread may poll it.
pub unsafe fn acknowledged(ui: *mut u8) -> Result<bool, Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let poll: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(DIALOG_POLL) };
    Ok(unsafe { poll(context) } == 4)
}

/// Native message 2: preparing Bank, with the warning not to turn off power.
/// Task 9 uses it for creation; utilities use it for their local work.
pub const CREATING_MESSAGE: u32 = 2;
/// Native message 0xE: "Communicating with the Pokémon Bank server...".
/// The scene set-up shows it after game selection, before task 0x10 loads
/// the Bank. The offline opening keeps the game scan's panel and text.
pub const BANK_LOADING_MESSAGE: u32 = 0xe;
/// Native message 0xA: "You previously used a different copy of this game.
/// Please try again using {name}'s game (Trainer ID No. {number})."
const OTHER_COPY_NOTICE: u32 = 0xa;

/// Starts the native loading panel, sound, and animation through 001d5b44,
/// then lets 0025da24 load the original localized message into the UI's own
/// string object. No text is written by this patch.
/// # Safety
/// `ui` has stop's native lifetime contract. The task must keep returning each
/// frame while the storage worker runs, so status and audio keep updating.
pub unsafe fn loading(ui: *mut u8, message: u32) -> Result<(), Error> {
    unsafe { ready(ui) }?;
    unsafe {
        let open_status: unsafe extern "aapcs" fn(*mut u8) = transmute(LOADING);
        open_status(ui);
        loading_message(ui, message);
    }
    Ok(())
}

/// Changes only the native loading text, retaining the panel, sound and spinner.
/// # Safety
/// `ui` has loading's native lifetime contract and its loading panel is already
/// active, set up by `loading` or the original's native task initializer.
pub unsafe fn loading_message(ui: *mut u8, message: u32) {
    let show_message: unsafe extern "aapcs" fn(*mut u8, u32) = unsafe { transmute(LOADING_TEXT) };
    unsafe { show_message(ui, message) };
}

/// The three operations native tasks perform when loading ends: stop sound
/// 0x50011, hide pane 0x97, and unbind animation 0. The skipped main-menu
/// initializer 002a6e38 did this before; game selection does not.
/// # Safety
/// `ui` has stop's native lifetime contract, on the main task thread.
pub unsafe fn stop_loading(ui: *mut u8) -> Result<(), Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    unsafe {
        let stop_sound: unsafe extern "aapcs" fn(u32, u32, u32) = transmute(STOP_SOUND);
        let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(HIDE_PANE);
        let unbind: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(UNBIND_ANIMATION);
        stop_sound(0x0005_0011, 0, u32::MAX);
        pane(context, 0, 0x97, 0);
        unbind(context, 0, 0, 0);
    }
    Ok(())
}

/// `001d7ed8(layout, pane)`: a pane of a layout, or null, as `001e7084`
/// finds the pane it shows or hides.
const FIND_PANE: usize = 0x001d_7ed8;

/// Whether the loading panel is on screen: its pane 0x97, which `001d5b44`
/// shows and every end of loading hides, through `001e7084(context, 0,
/// 0x97, shown)`, which keeps it in bit 0 of the pane's byte `+0xb7`. The
/// pane is found as `001e7084` finds it: in the layout of group 0 of the
/// UI's context (`[[context + 0x1c]]`).
/// # Safety
/// `ui` is the live shared UI, on the main task thread.
pub unsafe fn loading_shown(ui: *mut u8) -> Result<bool, Error> {
    let context = unsafe { ready(ui) }?;
    let groups = pointer(unsafe { word(context, 0x1c) })?;
    let layout = pointer(unsafe { word(groups, 0) })?;
    let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 = unsafe { transmute(FIND_PANE) };
    let pane = unsafe { find(layout, 0x97) };
    if pane.is_null() {
        return Ok(false);
    }
    pointer(pane as u32)?;
    Ok(unsafe { pane.add(0xb7).read() } & 1 != 0)
}

/// Native message 9: "Success! Changes saved."
const SAVED_NOTICE: u32 = 9;

/// How the original's Save and Quit ends when it succeeded (002b21c4..
/// 002b2224): the success sound 0x50012 through 001e9bc0, the end of the
/// loading panel, and message 9, which waits. Its failure (002b2238) plays
/// no sound. 001e9bc0(id, fade, tag, mode) starts a sound once, on the first
/// free of the eleven sound players, without a fade-in, with the tag -1
/// that lets any stop call (001d6200, 002b6084) end it; nothing is left to
/// end when it is over. Poll `acknowledged` afterwards.
/// # Safety
/// `ui` has stop's native lifetime contract, with the loading panel up and
/// no dialog active, on the main task thread.
pub unsafe fn saved(ui: *mut u8) -> Result<(), Error> {
    unsafe { ready(ui) }?;
    unsafe {
        let play: unsafe extern "aapcs" fn(u32, u32, u32, u32) = transmute(PLAY_SOUND);
        play(0x0005_0012, 0, u32::MAX, 0);
        stop_loading(ui)?;
        display(ui, SAVED_NOTICE, true)
    }
}

/// What native task 9 does on entering its welcome for a new Bank
/// (002ae634): end loading, show the dialog scene through 001d6194, and
/// 002b6114(0x10004, 0x3c, 0), the call before each of that character's
/// speeches. The messages themselves are shown by the task's own states.
/// # Safety
/// `ui` has stop's native lifetime contract, on the main task thread.
pub unsafe fn welcome(ui: *mut u8) -> Result<(), Error> {
    unsafe { stop_loading(ui)? };
    unsafe {
        let scene: unsafe extern "aapcs" fn(*mut u8) = transmute(DIALOG_SCENE);
        let sound: unsafe extern "aapcs" fn(u32, u32, u32) = transmute(SPEECH);
        scene(ui);
        sound(0x0001_0004, 0x3c, 0);
    }
    Ok(())
}

/// Shows an original localized message through 001d6650, exactly as native
/// tasks call it.
unsafe fn display(ui: *mut u8, message: u32, wait: bool) -> Result<(), Error> {
    unsafe { ready(ui) }?;
    let show: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
    unsafe { show(ui, 0, message, u32::from(wait)) };
    Ok(())
}

/// Shows the first `pages` pages of an original notice. The final retained
/// page has no page-break command, so the native renderer treats it as the
/// end of the message; A closes it rather than advancing to another page.
/// # Safety
/// `ui` has stop's native lifetime contract, with no other dialog active.
pub unsafe fn notice_pages(ui: *mut u8, message: u32, pages: usize) -> Result<(), Error> {
    unsafe { display(ui, message, true)? };
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let Some(Text {
        object: text,
        room,
        len,
    }) = (unsafe { text(ui)? })
    else {
        return Err(Error::MissingBuffer);
    };
    let end = message::page_end(&room[..len], pages).ok_or(Error::InvalidMessage)?;
    unsafe { begin(context, text, room, end) };
    Ok(())
}

/// Shows an original message without its first part (`message::rest`),
/// waiting for A. The rest moves to the front of the UI's own string object.
/// # Safety
/// `ui` has stop's native lifetime contract, with no other dialog active.
pub unsafe fn notice_rest(ui: *mut u8, message: u32) -> Result<(), Error> {
    unsafe { display(ui, message, true)? };
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let Some(Text {
        object: text,
        room,
        len,
    }) = (unsafe { text(ui)? })
    else {
        return Err(Error::MissingBuffer);
    };
    let len = message::keep_rest(room, len).ok_or(Error::InvalidMessage)?;
    unsafe { begin(context, text, room, len) };
    Ok(())
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
/// `ui` has stop's native lifetime contract, with no other dialog active;
/// `heap` is the live heap of the task that owns `ui`.
pub unsafe fn other_copy(
    ui: *mut u8,
    heap: *mut u8,
    name: &[u16; 12],
    number: u32,
    digits: u32,
) -> Result<(), Error> {
    pointer(heap as u32)?;
    let context = unsafe { ready(ui) }?;
    // The original's source is zero-terminated: 12 units and the end mark.
    let mut source = [0u16; 13];
    source[..12].copy_from_slice(name);
    // The string object: vtable, buffer, room, length, flag (16 bytes).
    let mut string = [0u32; 4];
    unsafe {
        let create: unsafe extern "aapcs" fn(*mut u32, *const u16, *mut u8) = transmute(STRING);
        let set_name: unsafe extern "aapcs" fn(*mut u8, u32, *mut u32, u32) =
            transmute(VALUE_STRING);
        let set_number: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32, u32) =
            transmute(VALUE_NUMBER);
        let destroy: unsafe extern "aapcs" fn(*mut u32) = transmute(STRING_END);
        create(string.as_mut_ptr(), source.as_ptr(), heap);
        set_name(context, 0, string.as_mut_ptr(), 0);
        set_number(context, 1, number, digits, 2, 1);
        let shown = display(ui, OTHER_COPY_NOTICE, true);
        destroy(string.as_mut_ptr());
        shown
    }
}

/// Test builds: labels a button with the original's `message` and `suffix`
/// behind it. The original's 001e0ef8(ui, group, pane, message), as its
/// menus label their buttons, loads the message into the second work string
/// and hands it to the pane; the suffix is added to that string in place and
/// the pane given it again.
/// # Safety
/// `ui` is the live native task UI with the layout of `group` built, on the
/// main thread.
#[cfg(feature = "test-build")]
pub unsafe fn button(ui: *mut u8, group: u32, pane: u32, message: u32, suffix: &[u8]) {
    unsafe {
        let Ok(context) = pointer(word(ui, 0x5c)) else {
            return;
        };
        let label: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(BUTTON_MESSAGE);
        label(ui, group, pane, message);
        patch_common::ui::add_suffix::<crate::program::Bank15>(context, group, pane, suffix);
    }
}
