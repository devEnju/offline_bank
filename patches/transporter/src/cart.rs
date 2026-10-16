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

use crate::{
    fs::{Archive, File, FLUSH_FLAGS, OPEN_READ, OPEN_READ_WRITE, OPEN_WRITE_CREATE},
    language,
    sdsave::{Save, Saves, PATH_CAPACITY, SAVE_SIZE},
};
use core::{cell::UnsafeCell, mem::transmute};

const ORIGINAL_CART_ID: usize = 0x0021_aa0c;
const ORIGINAL_CART_READ: usize = 0x0021_a7e0;
const ORIGINAL_CART_WRITE: usize = 0x0021_ab50;
/// `(manager, heap)`: starts the cartridge task in read mode (`0019ADEC`).
const START_CART_READ: usize = 0x0019_adec;
/// `(manager)`: nonzero once the card-removed signal was raised (`001DE43C`).
const CARD_REMOVED: usize = 0x001d_e43c;

/// Mode byte of the cartridge task; 2 is the scan before the game list.
const TASK_MODE: usize = 0x30;
const MODE_SCAN: u8 = 2;
/// Fields of the game-list task.
const LIST_MANAGER: usize = 0x08;
const LIST_STATE: usize = 0x10;
const LIST_KINDS: usize = 0x3c;
const LIST_COUNT: usize = 0x64;
const LIST_CURSOR: usize = 0x66;
const LIST_HEAP: usize = 0x68;
const STATE_READING_CARTRIDGE: u32 = 2;
const STATE_VIRTUAL_CONSOLE: u32 = 3;
/// The original list holds 40 entries; leave room as the original does.
const LIST_LIMIT: usize = 0x27;

/// Any negative value is a failure to the callers; this one is none of the
/// codes they treat specially.
const FAILED: i32 = -1;
const COPY_CHUNK: usize = 0x8000;

struct Shared<T>(UnsafeCell<T>);
// SAFETY: the cartridge task and the game-list hooks never run at the same
// time: the list task starts the cartridge task and waits for it.
unsafe impl<T> Sync for Shared<T> {}
static SAVES: Shared<Saves> = Shared(UnsafeCell::new(Saves::EMPTY));
/// Copy buffer for the backup; used only on the cartridge task's thread.
/// Kept apart from `SAVES` so that it stays zero-initialised memory.
static CHUNK: Shared<[u8; COPY_CHUNK]> = Shared(UnsafeCell::new([0; COPY_CHUNK]));
fn saves() -> &'static mut Saves {
    // SAFETY: see `Shared`.
    unsafe { &mut *SAVES.0.get() }
}

/// For a file of exactly the size of a Gen 5 save that opens for reading and
/// writing, the two bytes at each offset. Nothing is created.
fn identity(path: &[u8], offsets: [u64; 2]) -> Option<[[u8; 2]; 2]> {
    let file = unsafe { File::open(Archive::Sd, path, OPEN_READ_WRITE) }?;
    if file.size() != Some(SAVE_SIZE) {
        return None;
    }
    let mut copies = [[0; 2]; 2];
    for (copy, offset) in copies.iter_mut().zip(offsets) {
        if !file.read(offset, copy.as_mut_ptr(), 2) {
            return None;
        }
    }
    Some(copies)
}

fn open_save(save: Save, backup: bool, flags: u32) -> Option<File> {
    let mut path = [0; PATH_CAPACITY];
    let length = save.path(backup, &mut path);
    let file = unsafe { File::open(Archive::Sd, &path[..length], flags) }?;
    // The save itself must still be what the scan found.
    (backup || file.size() == Some(SAVE_SIZE)).then_some(file)
}

fn in_save(offset: u32, length: u32) -> bool {
    u64::from(offset) + u64::from(length) <= SAVE_SIZE
}

/// Copies the untouched save to `<name>.sav.bak`, flushed, before the first
/// change of a session.
fn back_up(save: Save) -> bool {
    let (Some(source), Some(target)) = (
        open_save(save, false, OPEN_READ),
        open_save(save, true, OPEN_WRITE_CREATE),
    ) else {
        return false;
    };
    let chunk = CHUNK.0.get().cast::<u8>();
    let mut offset = 0;
    while offset < SAVE_SIZE {
        let length = COPY_CHUNK as u32;
        let last = offset + u64::from(length) == SAVE_SIZE;
        let flags = if last { FLUSH_FLAGS } else { 0 };
        if !source.read(offset, chunk, length) || !target.write(offset, chunk, length, flags) {
            return false;
        }
        offset += u64::from(length);
    }
    target.size() == Some(SAVE_SIZE)
}

/// Replaces the call to "read the cartridge's game code" at `00243528`.
/// `task` is the cartridge task (the caller's `r4`).
/// # Safety
/// Called only from that site, on the cartridge task's thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_cart_id(out: *mut u32, task: *const u8) -> i32 {
    let original: unsafe extern "aapcs" fn(*mut u32) -> i32 =
        unsafe { transmute(ORIGINAL_CART_ID) };
    let code = unsafe { original(out) };
    let saves = saves();
    if unsafe { task.add(TASK_MODE).read() } == MODE_SCAN {
        // A new session starts. Only games in the language chosen on the
        // language screen are offered; a Gen 5 cartridge in that language
        // hides the save of its own game.
        let cartridge = (code >= 0).then(|| unsafe { out.read() }.to_le_bytes());
        saves.scan(cartridge, language::filter_letter(), identity);
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
    let done = in_save(offset, length)
        && open_save(save, false, OPEN_READ)
            .is_some_and(|file| file.read(u64::from(offset), buffer, length));
    if done {
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
        return unsafe { original(kind, offset, buffer, length) };
    };
    if !in_save(offset, length) {
        return FAILED;
    }
    if saves.take_backup_duty() && !back_up(save) {
        // Without a backup nothing is changed.
        saves.backup_failed();
        return FAILED;
    }
    let done = open_save(save, false, OPEN_READ_WRITE)
        .is_some_and(|file| file.write(u64::from(offset), buffer, length, FLUSH_FLAGS));
    if done {
        0
    } else {
        FAILED
    }
}

/// Replaces "go on to the Virtual Console titles" (`00244F10`) in the
/// game-list task. After the cartridge step has handled one item (the
/// cartridge or an SD save), the next one is read the same way, so each gets
/// its own entry, in the fixed order of the games.
/// # Safety
/// Called only from that site, with the live game-list task.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_next(task: *mut u8) {
    unsafe {
        let state = task.add(LIST_STATE).cast::<u32>();
        if state.read() == STATE_READING_CARTRIDGE {
            let count = usize::from(task.add(LIST_COUNT).cast::<u16>().read());
            let manager = task.add(LIST_MANAGER).cast::<*mut u8>().read();
            if let Some(item) = saves().next_for_list(count) {
                let removed: unsafe extern "aapcs" fn(*mut u8) -> u32 = transmute(CARD_REMOVED);
                if count < LIST_LIMIT && removed(manager) == 0 {
                    // What the original does for the first entry (00244EE8,
                    // 00244F34): note the kind, then read the save.
                    task.add(LIST_KINDS + count).write(item.kind());
                    let start: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(START_CART_READ);
                    start(manager, task.add(LIST_HEAP).cast::<u32>().read());
                    return;
                }
            }
        }
        state.write(STATE_VIRTUAL_CONSOLE);
    }
}

/// Runs when a DS entry of the game list is confirmed (`00244908`).
/// # Safety
/// Called only from that site, with the live game-list task.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_select(task: *const u8) {
    let position = unsafe { task.add(LIST_CURSOR).cast::<u16>().read() };
    saves().select(usize::from(position));
}
