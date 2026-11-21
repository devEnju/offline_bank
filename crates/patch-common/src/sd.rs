//! Plain files and folders of the SD card, the same for both programs: a
//! file is opened with its archive in one call, the SDK's own
//! (`Program::OPEN_FILE_DIRECTLY`); listing, creating and deleting a folder,
//! renaming a folder, and deleting a file are `fs:USER` commands the patch sends
//! itself on the program's session, built as the SDK builds its own: the
//! command in the thread's buffer, then the kernel's SendSyncRequest
//! (`kernel`). They go to the SD card's archive (ID 9 with an empty path) or
//! to an SD extdata archive (ID 6 with its ID), opened once for all of a
//! job's commands (`Archive`). A path is kept as
//! one byte per character with its terminator, ASCII and the upper half of
//! Latin-1 (the "é" of the box files' folder), and sent as UTF-16 (`wide`),
//! the path type libctru uses for every path of the SD card.

/// The SDK's open flags: read, read and write, write and create.
pub const OPEN_READ: u32 = 1;
pub const OPEN_READ_WRITE: u32 = 3;
pub const OPEN_WRITE_CREATE: u32 = 6;

/// The SD card's archive and the path types of its calls.
pub const ARCHIVE_SDMC: u32 = 9;
pub const ARCHIVE_EXTDATA: u32 = 6;
pub const PATH_EMPTY: u32 = 1;
pub const PATH_BINARY: u32 = 2;
pub const PATH_UTF16: u32 = 4;

/// The longest path the patch passes, in characters with its terminator.
pub const PATH_MAX: usize = 64;

/// Whether `path` is one the commands take: printable characters from `/`,
/// ASCII or `A0`..`FF` of Latin-1, with its terminator as the last byte and
/// nowhere else.
pub fn valid(path: &[u8]) -> bool {
    match path.split_last() {
        Some((0, text)) => {
            path.len() <= PATH_MAX
                && text.first() == Some(&b'/')
                && text
                    .iter()
                    .all(|byte| (0x20..0x7f).contains(byte) || *byte >= 0xa0)
        }
        _ => false,
    }
}

/// `path` as the card takes it: each byte a UTF-16 unit, which is what a
/// Latin-1 character is. What does not fit is left out; a valid path fits.
pub fn wide(path: &[u8]) -> [u16; PATH_MAX] {
    let mut units = [0u16; PATH_MAX];
    for (unit, byte) in units.iter_mut().zip(path) {
        *unit = u16::from(*byte);
    }
    units
}

/// The size in bytes of `path` as `wide` holds it, with its terminator.
pub const fn wide_size(path: &[u8]) -> u32 {
    (path.len() * 2) as u32
}

/// The command words, as the `fs:USER` service takes them
/// (libctru's `FSUSER_*`): the header, the arguments, and each path as a
/// static buffer with its own ID. `size` is the path's size in bytes as it
/// is sent (`wide_size`), `pointer` its address.
pub mod command {
    pub const DELETE_FILE: u32 = 0x0804_0142;
    pub const DELETE_DIRECTORY: u32 = 0x0806_0142;
    pub const OPEN_DIRECTORY: u32 = 0x080B_0102;
    /// On a folder's handle.
    pub const READ_DIRECTORY: u32 = 0x0801_0042;
    pub const CLOSE_DIRECTORY: u32 = 0x0802_0000;
    /// One entry of a folder as Read gives it (libctru's
    /// `FS_DirectoryEntry`): the name as UTF-16 with its end at 0, the
    /// attributes at `0x21C`, bit 0 a folder.
    pub const ENTRY_SIZE: usize = 0x228;
    pub const ENTRY_NAME: usize = 0x106;
    pub const ENTRY_ATTRIBUTES: usize = 0x21C;

    /// A buffer the service writes into (libctru's `IPC_Desc_Buffer` with
    /// `IPC_BUFFER_W`).
    pub const fn written_buffer(size: u32) -> u32 {
        (size << 4) | 0x8 | 0x4
    }

    pub fn open_directory(archive: u64, size: u32, pointer: u32) -> [u32; 7] {
        [
            OPEN_DIRECTORY,
            archive as u32,
            (archive >> 32) as u32,
            super::PATH_UTF16,
            size,
            static_buffer(size, 0),
            pointer,
        ]
    }

    pub fn read_directory(count: u32, pointer: u32) -> [u32; 4] {
        [
            READ_DIRECTORY,
            count,
            written_buffer(count * ENTRY_SIZE as u32),
            pointer,
        ]
    }
    /// RenameDirectory: the same words as RenameFile (`0x0805_0244`).
    pub const RENAME_DIRECTORY: u32 = 0x080A_0244;
    pub const CREATE_DIRECTORY: u32 = 0x0809_0182;

    /// A static buffer of `size` bytes with buffer ID `id`.
    pub const fn static_buffer(size: u32, id: u32) -> u32 {
        (size << 14) | (id << 10) | 2
    }

    pub fn delete_file(archive: u64, size: u32, pointer: u32) -> [u32; 8] {
        delete(DELETE_FILE, archive, size, pointer)
    }

    pub fn delete_directory(archive: u64, size: u32, pointer: u32) -> [u32; 8] {
        delete(DELETE_DIRECTORY, archive, size, pointer)
    }

    /// DeleteFile and DeleteDirectory take the same words.
    fn delete(header: u32, archive: u64, size: u32, pointer: u32) -> [u32; 8] {
        [
            header,
            0,
            archive as u32,
            (archive >> 32) as u32,
            super::PATH_UTF16,
            size,
            static_buffer(size, 0),
            pointer,
        ]
    }

    pub fn create_directory(archive: u64, size: u32, pointer: u32) -> [u32; 9] {
        [
            CREATE_DIRECTORY,
            0,
            archive as u32,
            (archive >> 32) as u32,
            super::PATH_UTF16,
            size,
            0,
            static_buffer(size, 0),
            pointer,
        ]
    }

    pub fn rename_directory(
        archive: u64,
        from_size: u32,
        from_pointer: u32,
        to_size: u32,
        to_pointer: u32,
    ) -> [u32; 14] {
        [
            RENAME_DIRECTORY,
            0,
            archive as u32,
            (archive >> 32) as u32,
            super::PATH_UTF16,
            from_size,
            archive as u32,
            (archive >> 32) as u32,
            super::PATH_UTF16,
            to_size,
            static_buffer(from_size, 1),
            from_pointer,
            static_buffer(to_size, 2),
            to_pointer,
        ]
    }
}

/// The console's answer for a path that is not there.
pub use crate::fs_result::not_found;
/// The console's answer for a folder or file that is there already
/// (`0xC82044BE`, `0xC82044B9`).
pub const fn already_exists(code: u32) -> bool {
    matches!(code, 0xc820_44be | 0xc820_44b9)
}

#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(target_arch = "arm")]
mod native {
    use super::{
        command, valid, wide, wide_size, ARCHIVE_EXTDATA, ARCHIVE_SDMC, PATH_BINARY, PATH_EMPTY,
        PATH_UTF16,
    };
    use crate::{
        file,
        kernel::{close_handle, send_sync_request},
        program::Program,
    };
    use core::{arch::asm, marker::PhantomData, mem::transmute};

    /// A failure: the console's result, or -1 for a path the commands do
    /// not take or a handle of 0 the console gave for one that opened.
    pub type Code = file::Code;
    const BAD_PATH: Code = -1;

    fn checked(path: &[u8]) -> Result<(), Code> {
        if valid(path) {
            Ok(())
        } else {
            Err(BAD_PATH)
        }
    }

    /// Opens the file at `path` with the SDK's `flags`.
    pub fn open<P: Program>(path: &[u8], flags: u32) -> Result<file::Handle<P>, Code> {
        checked(path)?;
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
        let open: Open = unsafe { transmute(P::OPEN_FILE_DIRECTLY) };
        let empty = [0u8; 4];
        let units = wide(path);
        let mut handle = 0;
        let code = unsafe {
            open(
                P::FS_SESSION as *const u32,
                &mut handle,
                0,
                ARCHIVE_SDMC,
                PATH_EMPTY,
                empty.as_ptr(),
                1,
                PATH_UTF16,
                units.as_ptr().cast(),
                wide_size(path),
                flags,
                0,
            )
        };
        if code < 0 {
            return Err(code);
        }
        unsafe { file::Handle::new(handle) }.ok_or(BAD_PATH)
    }

    #[repr(C)]
    struct Path {
        kind: u32,
        data: *const u8,
        byte_len: u32,
    }

    /// An archive, the SD card's or an extdata's, open: the commands are sent on it, so
    /// that a job of many of them opens it once. It is closed by `close`,
    /// or when it is dropped, where a failure to close counts for nothing:
    /// every command on it has answered by then.
    pub struct Archive<P: Program> {
        handle: u64,
        program: PhantomData<(P, *mut ())>,
    }

    type CloseArchive = unsafe extern "aapcs" fn(*const u32, u64) -> i32;

    impl<P: Program> Archive<P> {
        /// Opens archive 9 with an empty path on the program's session.
        pub fn open() -> Result<Self, Code> {
            let empty = [0u8; 4];
            Self::open_at(
                ARCHIVE_SDMC,
                Path {
                    kind: PATH_EMPTY,
                    data: empty.as_ptr(),
                    byte_len: 1,
                },
            )
        }

        /// Opens the SD extdata archive `id` on the program's session: media
        /// 1 (the SD card), then the ID's low and high word.
        pub fn open_extdata(id: u32) -> Result<Self, Code> {
            let binary = [1u32, id, 0];
            Self::open_at(
                ARCHIVE_EXTDATA,
                Path {
                    kind: PATH_BINARY,
                    data: binary.as_ptr().cast(),
                    byte_len: 12,
                },
            )
        }

        fn open_at(archive: u32, path: Path) -> Result<Self, Code> {
            type OpenArchive = unsafe extern "aapcs" fn(*const u32, *mut u64, u32, Path) -> i32;
            let session = unsafe { P::fs_session() };
            let mut handle = 0;
            let open: OpenArchive = unsafe { transmute(P::OPEN_ARCHIVE) };
            let code = unsafe { open(&session, &mut handle, archive, path) };
            if code < 0 {
                return Err(code);
            }
            if handle == 0 {
                return Err(BAD_PATH);
            }
            Ok(Self {
                handle,
                program: PhantomData,
            })
        }

        /// Closes it, with the console's answer.
        pub fn close(self) -> Result<(), Code> {
            let code = Self::close_handle(self.handle);
            core::mem::forget(self);
            if code < 0 {
                Err(code)
            } else {
                Ok(())
            }
        }

        fn close_handle(handle: u64) -> i32 {
            let session = unsafe { P::fs_session() };
            let close: CloseArchive = unsafe { transmute(P::CLOSE_ARCHIVE) };
            unsafe { close(&session, handle) }
        }
    }

    impl<P: Program> Drop for Archive<P> {
        fn drop(&mut self) {
            let _ = Self::close_handle(self.handle);
        }
    }

    /// Sends `words` to the program's `fs:USER` session from this thread's
    /// command buffer: the kernel's result, then the service's.
    fn send<P: Program>(words: &[u32]) -> Result<(), Code> {
        send_on(unsafe { P::fs_session() }, words).map(|_| ())
    }

    /// Sends `words` to the session `handle` from this thread's command
    /// buffer: the kernel's result, then the service's; the reply's words 2
    /// and 3.
    fn send_on(handle: u32, words: &[u32]) -> Result<[u32; 2], Code> {
        let buffer: *mut u32;
        unsafe {
            asm!("mrc p15, 0, {0}, c13, c0, 3", out(reg) buffer, options(nomem, nostack));
            let buffer = buffer.add(0x80 / 4);
            for (index, word) in words.iter().enumerate() {
                buffer.add(index).write_volatile(*word);
            }
            let code = send_sync_request(handle);
            if code < 0 {
                return Err(code);
            }
            let result = buffer.add(1).read_volatile() as i32;
            if result < 0 {
                return Err(result);
            }
            Ok([buffer.add(2).read_volatile(), buffer.add(3).read_volatile()])
        }
    }

    impl<P: Program> Archive<P> {
        /// Calls `each` with the name of every entry of the folder at
        /// `path`, as UTF-16 units without its end, and whether it is a
        /// folder. A folder that is not there has none. The folder is closed
        /// whatever happened.
        pub fn list(&self, path: &[u8], each: &mut dyn FnMut(&[u16], bool)) -> Result<(), Code> {
            use command::{ENTRY_ATTRIBUTES, ENTRY_NAME, ENTRY_SIZE};
            const BATCH: usize = 4;
            checked(path)?;
            let units = wide(path);
            let opened = send_on(
                unsafe { P::fs_session() },
                &command::open_directory(self.handle, wide_size(path), units.as_ptr() as u32),
            );
            let folder = match opened {
                Ok([_, handle]) => handle,
                Err(code) if super::not_found(code as u32) => return Ok(()),
                Err(code) => return Err(code),
            };
            // Entries are 8-byte aligned: their size is in the last word.
            let mut entries = [0u64; BATCH * ENTRY_SIZE / 8];
            let read = (|| loop {
                let [count, _] = send_on(
                    folder,
                    &command::read_directory(BATCH as u32, entries.as_mut_ptr() as u32),
                )?;
                if count == 0 {
                    return Ok(());
                }
                let bytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(entries.as_ptr().cast(), BATCH * ENTRY_SIZE)
                };
                for entry in bytes.chunks_exact(ENTRY_SIZE).take(count as usize) {
                    let mut name = [0u16; ENTRY_NAME];
                    let mut length = 0;
                    for (unit, pair) in name.iter_mut().zip(entry.chunks_exact(2)) {
                        *unit = u16::from_le_bytes([pair[0], pair[1]]);
                        if *unit == 0 {
                            break;
                        }
                        length += 1;
                    }
                    let attributes = u32::from_le_bytes([
                        entry[ENTRY_ATTRIBUTES],
                        entry[ENTRY_ATTRIBUTES + 1],
                        entry[ENTRY_ATTRIBUTES + 2],
                        entry[ENTRY_ATTRIBUTES + 3],
                    ]);
                    each(&name[..length], attributes & 1 != 0);
                }
            })();
            let closed = send_on(folder, &[command::CLOSE_DIRECTORY]).map(|_| ());
            let handle = unsafe { close_handle(folder) };
            read.and(closed)
                .and(if handle < 0 { Err(handle) } else { Ok(()) })
        }

        /// Creates the folder at `path`; one that is there already counts as
        /// made.
        pub fn create_directory(&self, path: &[u8]) -> Result<(), Code> {
            checked(path)?;
            let units = wide(path);
            match send::<P>(&command::create_directory(
                self.handle,
                wide_size(path),
                units.as_ptr() as u32,
            )) {
                Err(code) if super::already_exists(code as u32) => Ok(()),
                other => other,
            }
        }

        /// Deletes the file at `path`; one that is not there counts as
        /// deleted.
        pub fn delete(&self, path: &[u8]) -> Result<(), Code> {
            checked(path)?;
            let units = wide(path);
            match send::<P>(&command::delete_file(
                self.handle,
                wide_size(path),
                units.as_ptr() as u32,
            )) {
                Err(code) if super::not_found(code as u32) => Ok(()),
                other => other,
            }
        }

        /// Deletes the empty folder at `path`; one that is not there counts
        /// as deleted.
        pub fn delete_directory(&self, path: &[u8]) -> Result<(), Code> {
            checked(path)?;
            let units = wide(path);
            match send::<P>(&command::delete_directory(
                self.handle,
                wide_size(path),
                units.as_ptr() as u32,
            )) {
                Err(code) if super::not_found(code as u32) => Ok(()),
                other => other,
            }
        }

        /// Creates the file at `path` with `length` bytes, as an extdata
        /// file is made: its size is fixed from then on. One that is there
        /// is a failure.
        pub fn create_file(&self, path: &[u8], length: u64) -> Result<(), Code> {
            type Create = unsafe extern "aapcs" fn(*const u32, u32, u64, Path, u32, u64) -> i32;
            checked(path)?;
            let address = P::CREATE_FILE.ok_or(BAD_PATH)?;
            let create: Create = unsafe { transmute(address) };
            let units = wide(path);
            let session = unsafe { P::fs_session() };
            let code = unsafe {
                create(
                    &session,
                    0,
                    self.handle,
                    Path {
                        kind: PATH_UTF16,
                        data: units.as_ptr().cast(),
                        byte_len: wide_size(path),
                    },
                    0,
                    length,
                )
            };
            if code < 0 {
                Err(code)
            } else {
                Ok(())
            }
        }

        /// Opens the file at `path` of this archive with the SDK's `flags`.
        pub fn open_file(&self, path: &[u8], flags: u32) -> Result<file::Handle<P>, Code> {
            type Open =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u32, u64, Path, u32, u32) -> i32;
            checked(path)?;
            let open: Open = unsafe { transmute(P::OPEN_FILE) };
            let units = wide(path);
            let session = unsafe { P::fs_session() };
            let mut handle = 0;
            let code = unsafe {
                open(
                    &session,
                    &mut handle,
                    0,
                    self.handle,
                    Path {
                        kind: PATH_UTF16,
                        data: units.as_ptr().cast(),
                        byte_len: wide_size(path),
                    },
                    flags,
                    0,
                )
            };
            if code < 0 {
                return Err(code);
            }
            unsafe { file::Handle::new(handle) }.ok_or(BAD_PATH)
        }

        /// Renames the folder at `from` to `to`, which must not be there.
        pub fn rename_directory(&self, from: &[u8], to: &[u8]) -> Result<(), Code> {
            checked(from)?;
            checked(to)?;
            let (from_units, to_units) = (wide(from), wide(to));
            send::<P>(&command::rename_directory(
                self.handle,
                wide_size(from),
                from_units.as_ptr() as u32,
                wide_size(to),
                to_units.as_ptr() as u32,
            ))
        }
    }

    /// Creates the folders at `paths` in order, on one archive opened for
    /// them alone; one that is there already counts as made.
    pub fn create_directories<P: Program>(paths: &[&[u8]]) -> Result<(), Code> {
        let archive = Archive::<P>::open()?;
        let made = paths
            .iter()
            .try_for_each(|path| archive.create_directory(path));
        made.and(archive.close())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_rooted_printable_characters_with_one_terminator() {
        assert!(valid(b"/3ds/Pok\xe9mon Bank/mover1.box\0"));
        assert!(!valid(b"/3ds/Pok\xe9mon Bank/mover1.box"));
        assert!(!valid(b"3ds\0"));
        assert!(!valid(b"/3d\0s\0"));
        assert!(!valid(b"\0"));
        // Control characters, of ASCII and of Latin-1.
        assert!(!valid(b"/a\x1fb\0"));
        assert!(!valid(b"/a\x7fb\0"));
        assert!(!valid(b"/a\x85b\0"));
        assert!(!valid(&[b'/'; PATH_MAX]));
        let mut longest = [b'a'; PATH_MAX];
        longest[0] = b'/';
        longest[PATH_MAX - 1] = 0;
        assert!(valid(&longest));
    }

    #[test]
    fn a_path_is_sent_as_utf16_with_its_terminator() {
        let path = b"/Pok\xe9\0";
        let units = wide(path);
        assert_eq!(units[..6], [0x2f, 0x50, 0x6f, 0x6b, 0xe9, 0]);
        assert!(units[6..].iter().all(|unit| *unit == 0));
        assert_eq!(wide_size(path), 12);
    }

    #[test]
    fn commands_are_libctrus_words() {
        // "/3ds" with its terminator, as UTF-16: 10 bytes.
        let size = wide_size(b"/3ds\0");
        assert_eq!(size, 10);
        let archive = 0x1122_3344_5566_7788;
        assert_eq!(
            command::create_directory(archive, size, 0x1000),
            [
                0x0809_0182,
                0,
                0x5566_7788,
                0x1122_3344,
                4,
                10,
                0,
                (10 << 14) | 2,
                0x1000
            ]
        );
        assert_eq!(
            command::delete_file(archive, size, 0x1000),
            [
                0x0804_0142,
                0,
                0x5566_7788,
                0x1122_3344,
                4,
                10,
                (10 << 14) | 2,
                0x1000
            ]
        );
        assert_eq!(
            command::delete_directory(archive, size, 0x1000),
            [
                0x0806_0142,
                0,
                0x5566_7788,
                0x1122_3344,
                4,
                10,
                (10 << 14) | 2,
                0x1000
            ]
        );
        assert_eq!(
            command::open_directory(archive, size, 0x1000),
            [
                0x080B_0102,
                0x5566_7788,
                0x1122_3344,
                4,
                10,
                (10 << 14) | 2,
                0x1000
            ]
        );
        // Four entries of 0x228 bytes into a buffer the service writes.
        assert_eq!(
            command::read_directory(4, 0x2000),
            [0x0801_0042, 4, ((4 * 0x228) << 4) | 0xC, 0x2000]
        );
        assert_eq!(command::ENTRY_ATTRIBUTES + 4 + 8, command::ENTRY_SIZE);
        let to = wide_size(b"/x\0");
        assert_eq!(
            command::rename_directory(archive, size, 0x1000, to, 0x2000),
            [
                0x080A_0244,
                0,
                0x5566_7788,
                0x1122_3344,
                4,
                10,
                0x5566_7788,
                0x1122_3344,
                4,
                6,
                (10 << 14) | (1 << 10) | 2,
                0x1000,
                (6 << 14) | (2 << 10) | 2,
                0x2000
            ]
        );
    }
}
