//! Fixed-size storage in Bank's existing SD extdata archive.
//!
//! Native calls are specific to Bank 1.5 code SHA256
//! 2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf.
//! See docs/internals.md for the verified call ABIs.

#[cfg(target_arch = "arm")]
use crate::bank_files::FileName;
#[cfg(any(test, target_arch = "arm"))]
use offline_core::Storage;

#[cfg(any(test, target_arch = "arm"))]
const IO_CHUNK: usize = 0x10000;
#[cfg(any(test, target_arch = "arm"))]
const FLUSH_FLAGS: u32 = 0x0001_0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    OpenArchive,
    CloseArchive,
    CreateFile,
    OpenFile,
    Size,
    Read,
    Write,
    Sync,
    CloseFile,
    CloseHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Native {
        operation: Operation,
        code: u32,
    },
    InvalidHandle(Operation),
    InvalidLength,
    WrongSize {
        expected: u64,
        actual: u64,
    },
    Bounds,
    Count {
        operation: Operation,
        expected: u32,
        actual: u32,
    },
    Closed,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportedAbsence {
    /// The service reported NotFound while opening the archive. This does not
    /// prove that no archive bytes exist or authorize formatting anything.
    Archive,
    File,
}

impl Error {
    /// Second number of the on-screen error. Native results are negative
    /// (`8xxxxxxx`..`Fxxxxxxx`) and pass through. Local conditions use:
    /// `71oooooo` a call succeeded but returned a null handle, `72oooooo` a
    /// short read/write, `73`..`76` bounds, closed, zero length, read-only,
    /// where `oo` is the `Operation` index (0 OpenArchive, 1 CloseArchive,
    /// 2 CreateFile, 3 OpenFile, 4 Size, 5 Read, 6 Write, 7 Sync, 8 CloseFile,
    /// 9 CloseHandle). A wrong file size is shown as that size itself.
    pub fn diagnostic(self) -> u32 {
        match self {
            Self::Native { code, .. } => code,
            Self::WrongSize { actual, .. } => (actual as u32) & 0x6fff_ffff,
            Self::InvalidHandle(operation) => 0x7100_0000 | operation as u32,
            Self::Count { operation, .. } => 0x7200_0000 | operation as u32,
            Self::Bounds => 0x7300_0000,
            Self::Closed => 0x7400_0000,
            Self::InvalidLength => 0x7500_0000,
            Self::ReadOnly => 0x7600_0000,
        }
    }

    /// Conservative diagnostic classification, never a create/reset policy.
    /// Other errors retain their full native result, including permission,
    /// media, quota, format and I/O failures.
    pub fn reported_absence(self) -> Option<ReportedAbsence> {
        match self {
            Self::Native { operation, code }
                if ((code >> 10) & 0xff) == 17 && ((code >> 21) & 0x3f) == 4 =>
            {
                match operation {
                    Operation::OpenArchive => Some(ReportedAbsence::Archive),
                    Operation::OpenFile => Some(ReportedAbsence::File),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

// Private seam for testing I/O policy without executing native addresses.
#[cfg(any(test, target_arch = "arm"))]
trait Api {
    fn open_archive(&mut self) -> Result<u64, Error>;
    fn close_archive(&mut self, archive: u64) -> Result<(), Error>;
    fn create_file(&mut self, archive: u64, length: u64) -> Result<(), Error>;
    fn open_file(&mut self, archive: u64) -> Result<u32, Error>;
    fn size(&mut self, file: u32) -> Result<u64, Error>;
    fn read(&mut self, file: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error>;
    fn write(&mut self, file: u32, offset: u64, bytes: &[u8], flags: u32) -> Result<u32, Error>;
    /// Always release the kernel handle, even if the file-close IPC fails.
    fn close_file(&mut self, file: u32) -> Result<(), Error>;
}

#[cfg(any(test, target_arch = "arm"))]
struct File<A: Api> {
    api: A,
    archive: Option<u64>,
    handle: Option<u32>,
    length: u64,
}

#[cfg(any(test, target_arch = "arm"))]
impl<A: Api> File<A> {
    fn open(mut api: A, length: u64, create: bool) -> Result<Self, Error> {
        if length == 0 {
            return Err(Error::InvalidLength);
        }
        let archive = api.open_archive()?;
        let mut this = Self {
            api,
            archive: Some(archive),
            handle: None,
            length,
        };
        if create {
            // Create is exclusive. An existing file is never truncated, deleted,
            // resized, or opened as a fallback when this operation fails.
            this.api.create_file(archive, length)?;
        }
        let handle = this.api.open_file(archive)?;
        this.handle = Some(handle);
        let actual = this.api.size(handle)?;
        if actual != length {
            return Err(Error::WrongSize {
                expected: length,
                actual,
            });
        }
        if create {
            // Explicit initialization avoids assuming newly allocated extdata
            // contents are zero. Interrupted creation remains for inspection.
            // The buffer stays on the stack: IPC writes from ordinary data
            // memory are the only kind confirmed on a console.
            let zeros = [0; 512];
            let mut offset = 0;
            while offset < length {
                let count = (length - offset).min(zeros.len() as u64) as usize;
                this.write(offset, &zeros[..count])?;
                offset += count as u64;
            }
            this.sync()?;
        }
        Ok(this)
    }

    fn check(&self, offset: u64, count: usize) -> Result<u32, Error> {
        let handle = self.handle.ok_or(Error::Closed)?;
        if offset
            .checked_add(count as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::Bounds);
        }
        Ok(handle)
    }

    fn close(&mut self) -> Result<(), Error> {
        let file_result = match self.handle.take() {
            Some(handle) => self.api.close_file(handle),
            None => Ok(()),
        };
        let archive_result = match self.archive.take() {
            Some(archive) => self.api.close_archive(archive),
            None => Ok(()),
        };
        file_result.and(archive_result)
    }
}

#[cfg(any(test, target_arch = "arm"))]
impl<A: Api> Drop for File<A> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(any(test, target_arch = "arm"))]
impl<A: Api> Storage for File<A> {
    type Error = Error;

    fn read(&mut self, mut offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        let handle = self.check(offset, bytes.len())?;
        for chunk in bytes.chunks_mut(IO_CHUNK) {
            let actual = self.api.read(handle, offset, chunk)?;
            let expected = chunk.len() as u32;
            if actual != expected {
                return Err(Error::Count {
                    operation: Operation::Read,
                    expected,
                    actual,
                });
            }
            offset += u64::from(expected);
        }
        Ok(())
    }

    fn write(&mut self, mut offset: u64, bytes: &[u8]) -> Result<(), Error> {
        let handle = self.check(offset, bytes.len())?;
        for chunk in bytes.chunks(IO_CHUNK) {
            let actual = self.api.write(handle, offset, chunk, 0)?;
            let expected = chunk.len() as u32;
            if actual != expected {
                return Err(Error::Count {
                    operation: Operation::Write,
                    expected,
                    actual,
                });
            }
            offset += u64::from(expected);
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> {
        let handle = self.handle.ok_or(Error::Closed)?;
        let dummy = [0u8; 1];
        let actual = self
            .api
            .write(handle, 0, &dummy[..0], FLUSH_FLAGS)
            .map_err(|error| match error {
                Error::Native { code, .. } => Error::Native {
                    operation: Operation::Sync,
                    code,
                },
                error => error,
            })?;
        if actual != 0 {
            return Err(Error::Count {
                operation: Operation::Sync,
                expected: 0,
                actual,
            });
        }
        Ok(())
    }
}

/// One exclusively accessed file of `bank_files::FileName` in SD extdata
/// 0x00000c9b, with that file's exact allocation size.
/// This borrows Bank's initialized FS session; it never closes that session.
#[cfg(target_arch = "arm")]
pub struct ExtdataStorage {
    file: File<native::Native>,
}

#[cfg(target_arch = "arm")]
impl ExtdataStorage {
    /// Opens an existing file and requires its exact allocation size.
    ///
    /// # Safety
    /// The executable must match the SHA256 documented above. `session` must be
    /// an initialized fs:USER handle owned by this Bank process and remain live
    /// until close. Call on an initialized native thread with valid IPC TLS.
    /// The caller must guarantee exclusive access to this file and prevent
    /// native extdata deletion/reformatting throughout its lifetime.
    pub unsafe fn open_existing(session: u32, file: FileName) -> Result<Self, Error> {
        let api = native::Native::new(session, file)?;
        Ok(Self {
            file: File::open(api, file.size(), false)?,
        })
    }

    /// Exclusively creates the private container in an already existing Bank
    /// archive, zeros it, and flushes it. Failure never resets existing data.
    /// Missing archives are returned as errors; no archive is created here.
    ///
    /// # Safety
    /// The same requirements as `open_existing` apply. The caller must have
    /// selected fresh initialization explicitly, not inferred it from a corrupt
    /// snapshot, a failed read, or an inaccessible archive.
    pub unsafe fn create_new(session: u32, file: FileName) -> Result<Self, Error> {
        let api = native::Native::new(session, file)?;
        Ok(Self {
            file: File::open(api, file.size(), true)?,
        })
    }

    /// Returns close failures; Drop also attempts cleanup but cannot report them.
    /// Explicit sync is required before relying on durability.
    pub fn close(&mut self) -> Result<(), Error> {
        self.file.close()
    }
}

#[cfg(target_arch = "arm")]
impl Storage for ExtdataStorage {
    type Error = Error;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        self.file.read(offset, bytes)
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        self.file.write(offset, bytes)
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.file.sync()
    }
}

/// Read-only `main` in a borrowed, already mounted game archive. The archive and
/// fs:USER session remain owned by native Bank; only this file handle is closed.
#[cfg(target_arch = "arm")]
pub struct GameMainReader<'a> {
    api: native::Native,
    handle: Option<u32>,
    length: u64,
    archive_borrow: core::marker::PhantomData<&'a ()>,
}

#[cfg(target_arch = "arm")]
impl<'a> GameMainReader<'a> {
    /// # Safety
    /// The exact native executable must be verified. The caller must supply the
    /// initialized session and mounted archive of the selected game, keep both
    /// live throughout `'a`, prevent main-file changes while reading, and run on
    /// a native thread with valid IPC TLS. No file or archive is created here.
    pub unsafe fn open(session: u32, archive: u64) -> Result<Self, Error> {
        if archive == 0 {
            return Err(Error::InvalidHandle(Operation::OpenArchive));
        }
        let mut api = native::Native::new(session, FileName::Bank)?;
        let handle = api.open_main(archive)?;
        let mut this = Self {
            api,
            handle: Some(handle),
            length: 0,
            archive_borrow: core::marker::PhantomData,
        };
        this.length = this.api.size(handle)?;
        if this.length == 0 {
            return Err(Error::InvalidLength);
        }
        Ok(this)
    }
    pub fn len(&self) -> u64 {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn close(&mut self) -> Result<(), Error> {
        match self.handle.take() {
            Some(handle) => self.api.close_file(handle),
            None => Ok(()),
        }
    }
}

#[cfg(target_arch = "arm")]
impl Drop for GameMainReader<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(target_arch = "arm")]
impl Storage for GameMainReader<'_> {
    type Error = Error;
    fn read(&mut self, mut offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        let handle = self.handle.ok_or(Error::Closed)?;
        if offset
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::Bounds);
        }
        for chunk in bytes.chunks_mut(IO_CHUNK) {
            let actual = self.api.read(handle, offset, chunk)?;
            let expected = chunk.len() as u32;
            if actual != expected {
                return Err(Error::Count {
                    operation: Operation::Read,
                    expected,
                    actual,
                });
            }
            offset += u64::from(expected);
        }
        Ok(())
    }
    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), Error> {
        Err(Error::ReadOnly)
    }
    fn sync(&mut self) -> Result<(), Error> {
        Err(Error::ReadOnly)
    }
}
#[cfg(target_arch = "arm")]
mod native {
    use super::{Api, Error, Operation};
    use crate::bank_files::FileName;
    use core::{arch::asm, marker::PhantomData, mem::transmute};

    #[repr(C)]
    struct Path {
        kind: u32,
        data: *const u8,
        byte_len: u32,
    }

    /// UTF-16 with terminator, as the native path type expects.
    const fn utf16<const N: usize>(text: &str) -> [u16; N] {
        let bytes = text.as_bytes();
        assert!(bytes.len() + 1 == N);
        let mut out = [0; N];
        let mut index = 0;
        while index < bytes.len() {
            out[index] = bytes[index] as u16;
            index += 1;
        }
        out
    }
    static BANK: [u16; 10] = utf16(FileName::Bank.path());
    static DEX: [u16; 9] = utf16(FileName::Dex.path());
    static TRANSPORT: [u16; 15] = utf16(FileName::Transport.path());
    static REWARDS: [u16; 13] = utf16(FileName::Rewards.path());

    pub(super) struct Native {
        session: u32,
        name: &'static [u16],
        _thread: PhantomData<*mut ()>,
    }
    impl Native {
        pub(super) fn open_main(&mut self, archive: u64) -> Result<u32, Error> {
            const MAIN: [u16; 6] = [47, 109, 97, 105, 110, 0];
            let path = Path {
                kind: 4,
                data: MAIN.as_ptr().cast(),
                byte_len: 12,
            };
            type Call =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u32, u64, Path, u32, u32) -> i32;
            let call: Call = unsafe { transmute(0x0016_57ecusize) };
            let mut file = 0;
            result(Operation::OpenFile, unsafe {
                call(&self.session, &mut file, 0, archive, path, 1, 0)
            })?;
            if file == 0 {
                return Err(Error::InvalidHandle(Operation::OpenFile));
            }
            Ok(file)
        }
        fn name(&self) -> Path {
            Path {
                kind: 4,
                data: self.name.as_ptr().cast(),
                byte_len: (self.name.len() * 2) as u32,
            }
        }
        pub(super) fn new(session: u32, file: FileName) -> Result<Self, Error> {
            if session == 0 {
                return Err(Error::InvalidHandle(Operation::OpenArchive));
            }
            Ok(Self {
                session,
                name: match file {
                    FileName::Bank => &BANK,
                    FileName::Dex => &DEX,
                    FileName::Transport => &TRANSPORT,
                    FileName::Rewards => &REWARDS,
                },
                _thread: PhantomData,
            })
        }
    }

    fn result(operation: Operation, code: i32) -> Result<(), Error> {
        if code < 0 {
            Err(Error::Native {
                operation,
                code: code as u32,
            })
        } else {
            Ok(())
        }
    }

    impl Api for Native {
        fn open_archive(&mut self) -> Result<u64, Error> {
            // media byte=1, reserved bytes=0, extdata ID low/high.
            let binary = [1u32, 0xc9b, 0];
            let path = Path {
                kind: 2,
                data: binary.as_ptr().cast(),
                byte_len: 12,
            };
            let mut archive = 0;
            type Call = unsafe extern "aapcs" fn(*const u32, *mut u64, u32, Path) -> i32;
            let call: Call = unsafe { transmute(0x0020_a468usize) };
            result(Operation::OpenArchive, unsafe {
                call(&self.session, &mut archive, 6, path)
            })?;
            if archive == 0 {
                return Err(Error::InvalidHandle(Operation::OpenArchive));
            }
            Ok(archive)
        }
        fn close_archive(&mut self, archive: u64) -> Result<(), Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, u64) -> i32;
            let call: Call = unsafe { transmute(0x0020_a438usize) };
            result(Operation::CloseArchive, unsafe {
                call(&self.session, archive)
            })
        }
        fn create_file(&mut self, archive: u64, length: u64) -> Result<(), Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, u32, u64, Path, u32, u64) -> i32;
            let call: Call = unsafe { transmute(0x0016_54f8usize) };
            result(Operation::CreateFile, unsafe {
                call(&self.session, 0, archive, self.name(), 0, length)
            })
        }
        fn open_file(&mut self, archive: u64) -> Result<u32, Error> {
            type Call =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u32, u64, Path, u32, u32) -> i32;
            let call: Call = unsafe { transmute(0x0016_57ecusize) };
            let mut handle = 0;
            result(Operation::OpenFile, unsafe {
                call(&self.session, &mut handle, 0, archive, self.name(), 3, 0)
            })?;
            if handle == 0 {
                return Err(Error::InvalidHandle(Operation::OpenFile));
            }
            Ok(handle)
        }
        fn size(&mut self, file: u32) -> Result<u64, Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, *mut u64) -> i32;
            let call: Call = unsafe { transmute(0x0016_59acusize) };
            let mut size = 0;
            result(Operation::Size, unsafe { call(&file, &mut size) })?;
            Ok(size)
        }
        fn read(&mut self, file: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *mut u8, u32) -> i32;
            let call: Call = unsafe { transmute(0x0016_58c8usize) };
            let mut count = 0;
            result(Operation::Read, unsafe {
                call(
                    &file,
                    &mut count,
                    offset,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                )
            })?;
            Ok(count)
        }
        fn write(
            &mut self,
            file: u32,
            offset: u64,
            bytes: &[u8],
            flags: u32,
        ) -> Result<u32, Error> {
            type Call =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u64, *const u8, u32, u32) -> i32;
            let call: Call = unsafe { transmute(0x0016_594cusize) };
            let mut count = 0;
            result(Operation::Write, unsafe {
                call(
                    &file,
                    &mut count,
                    offset,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    flags,
                )
            })?;
            Ok(count)
        }
        fn close_file(&mut self, file: u32) -> Result<(), Error> {
            type Call = unsafe extern "aapcs" fn(*const u32) -> i32;
            let call: Call = unsafe { transmute(0x0016_5920usize) };
            let close_result = result(Operation::CloseFile, unsafe { call(&file) });
            let mut code = file;
            unsafe {
                asm!("svc 0x23", inout("r0") code, lateout("r1") _, lateout("r2") _, lateout("r3") _, lateout("r12") _, options(nostack));
            }
            let handle_result = result(Operation::CloseHandle, code as i32);
            close_result.and(handle_result)
        }
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn every_local_error_has_a_distinct_nonzero_code() {
        let native = Error::Native {
            operation: Operation::OpenFile,
            code: 0xc880_4478,
        };
        assert_eq!(native.diagnostic(), 0xc880_4478);
        let wrong = Error::WrongSize {
            expected: 1_535_024,
            actual: 1_534_960,
        };
        assert_eq!(wrong.diagnostic(), 1_534_960);
        let codes = [
            Error::InvalidHandle(Operation::OpenArchive).diagnostic(),
            Error::InvalidHandle(Operation::OpenFile).diagnostic(),
            Error::Count {
                operation: Operation::Write,
                expected: 512,
                actual: 0,
            }
            .diagnostic(),
            Error::Bounds.diagnostic(),
            Error::Closed.diagnostic(),
            Error::InvalidLength.diagnostic(),
            Error::ReadOnly.diagnostic(),
        ];
        assert_eq!(codes[..3], [0x7100_0000, 0x7100_0003, 0x7200_0006]);
        for (index, code) in codes.iter().enumerate() {
            assert!(*code != 0 && *code < 0x8000_0000);
            assert!(!codes[..index].contains(code));
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

    #[derive(Default)]
    struct State {
        bytes: Vec<u8>,
        writes: Vec<(u64, usize, u32)>,
        reads: usize,
        closes: Vec<&'static str>,
        short: Option<u32>,
        failure: Option<Operation>,
        creates: usize,
    }
    struct Mock(Rc<RefCell<State>>);
    fn failure(operation: Operation) -> Error {
        Error::Native {
            operation,
            code: 0xd900_0001,
        }
    }
    impl Api for Mock {
        fn open_archive(&mut self) -> Result<u64, Error> {
            if self.0.borrow().failure == Some(Operation::OpenArchive) {
                Err(failure(Operation::OpenArchive))
            } else {
                Ok(1)
            }
        }
        fn close_archive(&mut self, _: u64) -> Result<(), Error> {
            self.0.borrow_mut().closes.push("archive");
            Ok(())
        }
        fn create_file(&mut self, _: u64, length: u64) -> Result<(), Error> {
            let mut s = self.0.borrow_mut();
            s.creates += 1;
            if s.failure == Some(Operation::CreateFile) {
                return Err(failure(Operation::CreateFile));
            }
            s.bytes = vec![0xa5; length as usize];
            Ok(())
        }
        fn open_file(&mut self, _: u64) -> Result<u32, Error> {
            if self.0.borrow().failure == Some(Operation::OpenFile) {
                Err(failure(Operation::OpenFile))
            } else {
                Ok(2)
            }
        }
        fn size(&mut self, _: u32) -> Result<u64, Error> {
            Ok(self.0.borrow().bytes.len() as u64)
        }
        fn read(&mut self, _: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error> {
            let mut s = self.0.borrow_mut();
            s.reads += 1;
            if s.failure == Some(Operation::Read) {
                return Err(failure(Operation::Read));
            }
            bytes.copy_from_slice(&s.bytes[offset as usize..offset as usize + bytes.len()]);
            Ok(s.short.unwrap_or(bytes.len() as u32))
        }
        fn write(&mut self, _: u32, offset: u64, bytes: &[u8], flags: u32) -> Result<u32, Error> {
            let mut s = self.0.borrow_mut();
            s.writes.push((offset, bytes.len(), flags));
            if s.failure == Some(Operation::Write) {
                return Err(failure(Operation::Write));
            }
            s.bytes[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
            Ok(s.short.unwrap_or(bytes.len() as u32))
        }
        fn close_file(&mut self, _: u32) -> Result<(), Error> {
            let mut s = self.0.borrow_mut();
            s.closes.push("file");
            if s.failure == Some(Operation::CloseFile) {
                Err(failure(Operation::CloseFile))
            } else {
                Ok(())
            }
        }
    }
    fn existing(length: usize) -> (File<Mock>, Rc<RefCell<State>>) {
        let state = Rc::new(RefCell::new(State {
            bytes: vec![0; length],
            ..State::default()
        }));
        (
            File::open(Mock(state.clone()), length as u64, false).unwrap(),
            state,
        )
    }

    #[test]
    fn bounds_and_overflow_make_no_io_calls() {
        let (mut file, state) = existing(16);
        assert_eq!(file.write(15, &[1, 2]), Err(Error::Bounds));
        assert_eq!(file.read(u64::MAX, &mut [0; 2]), Err(Error::Bounds));
        assert_eq!(file.write(17, &[]), Err(Error::Bounds));
        assert!(state.borrow().writes.is_empty());
        assert_eq!(state.borrow().reads, 0);
    }
    #[test]
    fn short_io_and_native_errors_are_not_success() {
        let (mut file, state) = existing(16);
        state.borrow_mut().short = Some(3);
        assert_eq!(
            file.write(0, &[1; 4]),
            Err(Error::Count {
                operation: Operation::Write,
                expected: 4,
                actual: 3
            })
        );
        assert_eq!(
            file.read(0, &mut [0; 4]),
            Err(Error::Count {
                operation: Operation::Read,
                expected: 4,
                actual: 3
            })
        );
        state.borrow_mut().failure = Some(Operation::Write);
        assert_eq!(file.sync(), Err(failure(Operation::Sync)));
    }
    #[test]
    fn sync_uses_zero_write_flush_and_checks_result_count() {
        let (mut file, state) = existing(16);
        file.write(2, &[3, 4]).unwrap();
        file.sync().unwrap();
        assert_eq!(state.borrow().writes, [(2, 2, 0), (0, 0, 0x10001)]);
        state.borrow_mut().short = Some(1);
        assert_eq!(
            file.sync(),
            Err(Error::Count {
                operation: Operation::Sync,
                expected: 0,
                actual: 1
            })
        );
    }
    #[test]
    fn large_io_is_chunked_inside_ipc_limits() {
        let (mut file, state) = existing(IO_CHUNK + 7);
        file.write(0, &vec![0xab; IO_CHUNK + 7]).unwrap();
        let mut actual = vec![0; IO_CHUNK + 7];
        file.read(0, &mut actual).unwrap();
        assert!(actual.iter().all(|b| *b == 0xab));
        assert_eq!(
            state.borrow().writes,
            [(0, IO_CHUNK, 0), (IO_CHUNK as u64, 7, 0)]
        );
        assert_eq!(state.borrow().reads, 2);
    }
    #[test]
    fn create_zeros_allocated_file_then_flushes() {
        let state = Rc::new(RefCell::new(State::default()));
        let file = File::open(Mock(state.clone()), 513, true).unwrap();
        assert!(state.borrow().bytes.iter().all(|b| *b == 0));
        assert_eq!(
            state.borrow().writes,
            [(0, 512, 0), (512, 1, 0), (0, 0, FLUSH_FLAGS)]
        );
        drop(file);
        assert_eq!(state.borrow().closes, ["file", "archive"]);
    }
    #[test]
    fn create_failure_never_opens_or_overwrites_existing_file() {
        let state = Rc::new(RefCell::new(State {
            bytes: vec![0xab; 16],
            failure: Some(Operation::CreateFile),
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 16, true),
            Err(Error::Native {
                operation: Operation::CreateFile,
                ..
            })
        ));
        assert_eq!(state.borrow().bytes, [0xab; 16]);
        assert!(state.borrow().writes.is_empty());
        assert_eq!(state.borrow().closes, ["archive"]);
    }
    #[test]
    fn wrong_size_closes_both_resources_without_writes() {
        let state = Rc::new(RefCell::new(State {
            bytes: vec![0; 8],
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 16, false),
            Err(Error::WrongSize {
                expected: 16,
                actual: 8
            })
        ));
        assert_eq!(state.borrow().closes, ["file", "archive"]);
        assert!(state.borrow().writes.is_empty());
    }
    #[test]
    fn open_failure_preserves_operation_and_closes_archive() {
        let state = Rc::new(RefCell::new(State {
            failure: Some(Operation::OpenFile),
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 16, false),
            Err(Error::Native {
                operation: Operation::OpenFile,
                ..
            })
        ));
        assert_eq!(state.borrow().closes, ["archive"]);
        assert_eq!(state.borrow().creates, 0);
    }
    #[test]
    fn close_failure_still_closes_archive_and_cannot_repeat_io() {
        let (mut file, state) = existing(16);
        state.borrow_mut().failure = Some(Operation::CloseFile);
        assert_eq!(file.close(), Err(failure(Operation::CloseFile)));
        assert_eq!(file.read(0, &mut [0; 1]), Err(Error::Closed));
        assert_eq!(file.sync(), Err(Error::Closed));
        file.close().unwrap();
        drop(file);
        assert_eq!(state.borrow().closes, ["file", "archive"]);
    }
    #[test]
    fn absence_classification_never_conflates_open_stages_or_other_failures() {
        let code = 0xc880_4464;
        assert_eq!(
            Error::Native {
                operation: Operation::OpenArchive,
                code
            }
            .reported_absence(),
            Some(ReportedAbsence::Archive)
        );
        assert_eq!(
            Error::Native {
                operation: Operation::OpenFile,
                code
            }
            .reported_absence(),
            Some(ReportedAbsence::File)
        );
        assert_eq!(
            Error::Native {
                operation: Operation::Read,
                code
            }
            .reported_absence(),
            None
        );
        assert_eq!(failure(Operation::OpenArchive).reported_absence(), None);
    }
}
