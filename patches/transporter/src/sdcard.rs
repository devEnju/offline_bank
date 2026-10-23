//! The SD card, where the Black/White saves are (docs/internals.md, "Saves
//! on the SD card"): what the cartridge hooks (`cart`) do with a save's file
//! there. A scan reads its identity, a read and a write go to the save, and
//! the first write of a session copies the save to its backup before
//! anything else. This is plain code over a file of the card (`Card`,
//! `SdFile`), tested on a simulated card; on the console the file is the
//! SDK's (`native`), as Bank's files are reached through `bank_common::fs`.
//! Bank's files themselves are reached through bank-common
//! (`bank_common::fs::Visit`).

use crate::sdsave::{Save, Saves, PATH_CAPACITY, SAVE_SIZE};
use offline_core::Crc32;

/// The SDK's open flags: read, read and write, write and create.
pub const OPEN_READ: u32 = 1;
pub const OPEN_READ_WRITE: u32 = 3;
pub const OPEN_WRITE_CREATE: u32 = 6;
/// The size of one copy step of a backup; the steps make up the save.
pub const COPY_CHUNK: usize = 0x8000;
const _: () = assert!(SAVE_SIZE % COPY_CHUNK as u64 == 0);

/// Why the card refused to open or write a file: for lack of space
/// (`patch_common::fs_result::lack_of_space`), or for anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub full: bool,
}
impl Refused {
    pub const OTHER: Self = Self { full: false };
    pub const FULL: Self = Self { full: true };
}

/// An open file on the SD card.
pub trait SdFile {
    fn size(&self) -> Option<u64>;
    /// Whether all of `bytes` was read at `offset`.
    fn read(&self, offset: u64, bytes: &mut [u8]) -> bool;
    /// Writes all of `bytes` at `offset`, flushed with it if `flush`.
    fn write(&self, offset: u64, bytes: &[u8], flush: bool) -> Result<(), Refused>;
    /// Closes the file: whether it closed without an error.
    fn close(self) -> bool;
}

/// The SD card's archive.
pub trait Card {
    type File: SdFile;
    /// `path` is ASCII with its terminator.
    fn open(&mut self, path: &[u8], flags: u32) -> Result<Self::File, Refused>;
}

/// For a file of exactly the size of a Gen 5 save that opens for reading and
/// writing, the two bytes at each offset (`Saves::scan`). Nothing is created.
pub fn identity<C: Card>(card: &mut C, path: &[u8], offsets: [u64; 2]) -> Option<[[u8; 2]; 2]> {
    let file = card.open(path, OPEN_READ_WRITE).ok()?;
    if file.size() != Some(SAVE_SIZE) {
        return None;
    }
    let mut copies = [[0; 2]; 2];
    for (copy, offset) in copies.iter_mut().zip(offsets) {
        if !file.read(offset, copy) {
            return None;
        }
    }
    Some(copies)
}

fn open_save<C: Card>(
    card: &mut C,
    save: Save,
    backup: bool,
    flags: u32,
) -> Result<C::File, Refused> {
    let mut path = [0; PATH_CAPACITY];
    let length = save.path(backup, &mut path);
    let file = card.open(&path[..length], flags)?;
    // The save itself must still be what the scan found.
    if backup || file.size() == Some(SAVE_SIZE) {
        Ok(file)
    } else {
        Err(Refused::OTHER)
    }
}

fn in_save(offset: u32, length: usize) -> bool {
    u64::from(offset) + length as u64 <= SAVE_SIZE
}

/// Copies the untouched save to `<name>.sav.bak`, flushed and closed, before
/// the first change of a session, and reads the copy back: it must hold
/// what was read from the save, as Bank's own files are read back before
/// they count. `chunk` is the copy buffer. A card without room for the copy
/// refuses to make or grow it (`Refused::FULL`).
fn back_up<C: Card>(card: &mut C, save: Save, chunk: &mut [u8; COPY_CHUNK]) -> Result<(), Refused> {
    let source = open_save(card, save, false, OPEN_READ)?;
    let target = open_save(card, save, true, OPEN_WRITE_CREATE)?;
    let mut copied = Crc32::new();
    let mut offset = 0;
    while offset < SAVE_SIZE {
        let last = offset + COPY_CHUNK as u64 == SAVE_SIZE;
        if !source.read(offset, chunk) {
            return Err(Refused::OTHER);
        }
        target.write(offset, chunk, last)?;
        copied.update(chunk);
        offset += COPY_CHUNK as u64;
    }
    if target.size() != Some(SAVE_SIZE) || !target.close() {
        return Err(Refused::OTHER);
    }
    let backup = open_save(card, save, true, OPEN_READ)?;
    let mut read = Crc32::new();
    let mut offset = 0;
    while offset < SAVE_SIZE {
        if !backup.read(offset, chunk) {
            return Err(Refused::OTHER);
        }
        read.update(chunk);
        offset += COPY_CHUNK as u64;
    }
    if read.finish() == copied.finish() {
        Ok(())
    } else {
        Err(Refused::OTHER)
    }
}

/// Reads `bytes` at `offset` of the save, as the cartridge read does.
pub fn read<C: Card>(card: &mut C, save: Save, offset: u32, bytes: &mut [u8]) -> bool {
    in_save(offset, bytes.len())
        && open_save(card, save, false, OPEN_READ)
            .is_ok_and(|file| file.read(u64::from(offset), bytes))
}

/// Writes `bytes` at `offset` of the save, flushed, as the cartridge write
/// does, reads them back and closes the save: it is written only if the
/// card holds them and the close reports no error, as the backup and Bank's
/// own files are. The first write of a session to a save backs it up
/// first; without the backup nothing is changed, and the next write tries
/// it again. `chunk` is the copy buffer. A backup the card has no room for
/// is `Refused::FULL`; the save itself is never made larger.
pub fn write<C: Card>(
    card: &mut C,
    saves: &mut Saves,
    save: Save,
    offset: u32,
    bytes: &[u8],
    chunk: &mut [u8; COPY_CHUNK],
) -> Result<(), Refused> {
    if !in_save(offset, bytes.len()) {
        return Err(Refused::OTHER);
    }
    if saves.take_backup_duty() {
        if let Err(refused) = back_up(card, save, chunk) {
            saves.backup_failed();
            return Err(refused);
        }
    }
    let file = open_save(card, save, false, OPEN_READ_WRITE)?;
    file.write(u64::from(offset), bytes, true)?;
    for (index, written) in bytes.chunks(COPY_CHUNK).enumerate() {
        let read = &mut chunk[..written.len()];
        let at = u64::from(offset) + (index * COPY_CHUNK) as u64;
        if !file.read(at, read) || read != written {
            return Err(Refused::OTHER);
        }
    }
    if file.close() {
        Ok(())
    } else {
        Err(Refused::OTHER)
    }
}

#[cfg(target_arch = "arm")]
pub use native::Sd;

#[cfg(target_arch = "arm")]
mod native {
    use super::{Card, Refused, SdFile};
    use crate::program::Transporter15;
    use core::mem::transmute;
    use patch_common::{file, fs_result::lack_of_space, program::Program};

    /// How the card refused a call that failed with `code`.
    fn refused(code: file::Code) -> Refused {
        Refused {
            full: lack_of_space(code as u32),
        }
    }

    /// The SDK's "open a file in an archive in one call", `(session, &file,
    /// 0, archive id, path type, path, path size, 3, file path, its size,
    /// flags, 0)`.
    const OPEN_FILE_DIRECTLY: usize = 0x001d_f448;

    /// The SD card through Transporter's SDK: archive 9 with an empty path,
    /// each file opened with its archive in one call. Transporter's file
    /// system session is set up at start-up.
    pub struct Sd;
    impl Card for Sd {
        type File = File;
        fn open(&mut self, path: &[u8], flags: u32) -> Result<File, Refused> {
            type Open = unsafe extern "aapcs" fn(
                *const u32,
                *mut u32,
                u32,
                u32,
                u32,
                *const u8,
                u32,
                u32,
                *const u8,
                u32,
                u32,
                u32,
            ) -> i32;
            let open: Open = unsafe { transmute(OPEN_FILE_DIRECTLY) };
            let empty = [0u8; 4];
            let mut handle = 0;
            let code = unsafe {
                open(
                    Transporter15::FS_SESSION as *const u32,
                    &mut handle,
                    0,
                    9,
                    1,
                    empty.as_ptr(),
                    1,
                    3,
                    path.as_ptr(),
                    path.len() as u32,
                    flags,
                    0,
                )
            };
            if code < 0 {
                return Err(refused(code));
            }
            unsafe { file::Handle::new(handle) }
                .map(File)
                .ok_or(Refused::OTHER)
        }
    }

    /// An open file on the SD card (`patch_common::file::Handle`).
    pub struct File(file::Handle<Transporter15>);
    impl SdFile for File {
        fn size(&self) -> Option<u64> {
            self.0.size().ok()
        }
        fn read(&self, offset: u64, bytes: &mut [u8]) -> bool {
            self.0.read(offset, bytes) == Ok(bytes.len() as u32)
        }
        fn write(&self, offset: u64, bytes: &[u8], flush: bool) -> Result<(), Refused> {
            let flags = if flush { file::FLUSH } else { 0 };
            match self.0.write(offset, bytes, flags) {
                Ok(written) if written == bytes.len() as u32 => Ok(()),
                Ok(_) => Err(Refused::OTHER),
                Err(code) => Err(refused(code)),
            }
        }
        fn close(self) -> bool {
            let (closed, handle) = self.0.close();
            closed.is_ok() && handle.is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc, vec, vec::Vec};

    /// What fails on the simulated card, for files whose path ends so.
    #[derive(Clone, Copy, Default)]
    struct Faults {
        short_read: Option<&'static [u8]>,
        short_write: Option<&'static [u8]>,
        close: Option<&'static [u8]>,
        /// The write of this number to the file, counted from 0, writes the
        /// first half of its bytes and fails, as a write cut short on the
        /// card does.
        torn_write: Option<(&'static [u8], usize)>,
        /// Writes to the file report success but store one byte changed,
        /// as a card that loses data without telling would.
        garbled_write: Option<&'static [u8]>,
        /// The card has no room: no file is made and none grows.
        full: bool,
    }
    #[derive(Default)]
    struct Disk {
        files: BTreeMap<Vec<u8>, Vec<u8>>,
        faults: Faults,
        /// Writes with a flush: path and offset.
        flushes: Vec<(Vec<u8>, u64)>,
        /// Writes to the file of `torn_write` so far.
        writes: usize,
    }
    fn matches(path: &[u8], end: Option<&[u8]>) -> bool {
        end.is_some_and(|end| path.ends_with(end))
    }
    struct Sim(Rc<RefCell<Disk>>);
    struct SimFile(Rc<RefCell<Disk>>, Vec<u8>);
    impl Card for Sim {
        type File = SimFile;
        fn open(&mut self, path: &[u8], flags: u32) -> Result<SimFile, Refused> {
            let mut disk = self.0.borrow_mut();
            if flags == OPEN_WRITE_CREATE && !disk.files.contains_key(path) {
                if disk.faults.full {
                    return Err(Refused::FULL);
                }
                disk.files.insert(path.to_vec(), Vec::new());
            }
            disk.files
                .contains_key(path)
                .then(|| SimFile(self.0.clone(), path.to_vec()))
                .ok_or(Refused::OTHER)
        }
    }
    impl SdFile for SimFile {
        fn size(&self) -> Option<u64> {
            Some(self.0.borrow().files[&self.1].len() as u64)
        }
        fn read(&self, offset: u64, bytes: &mut [u8]) -> bool {
            let disk = self.0.borrow();
            let data = &disk.files[&self.1];
            let end = offset as usize + bytes.len();
            if matches(&self.1, disk.faults.short_read) || end > data.len() {
                return false;
            }
            bytes.copy_from_slice(&data[offset as usize..end]);
            true
        }
        fn write(&self, offset: u64, bytes: &[u8], flush: bool) -> Result<(), Refused> {
            let mut disk = self.0.borrow_mut();
            if matches(&self.1, disk.faults.short_write) {
                return Err(Refused::OTHER);
            }
            if disk.faults.full && disk.files[&self.1].len() < offset as usize + bytes.len() {
                return Err(Refused::FULL);
            }
            let mut bytes = bytes;
            let mut torn = false;
            if let Some((end, at)) = disk.faults.torn_write {
                if self.1.ends_with(end) {
                    torn = disk.writes == at;
                    disk.writes += 1;
                }
            }
            if torn {
                bytes = &bytes[..bytes.len() / 2];
            }
            let mut garbled;
            if matches(&self.1, disk.faults.garbled_write) && !bytes.is_empty() {
                garbled = bytes.to_vec();
                garbled[0] ^= 0x40;
                bytes = &garbled;
            }
            let data = disk.files.get_mut(&self.1).unwrap();
            let end = offset as usize + bytes.len();
            if data.len() < end {
                data.resize(end, 0);
            }
            data[offset as usize..end].copy_from_slice(bytes);
            if torn {
                return Err(Refused::OTHER);
            }
            if flush {
                disk.flushes.push((self.1.clone(), offset));
            }
            Ok(())
        }
        fn close(self) -> bool {
            !matches(&self.1, self.0.borrow().faults.close)
        }
    }

    const SAV: &[u8] = b".sav\0";
    const BAK: &[u8] = b".bak\0";

    /// A card with a German Black save, and the saves the scan found on it.
    fn card() -> (Sim, Saves, Save, Vec<u8>) {
        let save = Save {
            game: 0,
            language: b'D',
        };
        let mut path = [0; PATH_CAPACITY];
        let length = save.path(false, &mut path);
        let mut data = vec![0u8; SAVE_SIZE as usize];
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = index as u8;
        }
        for offset in save.identity_offsets() {
            data[offset as usize..offset as usize + 2].copy_from_slice(&[5, 21]);
        }
        let disk = Rc::new(RefCell::new(Disk::default()));
        disk.borrow_mut()
            .files
            .insert(path[..length].to_vec(), data.clone());
        let mut sim = Sim(disk);
        let mut saves = Saves::EMPTY;
        saves.scan(None, b'D', |path, offsets| {
            identity(&mut sim, path, offsets)
        });
        assert_eq!(saves.current(), Some(save));
        (sim, saves, save, data)
    }
    fn file(sim: &Sim, end: &[u8]) -> Vec<u8> {
        let disk = sim.0.borrow();
        let (_, data) = disk
            .files
            .iter()
            .find(|(path, _)| path.ends_with(end))
            .unwrap();
        data.clone()
    }

    #[test]
    fn the_first_write_backs_the_save_up_and_every_write_is_flushed() {
        let (mut sim, mut saves, save, original) = card();
        let mut chunk = [0; COPY_CHUNK];
        assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_ok());
        assert_eq!(file(&sim, BAK), original);
        assert_eq!(file(&sim, SAV)[0x100..0x104], [1, 2, 3, 4]);
        // The backup is flushed with its last chunk, the save with its write.
        let flushes = sim.0.borrow().flushes.clone();
        assert_eq!(flushes.len(), 2);
        assert!(flushes[0].0.ends_with(BAK) && flushes[0].1 == SAVE_SIZE - COPY_CHUNK as u64);
        assert!(flushes[1].0.ends_with(SAV) && flushes[1].1 == 0x100);
        // The next write of the session makes no second backup.
        assert!(write(&mut sim, &mut saves, save, 0x200, &[9], &mut chunk).is_ok());
        assert_eq!(file(&sim, BAK), original);
        assert_eq!(sim.0.borrow().flushes.len(), 3);
    }

    #[test]
    fn without_its_backup_the_save_is_not_changed_and_the_next_write_tries_again() {
        let failures = [
            Faults {
                short_read: Some(SAV),
                ..Faults::default()
            },
            Faults {
                short_write: Some(BAK),
                ..Faults::default()
            },
            Faults {
                close: Some(BAK),
                ..Faults::default()
            },
        ];
        for faults in failures {
            let (mut sim, mut saves, save, original) = card();
            let mut chunk = [0; COPY_CHUNK];
            sim.0.borrow_mut().faults = faults;
            assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_err());
            assert_eq!(file(&sim, SAV), original);
            sim.0.borrow_mut().faults = Faults::default();
            assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_ok());
            assert_eq!(file(&sim, BAK), original);
            assert_eq!(file(&sim, SAV)[0x100..0x104], [1, 2, 3, 4]);
        }
    }

    #[test]
    fn a_card_without_room_for_the_backup_says_so_and_leaves_the_save() {
        let (mut sim, mut saves, save, original) = card();
        let mut chunk = [0; COPY_CHUNK];
        sim.0.borrow_mut().faults.full = true;
        let written = write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk);
        assert_eq!(written, Err(Refused::FULL));
        assert_eq!(file(&sim, SAV), original);
        assert!(!sim.0.borrow().files.keys().any(|path| path.ends_with(BAK)));
        // Every other refusal is no lack of space.
        sim.0.borrow_mut().faults = Faults {
            short_write: Some(BAK),
            ..Faults::default()
        };
        let written = write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk);
        assert_eq!(written, Err(Refused::OTHER));
        // With room again the next write makes the backup.
        sim.0.borrow_mut().faults = Faults::default();
        let written = write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk);
        assert_eq!(written, Ok(()));
        assert_eq!(file(&sim, BAK), original);
    }

    #[test]
    fn a_backup_cut_short_after_some_chunks_leaves_the_save_and_is_made_whole_next_time() {
        let chunks = (SAVE_SIZE / COPY_CHUNK as u64) as usize;
        for at in [0, 2, chunks - 1] {
            let (mut sim, mut saves, save, original) = card();
            // The backup of an earlier session, of other bytes.
            let mut path = [0; PATH_CAPACITY];
            let length = save.path(true, &mut path);
            sim.0
                .borrow_mut()
                .files
                .insert(path[..length].to_vec(), vec![0xee; SAVE_SIZE as usize]);
            let mut chunk = [0; COPY_CHUNK];
            sim.0.borrow_mut().faults = Faults {
                torn_write: Some((BAK, at)),
                ..Faults::default()
            };
            assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_err());
            // The save is untouched, and the backup is neither the old one
            // nor the new one.
            assert_eq!(file(&sim, SAV), original, "chunk {at}");
            let torn = file(&sim, BAK);
            assert!(torn != original && torn != vec![0xee; SAVE_SIZE as usize]);
            // The next write backs up again, whole, and then writes.
            sim.0.borrow_mut().faults = Faults::default();
            assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_ok());
            assert_eq!(file(&sim, BAK), original, "chunk {at}");
            assert_eq!(file(&sim, SAV)[0x100..0x104], [1, 2, 3, 4]);
        }
    }

    #[test]
    fn a_backup_that_does_not_read_back_as_the_save_leaves_the_save_unchanged() {
        let (mut sim, mut saves, save, original) = card();
        let mut chunk = [0; COPY_CHUNK];
        sim.0.borrow_mut().faults = Faults {
            garbled_write: Some(BAK),
            ..Faults::default()
        };
        assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_err());
        assert_eq!(file(&sim, SAV), original);
        assert!(file(&sim, BAK) != original);
        // The next write backs up again; read back whole, the save changes.
        sim.0.borrow_mut().faults = Faults::default();
        assert!(write(&mut sim, &mut saves, save, 0x100, &[1, 2, 3, 4], &mut chunk).is_ok());
        assert_eq!(file(&sim, BAK), original);
        assert_eq!(file(&sim, SAV)[0x100..0x104], [1, 2, 3, 4]);
    }

    #[test]
    fn a_save_write_counts_only_when_the_card_holds_it_and_closes_the_save() {
        for faults in [
            // A card that changes a byte without telling.
            Faults {
                garbled_write: Some(SAV),
                ..Faults::default()
            },
            // A close of the save that reports an error.
            Faults {
                close: Some(SAV),
                ..Faults::default()
            },
        ] {
            let (mut sim, mut saves, save, original) = card();
            let mut chunk = [0; COPY_CHUNK];
            // The session's backup is made first, as it always is.
            assert!(write(&mut sim, &mut saves, save, 0x10, &[7; 4], &mut chunk).is_ok());
            sim.0.borrow_mut().faults = faults;
            // A write longer than the copy buffer is read back in parts.
            let bytes = vec![0x5a; COPY_CHUNK + 100];
            assert!(write(&mut sim, &mut saves, save, 0x200, &bytes, &mut chunk).is_err());
            assert_eq!(file(&sim, BAK), original);
            sim.0.borrow_mut().faults = Faults::default();
            assert!(write(&mut sim, &mut saves, save, 0x200, &bytes, &mut chunk).is_ok());
            assert_eq!(file(&sim, SAV)[0x200..0x200 + bytes.len()], bytes[..]);
        }
    }

    #[test]
    fn a_read_or_write_past_the_save_or_short_of_it_fails() {
        let (mut sim, mut saves, save, original) = card();
        let mut chunk = [0; COPY_CHUNK];
        let mut bytes = [0; 4];
        assert!(read(&mut sim, save, 0x10, &mut bytes));
        assert_eq!(bytes, original[0x10..0x14]);
        assert!(!read(&mut sim, save, SAVE_SIZE as u32 - 2, &mut bytes));
        assert!(write(
            &mut sim,
            &mut saves,
            save,
            SAVE_SIZE as u32 - 2,
            &[0; 4],
            &mut chunk
        )
        .is_err());
        sim.0.borrow_mut().faults.short_read = Some(SAV);
        assert!(!read(&mut sim, save, 0x10, &mut bytes));
        sim.0.borrow_mut().faults = Faults {
            short_write: Some(SAV),
            ..Faults::default()
        };
        assert!(write(&mut sim, &mut saves, save, 0x10, &[7; 4], &mut chunk).is_err());
        assert_eq!(file(&sim, SAV)[0x10..0x14], original[0x10..0x14]);
    }
}
