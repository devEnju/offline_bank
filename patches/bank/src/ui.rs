//! Short local-error dialog through the native Bank UI.
//!
//! Native 001D6650 provides the setup/poll pattern. The text object is the
//! already allocated UI buffer, so no foreign allocator or temporary pointer
//! escapes this call. The caller retains the task until acknowledgement.

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

/// # Safety
/// `ui` is the live initialized native task UI at task+3c (tasks9/7) or +38
/// (task10). Its context and allocated string object must remain live until
/// poll returns true. No other writer may modify that dialog while it is shown.
pub unsafe fn show(ui: *mut u8, fault: u32, native_result: u32) -> Result<(), Error> {
    pointer(ui as u32)?;
    let context = pointer(unsafe { word(ui, 0x5c) })?;
    let text = pointer(unsafe { word(ui, 0x94) })?;
    let data = pointer(unsafe { word(text, 4) })?.cast::<u16>();
    let capacity = unsafe { text.add(8).cast::<u16>().read() } as usize;
    let allocated = unsafe { text.add(0xc).read() };
    if allocated == 0 {
        return Err(Error::MissingBuffer);
    }
    let mut message = [0u16; 128];
    let mut len = 0;
    for &byte in b"Offline Bank error " {
        message[len] = u16::from(byte);
        len += 1;
    }
    for value in [fault, native_result] {
        for shift in (0..8).rev() {
            let digit = ((value >> (shift * 4)) & 15) as u8;
            message[len] = u16::from(if digit < 10 {
                b'0' + digit
            } else {
                b'A' + digit - 10
            });
            len += 1;
        }
        message[len] = u16::from(b' ');
        len += 1;
    }
    for &byte in b"\nRestart Bank. Keep all save data." {
        message[len] = u16::from(byte);
        len += 1;
    }
    if capacity <= len {
        return Err(Error::InsufficientCapacity);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(message.as_ptr(), data, len + 1);
        text.add(0xa).cast::<u16>().write(len as u16);
        let configure: unsafe extern "aapcs" fn(*mut u8, u32, u32) = transmute(0x0029_749cusize);
        let begin: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(0x001d_643cusize);
        let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(0x001e_7084usize);
        configure(context, 1, 0);
        begin(context, text);
        pane(context, 0, 0x99, 0);
        pane(context, 0, 0xa9, 1);
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

/// Native message 2, shown by task 9 while it opens the Bank.
pub const OPENING_MESSAGE: u32 = 2;
/// Native message 0xE, shown by task 0x10 while it loads the Bank.
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
