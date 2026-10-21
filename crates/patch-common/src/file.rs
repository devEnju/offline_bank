//! The SDK's calls on an open file, the same in Bank and Transporter at the
//! addresses of `Program`: its size, a read, a write, and the close, which
//! also closes the kernel handle (the SDK's close does not). Each answers the
//! native result, negative for a failure. Opening a file is the caller's:
//! Bank's files are opened in their archive (`bank_common::fs`), the SD
//! card's with their archive in one call (Transporter's `fs`).

/// A failure: the native result.
pub type Code = i32;

/// The write flags that flush the file and update its time, as Bank's
/// original writer writes; a zero-length write with them is a sync.
pub const FLUSH: u32 = 0x0001_0001;

#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(target_arch = "arm")]
mod native {
    use super::Code;
    use crate::{kernel::close_handle, program::Program};
    use core::mem::transmute;

    fn result(code: i32) -> Result<(), Code> {
        if code < 0 {
            Err(code)
        } else {
            Ok(())
        }
    }

    /// The size of the open file `file`.
    pub fn size<P: Program>(file: u32) -> Result<u64, Code> {
        type Call = unsafe extern "aapcs" fn(*const u32, *mut u64) -> i32;
        let call: Call = unsafe { transmute(P::FILE_SIZE) };
        let mut size = 0;
        result(unsafe { call(&file, &mut size) })?;
        Ok(size)
    }

    /// Reads up to `length` bytes at `offset` into `bytes`: the count read.
    /// # Safety
    /// `bytes` holds `length` writable bytes.
    pub unsafe fn read<P: Program>(
        file: u32,
        offset: u64,
        bytes: *mut u8,
        length: u32,
    ) -> Result<u32, Code> {
        type Call = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *mut u8, u32) -> i32;
        let call: Call = unsafe { transmute(P::FILE_READ) };
        let mut count = 0;
        result(unsafe { call(&file, &mut count, offset, bytes, length) })?;
        Ok(count)
    }

    /// Writes `length` bytes from `bytes` at `offset`, with the SDK's `flags`:
    /// the count written.
    /// # Safety
    /// `bytes` holds `length` readable bytes.
    pub unsafe fn write<P: Program>(
        file: u32,
        offset: u64,
        bytes: *const u8,
        length: u32,
        flags: u32,
    ) -> Result<u32, Code> {
        type Call = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *const u8, u32, u32) -> i32;
        let call: Call = unsafe { transmute(P::FILE_WRITE) };
        let mut count = 0;
        result(unsafe { call(&file, &mut count, offset, bytes, length, flags) })?;
        Ok(count)
    }

    /// Closes the open file `file` and then its kernel handle, whatever the
    /// first answered: the SDK's result, then the kernel's.
    pub fn close<P: Program>(file: u32) -> (Result<(), Code>, Result<(), Code>) {
        type Call = unsafe extern "aapcs" fn(*const u32) -> i32;
        let call: Call = unsafe { transmute(P::FILE_CLOSE) };
        let closed = result(unsafe { call(&file) });
        (closed, result(unsafe { close_handle(file) }))
    }
}
