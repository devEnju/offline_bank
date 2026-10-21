//! The SD card, where the Black/White saves are (docs/internals.md, "Saves
//! on the SD card"), through Transporter's SDK: a file is opened with its
//! archive in one call, and the kernel handle is closed with it. Bank's
//! files are reached through bank-common (`bank_common::fs::Visit`).

use crate::program::Transporter15;
use core::mem::transmute;
use patch_common::{kernel::close_handle, program::Program};

/// The SDK's "open a file in an archive in one call", `(session, &file, 0,
/// archive id, path type, path, path size, 3, file path, its size, flags,
/// 0)`.
const OPEN_FILE_DIRECTLY: usize = 0x001d_f448;
/// Flush and update the file's time, as Bank's writer does.
pub(crate) const FLUSH_FLAGS: u32 = 0x0001_0001;
/// Open flags: read, read and write, write and create.
pub(crate) const OPEN_READ: u32 = 1;
pub(crate) const OPEN_READ_WRITE: u32 = 3;
pub(crate) const OPEN_WRITE_CREATE: u32 = 6;

/// An open file on the SD card; dropping it closes it.
pub(crate) struct File(u32);
impl File {
    /// `path` is ASCII with its terminator, in the SD card's archive (9,
    /// with an empty path). `None` when the file does not open.
    /// # Safety
    /// Transporter's file system session must be set up.
    pub(crate) unsafe fn open(path: &[u8], flags: u32) -> Option<Self> {
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
        (code >= 0 && handle != 0).then(|| Self(handle))
    }
    pub(crate) fn size(&self) -> Option<u64> {
        type Size = unsafe extern "aapcs" fn(*const u32, *mut u64) -> i32;
        let size: Size = unsafe { transmute(Transporter15::FILE_SIZE) };
        let mut length = 0;
        (unsafe { size(&self.0, &mut length) } >= 0).then_some(length)
    }
    /// Whether all `length` bytes were read.
    pub(crate) fn read(&self, offset: u64, bytes: *mut u8, length: u32) -> bool {
        type Read = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *mut u8, u32) -> i32;
        let read: Read = unsafe { transmute(Transporter15::FILE_READ) };
        let mut count = 0;
        unsafe { read(&self.0, &mut count, offset, bytes, length) >= 0 && count == length }
    }
    /// Whether all `length` bytes were written.
    pub(crate) fn write(&self, offset: u64, bytes: *const u8, length: u32, flags: u32) -> bool {
        type Write =
            unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *const u8, u32, u32) -> i32;
        let write: Write = unsafe { transmute(Transporter15::FILE_WRITE) };
        let mut count = 0;
        unsafe { write(&self.0, &mut count, offset, bytes, length, flags) >= 0 && count == length }
    }
}
impl Drop for File {
    fn drop(&mut self) {
        type Close = unsafe extern "aapcs" fn(*const u32) -> i32;
        let close: Close = unsafe { transmute(Transporter15::FILE_CLOSE) };
        unsafe {
            close(&self.0);
            close_handle(self.0);
        }
    }
}
