//! File access through Transporter's own SDK wrappers: Bank's extdata, where
//! the journal records and the transport box are, and the SD card, where
//! the Black/White saves are (docs/internals.md, "File access").
//!
//! A file is opened with its archive in one call, and the kernel handle is
//! closed with it.

use crate::layout::BANK_EXTDATA;
use core::mem::transmute;
use patch_common::kernel::close_handle;

/// Transporter's `fs:USER` session handle.
const FS_SESSION: *const u32 = 0x0031_1f80 as *const u32;
/// The SDK wrappers: open a file in an archive in one call, read, write,
/// size, close.
const OPEN_FILE_DIRECTLY: usize = 0x001d_f448;
const FILE_READ: usize = 0x0015_930c;
const FILE_WRITE: usize = 0x0015_9390;
const FILE_SIZE: usize = 0x0015_93f0;
const FILE_CLOSE: usize = 0x0015_9364;
/// Flush and update the file's time, as Bank's writer does.
pub(crate) const FLUSH_FLAGS: u32 = 0x0001_0001;
/// Open flags: read, read and write, write and create.
pub(crate) const OPEN_READ: u32 = 1;
pub(crate) const OPEN_READ_WRITE: u32 = 3;
pub(crate) const OPEN_WRITE_CREATE: u32 = 6;

/// `name` with the terminator the SDK wrappers expect, and its length with
/// it. Bank's file names are short (`offline_core::files`).
pub(crate) fn terminated(name: &str) -> ([u8; 20], usize) {
    let mut path = [0; 20];
    let length = name.len().min(path.len() - 1);
    path[..length].copy_from_slice(&name.as_bytes()[..length]);
    (path, length + 1)
}

/// The archive a file lies in.
#[derive(Clone, Copy)]
pub(crate) enum Archive {
    /// Bank's extdata on the SD card: archive 6 with the binary path media
    /// type SD, extdata id low and high.
    BankExtdata,
    /// The SD card: archive 9 with an empty path.
    Sd,
}

/// An open file; dropping it closes it.
pub(crate) struct File(u32);
impl File {
    /// `path` is ASCII with its terminator. `None` when the file does not
    /// open.
    /// # Safety
    /// Transporter's file system session must be set up.
    pub(crate) unsafe fn open(archive: Archive, path: &[u8], flags: u32) -> Option<Self> {
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
        let extdata = [1u32, BANK_EXTDATA, 0];
        let empty = [0u8; 4];
        let (id, kind, data, length) = match archive {
            Archive::BankExtdata => (6, 2, extdata.as_ptr().cast::<u8>(), 12),
            Archive::Sd => (9, 1, empty.as_ptr(), 1),
        };
        let mut handle = 0;
        let code = unsafe {
            open(
                FS_SESSION,
                &mut handle,
                0,
                id,
                kind,
                data,
                length,
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
        let size: Size = unsafe { transmute(FILE_SIZE) };
        let mut length = 0;
        (unsafe { size(&self.0, &mut length) } >= 0).then_some(length)
    }
    /// Whether all `length` bytes were read.
    pub(crate) fn read(&self, offset: u64, bytes: *mut u8, length: u32) -> bool {
        type Read = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *mut u8, u32) -> i32;
        let read: Read = unsafe { transmute(FILE_READ) };
        let mut count = 0;
        unsafe { read(&self.0, &mut count, offset, bytes, length) >= 0 && count == length }
    }
    /// Whether all `length` bytes were written.
    pub(crate) fn write(&self, offset: u64, bytes: *const u8, length: u32, flags: u32) -> bool {
        type Write =
            unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *const u8, u32, u32) -> i32;
        let write: Write = unsafe { transmute(FILE_WRITE) };
        let mut count = 0;
        unsafe { write(&self.0, &mut count, offset, bytes, length, flags) >= 0 && count == length }
    }
    /// Makes what was written durable.
    pub(crate) fn flush(&self) -> bool {
        self.write(0, [0u8; 1].as_ptr(), 0, FLUSH_FLAGS)
    }
}
impl Drop for File {
    fn drop(&mut self) {
        type Close = unsafe extern "aapcs" fn(*const u32) -> i32;
        let close: Close = unsafe { transmute(FILE_CLOSE) };
        unsafe {
            close(&self.0);
            close_handle(self.0);
        }
    }
}
