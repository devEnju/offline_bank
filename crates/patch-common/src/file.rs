//! The SDK's calls on an open file, the same in Bank and Transporter at the
//! addresses of `Program`: its size, a read, a write, and the close, which
//! also closes the kernel handle (the SDK's close does not). Each answers the
//! native result, negative for a failure. Opening a file is the caller's:
//! Bank's files are opened in their archive (`bank_common::fs`), the SD
//! card's with their archive in one call (Transporter's `sdcard`).

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
    use core::{marker::PhantomData, mem::transmute};

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

    /// An open file of program `P`, owned: closed by `close`, which answers
    /// how the close went, or else when dropped. It stays on the thread that
    /// opened it, which the SDK's session belongs to.
    pub struct Handle<P: Program> {
        file: Option<u32>,
        _owner: PhantomData<(P, *mut ())>,
    }
    impl<P: Program> Handle<P> {
        /// The file the SDK has just opened; `None` for none (handle 0).
        /// # Safety
        /// `file` is an open file of `P`'s SDK that nothing else closes.
        pub unsafe fn new(file: u32) -> Option<Self> {
            (file != 0).then(|| Self {
                file: Some(file),
                _owner: PhantomData,
            })
        }
        fn file(&self) -> u32 {
            self.file.unwrap_or(0)
        }
        pub fn size(&self) -> Result<u64, Code> {
            size::<P>(self.file())
        }
        /// Reads up to `bytes.len()` bytes at `offset`: the count read.
        pub fn read(&self, offset: u64, bytes: &mut [u8]) -> Result<u32, Code> {
            let length = bytes.len() as u32;
            unsafe { read::<P>(self.file(), offset, bytes.as_mut_ptr(), length) }
        }
        /// Writes `bytes` at `offset` with the SDK's `flags`: the count
        /// written.
        pub fn write(&self, offset: u64, bytes: &[u8], flags: u32) -> Result<u32, Code> {
            let length = bytes.len() as u32;
            unsafe { write::<P>(self.file(), offset, bytes.as_ptr(), length, flags) }
        }
        /// Closes it (`close`): the SDK's result, then the kernel's.
        pub fn close(mut self) -> (Result<(), Code>, Result<(), Code>) {
            close::<P>(self.file.take().unwrap_or(0))
        }
    }
    impl<P: Program> Drop for Handle<P> {
        fn drop(&mut self) {
            if let Some(file) = self.file.take() {
                let _ = close::<P>(file);
            }
        }
    }
}
