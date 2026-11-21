//! The plain files of the SD card that Bank's box files live in
//! (`/3ds/Pokémon Bank/`, docs/internals.md, "Box files"), as the backups and
//! the transfer screen reach them: `Sd`, on the console over
//! `patch_common::sd`, in the tests a simulated card (`testing`). Each
//! operation is whole or reports a failure: a file is written, flushed,
//! closed and read back before `put` answers, and a reader checks what it
//! reads (`offline_core::boxfile`). `Extdata` reaches folders and files of
//! Bank's extdata archive the same way, by the rules of that archive: a
//! file has the size it was made with.

use offline_core::{boxfile::FILE_SIZE, native_blob::BOX_COUNT};

/// Bank's SD extdata archive.
pub const BANK_EXTDATA: u32 = 0xc9b;

/// A date and time of the console's clock, which names a folder: year,
/// month, day, hour, minute, second. Bank reads it on the main thread with
/// the original's clock and hands it to the worker with the job.
pub type Date = [u16; 6];

/// The date one second after `date`, by the Gregorian calendar.
pub fn next_second(date: Date) -> Date {
    let [year, month, day, hour, minute, second] = date;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    match () {
        () if second < 59 => [year, month, day, hour, minute, second + 1],
        () if minute < 59 => [year, month, day, hour, minute + 1, 0],
        () if hour < 23 => [year, month, day, hour + 1, 0, 0],
        () if day < days => [year, month, day + 1, 0, 0, 0],
        () if month < 12 => [year, month + 1, 1, 0, 0, 0],
        () => [year.wrapping_add(1), 1, 1, 0, 0, 0],
    }
}

/// `name` without `prefix` at its start, compared as the SD card's FAT
/// compares names: without regard to case. A name that fits 8.3, such as
/// `mover1.bak`, may be listed in capitals.
pub fn strip_prefix_ignoring_case<'a>(name: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let (start, rest) = name.split_at_checked(prefix.len())?;
    start.eq_ignore_ascii_case(prefix).then_some(rest)
}

/// `name` without `suffix` at its end, without regard to case.
pub fn strip_suffix_ignoring_case<'a>(name: &'a [u8], suffix: &[u8]) -> Option<&'a [u8]> {
    let (rest, end) = name.split_at_checked(name.len().checked_sub(suffix.len())?)?;
    end.eq_ignore_ascii_case(suffix).then_some(rest)
}

/// The folder the root lies in, where homebrew keeps its data.
pub const PARENT: &[u8] = b"/3ds";
/// The root folder of everything the patches create on the SD card,
/// `/3ds/Pokémon Bank`: one byte per character, the "é" as Latin-1 has it.
pub const ROOT: &[u8] = b"/3ds/Pok\xe9mon Bank";

/// A path of the SD card, one byte per character (ASCII and the upper half
/// of Latin-1), kept with its terminator.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Path {
    bytes: [u8; patch_common::sd::PATH_MAX],
    /// Without the terminator.
    len: usize,
}
impl core::fmt::Debug for Path {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.bytes[..self.len] {
            write!(out, "{}", *byte as char)?;
        }
        Ok(())
    }
}
impl Path {
    pub fn new(text: &[u8]) -> Self {
        let mut path = Self {
            bytes: [0; patch_common::sd::PATH_MAX],
            len: 0,
        };
        path.push(text);
        path
    }
    /// Appends `text`; what does not fit is left out, and the path is then
    /// one the SD card is never asked for (`valid`).
    pub fn push(&mut self, text: &[u8]) -> &mut Self {
        for byte in text {
            if self.len + 1 < self.bytes.len() {
                self.bytes[self.len] = *byte;
                self.len += 1;
            } else {
                self.len = self.bytes.len();
            }
        }
        self
    }
    /// Appends `value` as 8 upper-case hexadecimal digits.
    pub fn push_hex(&mut self, value: u32) -> &mut Self {
        let mut digits = [0u8; 8];
        for (index, digit) in digits.iter_mut().enumerate() {
            let nibble = (value >> (28 - 4 * index)) & 15;
            *digit = b"0123456789ABCDEF"[nibble as usize];
        }
        self.push(&digits)
    }
    /// Appends `value` as decimal digits, at least `width` of them.
    pub fn push_decimal(&mut self, value: u32, width: usize) -> &mut Self {
        let mut digits = [b'0'; 10];
        let mut rest = value;
        let mut count = 0;
        while rest != 0 || count < width.max(1) {
            digits[9 - count] = b'0' + (rest % 10) as u8;
            rest /= 10;
            count += 1;
        }
        self.push(&digits[10 - count..])
    }
    /// Appends the name of a box by its place: `bank01`..`bank99` for the
    /// boxes 1 to 99 of the Bank and `bank00` for box 100, `mover0` for the
    /// transport box (place 100), `mover1`.. for the cells behind it.
    pub fn push_box(&mut self, place: usize) -> &mut Self {
        if place < BOX_COUNT {
            self.push(b"bank").push_decimal((place as u32 + 1) % 100, 2)
        } else {
            self.push(b"mover")
                .push_decimal((place - BOX_COUNT) as u32, 1)
        }
    }
    /// The path with its terminator, as the SD card takes it; nothing for a
    /// path that did not fit, which the card's commands refuse
    /// (`patch_common::sd::valid`).
    pub fn terminated(&self) -> &[u8] {
        if self.len < self.bytes.len() {
            &self.bytes[..self.len + 1]
        } else {
            &[]
        }
    }
    /// The path without its terminator.
    pub fn text(&self) -> &[u8] {
        &self.bytes[..self.len.min(self.bytes.len() - 1)]
    }
    pub fn valid(&self) -> bool {
        self.len < self.bytes.len() && patch_common::sd::valid(self.terminated())
    }
}

/// A failure of work with the box files of the SD card: of Bank's own files
/// (`E`, their storage's error), of the card (`S`), or of what was found.
#[derive(Debug)]
pub enum Error<E, S> {
    Files(crate::bank_files::Error<E>),
    Sd(S),
    /// The box file of this place is not a whole box file.
    BadFile(usize),
    /// The room handed over is not what the work needs.
    Staging,
    /// What was written does not read back as it was meant.
    NotAsWritten,
}

/// `Error` for Bank's files `F` and the card `S`.
pub type Failed<F, S> = Error<
    <<F as crate::bank_files::Files>::Storage as offline_core::Storage>::Error,
    <S as Store>::Error,
>;

/// What a read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// There is no such file.
    Absent,
    /// The file is there, but not of the size asked for.
    OtherSize,
    /// The file was read whole.
    Read,
}

/// What a read of a piece of an extdata file found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// It was read whole.
    Read,
    /// There is no such file, or it ends before the piece does.
    Missing,
    /// The console says its data does not read (`fs_result::damaged`): an
    /// interrupted write left it.
    Damaged,
}

/// What the SD card and Bank's extdata do alike: folders, and deleting.
pub trait Store {
    type Error: Copy + core::fmt::Debug;
    /// Makes the folder; one that is there counts as made.
    fn create_dir(&mut self, path: &Path) -> Result<(), Self::Error>;
    /// Deletes the file; one that is not there counts as deleted.
    fn delete(&mut self, path: &Path) -> Result<(), Self::Error>;
    /// Deletes the folder if it is empty; one that is not there counts as
    /// deleted. A folder that is not empty is an error, which the callers
    /// leave as it is: nothing depends on a folder.
    fn remove_dir(&mut self, path: &Path) -> Result<(), Self::Error>;
    /// Calls `each` with the name of every entry of the folder at `path`,
    /// one byte per character, and whether it is a folder. A name with a
    /// character beyond Latin-1 is left out: the patches name none so. A
    /// folder that is not there has no entries.
    fn list(&mut self, path: &Path, each: &mut dyn FnMut(&[u8], bool)) -> Result<(), Self::Error>;
    /// Whether the card refused for lack of space.
    fn no_room(error: &Self::Error) -> bool;
    /// The second number of the error screen for `error`.
    fn code(error: &Self::Error) -> u32;
}

/// The plain files of the SD card.
pub trait Sd: Store {
    /// Makes the file hold exactly `bytes`: written, flushed, closed and read
    /// back. A file that was there is replaced.
    fn put(&mut self, path: &Path, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Reads the whole file into `out`, if it is `out.len()` bytes.
    fn get(&mut self, path: &Path, out: &mut [u8]) -> Result<Found, Self::Error>;
    /// Renames a folder, with everything in it, within its folder; `to`
    /// must not be there.
    fn rename_dir(&mut self, from: &Path, to: &Path) -> Result<(), Self::Error>;
}

/// Files in folders of Bank's extdata. A file has the size it is made with,
/// and a write never changes it.
pub trait Extdata: Store {
    /// Makes the file anew with `length` bytes; one that was there is gone.
    fn create(&mut self, path: &Path, length: u32) -> Result<(), Self::Error>;
    /// Writes `bytes` at `offset` and flushes them.
    fn write_at(&mut self, path: &Path, offset: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Reads `out.len()` bytes at `offset`.
    fn read_at(&mut self, path: &Path, offset: u32, out: &mut [u8]) -> Result<Piece, Self::Error>;
    /// Lets go of the archive; the next call opens it again.
    fn rest(&mut self);
}

/// The folder of `date` (year, month, day, hour, minute, second) in the
/// root folder: `YYYYMMDD-HHMMSS`.
pub fn dated_folder(date: Date) -> Path {
    let mut path = Path::new(ROOT);
    path.push(b"/");
    path.push_decimal(u32::from(date[0]), 4)
        .push_decimal(u32::from(date[1]), 2)
        .push_decimal(u32::from(date[2]), 2)
        .push(b"-")
        .push_decimal(u32::from(date[3]), 2)
        .push_decimal(u32::from(date[4]), 2)
        .push_decimal(u32::from(date[5]), 2);
    path
}

/// A name in the root folder that is not there, named as `dated_folder`
/// names `date`, with `suffix` behind: while an entry of the root folder
/// already has the name, the date goes on by a second (`next_second`), so
/// that nothing goes over a box an earlier folder keeps, should the
/// console's clock come back to its second.
pub fn free_dated_folder<S: Sd>(
    sd: &mut S,
    mut date: Date,
    suffix: &[u8],
) -> Result<Path, S::Error> {
    loop {
        let mut name = dated_folder(date);
        name.push(suffix);
        let mut taken = false;
        sd.list(&Path::new(ROOT), &mut |entry, _| {
            taken |= entry.eq_ignore_ascii_case(&name.text()[ROOT.len() + 1..]);
        })?;
        if !taken {
            return Ok(name);
        }
        date = next_second(date);
    }
}

/// Makes a folder of the root folder that was not there, named by `date`
/// (`free_dated_folder`). The root folder must be there.
pub fn create_dated_folder<S: Sd>(sd: &mut S, date: Date) -> Result<Path, S::Error> {
    let folder = free_dated_folder(sd, date, b"")?;
    sd.create_dir(&folder)?;
    Ok(folder)
}

/// The box file of `place` (`Path::push_box`) in `folder`: `bank01.box`..,
/// `mover0.box`..
pub fn box_file(folder: &Path, place: usize) -> Path {
    let mut path = *folder;
    path.push(b"/").push_box(place).push(b".box");
    path
}

/// Makes the root folder, and the folder it lies in before it.
pub fn create_root<S: Sd>(sd: &mut S) -> Result<(), S::Error> {
    sd.create_dir(&Path::new(PARENT))?;
    sd.create_dir(&Path::new(ROOT))
}

/// Every call once, on files of its own, and nothing left behind: the
/// root folder, a folder of its own, a file written into it and read back,
/// the folder found in the root folder's list, renamed, the file read under
/// its new name, then both deleted. The console test build makes it at
/// every opening. The answer is whether the file read back as written and
/// the folder was listed.
pub fn probe<S: Sd>(sd: &mut S) -> Result<bool, S::Error> {
    let at = |name: &[u8]| {
        let mut path = Path::new(ROOT);
        path.push(name);
        path
    };
    let (first, second) = (at(b"/probe"), at(b"/probe2"));
    let (file, moved) = (at(b"/probe/probe.bin"), at(b"/probe2/probe.bin"));
    let bytes = *b"BKPROBE1";
    create_root(sd)?;
    // What a probe cut short left.
    for (file, folder) in [(&file, &first), (&moved, &second)] {
        sd.delete(file)?;
        sd.remove_dir(folder)?;
    }
    sd.create_dir(&first)?;
    sd.put(&file, &bytes)?;
    let mut listed = false;
    sd.list(&Path::new(ROOT), &mut |name, folder| {
        listed |= folder && name.eq_ignore_ascii_case(b"probe");
    })?;
    sd.rename_dir(&first, &second)?;
    let mut read = [0u8; 8];
    let found = sd.get(&moved, &mut read)?;
    sd.delete(&moved)?;
    sd.remove_dir(&second)?;
    Ok(found == Found::Read && read == bytes && listed)
}

/// Every call Bank's extdata folders need, on a folder of its own with the
/// longest name the archive stores, and nothing left behind: a file of the
/// largest size a backup's file has, written whole, a piece read at an
/// offset inside it and one past its end, the folder and the file found in
/// their lists, then both deleted. The console test build makes it at every
/// opening. The answer is whether all of it was as written.
pub fn probe_extdata<X: Extdata>(ext: &mut X) -> Result<bool, X::Error> {
    const LENGTH: usize = 4 * FILE_SIZE;
    const PIECE: usize = 512;
    let (root, folder) = (Path::new(b"/"), Path::new(b"/0123456789ABCDEF"));
    let file = Path::new(b"/0123456789ABCDEF/bank00.old");
    // What a probe cut short left.
    ext.delete(&file)?;
    ext.remove_dir(&folder)?;
    ext.create_dir(&folder)?;
    ext.create(&file, LENGTH as u32)?;
    for offset in (0..LENGTH).step_by(PIECE) {
        let bytes = [(offset / PIECE) as u8; PIECE];
        let count = PIECE.min(LENGTH - offset);
        ext.write_at(&file, offset as u32, &bytes[..count])?;
    }
    // A piece across the pieces written, at the last box's start.
    let at = 3 * FILE_SIZE;
    let mut read = [0u8; 2 * PIECE];
    let found = ext.read_at(&file, at as u32, &mut read)?;
    let same = found == Piece::Read
        && read
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == ((at + index) / PIECE) as u8);
    let past = ext.read_at(&file, LENGTH as u32 - 4, &mut [0; 8])?;
    let mut listed = false;
    ext.list(&root, &mut |name, is_folder| {
        listed |= is_folder && name.eq_ignore_ascii_case(b"0123456789ABCDEF");
    })?;
    let mut inside = false;
    ext.list(&folder, &mut |name, is_folder| {
        inside |= !is_folder && name.eq_ignore_ascii_case(b"bank00.old");
    })?;
    ext.delete(&file)?;
    ext.remove_dir(&folder)?;
    let mut left = false;
    ext.list(&root, &mut |name, _| {
        left |= name.eq_ignore_ascii_case(b"0123456789ABCDEF");
    })?;
    Ok(same && past == Piece::Missing && listed && inside && !left)
}

/// No SD card: every call fails. For a settle that writes no backup.
pub struct NoSd;
impl Sd for NoSd {
    fn put(&mut self, _: &Path, _: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn get(&mut self, _: &Path, _: &mut [u8]) -> Result<Found, ()> {
        Err(())
    }
    fn rename_dir(&mut self, _: &Path, _: &Path) -> Result<(), ()> {
        Err(())
    }
}
impl Extdata for NoSd {
    fn create(&mut self, _: &Path, _: u32) -> Result<(), ()> {
        Err(())
    }
    fn write_at(&mut self, _: &Path, _: u32, _: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn read_at(&mut self, _: &Path, _: u32, _: &mut [u8]) -> Result<Piece, ()> {
        Err(())
    }
    fn rest(&mut self) {}
}
impl Store for NoSd {
    type Error = ();
    fn create_dir(&mut self, _: &Path) -> Result<(), ()> {
        Err(())
    }
    fn delete(&mut self, _: &Path) -> Result<(), ()> {
        Err(())
    }
    fn remove_dir(&mut self, _: &Path) -> Result<(), ()> {
        Err(())
    }
    fn list(&mut self, _: &Path, _: &mut dyn FnMut(&[u8], bool)) -> Result<(), ()> {
        Err(())
    }
    fn no_room(_: &()) -> bool {
        false
    }
    fn code(_: &()) -> u32 {
        0
    }
}

#[cfg(target_arch = "arm")]
pub use console::{ConsoleExtdata, ConsoleSd};

#[cfg(target_arch = "arm")]
mod console {
    use super::{Extdata, Found, Path, Piece, Sd, Store, BANK_EXTDATA};
    use core::marker::PhantomData;
    use patch_common::{
        file::FLUSH,
        fs_result::{damaged, lack_of_space},
        program::Program,
        sd::{self, OPEN_READ, OPEN_READ_WRITE, OPEN_WRITE_CREATE},
    };

    /// Which archive a `Console` works in.
    pub trait Place {
        fn open<P: Program>() -> Result<sd::Archive<P>, patch_common::file::Code>;
    }
    /// The SD card.
    pub struct Card;
    impl Place for Card {
        fn open<P: Program>() -> Result<sd::Archive<P>, patch_common::file::Code> {
            sd::Archive::open()
        }
    }
    /// Bank's extdata.
    pub struct BankData;
    impl Place for BankData {
        fn open<P: Program>() -> Result<sd::Archive<P>, patch_common::file::Code> {
            sd::Archive::open_extdata(BANK_EXTDATA)
        }
    }

    /// An archive through the program's own session, on the thread that
    /// owns the program's file handles (the worker). It is opened at the
    /// first command and kept until it is dropped, at the end of the job, so
    /// that a job of many commands opens it once.
    pub struct Console<P: Program, W: Place> {
        archive: Option<sd::Archive<P>>,
        program: PhantomData<(P, W, *mut ())>,
    }
    /// The SD card.
    pub type ConsoleSd<P> = Console<P, Card>;
    /// Bank's extdata.
    pub type ConsoleExtdata<P> = Console<P, BankData>;

    impl<P: Program, W: Place> Console<P, W> {
        /// # Safety
        /// Only on the worker of program `P`, after its start-up.
        pub const unsafe fn new() -> Self {
            Self {
                archive: None,
                program: PhantomData,
            }
        }

        fn archive(&mut self) -> Result<&sd::Archive<P>, Refused> {
            if self.archive.is_none() {
                self.archive = Some(W::open().map_err(refused)?);
            }
            self.archive.as_ref().ok_or(Refused(0))
        }
    }

    /// A failure: the console's result, or 0 for a file that did not read
    /// back as written.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Refused(pub u32);

    fn refused(code: i32) -> Refused {
        Refused(code as u32)
    }

    impl<P: Program, W: Place> Store for Console<P, W> {
        type Error = Refused;
        fn create_dir(&mut self, path: &Path) -> Result<(), Refused> {
            self.archive()?
                .create_directory(path.terminated())
                .map_err(refused)
        }
        fn delete(&mut self, path: &Path) -> Result<(), Refused> {
            self.archive()?.delete(path.terminated()).map_err(refused)
        }
        fn remove_dir(&mut self, path: &Path) -> Result<(), Refused> {
            self.archive()?
                .delete_directory(path.terminated())
                .map_err(refused)
        }
        fn list(&mut self, path: &Path, each: &mut dyn FnMut(&[u8], bool)) -> Result<(), Refused> {
            self.archive()?
                .list(path.terminated(), &mut |units, folder| {
                    let mut name = [0u8; sd::command::ENTRY_NAME];
                    for (byte, unit) in name.iter_mut().zip(units) {
                        match u8::try_from(*unit) {
                            Ok(value) => *byte = value,
                            Err(_) => return,
                        }
                    }
                    each(&name[..units.len().min(name.len())], folder);
                })
                .map_err(refused)
        }
        fn no_room(error: &Refused) -> bool {
            lack_of_space(error.0)
        }
        fn code(error: &Refused) -> u32 {
            error.0
        }
    }

    impl<P: Program> Extdata for ConsoleExtdata<P> {
        fn create(&mut self, path: &Path, length: u32) -> Result<(), Refused> {
            let archive = self.archive()?;
            archive.delete(path.terminated()).map_err(refused)?;
            archive
                .create_file(path.terminated(), u64::from(length))
                .map_err(refused)
        }
        fn write_at(&mut self, path: &Path, offset: u32, bytes: &[u8]) -> Result<(), Refused> {
            let file = self
                .archive()?
                .open_file(path.terminated(), OPEN_READ_WRITE)
                .map_err(refused)?;
            let written = file.write(u64::from(offset), bytes, FLUSH).map_err(refused);
            let (closed, handle) = file.close();
            if written? as usize != bytes.len() {
                return Err(Refused(0));
            }
            closed.map_err(refused)?;
            handle.map_err(refused)
        }
        fn read_at(&mut self, path: &Path, offset: u32, out: &mut [u8]) -> Result<Piece, Refused> {
            let file = match self
                .archive()?
                .open_file(path.terminated(), OPEN_READ_WRITE)
            {
                Ok(file) => file,
                Err(code) if sd::not_found(code as u32) => return Ok(Piece::Missing),
                Err(code) if damaged(code as u32) => return Ok(Piece::Damaged),
                Err(code) => return Err(refused(code)),
            };
            if u64::from(offset) + out.len() as u64 > file.size().map_err(refused)? {
                return Ok(Piece::Missing);
            }
            let count = match file.read(u64::from(offset), out) {
                Ok(count) => count,
                Err(code) if damaged(code as u32) => return Ok(Piece::Damaged),
                Err(code) => return Err(refused(code)),
            };
            let (closed, handle) = file.close();
            if count as usize != out.len() {
                return Err(Refused(0));
            }
            closed.map_err(refused)?;
            handle.map_err(refused)?;
            Ok(Piece::Read)
        }
        fn rest(&mut self) {
            self.archive = None;
        }
    }

    impl<P: Program> Sd for ConsoleSd<P> {
        fn put(&mut self, path: &Path, bytes: &[u8]) -> Result<(), Refused> {
            let path = path.terminated();
            self.archive()?.delete(path).map_err(refused)?;
            let file = sd::open::<P>(path, OPEN_WRITE_CREATE).map_err(refused)?;
            let written = file.write(0, bytes, FLUSH).map_err(refused);
            let (closed, handle) = file.close();
            match written? {
                count if count as usize == bytes.len() => {}
                _ => return Err(Refused(0)),
            }
            closed.map_err(refused)?;
            handle.map_err(refused)?;
            // Read back against what was meant, in pieces.
            let file = sd::open::<P>(path, OPEN_READ).map_err(refused)?;
            if file.size().map_err(refused)? != bytes.len() as u64 {
                return Err(Refused(0));
            }
            let mut piece = [0u8; 512];
            for (index, part) in bytes.chunks(piece.len()).enumerate() {
                let read = &mut piece[..part.len()];
                let count = file.read((index * 512) as u64, read).map_err(refused)?;
                if count as usize != part.len() || read != part {
                    return Err(Refused(0));
                }
            }
            let (closed, handle) = file.close();
            closed.map_err(refused)?;
            handle.map_err(refused)
        }
        fn get(&mut self, path: &Path, out: &mut [u8]) -> Result<Found, Refused> {
            let file = match sd::open::<P>(path.terminated(), OPEN_READ) {
                Ok(file) => file,
                Err(code) if sd::not_found(code as u32) => return Ok(Found::Absent),
                Err(code) => return Err(refused(code)),
            };
            if file.size().map_err(refused)? != out.len() as u64 {
                return Ok(Found::OtherSize);
            }
            let count = file.read(0, out).map_err(refused)?;
            let (closed, handle) = file.close();
            if count as usize != out.len() {
                return Err(Refused(0));
            }
            closed.map_err(refused)?;
            handle.map_err(refused)?;
            Ok(Found::Read)
        }
        fn rename_dir(&mut self, from: &Path, to: &Path) -> Result<(), Refused> {
            self.archive()?
                .rename_directory(from.terminated(), to.terminated())
                .map_err(refused)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_leaves_nothing_behind() {
        let disk = crate::testing::Shared::default();
        assert!(probe(&mut disk.clone()).unwrap());
        assert!(disk.0.borrow().sd.is_empty());
        assert!(disk.0.borrow().sd_dirs.contains(ROOT));
        assert!(disk.0.borrow().sd_dirs.contains(PARENT));
    }

    #[test]
    fn the_extdata_probe_leaves_nothing_behind() {
        let disk = crate::testing::Shared::default();
        let mut ext = crate::testing::ExtCard(disk.clone());
        assert!(probe_extdata(&mut ext).unwrap());
        assert!(disk.0.borrow().ext.is_empty());
        assert!(disk.0.borrow().ext_dirs.is_empty());
        // A cut at every step: the next probe finds what it left and ends
        // with nothing either.
        for budget in 0.. {
            let disk = crate::testing::Shared::default();
            disk.0.borrow_mut().budget = Some(budget);
            let mut ext = crate::testing::ExtCard(disk.clone());
            let done = probe_extdata(&mut ext).is_ok();
            disk.0.borrow_mut().budget = None;
            assert!(probe_extdata(&mut ext).unwrap(), "cut at {budget}");
            assert!(disk.0.borrow().ext.is_empty(), "cut at {budget}");
            assert!(disk.0.borrow().ext_dirs.is_empty(), "cut at {budget}");
            if done {
                break;
            }
        }
    }

    #[test]
    fn paths_are_built_with_their_terminator() {
        let mut path = Path::new(ROOT);
        path.push(b"/interrupted-").push_hex(0x00AB_12CD);
        assert_eq!(
            path.terminated(),
            b"/3ds/Pok\xe9mon Bank/interrupted-00AB12CD\0"
        );
        assert!(path.valid());
        let mut name = Path::new(b"/x/y");
        name.push_decimal(7, 3).push(b".bin");
        assert_eq!(name.text(), b"/x/y007.bin");
        let mut long = Path::new(b"/");
        long.push(&[b'a'; 80]);
        assert!(!long.valid());
        // A path that did not fit is never sent cut short.
        assert!(long.terminated().is_empty());
        assert!(!patch_common::sd::valid(long.terminated()));
    }

    #[test]
    fn a_dated_folder_is_the_first_free_second() {
        let disk = crate::testing::Shared::default();
        let mut sd = disk.clone();
        create_root(&mut sd).unwrap();
        // Twenty seconds taken from 23:59:50 on, across the day's end; one
        // of them by a file.
        let mut date = [2026, 12, 31, 23, 59, 50];
        for second in 0..20 {
            let mut path = dated_folder(date);
            path.push(b"-backup");
            if second == 7 {
                sd.put(&path, b"x").unwrap();
            } else {
                sd.create_dir(&path).unwrap();
            }
            date = next_second(date);
        }
        let free = free_dated_folder(&mut sd, [2026, 12, 31, 23, 59, 50], b"-backup");
        assert_eq!(
            free.unwrap().text(),
            [ROOT, b"/20270101-000010-backup"].concat()
        );
        // A session's folder, without the suffix: nothing of those is taken.
        let made = create_dated_folder(&mut sd, [2026, 12, 31, 23, 59, 50]);
        assert_eq!(made.unwrap().text(), [ROOT, b"/20261231-235950"].concat());
    }

    #[test]
    fn the_next_second_carries_into_the_minute_hour_day_month_and_year() {
        for (date, next) in [
            ([2026, 11, 16, 14, 3, 7], [2026, 11, 16, 14, 3, 8]),
            ([2026, 11, 16, 14, 3, 59], [2026, 11, 16, 14, 4, 0]),
            ([2026, 11, 16, 14, 59, 59], [2026, 11, 16, 15, 0, 0]),
            ([2026, 11, 16, 23, 59, 59], [2026, 11, 17, 0, 0, 0]),
            ([2026, 11, 30, 23, 59, 59], [2026, 12, 1, 0, 0, 0]),
            ([2026, 12, 31, 23, 59, 59], [2027, 1, 1, 0, 0, 0]),
            ([2027, 2, 28, 23, 59, 59], [2027, 3, 1, 0, 0, 0]),
            ([2028, 2, 28, 23, 59, 59], [2028, 2, 29, 0, 0, 0]),
            ([2100, 2, 28, 23, 59, 59], [2100, 3, 1, 0, 0, 0]),
            ([2000, 2, 28, 23, 59, 59], [2000, 2, 29, 0, 0, 0]),
        ] {
            assert_eq!(next_second(date), next);
        }
        // The seconds of a year that is no leap year, and of one that is.
        for (year, days) in [(2027u16, 365u32), (2028, 366)] {
            let mut date = [year, 1, 1, 0, 0, 0];
            for _ in 0..days * 86_400 {
                date = next_second(date);
            }
            assert_eq!(date, [year + 1, 1, 1, 0, 0, 0]);
        }
    }

    #[test]
    fn a_box_is_named_by_its_place() {
        let name = |place| {
            let mut path = Path::new(b"/");
            path.push_box(place).push(b".box");
            path.text().to_vec()
        };
        assert_eq!(name(0), b"/bank01.box");
        assert_eq!(name(20), b"/bank21.box");
        assert_eq!(name(98), b"/bank99.box");
        assert_eq!(name(99), b"/bank00.box");
        assert_eq!(name(100), b"/mover0.box");
        assert_eq!(name(101), b"/mover1.box");
        assert_eq!(name(103), b"/mover3.box");
    }

    #[test]
    fn names_compare_without_regard_to_case() {
        assert_eq!(
            strip_prefix_ignoring_case(b"INTERRUPTED-12", b"interrupted-"),
            Some(&b"12"[..])
        );
        assert_eq!(strip_prefix_ignoring_case(b"inter", b"interrupted-"), None);
        assert_eq!(
            strip_suffix_ignoring_case(b"MOVER1.BAK", b".bak"),
            Some(&b"MOVER1"[..])
        );
        assert_eq!(strip_suffix_ignoring_case(b"bak", b".bak"), None);
        assert_eq!(strip_suffix_ignoring_case(b"mover1.box", b".bak"), None);
    }
}
