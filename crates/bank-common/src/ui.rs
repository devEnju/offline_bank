//! Dialogs and loading texts through the native Bank UI.
//!
//! Every text is the original's own, in the language of the screens. The
//! error dialog is the first sentence of the original's message for a failed
//! save, and as its next page the start screen's line about the HOME Menu
//! with two code numbers on the dialog's third line; nothing else is written
//! here.

use crate::numbers;
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
/// Native message 0x4D, the start screen's own line: "Press HOME to return
/// to the HOME Menu". One line in every language of the original.
pub const HOME_NOTICE: u32 = 0x4d;

/// The longest line about the HOME Menu with its numbers that is carried
/// from one message to the other. The line has 11 to 42 units in the ten
/// languages of the original.
const HOME_ROOM: usize = 96;

/// The dialog's own string object at `ui + 0x94`, its room in units, and
/// the length of the text in it. `None` if it is not a text that can be
/// changed in place.
unsafe fn text(ui: *mut u8) -> Result<Option<(*mut u8, &'static mut [u16], usize)>, Error> {
    let text = pointer(unsafe { word(ui, 0x94) })?;
    let data = pointer(unsafe { word(text, 4) })?.cast::<u16>();
    let capacity = unsafe { text.add(8).cast::<u16>().read() } as usize;
    let len = unsafe { text.add(0xa).cast::<u16>().read() } as usize;
    if unsafe { text.add(0xc).read() } == 0 {
        return Err(Error::MissingBuffer);
    }
    if len == 0 || len >= capacity || unsafe { data.add(len).read() } != 0 {
        return Ok(None);
    }
    Ok(Some((
        text,
        unsafe { core::slice::from_raw_parts_mut(data, capacity) },
        len,
    )))
}

/// Shows the screen a patch ends on, as one dialog that does not end, the
/// way the original shows its message 0x1C (002a926c): a reason, and after
/// A the line about the HOME Menu, which stays. Without a reason the line
/// is the whole screen.
///
/// The reason is the original's message `reason` up to the end of its first
/// part, unchanged: for the failure message its first sentence. The line
/// about the HOME Menu, message 0x4D, becomes the page after it, with the
/// two code numbers on its third line.
///
/// Both texts are the original's, loaded by 001d6650 into the UI's own
/// string object: first the line, which is copied out, then the reason. The
/// string is changed in place and handed to the dialog again (001d643c, the
/// call 001d6650 ends its text set-up with). Without room for the numbers
/// the line stays as it is; without room for the page the reason stays
/// alone.
/// # Safety
/// `ui` is the live initialized native task UI at task+3c (tasks9/7) or +38
/// (task10). Its context and allocated string object must remain live while
/// the dialog is shown. No other writer may modify that dialog meanwhile.
pub unsafe fn stop(ui: *mut u8, reason: Option<u32>, first: u32, second: u32) -> Result<(), Error> {
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    // Ends the text at `len` and hands it to the dialog.
    let begin = |text: *mut u8, room: &mut [u16], len: usize| unsafe {
        let begin: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(0x001d_643cusize);
        room[len] = 0;
        text.add(0xa).cast::<u16>().write(len as u16);
        begin(context, text);
    };
    let mut home = [0u16; HOME_ROOM];
    let mut home_len = 0;
    unsafe { display(ui, HOME_NOTICE, false)? };
    if let Some((text, room, len)) = unsafe { text(ui)? } {
        let Some(_) = reason else {
            let len = numbers::put(room, len, first, second).unwrap_or(len);
            begin(text, room, len);
            return Ok(());
        };
        if len < HOME_ROOM {
            home[..len].copy_from_slice(&room[..len]);
            home_len = numbers::put(&mut home, len, first, second).unwrap_or(len);
        }
    }
    let Some(reason) = reason else {
        return Ok(());
    };
    unsafe { display(ui, reason, false)? };
    let Some((text, room, len)) = (unsafe { text(ui)? }) else {
        return Ok(());
    };
    let mut len = numbers::cut(&room[..len]);
    if home_len != 0 {
        len = numbers::append_page(room, len, &home[..home_len]).unwrap_or(len);
    }
    begin(text, room, len);
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
/// `ui` has stop's native lifetime contract. The task must keep returning each
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
/// `ui` has stop's native lifetime contract, on the main task thread.
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
/// `ui` has stop's native lifetime contract, on the main task thread.
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
/// `ui` has stop's native lifetime contract, with no other dialog active.
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
/// `ui` has stop's native lifetime contract, with no other dialog active;
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

/// Writes the start screen's version line with `suffix` behind it, "Ver.
/// 1.5" + suffix, so that a screen shows which patch runs. Stands in for
/// the one call of 001e21c8(context, group, pane, message) that the start
/// screen makes for its message 0x4E (002b49c4).
///
/// That function loads the message into the first of the context's two
/// work strings (`[context + 4]`), fills in its values into the second, and
/// hands the second to the pane (001e264c finds it, 0029a628 sets its
/// text). Here the original runs first, the suffix is added to the second
/// string in place, and the pane is given it again with the same two calls.
/// Without room for the suffix the line stays the original's.
/// # Safety
/// The arguments are those of the replaced call, on the main thread.
pub unsafe fn version(context: *mut u8, group: u32, pane: u32, message: u32, suffix: &[u8]) {
    unsafe {
        let original: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
            transmute(0x001e_21c8usize);
        original(context, group, pane, message);
        let Ok(strings) = pointer(word(context, 4)) else {
            return;
        };
        let Ok(text) = pointer(word(strings, 4)) else {
            return;
        };
        let Ok(data) = pointer(word(text, 4)) else {
            return;
        };
        let data = data.cast::<u16>();
        let room = text.add(8).cast::<u16>().read() as usize;
        if text.add(0xc).read() == 0 {
            return;
        }
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
        len += suffix.len();
        data.add(len).write(0);
        text.add(0xa).cast::<u16>().write(len as u16);
        let Ok(layouts) = pointer(word(context, 0x1c)) else {
            return;
        };
        let find: unsafe extern "aapcs" fn(u32, u32) -> *mut u8 = transmute(0x001e_264cusize);
        let set: unsafe extern "aapcs" fn(*mut u8, *mut u8, i32) = transmute(0x0029_a628usize);
        let found = find(word(layouts, group as usize * 8), pane);
        if !found.is_null() {
            set(found, text, -1);
        }
    }
}
