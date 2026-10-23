//! The version line on the start screen, which every patch extends by its
//! name ("Ver. 1.5-offline"), with the original's own calls; the original's
//! string objects, in which a message is changed in place (`text`, `end`),
//! here and in Bank's dialogs (`bank_common::ui`); the parts of a screen
//! that both programs lay out alike (`context`, `heap`, `work_strings`,
//! `layout`); and the end of the shared UI's loading panel (`end_loading`).

use crate::{
    message,
    native::{follow, pointer, NotNative},
    program::Program,
};
use core::{mem::transmute, slice};

/// What a string object of the original lacks for its text to be changed
/// in place: its text is not a pointer, or it holds no text of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringError {
    NotPointer,
    NoBuffer,
}

/// A screen of the original, the same in both programs: the shared UI, and
/// screens such as Transporter's language screen, keep their layout context
/// at `+0x5C`; the shared UI keeps the owner of its heap at `+0x78`, with the
/// heap at `+4` of it. A context holds its two work strings at `+4` and, at
/// `+0x1C`, a row of 8 bytes for each group of panes, the group's layout
/// first.
const SCREEN_CONTEXT: usize = 0x5c;
const UI_HEAP_OWNER: usize = 0x78;
const OWNER_HEAP: usize = 4;
const CONTEXT_STRINGS: usize = 4;
const CONTEXT_LAYOUTS: usize = 0x1c;
const LAYOUT_ROW: usize = 8;

/// The layout context of `screen`, if `screen` and it are pointers.
/// # Safety
/// A pointer `screen` is a live screen of the original, on the main thread.
pub unsafe fn context(screen: *mut u8) -> Option<*mut u8> {
    pointer(screen as u32)?;
    unsafe { follow(screen, SCREEN_CONTEXT) }
}
/// The owner of the shared UI's heap, if `ui` and it are pointers.
/// # Safety
/// As `context`, for the shared UI.
pub unsafe fn heap_owner(ui: *mut u8) -> Option<*mut u8> {
    pointer(ui as u32)?;
    unsafe { follow(ui, UI_HEAP_OWNER) }
}
/// The shared UI's heap (`[[ui + 0x78] + 4]`).
/// # Safety
/// As `heap_owner`.
pub unsafe fn heap(ui: *mut u8) -> Option<*mut u8> {
    unsafe { follow(heap_owner(ui)?, OWNER_HEAP) }
}
/// The two work strings of `context`.
/// # Safety
/// `context` is a live layout context of the original, on the main thread.
pub unsafe fn work_strings(context: *mut u8) -> Option<*mut u8> {
    unsafe { follow(context, CONTEXT_STRINGS) }
}
/// The layout of `group` of `context` (`[[context + 0x1C] + 8 * group]`).
/// # Safety
/// As `work_strings`, with the layout of `group` built.
pub unsafe fn layout(context: *mut u8, group: u32) -> Option<*mut u8> {
    unsafe {
        let layouts = follow(context, CONTEXT_LAYOUTS)?;
        follow(layouts, group as usize * LAYOUT_ROW)
    }
}

/// The text of a string object of the original, with its room: the text at
/// `+4`, the room in units at `+8`, the length at `+0xA`, and at `+0xC`
/// whether the object holds a text of its own.
pub struct Text<'a> {
    /// The string object.
    pub object: *mut u8,
    /// Its room, for as long as the object holds that text.
    pub room: &'a mut [u16],
    /// The length of the text in it.
    pub len: usize,
}

/// The room of string object `object`.
unsafe fn room<'a>(object: *mut u8) -> Result<&'a mut [u16], StringError> {
    unsafe {
        let data = follow(object, 4)
            .ok_or(StringError::NotPointer)?
            .cast::<u16>();
        if object.add(0xc).read() == 0 {
            return Err(StringError::NoBuffer);
        }
        let capacity = usize::from(object.add(8).cast::<u16>().read());
        Ok(slice::from_raw_parts_mut(data, capacity))
    }
}

/// The text of string object `object`, if it can be changed in place: its
/// length is that of a text that ends in its room. The caller uses it before
/// the object's text changes again.
/// # Safety
/// `object` is a live string object of the original, on the main thread.
pub unsafe fn text<'a>(object: *mut u8) -> Result<Option<Text<'a>>, StringError> {
    let room = unsafe { room(object)? };
    let len = usize::from(unsafe { object.add(0xa).cast::<u16>().read() });
    if len == 0 || len >= room.len() || room[len] != 0 {
        return Ok(None);
    }
    Ok(Some(Text { object, room, len }))
}

/// Ends the text of string object `object` at `len` in its `room`.
/// # Safety
/// `object` is a live string object of the original whose room is `room`,
/// and `len` lies inside it.
pub unsafe fn end(object: *mut u8, room: &mut [u16], len: usize) {
    room[len] = 0;
    unsafe { object.add(0xa).cast::<u16>().write(len as u16) };
}

/// The second of the context's two work strings (`[context + 4]`), which
/// the original fills and hands to a pane: the string object and its room.
unsafe fn work_string<'a>(context: *mut u8) -> Option<(*mut u8, &'a mut [u16])> {
    unsafe {
        let strings = work_strings(context)?;
        let object = follow(strings, 4)?;
        Some((object, room(object).ok()?))
    }
}

/// Keeps only what follows the first part of the message the original has
/// just loaded into the string object `object` (`message::keep_rest`). A
/// string whose length is not that of its text, or a message with nothing
/// after its first part, stays as it is.
/// # Safety
/// `object` is a live string object of the original, on the main thread.
pub unsafe fn keep_rest(object: *mut u8) {
    let Ok(Some(Text { room, len, .. })) = (unsafe { text(object) }) else {
        return;
    };
    if let Some(len) = message::keep_rest(room, len) {
        unsafe { end(object, room, len) };
    }
}

/// Ends the work string at `len` and gives it to pane `pane` of the layout
/// of `group` (`layout`), with the original's two calls.
unsafe fn hand_over<P: Program>(
    context: *mut u8,
    group: u32,
    pane: u32,
    (object, room, len): (*mut u8, &mut [u16], usize),
) {
    unsafe {
        end(object, room, len);
        let Some(layout) = layout(context, group) else {
            return;
        };
        let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 = transmute(P::FIND_PANE);
        let set: unsafe extern "aapcs" fn(*mut u8, *mut u8, i32) = transmute(P::PANE_TEXT);
        let found = find(layout, pane);
        if !found.is_null() {
            set(found, object, -1);
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
        let Some((object, room)) = work_string(context) else {
            return;
        };
        let len = room
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(room.len());
        if len == 0 || len + suffix.len() >= room.len() {
            return;
        }
        for (unit, &letter) in room[len..].iter_mut().zip(suffix) {
            *unit = u16::from(letter);
        }
        hand_over::<P>(context, group, pane, (object, room, len + suffix.len()));
    }
}

/// Writes the start screen's version line with `suffix` behind it, "Ver.
/// 1.5" + suffix, so that the screen shows which patch runs. Stands in for
/// the start screen's one call of `P::PANE_MESSAGE` for its version line:
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

/// The size of a message file's object (`0022EF50` in Transporter,
/// `001e7dd8` in Bank).
const MESSAGES_SIZE: u32 = 0x28;

/// Runs `use_messages` with message file `file` of the screens' language,
/// opened on `heap` as the original gives a screen its messages
/// (`Program::OPEN_MESSAGES`), and deletes the file object again through its
/// second virtual function, as a screen's end does (`0022F004` in
/// Transporter). `None`, with nothing run, if the object is not made or its
/// deleting function does not lie in the program's code.
/// # Safety
/// `heap` is a live heap of the original, on the main thread; `use_messages`
/// keeps nothing of the object.
pub unsafe fn with_messages<P: Program, T>(
    heap: *mut u8,
    file: u32,
    use_messages: impl FnOnce(*mut u8) -> T,
) -> Option<T> {
    unsafe {
        let allocate: unsafe extern "aapcs" fn(u32, *mut u8) -> *mut u8 = transmute(P::ALLOCATE);
        let archive: unsafe extern "aapcs" fn(u32) -> u32 = transmute(P::TEXT_ARCHIVE);
        let open: unsafe extern "aapcs" fn(*mut u8, u32, u32, *mut u8, u32, u32) -> *mut u8 =
            transmute(P::OPEN_MESSAGES);
        pointer(heap as u32)?;
        let memory = allocate(MESSAGES_SIZE, heap);
        if memory.is_null() {
            return None;
        }
        let messages = open(memory, archive(10), file, heap, 1, 5);
        // The file object and its deleting destructor are checked before
        // anything uses it.
        let vtable = pointer(messages as u32).and_then(|messages| follow(messages, 0))?;
        let delete = crate::native::code::<P>(crate::native::read_word(vtable, 4))?;
        let delete: unsafe extern "aapcs" fn(*mut u8) = transmute(delete);
        let result = use_messages(messages);
        delete(messages);
        Some(result)
    }
}

/// The loop sound of the shared UI's loading panel, and its spinner.
const LOADING_SOUND: u32 = 0x0005_0011;
const SPINNER_ANIMATION: u32 = 0;

/// Ends the shared UI's loading panel as the original's tasks end it: stops
/// its loop sound, hides its pane (`Program::LOADING_PANE`) and unbinds its
/// spinner. Fails, with nothing done, if `ui` holds no context.
/// # Safety
/// `ui` is the live shared UI of `P`, on the main thread.
pub unsafe fn end_loading<P: Program>(ui: *mut u8) -> Result<(), NotNative> {
    let context = unsafe { context(ui) }.ok_or(NotNative)?;
    unsafe {
        let stop: unsafe extern "aapcs" fn(u32, u32, i32) = transmute(P::STOP_SOUND);
        let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(P::PANE_SHOWN);
        let unbind: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
            transmute(P::UNBIND_ANIMATION);
        stop(LOADING_SOUND, 0, -1);
        pane(context, 0, P::LOADING_PANE, 0);
        unbind(context, 0, SPINNER_ANIMATION, 0);
    }
    Ok(())
}
