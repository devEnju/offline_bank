//! Presents Black/White saves from the SD card where the original expects a
//! DS cartridge, beside a Gen 5 cartridge if one is inserted.
//!
//! The original reaches a cartridge through three functions, all called from
//! its own cartridge task (`00243510`), which runs on a worker thread of the
//! original: read the game code, read the save, write part of the save. The
//! patch redirects those three calls. While the cartridge slot itself is
//! presented (a Gen 5 cartridge is in and is being listed or was picked),
//! they go straight to the original functions.
//!
//! Two more hooks sit in the game-list task (`00244D2C`, `002445A4`), on the
//! main thread: one adds an entry per save after the first DS entry, one
//! notes which entry was picked. They only touch `Saves` while no cartridge
//! task runs.
//!
//! The original asks whether the Game Card was taken out (`001DE43C`) before
//! it reads or saves a DS game. Those questions come here too, so that a
//! removed card concerns the cartridge alone (`Saves::card_missing`).

use crate::sdcard::WriteFault;
use crate::{
    language,
    sdcard::{self, Sd, COPY_CHUNK},
    sdsave::Saves,
    task::{Task, TASK_LIST, TASK_MANAGER},
};
use core::{
    cell::UnsafeCell,
    mem::transmute,
    slice,
    sync::atomic::{AtomicU8, Ordering},
};
use patch_common::native::read_word;

const ORIGINAL_CART_ID: usize = 0x0021_aa0c;
const ORIGINAL_CART_READ: usize = 0x0021_a7e0;
const ORIGINAL_CART_WRITE: usize = 0x0021_ab50;
/// `(manager, heap)`: starts the cartridge task in read mode (`0019ADEC`).
const START_CART_READ: usize = 0x0019_adec;
/// `(manager)`: nonzero once the card-removed signal was raised (`001DE43C`).
/// The signal is a sticky event, set by the notification that the card was
/// taken out and cleared by every scan (`00153E3C`) and every read that
/// starts (`0019ADEC`); asking does not change it.
const CARD_REMOVED: usize = 0x001d_e43c;

/// Mode byte of the cartridge task; 2 is the scan before the game list.
const TASK_MODE: usize = 0x30;
const MODE_SCAN: u8 = 2;
/// What the manager holds of the chosen game: the version stored when an
/// entry is confirmed, and a second word. Both are set only for a Virtual
/// Console title; the original asks about the card only when they are not
/// (`0024A27C`..`0024A290`, and the same at every other site).
const MANAGER_VERSION: usize = 0x148;
const MANAGER_TITLE: usize = 0x14c;
/// Fields of the game-list task.
const LIST_STATE: usize = 0x10;
const LIST_KINDS: usize = 0x3c;
const LIST_COUNT: usize = 0x64;
const LIST_CURSOR: usize = 0x66;
const LIST_HEAP: usize = 0x68;
/// State 1 notes the first item and starts its read; state 2 waits for a
/// read and adds its entry.
const STATE_FIRST_ITEM: u32 = 1;
const STATE_READING_CARTRIDGE: u32 = 2;
const STATE_VIRTUAL_CONSOLE: u32 = 3;
/// The original list holds 40 entries; leave room as the original does.
const LIST_LIMIT: usize = 0x27;

/// Any negative value is a failure to the callers; this one is none of the
/// codes they treat specially.
const FAILED: i32 = -1;

struct Shared<T>(UnsafeCell<T>);
// SAFETY: the cartridge task and the hooks on the main thread never run at
// the same time: the list task and the tasks after it start the cartridge
// task and wait for it, and ask about the card only before or after.
unsafe impl<T> Sync for Shared<T> {}
static SAVES: Shared<Saves> = Shared(UnsafeCell::new(Saves::EMPTY));
/// Copy buffer for the backup; used only on the cartridge task's thread.
/// Kept apart from `SAVES` so that it stays zero-initialised memory.
static CHUNK: Shared<[u8; COPY_CHUNK]> = Shared(UnsafeCell::new([0; COPY_CHUNK]));
fn saves() -> &'static mut Saves {
    // SAFETY: see `Shared`.
    unsafe { &mut *SAVES.0.get() }
}

/// The original's signal that the Game Card was taken out.
unsafe fn card_removed(manager: *mut u8) -> bool {
    let removed: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(CARD_REMOVED) };
    unsafe { removed(manager) != 0 }
}

// 00243528 (was: bl read-cartridge-id). r0 = out, r4 = cartridge task.
patch_common::stub!(
    transporter_cart_id_stub,
    ["mov r1, r4", "b {cart_id}"],
    cart_id = sym cart_id,
);
/// Replaces the call to "read the cartridge's game code" at `00243528`.
/// `task` is the cartridge task (the caller's `r4`).
/// # Safety
/// Called only from that site, on the cartridge task's thread.
unsafe extern "aapcs" fn cart_id(out: *mut u32, task: *const u8) -> i32 {
    let original: unsafe extern "aapcs" fn(*mut u32) -> i32 =
        unsafe { transmute(ORIGINAL_CART_ID) };
    let code = unsafe { original(out) };
    let saves = saves();
    if unsafe { task.add(TASK_MODE).read() } == MODE_SCAN {
        // A new session starts. Only games in the language chosen on the
        // language screen are offered; a Gen 5 cartridge in that language
        // hides the save of its own game.
        let cartridge = (code >= 0).then(|| unsafe { out.read() }.to_le_bytes());
        note(WriteFault::None);
        saves.scan(cartridge, language::filter_letter(), |path, offsets| {
            sdcard::identity(&mut Sd, path, offsets)
        });
    }
    match saves.current() {
        Some(save) => {
            unsafe { out.write(save.game_code()) };
            0
        }
        // A Gen 5 cartridge in another language: the original treats a game
        // code that does not start with "IR" as no Pokémon cartridge.
        None if saves.hides_cartridge() => {
            unsafe { out.write(0) };
            0
        }
        None => code,
    }
}

/// Replaces the call to "read the save" at `002439A8`.
/// # Safety
/// Called only from that site; `buffer` holds `length` bytes.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_cart_read(
    kind: u32,
    offset: u32,
    buffer: *mut u8,
    length: u32,
) -> i32 {
    let Some(save) = saves().current() else {
        let original: unsafe extern "aapcs" fn(u32, u32, *mut u8, u32) -> i32 =
            unsafe { transmute(ORIGINAL_CART_READ) };
        return unsafe { original(kind, offset, buffer, length) };
    };
    let bytes = unsafe { slice::from_raw_parts_mut(buffer, length as usize) };
    if sdcard::read(&mut Sd, save, offset, bytes) {
        0
    } else {
        FAILED
    }
}

/// Replaces the calls to "write part of the save" at `002437F4` and
/// `0024397C`.
/// # Safety
/// Called only from those sites; `buffer` holds `length` bytes.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_cart_write(
    kind: u32,
    offset: u32,
    buffer: *const u8,
    length: u32,
) -> i32 {
    let saves = saves();
    let Some(save) = saves.current() else {
        let original: unsafe extern "aapcs" fn(u32, u32, *const u8, u32) -> i32 =
            unsafe { transmute(ORIGINAL_CART_WRITE) };
        let code = unsafe { original(kind, offset, buffer, length) };
        if code < 0 {
            note(WriteFault::Cartridge);
        }
        return code;
    };
    let bytes = unsafe { slice::from_raw_parts(buffer, length as usize) };
    // SAFETY: see `Shared`; only this thread uses the copy buffer.
    let chunk = unsafe { &mut *CHUNK.0.get() };
    match sdcard::write(&mut Sd, saves, save, offset, bytes, chunk) {
        Ok(()) => 0,
        Err(failed) => {
            note(WriteFault::of_save(saves, failed));
            FAILED
        }
    }
}

/// The last write to the game that failed in this session (`WriteFault`),
/// for the message the original shows for it. Written by the cartridge
/// task's thread, read by the main thread once that task has ended.
static WRITE_FAULT: AtomicU8 = AtomicU8::new(0);
fn note(fault: WriteFault) {
    WRITE_FAULT.store(fault as u8, Ordering::Release);
}
/// What the last write to the game that failed left
/// (`hooks::transporter_message_text`). Asking clears it.
pub fn take_write_fault() -> WriteFault {
    WriteFault::of(WRITE_FAULT.swap(0, Ordering::Acquire))
}

// 00244F10 (was: str r6, [r4, #0x10], state = 3). r4 = list task.
patch_common::stub!(
    transporter_list_next_stub,
    [
        "push {{r0-r3, r12, lr}}",
        "mov r0, r4",
        "bl {list_next}",
        "pop {{r0-r3, r12, lr}}",
        "bx lr",
    ],
    list_next = sym list_next,
);
/// Replaces "go on to the Virtual Console titles" (`00244F10`) in the
/// game-list task. After one item has been handled (the
/// cartridge or an SD save), the next one is read the same way, so each gets
/// its own entry, in the fixed order of the games. The site is also reached
/// in state 1, before the first item was read, when its card is gone.
///
/// A cartridge whose card is gone is skipped without a read; every save is
/// read. The list is always built to the end.
/// # Safety
/// Called only from that site, with the live game-list task.
unsafe extern "aapcs" fn list_next(raw: *mut u8) {
    // Not the list: the object is left as it is, without the replaced
    // state store.
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_LIST]) }) else {
        return;
    };
    // Without its manager the list has no cartridge or save to read: it goes
    // on to the Virtual Console titles.
    let manager = task.pointer(TASK_MANAGER).ok();
    let task = task.raw();
    unsafe {
        let state = task.add(LIST_STATE).cast::<u32>();
        if let (Some(manager), STATE_FIRST_ITEM | STATE_READING_CARTRIDGE) = (manager, state.read())
        {
            let count = usize::from(task.add(LIST_COUNT).cast::<u16>().read());
            let saves = saves();
            while let Some(item) = saves.next_for_list(count) {
                if count >= LIST_LIMIT {
                    break;
                }
                if saves.card_missing(|| card_removed(manager)) {
                    continue;
                }
                // What the original does for the first entry (00244EE8,
                // 00244F34): note the kind, read the item, wait for the read.
                task.add(LIST_KINDS + count).write(item.kind());
                let start: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(START_CART_READ);
                start(manager, read_word(task, LIST_HEAP));
                state.write(STATE_READING_CARTRIDGE);
                return;
            }
        }
        state.write(STATE_VIRTUAL_CONSOLE);
    }
}

/// Whether the chosen game is a cartridge whose card is gone, asked as the
/// original asks before each save of a DS game: never for a Virtual Console
/// title or a save on the SD card.
/// # Safety
/// `manager` is the live manager, on the main thread, while no cartridge
/// task runs.
pub unsafe fn chosen_card_missing(manager: *mut u8) -> bool {
    unsafe {
        let version = read_word(manager, MANAGER_VERSION);
        let title = read_word(manager, MANAGER_TITLE);
        if version != 0 && title != 0 {
            return false;
        }
        saves().card_missing(|| card_removed(manager))
    }
}

/// Replaces the original's question whether the Game Card was taken out
/// (`bl 001DE43C`) before the first item of the list is read (`00244F00`),
/// before the chosen game is read (`00245658`), before the Bank task saves
/// its note (`00248E68`), and in the transfer before each save of the game
/// (`0024A298`, `0024A474`). Nonzero only while the cartridge slot is
/// presented and its card is gone. The original has skipped Virtual Console
/// titles before each of them.
/// # Safety
/// Called only from those sites, on the main thread, with the live manager.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_card_removed(manager: *mut u8) -> u32 {
    u32::from(saves().card_missing(|| unsafe { card_removed(manager) }))
}

/// Replaces the same question in the game list's listener when a DS entry
/// is confirmed (`0025C5A8`, with the task in place of the manager,
/// `0025C5A4`), before the select entry notes the entry: it is asked for
/// the entry at the cursor.
/// # Safety
/// Called only from that site, on the main thread, with the live game-list
/// task.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_entry_card_removed(task: *const u8) -> u32 {
    // Not the list, or a list without its manager: the card counts as
    // gone, and the original then shows its message instead of reading.
    let Ok(task) = (unsafe { Task::from_raw(task.cast_mut(), &[TASK_LIST]) }) else {
        return 1;
    };
    let Ok(manager) = task.pointer(TASK_MANAGER) else {
        return 1;
    };
    let task = task.raw();
    unsafe {
        let position = usize::from(task.add(LIST_CURSOR).cast::<u16>().read());
        u32::from(saves().card_missing_at(position, || card_removed(manager)))
    }
}

// 00244908 (was: mov r0, #0). r4 = list task; r5, r6 stay live.
patch_common::stub!(
    transporter_select_stub,
    [
        "push {{r4, lr}}",
        "mov r0, r4",
        "bl {select}",
        "mov r0, #0",
        "pop {{r4, pc}}",
    ],
    select = sym select,
);
/// Runs when a DS entry of the game list is confirmed (`00244908`).
/// # Safety
/// Called only from that site, with the live game-list task.
unsafe extern "aapcs" fn select(task: *const u8) {
    let Ok(task) = (unsafe { Task::from_raw(task.cast_mut(), &[TASK_LIST]) }) else {
        return;
    };
    let task = task.raw();
    let position = unsafe { task.add(LIST_CURSOR).cast::<u16>().read() };
    saves().select(usize::from(position));
}
