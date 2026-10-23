//! Fixed-size storage in Bank's existing SD extdata archive.
//!
//! The console keeps check values for every extdata file and writes new data
//! before it updates them. A write that is interrupted leaves what it touched
//! unreadable: reads fail with a result of the "corrupted data" class instead
//! of returning bad bytes. Each unit of a container (`offline_core::Storage`)
//! is therefore a file of its own here, so that only the unit being written
//! can be lost. A unit that is absent reads as zeros and is created by the
//! first write to it; one that cannot be opened or read is reported as such
//! and is replaced through `recreate`.
//!
//! The native calls are those of the program `P` (`patch_common::program`):
//! Bank 1.5 for Bank's patches, Poké Transporter 1.5 for Transporter's, which
//! visits Bank's files (`Visit`). See docs/internals.md for the verified call
//! ABIs.

#[cfg(any(test, target_arch = "arm"))]
use crate::bank_files::UnitFile;
#[cfg(any(test, target_arch = "arm"))]
use offline_core::Storage;
#[cfg(target_arch = "arm")]
use {crate::program::Bank15, core::marker::PhantomData, patch_common::program::Program};

#[cfg(any(test, target_arch = "arm"))]
const IO_CHUNK: usize = 0x10000;
#[cfg(any(test, target_arch = "arm"))]
use patch_common::file::FLUSH as FLUSH_FLAGS;

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
    DeleteFile,
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
    /// The unit exists but could not be opened: an interrupted write left it.
    Unreadable,
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
    /// `79` a unit that could not be opened, where `oo` is the `Operation`
    /// index (0 OpenArchive, 1 CloseArchive, 2 CreateFile, 3 OpenFile,
    /// 4 Size, 5 Read, 6 Write, 7 Sync, 8 CloseFile, 9 CloseHandle,
    /// A DeleteFile). A wrong file size is shown as that size itself.
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
            Self::Unreadable => 0x7900_0000,
        }
    }

    /// Whether the console refused the operation for lack of space on the
    /// card (`patch_common::fs_result::lack_of_space`).
    pub fn lack_of_space(self) -> bool {
        matches!(self, Self::Native { code, .. } if patch_common::fs_result::lack_of_space(code))
    }
}

/// Whether a failed operation on a card failed for lack of space: the
/// console's file errors, and the simulated card's in the tests.
pub trait NoRoom {
    fn no_room(&self) -> bool;
}
impl NoRoom for Error {
    fn no_room(&self) -> bool {
        self.lack_of_space()
    }
}

impl Error {
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
                    Operation::OpenFile | Operation::DeleteFile => Some(ReportedAbsence::File),
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
    fn delete_file(&mut self, archive: u64) -> Result<(), Error>;
    fn open_file(&mut self, archive: u64) -> Result<u32, Error>;
    fn size(&mut self, file: u32) -> Result<u64, Error>;
    fn read(&mut self, file: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error>;
    fn write(&mut self, file: u32, offset: u64, bytes: &[u8], flags: u32) -> Result<u32, Error>;
    /// Always release the kernel handle, even if the file-close IPC fails.
    fn close_file(&mut self, file: u32) -> Result<(), Error>;
}

/// Probe without creating or mounting an archive through the native manager.
#[cfg(any(test, target_arch = "arm"))]
fn archive_exists(api: &mut impl Api) -> Result<bool, Error> {
    match api.open_archive() {
        Ok(archive) => {
            if archive == 0 {
                return Err(Error::InvalidHandle(Operation::OpenArchive));
            }
            api.close_archive(archive)?;
            Ok(true)
        }
        Err(error) if error.reported_absence() == Some(ReportedAbsence::Archive) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(any(test, target_arch = "arm"))]
struct File<A: Api> {
    api: A,
    handle: Option<u32>,
    length: u64,
}

#[cfg(any(test, target_arch = "arm"))]
impl<A: Api> File<A> {
    /// Opens the file `api` names in the open `archive`, which stays the
    /// caller's to close.
    fn open(api: A, archive: u64, length: u64, create: bool) -> Result<Self, Error> {
        if length == 0 {
            return Err(Error::InvalidLength);
        }
        let mut this = Self {
            api,
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
        match self.handle.take() {
            Some(handle) => self.api.close_file(handle),
            None => Ok(()),
        }
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

    /// A single file is not a unit that can be replaced.
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), Error> {
        Err(Error::Bounds)
    }
}

/// Deletes the file `api` names in the open `archive`. One that is not
/// there is deleted already.
#[cfg(any(test, target_arch = "arm"))]
fn remove_in<A: Api>(api: &mut A, archive: u64) -> Result<(), Error> {
    match api.delete_file(archive) {
        Err(error) if error.reported_absence() == Some(ReportedAbsence::File) => Ok(()),
        result => result,
    }
}

/// Deletes the file `api` names, through an archive handle of its own.
#[cfg(target_arch = "arm")]
fn remove<A: Api>(mut api: A) -> Result<(), Error> {
    let archive = api.open_archive()?;
    let deleted = remove_in(&mut api, archive);
    deleted.and(api.close_archive(archive))
}

/// How the unit files of a container are reached.
#[cfg(any(test, target_arch = "arm"))]
trait Reach {
    type Api: Api;
    fn api(&self, unit: &UnitFile) -> Result<Self::Api, Error>;
}

#[cfg(any(test, target_arch = "arm"))]
const MAX_UNITS: usize = 4;

#[cfg(any(test, target_arch = "arm"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// There, with its size, and it opened when the container was opened.
    Present,
    /// Reads as zeros; the first write creates it.
    Absent,
    /// There, but not to be opened.
    Unreadable,
}

/// A container whose units are separate files, addressed as one range in
/// the order of `files`.
///
/// It holds one archive handle and at most one open unit file, however many
/// units it has: the console gives out only so many of each. With every unit
/// file of two containers open at once, eight files and eight archive
/// handles, creating a file failed with an out-of-resource result
/// (`D860466C`); five and five had worked. One container of this kind and
/// another, two and two, is what a Bank of one file per container used.
///
/// Between uses it can rest (`rest`): it holds nothing open then, and the
/// next read or write opens the archive and the unit file again.
#[cfg(any(test, target_arch = "arm"))]
struct Split<R: Reach> {
    reach: R,
    files: &'static [UnitFile],
    /// The container's archive handle, with the means to close it. `None`
    /// while the container rests, and once it is closed.
    archive: Option<(R::Api, u64)>,
    /// Closed (`close`): nothing is opened again.
    closed: bool,
    units: [Unit; MAX_UNITS],
    /// The unit file that is open, and whether it holds writes that are not
    /// flushed yet.
    open: Option<(usize, File<R::Api>, bool)>,
    /// A flush that failed when its unit was closed for another one. The
    /// next `sync` reports it, so that it is not taken for durable.
    unflushed: Option<Error>,
    /// Opened by a visitor (`visit`), who changes only what is there.
    visiting: bool,
}

#[cfg(any(test, target_arch = "arm"))]
impl<R: Reach> Split<R> {
    fn new(reach: R, files: &'static [UnitFile]) -> Result<Self, Error> {
        if files.is_empty() || files.len() > MAX_UNITS {
            return Err(Error::InvalidLength);
        }
        let mut this = Self {
            reach,
            files,
            archive: None,
            closed: false,
            units: [Unit::Absent; MAX_UNITS],
            open: None,
            unflushed: None,
            visiting: false,
        };
        this.archive()?;
        Ok(this)
    }

    /// The archive handle, opened if the container rests.
    fn archive(&mut self) -> Result<u64, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if let Some((_, archive)) = &self.archive {
            return Ok(*archive);
        }
        let mut api = self.reach.api(&self.files[0])?;
        let archive = api.open_archive()?;
        self.archive = Some((api, archive));
        Ok(archive)
    }

    /// `None` when not one of the files exists. A file of another size, or an
    /// archive that cannot be opened, is an error and nothing an interrupted
    /// write leaves behind.
    fn open(reach: R, files: &'static [UnitFile]) -> Result<Option<Self>, Error> {
        let mut this = Self::new(reach, files)?;
        let archive = this.archive()?;
        let mut found = false;
        for (index, file) in files.iter().enumerate() {
            // Looked at and closed again; it is opened when it is used.
            match File::open(this.reach.api(file)?, archive, file.len, false) {
                Ok(_) => this.units[index] = Unit::Present,
                Err(error) if error.reported_absence() == Some(ReportedAbsence::File) => continue,
                Err(error @ Error::WrongSize { .. }) => return Err(error),
                Err(_) => this.units[index] = Unit::Unreadable,
            }
            found = true;
        }
        Ok(found.then_some(this))
    }

    /// As `open`, for a visitor that uses only what is there: a unit that is
    /// not there is neither read nor written, and no unit is replaced.
    /// Nothing is created or deleted.
    fn visit(reach: R, files: &'static [UnitFile]) -> Result<Option<Self>, Error> {
        let mut this = Self::open(reach, files)?;
        if let Some(split) = &mut this {
            split.visiting = true;
        }
        Ok(this)
    }

    /// Creates every file anew, zero-filled. Fails if one exists.
    fn create(reach: R, files: &'static [UnitFile]) -> Result<Self, Error> {
        let mut this = Self::new(reach, files)?;
        let archive = this.archive()?;
        for (index, file) in files.iter().enumerate() {
            File::open(this.reach.api(file)?, archive, file.len, true)?;
            this.units[index] = Unit::Present;
        }
        Ok(this)
    }

    /// The unit that holds `offset`, and where that unit starts.
    fn locate(&self, offset: u64) -> Result<(usize, u64), Error> {
        let mut start = 0;
        for (index, file) in self.files.iter().enumerate() {
            if offset < start + file.len {
                return Ok((index, start));
            }
            start += file.len;
        }
        Err(Error::Bounds)
    }

    /// Closes the open unit file, flushing it first if it holds writes:
    /// what was written to a unit is durable before another one is touched.
    fn release(&mut self) -> Result<(), Error> {
        let Some((_, mut file, written)) = self.open.take() else {
            return Ok(());
        };
        let flushed = if written { file.sync() } else { Ok(()) };
        if let Err(error) = flushed {
            self.unflushed = Some(error);
        }
        flushed.and(file.close())
    }

    /// The open file of unit `index`, which must be there. `create` makes it
    /// anew first, zero-filled.
    fn select(&mut self, index: usize, create: bool) -> Result<&mut File<R::Api>, Error> {
        if create || !matches!(&self.open, Some((at, ..)) if *at == index) {
            self.release()?;
            let unit = self.files[index];
            let archive = self.archive()?;
            let file = File::open(self.reach.api(&unit)?, archive, unit.len, create)?;
            self.open = Some((index, file, false));
        }
        match &mut self.open {
            Some((_, file, _)) => Ok(file),
            None => Err(Error::Closed),
        }
    }

    /// Closes the open unit file, flushed, and the archive handle. What is
    /// known of the units stays; the next use opens what it needs again.
    fn rest(&mut self) -> Result<(), Error> {
        let released = self.release();
        let closed = match self.archive.take() {
            Some((mut api, archive)) => api.close_archive(archive),
            None => Ok(()),
        };
        released.and(closed)
    }

    fn close(&mut self) -> Result<(), Error> {
        self.closed = true;
        self.units = [Unit::Absent; MAX_UNITS];
        self.rest()
    }
}

#[cfg(any(test, target_arch = "arm"))]
impl<R: Reach> Drop for Split<R> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(any(test, target_arch = "arm"))]
impl<R: Reach> Storage for Split<R> {
    type Error = Error;

    fn read(&mut self, mut offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        // A closed container reads nothing, not even the zeros of an
        // absent unit.
        self.archive()?;
        let mut done = 0;
        while done < bytes.len() {
            let (index, start) = self.locate(offset)?;
            let within = offset - start;
            let count = (bytes.len() - done).min((self.files[index].len - within) as usize);
            let part = &mut bytes[done..done + count];
            match self.units[index] {
                Unit::Present => self.select(index, false)?.read(within, part)?,
                Unit::Absent if self.visiting => return Err(Error::Unreadable),
                Unit::Absent => part.fill(0),
                Unit::Unreadable => return Err(Error::Unreadable),
            }
            done += count;
            offset += count as u64;
        }
        Ok(())
    }

    /// A write lies within one unit.
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        let (index, start) = self.locate(offset)?;
        let within = offset - start;
        if within + bytes.len() as u64 > self.files[index].len {
            return Err(Error::Bounds);
        }
        let file = match self.units[index] {
            Unit::Present => self.select(index, false)?,
            Unit::Absent if self.visiting => return Err(Error::ReadOnly),
            Unit::Absent => {
                let created = self.select(index, true).map(|_| ());
                created?;
                self.units[index] = Unit::Present;
                self.select(index, false)?
            }
            Unit::Unreadable => return Err(Error::Unreadable),
        };
        let written = file.write(within, bytes);
        if let Some((_, _, unflushed)) = &mut self.open {
            *unflushed = true;
        }
        written
    }

    fn sync(&mut self) -> Result<(), Error> {
        if let Some(error) = self.unflushed.take() {
            return Err(error);
        }
        self.archive()?;
        if let Some((_, file, written)) = &mut self.open {
            if *written {
                file.sync()?;
                *written = false;
            }
        }
        Ok(())
    }

    fn recreate(&mut self, offset: u64, length: u64) -> Result<(), Error> {
        if self.visiting {
            return Err(Error::ReadOnly);
        }
        let archive = self.archive()?;
        let (index, start) = self.locate(offset)?;
        let unit = self.files[index];
        if start != offset || unit.len != length {
            return Err(Error::Bounds);
        }
        // Closed before it is deleted, without a flush: what it holds is
        // given up. If deleting fails it is not usable.
        if matches!(&self.open, Some((at, ..)) if *at == index) {
            self.open = None;
        }
        self.units[index] = Unit::Unreadable;
        remove_in(&mut self.reach.api(&unit)?, archive)?;
        self.units[index] = Unit::Absent;
        Ok(())
    }
}

/// One exclusively accessed container of `bank_files::FileName` in SD extdata
/// 0x00000c9b: its unit files, each with its exact allocation size, through
/// the file calls of program `P`. This borrows the program's initialized FS
/// session; it never closes that session.
#[cfg(target_arch = "arm")]
pub struct ExtdataStorage<P: Program = Bank15> {
    split: Split<native::Session<P>>,
}

#[cfg(target_arch = "arm")]
impl<P: Program> ExtdataStorage<P> {
    /// Whether Bank's archive can be opened. Only archive NotFound is false;
    /// no archive or file is created, and a successful handle is closed.
    /// # Safety
    /// The same session and native-thread requirements as `open_existing` apply.
    pub unsafe fn archive_exists(session: u32) -> Result<bool, Error> {
        archive_exists(&mut native::Native::<P>::new(session, "/")?)
    }

    /// Opens the unit files that exist and requires the exact allocation
    /// size of each. `None` when none of them exists.
    ///
    /// # Safety
    /// The executable must match the SHA256 documented above. `session` must be
    /// an initialized fs:USER handle owned by this Bank process and remain live
    /// until close. Call on an initialized native thread with valid IPC TLS.
    /// The caller must guarantee exclusive access to these files and prevent
    /// native extdata deletion/reformatting throughout its lifetime.
    pub unsafe fn open_existing(
        session: u32,
        files: &'static [UnitFile],
    ) -> Result<Option<Self>, Error> {
        Ok(Split::open(native::Session::new(session)?, files)?.map(|split| Self { split }))
    }

    /// Opens the unit files that exist for a visitor (`Split::visit`), as
    /// `open_existing` does. `None` when none of them exists.
    ///
    /// # Safety
    /// The same requirements as `open_existing` apply.
    pub unsafe fn visit(session: u32, files: &'static [UnitFile]) -> Result<Option<Self>, Error> {
        Ok(Split::visit(native::Session::new(session)?, files)?.map(|split| Self { split }))
    }

    /// Exclusively creates the unit files in an already existing Bank
    /// archive, zeros them, and flushes them. Failure never resets existing
    /// data. Missing archives are returned as errors; no archive is created
    /// here.
    ///
    /// # Safety
    /// The same requirements as `open_existing` apply. The caller must have
    /// selected fresh initialization explicitly, not inferred it from a corrupt
    /// snapshot, a failed read, or an inaccessible archive.
    pub unsafe fn create_new(session: u32, files: &'static [UnitFile]) -> Result<Self, Error> {
        Ok(Self {
            split: Split::create(native::Session::new(session)?, files)?,
        })
    }

    /// Returns close failures; Drop also attempts cleanup but cannot report them.
    /// Explicit sync is required before relying on durability.
    pub fn close(&mut self) -> Result<(), Error> {
        self.split.close()
    }

    /// Whether every one of `files` is there with exactly its size.
    ///
    /// # Safety
    /// The same requirements as `open_existing` apply.
    pub unsafe fn exists(session: u32, files: &'static [UnitFile]) -> Result<bool, Error> {
        match Split::open(native::Session::<P>::new(session)?, files) {
            Ok(Some(mut split)) => {
                let whole = split.units[..files.len()]
                    .iter()
                    .all(|unit| *unit == Unit::Present);
                split.close()?;
                Ok(whole)
            }
            Ok(None) | Err(Error::WrongSize { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Deletes those of `files` that are there.
    ///
    /// # Safety
    /// The same requirements as `open_existing` apply, and none of the files
    /// may be open.
    pub unsafe fn remove(session: u32, files: &'static [UnitFile]) -> Result<(), Error> {
        let reach = native::Session::<P>::new(session)?;
        for file in files {
            remove(reach.api(file)?)?;
        }
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
impl<P: Program> Storage for ExtdataStorage<P> {
    type Error = Error;
    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        self.split.read(offset, bytes)
    }
    fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        self.split.write(offset, bytes)
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.split.sync()
    }
    fn recreate(&mut self, offset: u64, length: u64) -> Result<(), Error> {
        self.split.recreate(offset, length)
    }
}

#[cfg(target_arch = "arm")]
impl<P: Program> crate::bank_files::Rest for ExtdataStorage<P> {
    /// The archive handle and the open unit file, flushed first if it holds
    /// writes.
    fn rest(&mut self) -> Result<(), Error> {
        self.split.rest()
    }
}

/// Bank's files as the Transporter patch reaches them (`transport::Files`),
/// through the file calls of program `P`: the Bank container for its two
/// journal records, and the transport box, each visited (`Split::visit`)
/// and one at a time. Its simulated twin is `testing::Visit`.
#[cfg(target_arch = "arm")]
pub struct Visit<P> {
    session: u32,
    program: PhantomData<P>,
}
#[cfg(target_arch = "arm")]
impl<P: Program> Visit<P> {
    /// # Safety
    /// The requirements of `ExtdataStorage::open_existing` apply to
    /// `session`, for as long as this is used.
    pub const unsafe fn new(session: u32) -> Self {
        Self {
            session,
            program: PhantomData,
        }
    }
    fn visit(&self, file: crate::bank_files::FileName) -> Option<ExtdataStorage<P>> {
        unsafe { ExtdataStorage::visit(self.session, file.units()) }
            .ok()
            .flatten()
    }
}
#[cfg(target_arch = "arm")]
impl<P: Program> offline_core::transport::Files for Visit<P> {
    type Storage = ExtdataStorage<P>;
    fn journal(&mut self) -> Option<ExtdataStorage<P>> {
        self.visit(crate::bank_files::FileName::Bank)
    }
    fn mover(&mut self) -> Option<ExtdataStorage<P>> {
        self.visit(crate::bank_files::FileName::Transport)
    }
}

/// Read-only `main` in a borrowed, already mounted game archive. The archive and
/// fs:USER session remain owned by native Bank; only this file handle is closed.
#[cfg(target_arch = "arm")]
pub struct GameMainReader<'a> {
    handle: Option<patch_common::file::Handle<Bank15>>,
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
        let mut api = native::Native::<Bank15>::new(session, "/main")?;
        let handle = api.open_main(archive)?;
        let handle = unsafe { patch_common::file::Handle::new(handle) }
            .ok_or(Error::InvalidHandle(Operation::OpenFile))?;
        let length = handle.size().map_err(native::failed(Operation::Size))?;
        if length == 0 {
            return Err(Error::InvalidLength);
        }
        Ok(Self {
            handle: Some(handle),
            length,
            archive_borrow: core::marker::PhantomData,
        })
    }
    pub fn len(&self) -> u64 {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn close(&mut self) -> Result<(), Error> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        let (closed, kernel) = handle.close();
        closed
            .map_err(native::failed(Operation::CloseFile))
            .and(kernel.map_err(native::failed(Operation::CloseHandle)))
    }
}

#[cfg(target_arch = "arm")]
impl Storage for GameMainReader<'_> {
    type Error = Error;
    fn read(&mut self, mut offset: u64, bytes: &mut [u8]) -> Result<(), Error> {
        let handle = self.handle.as_ref().ok_or(Error::Closed)?;
        if offset
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(Error::Bounds);
        }
        for chunk in bytes.chunks_mut(IO_CHUNK) {
            let actual = handle
                .read(offset, chunk)
                .map_err(native::failed(Operation::Read))?;
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
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), Error> {
        Err(Error::ReadOnly)
    }
}
#[cfg(target_arch = "arm")]
mod native {
    use super::{Api, Error, Operation, Reach};
    use crate::bank_files::UnitFile;
    use core::{marker::PhantomData, mem::transmute};
    use patch_common::{file, program::Program};

    #[repr(C)]
    struct Path {
        kind: u32,
        data: *const u8,
        byte_len: u32,
    }

    /// A path of the longest name the archive stores, as UTF-16 with its
    /// terminator: the form the native path type expects.
    const NAME: usize = 1 + UnitFile::MAX_NAME + 1;

    /// The program's file system session, through which each unit file is
    /// reached.
    pub(super) struct Session<P>(u32, PhantomData<P>);
    impl<P: Program> Session<P> {
        pub(super) fn new(session: u32) -> Result<Self, Error> {
            if session == 0 {
                return Err(Error::InvalidHandle(Operation::OpenArchive));
            }
            Ok(Self(session, PhantomData))
        }
    }
    impl<P: Program> Reach for Session<P> {
        type Api = Native<P>;
        fn api(&self, unit: &UnitFile) -> Result<Native<P>, Error> {
            Native::new(self.0, unit.path)
        }
    }

    pub(super) struct Native<P> {
        session: u32,
        name: [u16; NAME],
        /// Bytes of `name` with its terminator.
        byte_len: u32,
        _thread: PhantomData<*mut ()>,
        _program: PhantomData<P>,
    }
    impl<P: Program> Native<P> {
        pub(super) fn open_main(&mut self, archive: u64) -> Result<u32, Error> {
            const MAIN: [u16; 6] = [47, 109, 97, 105, 110, 0];
            let path = Path {
                kind: 4,
                data: MAIN.as_ptr().cast(),
                byte_len: 12,
            };
            type Call =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u32, u64, Path, u32, u32) -> i32;
            let call: Call = unsafe { transmute(P::OPEN_FILE) };
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
                byte_len: self.byte_len,
            }
        }
        pub(super) fn new(session: u32, path: &str) -> Result<Self, Error> {
            if session == 0 {
                return Err(Error::InvalidHandle(Operation::OpenArchive));
            }
            let bytes = path.as_bytes();
            if bytes.len() >= NAME {
                return Err(Error::InvalidLength);
            }
            let mut name = [0; NAME];
            for (unit, byte) in name.iter_mut().zip(bytes) {
                *unit = u16::from(*byte);
            }
            Ok(Self {
                session,
                name,
                byte_len: (bytes.len() as u32 + 1) * 2,
                _thread: PhantomData,
                _program: PhantomData,
            })
        }
    }

    fn result(operation: Operation, code: i32) -> Result<(), Error> {
        if code < 0 {
            Err(failed(operation)(code))
        } else {
            Ok(())
        }
    }
    pub(super) fn failed(operation: Operation) -> impl Fn(file::Code) -> Error {
        move |code| Error::Native {
            operation,
            code: code as u32,
        }
    }

    impl<P: Program> Api for Native<P> {
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
            let call: Call = unsafe { transmute(P::OPEN_ARCHIVE) };
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
            let call: Call = unsafe { transmute(P::CLOSE_ARCHIVE) };
            result(Operation::CloseArchive, unsafe {
                call(&self.session, archive)
            })
        }
        fn create_file(&mut self, archive: u64, length: u64) -> Result<(), Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, u32, u64, Path, u32, u64) -> i32;
            let Some(address) = P::CREATE_FILE else {
                return Err(Error::ReadOnly);
            };
            let call: Call = unsafe { transmute(address) };
            result(Operation::CreateFile, unsafe {
                call(&self.session, 0, archive, self.name(), 0, length)
            })
        }
        fn delete_file(&mut self, archive: u64) -> Result<(), Error> {
            type Call = unsafe extern "aapcs" fn(*const u32, u32, u64, Path) -> i32;
            let Some(address) = P::DELETE_FILE else {
                return Err(Error::ReadOnly);
            };
            let call: Call = unsafe { transmute(address) };
            result(Operation::DeleteFile, unsafe {
                call(&self.session, 0, archive, self.name())
            })
        }
        fn open_file(&mut self, archive: u64) -> Result<u32, Error> {
            type Call =
                unsafe extern "aapcs" fn(*const u32, *mut u32, u32, u64, Path, u32, u32) -> i32;
            let call: Call = unsafe { transmute(P::OPEN_FILE) };
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
            file::size::<P>(file).map_err(failed(Operation::Size))
        }
        fn read(&mut self, file: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error> {
            let length = bytes.len() as u32;
            unsafe { file::read::<P>(file, offset, bytes.as_mut_ptr(), length) }
                .map_err(failed(Operation::Read))
        }
        fn write(
            &mut self,
            file: u32,
            offset: u64,
            bytes: &[u8],
            flags: u32,
        ) -> Result<u32, Error> {
            let length = bytes.len() as u32;
            unsafe { file::write::<P>(file, offset, bytes.as_ptr(), length, flags) }
                .map_err(failed(Operation::Write))
        }
        fn close_file(&mut self, file: u32) -> Result<(), Error> {
            let (closed, handle) = file::close::<P>(file);
            closed
                .map_err(failed(Operation::CloseFile))
                .and(handle.map_err(failed(Operation::CloseHandle)))
        }
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn only_the_consoles_own_refusal_for_lack_of_space_is_a_full_card() {
        let native = |code| Error::Native {
            operation: Operation::CreateFile,
            code,
        };
        assert!(native(0xc860_44d2).lack_of_space());
        assert!(!native(0xc880_4478).lack_of_space());
        // A short write is no answer of the console about its space.
        let short = Error::Count {
            operation: Operation::Write,
            expected: 512,
            actual: 0,
        };
        assert!(!short.lack_of_space());
    }

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
        archive_error: Option<Error>,
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
            if let Some(error) = self.0.borrow().archive_error {
                return Err(error);
            }
            if self.0.borrow().failure == Some(Operation::OpenArchive) {
                Err(failure(Operation::OpenArchive))
            } else {
                Ok(1)
            }
        }
        fn close_archive(&mut self, _: u64) -> Result<(), Error> {
            self.0.borrow_mut().closes.push("archive");
            if self.0.borrow().failure == Some(Operation::CloseArchive) {
                Err(failure(Operation::CloseArchive))
            } else {
                Ok(())
            }
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
        fn delete_file(&mut self, _: u64) -> Result<(), Error> {
            self.0.borrow_mut().bytes.clear();
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
            File::open(Mock(state.clone()), 1, length as u64, false).unwrap(),
            state,
        )
    }

    #[test]
    fn archive_probe_closes_its_handle_and_never_touches_files() {
        let state = Rc::new(RefCell::new(State::default()));
        assert_eq!(archive_exists(&mut Mock(state.clone())), Ok(true));
        let state = state.borrow();
        assert_eq!(state.closes, ["archive"]);
        assert_eq!(state.creates, 0);
        assert_eq!(state.reads, 0);
        assert!(state.writes.is_empty());
    }

    #[test]
    fn only_archive_not_found_is_absence_and_close_errors_propagate() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut api = Mock(state.clone());
        // Usage/NotFound/FS/NotFound: use the same class as reported_absence.
        let not_found = Error::Native {
            operation: Operation::OpenArchive,
            code: 0xc880_4478,
        };
        state.borrow_mut().archive_error = Some(not_found);
        assert_eq!(archive_exists(&mut api), Ok(false));
        for error in [
            failure(Operation::OpenArchive),
            Error::Native {
                operation: Operation::OpenArchive,
                code: 0xd8a0_4565,
            },
            Error::Native {
                operation: Operation::OpenFile,
                code: 0xc880_4478,
            },
            Error::InvalidHandle(Operation::OpenArchive),
        ] {
            state.borrow_mut().archive_error = Some(error);
            assert_eq!(archive_exists(&mut api), Err(error));
        }
        assert!(state.borrow().closes.is_empty());
        state.borrow_mut().archive_error = None;
        state.borrow_mut().failure = Some(Operation::CloseArchive);
        assert_eq!(
            archive_exists(&mut api),
            Err(failure(Operation::CloseArchive))
        );
        assert_eq!(state.borrow().closes, ["archive"]);
        assert_eq!(state.borrow().creates, 0);
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
        let file = File::open(Mock(state.clone()), 1, 513, true).unwrap();
        assert!(state.borrow().bytes.iter().all(|b| *b == 0));
        assert_eq!(
            state.borrow().writes,
            [(0, 512, 0), (512, 1, 0), (0, 0, FLUSH_FLAGS)]
        );
        drop(file);
        // The archive is the container's to close.
        assert_eq!(state.borrow().closes, ["file"]);
    }
    #[test]
    fn create_failure_never_opens_or_overwrites_existing_file() {
        let state = Rc::new(RefCell::new(State {
            bytes: vec![0xab; 16],
            failure: Some(Operation::CreateFile),
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 1, 16, true),
            Err(Error::Native {
                operation: Operation::CreateFile,
                ..
            })
        ));
        assert_eq!(state.borrow().bytes, [0xab; 16]);
        assert!(state.borrow().writes.is_empty());
        assert!(state.borrow().closes.is_empty());
    }
    #[test]
    fn wrong_size_closes_the_file_without_writes() {
        let state = Rc::new(RefCell::new(State {
            bytes: vec![0; 8],
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 1, 16, false),
            Err(Error::WrongSize {
                expected: 16,
                actual: 8
            })
        ));
        assert_eq!(state.borrow().closes, ["file"]);
        assert!(state.borrow().writes.is_empty());
    }
    #[test]
    fn open_failure_preserves_operation_and_leaves_nothing_open() {
        let state = Rc::new(RefCell::new(State {
            failure: Some(Operation::OpenFile),
            ..State::default()
        }));
        assert!(matches!(
            File::open(Mock(state.clone()), 1, 16, false),
            Err(Error::Native {
                operation: Operation::OpenFile,
                ..
            })
        ));
        assert!(state.borrow().closes.is_empty());
        assert_eq!(state.borrow().creates, 0);
    }
    #[test]
    fn close_failure_cannot_repeat_io() {
        let (mut file, state) = existing(16);
        state.borrow_mut().failure = Some(Operation::CloseFile);
        assert_eq!(file.close(), Err(failure(Operation::CloseFile)));
        assert_eq!(file.read(0, &mut [0; 1]), Err(Error::Closed));
        assert_eq!(file.sync(), Err(Error::Closed));
        file.close().unwrap();
        drop(file);
        assert_eq!(state.borrow().closes, ["file"]);
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

#[cfg(test)]
mod split_tests {
    extern crate std;
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        rc::Rc,
        vec,
        vec::Vec,
    };

    const NOT_FOUND: u32 = 0xc880_4478;
    const CORRUPTED: u32 = 0xd900_458b;

    #[derive(Default)]
    struct Entry {
        bytes: Vec<u8>,
        /// What the last flush made durable.
        flushed: Vec<u8>,
        /// Left behind by an interrupted write.
        torn: bool,
    }
    /// Handles open right now, and the most there ever were.
    #[derive(Clone, Default)]
    struct Count(Rc<Cell<(usize, usize)>>);
    impl Count {
        fn opened(&self) {
            let (now, most) = self.0.get();
            self.0.set((now + 1, most.max(now + 1)));
        }
        fn closed(&self) {
            let (now, most) = self.0.get();
            self.0
                .set((now.checked_sub(1).expect("closed more than opened"), most));
        }
        fn now(&self) -> usize {
            self.0.get().0
        }
        fn most(&self) -> usize {
            self.0.get().1
        }
    }
    #[derive(Clone, Default)]
    struct Card {
        files: Rc<RefCell<BTreeMap<&'static str, Entry>>>,
        archive_fails: Rc<RefCell<bool>>,
        flush_fails: Rc<RefCell<bool>>,
        archives: Count,
        handles: Count,
    }
    struct OneFile(Card, &'static str);
    impl Reach for Card {
        type Api = OneFile;
        fn api(&self, unit: &UnitFile) -> Result<OneFile, Error> {
            Ok(OneFile(self.clone(), unit.path))
        }
    }
    fn native(operation: Operation, code: u32) -> Error {
        Error::Native { operation, code }
    }
    impl Api for OneFile {
        fn open_archive(&mut self) -> Result<u64, Error> {
            if *self.0.archive_fails.borrow() {
                return Err(native(Operation::OpenArchive, CORRUPTED));
            }
            self.0.archives.opened();
            Ok(1)
        }
        fn close_archive(&mut self, _: u64) -> Result<(), Error> {
            self.0.archives.closed();
            Ok(())
        }
        fn create_file(&mut self, _: u64, length: u64) -> Result<(), Error> {
            let mut files = self.0.files.borrow_mut();
            if files.contains_key(self.1) {
                return Err(native(Operation::CreateFile, 0xc820_44be));
            }
            files.insert(
                self.1,
                Entry {
                    bytes: vec![0xa5; length as usize],
                    flushed: vec![0xa5; length as usize],
                    torn: false,
                },
            );
            Ok(())
        }
        fn delete_file(&mut self, _: u64) -> Result<(), Error> {
            match self.0.files.borrow_mut().remove(self.1) {
                Some(_) => Ok(()),
                None => Err(native(Operation::DeleteFile, NOT_FOUND)),
            }
        }
        fn open_file(&mut self, _: u64) -> Result<u32, Error> {
            match self.0.files.borrow().get(self.1) {
                Some(_) => {
                    self.0.handles.opened();
                    Ok(7)
                }
                None => Err(native(Operation::OpenFile, NOT_FOUND)),
            }
        }
        fn size(&mut self, _: u32) -> Result<u64, Error> {
            Ok(self.0.files.borrow()[self.1].bytes.len() as u64)
        }
        fn read(&mut self, _: u32, offset: u64, bytes: &mut [u8]) -> Result<u32, Error> {
            let files = self.0.files.borrow();
            let entry = &files[self.1];
            if entry.torn {
                return Err(native(Operation::Read, CORRUPTED));
            }
            bytes.copy_from_slice(&entry.bytes[offset as usize..offset as usize + bytes.len()]);
            Ok(bytes.len() as u32)
        }
        fn write(&mut self, _: u32, offset: u64, bytes: &[u8], flags: u32) -> Result<u32, Error> {
            let mut files = self.0.files.borrow_mut();
            let entry = files.get_mut(self.1).unwrap();
            if entry.torn {
                return Err(native(Operation::Write, CORRUPTED));
            }
            entry.bytes[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
            if flags == FLUSH_FLAGS {
                if *self.0.flush_fails.borrow() {
                    return Err(native(Operation::Write, CORRUPTED));
                }
                entry.flushed = entry.bytes.clone();
            }
            Ok(bytes.len() as u32)
        }
        fn close_file(&mut self, _: u32) -> Result<(), Error> {
            self.0.handles.closed();
            Ok(())
        }
    }

    static FILES: [UnitFile; 3] = [
        UnitFile::new("/one.bin", 4),
        UnitFile::new("/two.bin", 6),
        UnitFile::new("/three.bin", 5),
    ];
    fn card() -> Card {
        let card = Card::default();
        let mut created = Split::create(card.clone(), &FILES).unwrap();
        created.write(0, b"abcd").unwrap();
        created.write(4, b"efghij").unwrap();
        created.write(10, b"klmno").unwrap();
        created.sync().unwrap();
        card
    }
    fn open(card: &Card) -> Split<Card> {
        Split::open(card.clone(), &FILES).unwrap().unwrap()
    }

    #[test]
    fn the_unit_files_read_and_write_as_one_range() {
        let card = card();
        assert_eq!(card.files.borrow().len(), 3);
        let mut split = open(&card);
        let mut all = [0; 15];
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"abcdefghijklmno");
        let mut across = [0; 7];
        split.read(2, &mut across).unwrap();
        assert_eq!(&across, b"cdefghi");
        split.write(6, b"XY").unwrap();
        assert_eq!(card.files.borrow()["/two.bin"].bytes, b"efXYij");
        // A write never crosses into the next unit file, and nothing lies
        // past the last one.
        assert_eq!(split.write(3, b"zz"), Err(Error::Bounds));
        assert_eq!(split.read(14, &mut [0; 2]), Err(Error::Bounds));
        assert_eq!(card.files.borrow()["/one.bin"].bytes, b"abcd");
    }

    #[test]
    fn nothing_is_found_only_when_no_unit_file_exists() {
        let empty = Card::default();
        assert!(Split::open(empty.clone(), &FILES).unwrap().is_none());
        assert_eq!((empty.archives.now(), empty.handles.now()), (0, 0));
        // Creating again is refused while one of them exists.
        let card = card();
        assert!(Split::create(card.clone(), &FILES).is_err());
        card.files.borrow_mut().remove("/one.bin");
        card.files.borrow_mut().remove("/three.bin");
        assert!(Split::open(card.clone(), &FILES).unwrap().is_some());
        card.files.borrow_mut().remove("/two.bin");
        assert!(Split::open(card.clone(), &FILES).unwrap().is_none());
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
    }

    #[test]
    fn an_absent_unit_file_reads_as_zeros_and_the_first_write_creates_it() {
        let card = card();
        card.files.borrow_mut().remove("/two.bin");
        let mut split = open(&card);
        let mut all = [0xff; 15];
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"abcd\0\0\0\0\0\0klmno");
        assert!(!card.files.borrow().contains_key("/two.bin"));
        split.write(5, b"Q").unwrap();
        split.sync().unwrap();
        // Created whole and zero-filled, then written.
        assert_eq!(card.files.borrow()["/two.bin"].bytes, b"\0Q\0\0\0\0");
    }

    #[test]
    fn a_visitor_changes_only_what_is_there() {
        let card = card();
        card.files.borrow_mut().remove("/two.bin");
        let mut split = Split::visit(card.clone(), &FILES).unwrap().unwrap();
        // A unit that is not there is not read, unlike for Bank, which reads
        // it as zeros; the units that are there are.
        let mut all = [0xff; 15];
        assert_eq!(split.read(0, &mut all), Err(Error::Unreadable));
        split.read(0, &mut all[..4]).unwrap();
        split.read(10, &mut all[10..]).unwrap();
        assert_eq!(&all[..4], b"abcd");
        assert_eq!(&all[10..], b"klmno");
        // It is neither created nor replaced, and no unit is.
        assert_eq!(split.write(5, b"Q"), Err(Error::ReadOnly));
        assert_eq!(split.recreate(4, 6), Err(Error::ReadOnly));
        assert_eq!(split.recreate(0, 4), Err(Error::ReadOnly));
        assert!(!card.files.borrow().contains_key("/two.bin"));
        // A unit that was only read is sent no flush.
        *card.flush_fails.borrow_mut() = true;
        split.read(0, &mut [0; 4]).unwrap();
        split.sync().unwrap();
        *card.flush_fails.borrow_mut() = false;
        // What is there is written and made durable.
        split.write(10, b"KL").unwrap();
        split.sync().unwrap();
        assert_eq!(card.files.borrow()["/three.bin"].flushed, b"KLmno");
        assert_eq!(card.files.borrow()["/one.bin"].bytes, b"abcd");
        drop(split);
        assert_eq!(card.files.borrow().len(), 2);
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        assert_eq!((card.archives.most(), card.handles.most()), (1, 1));
        // Without any of its files there is nothing to visit.
        assert!(Split::visit(Card::default(), &FILES).unwrap().is_none());
    }

    #[test]
    fn a_unit_file_left_by_an_interrupted_write_fails_alone_and_is_replaced() {
        let card = card();
        card.files.borrow_mut().get_mut("/two.bin").unwrap().torn = true;
        let mut split = open(&card);
        let mut part = [0; 4];
        split.read(0, &mut part).unwrap();
        assert_eq!(&part, b"abcd");
        split.read(10, &mut part).unwrap();
        assert_eq!(&part, b"klmn");
        assert_eq!(
            split.read(4, &mut part),
            Err(native(Operation::Read, CORRUPTED))
        );
        assert_eq!(
            split.write(4, b"new"),
            Err(native(Operation::Write, CORRUPTED))
        );
        // Only the exact unit is replaced, and the others are untouched.
        assert_eq!(split.recreate(4, 5), Err(Error::Bounds));
        assert_eq!(split.recreate(5, 5), Err(Error::Bounds));
        split.recreate(4, 6).unwrap();
        assert!(!card.files.borrow().contains_key("/two.bin"));
        split.read(4, &mut part).unwrap();
        assert_eq!(part, [0; 4]);
        split.write(4, b"new").unwrap();
        split.sync().unwrap();
        assert_eq!(card.files.borrow()["/two.bin"].bytes, b"new\0\0\0");
        assert_eq!(card.files.borrow()["/one.bin"].bytes, b"abcd");
        assert_eq!(card.files.borrow()["/three.bin"].bytes, b"klmno");
        // Replacing one that is not there is the same as having done it.
        split.recreate(0, 4).unwrap();
        split.recreate(0, 4).unwrap();
        drop(split);
        assert_eq!((card.archives.most(), card.handles.most()), (1, 1));
    }

    #[test]
    fn a_file_of_another_size_or_an_unreachable_archive_is_an_error_and_nothing_else() {
        let card = card();
        card.files
            .borrow_mut()
            .get_mut("/two.bin")
            .unwrap()
            .bytes
            .push(0);
        assert_eq!(
            Split::open(card.clone(), &FILES).err(),
            Some(Error::WrongSize {
                expected: 6,
                actual: 7
            })
        );
        // It is not replaced either: whoever asks never gets that far.
        assert_eq!(card.files.borrow()["/two.bin"].bytes.len(), 7);
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        // Files that are not there before it do not hide it: the error
        // screen shows its size, as for the Bank file of v0.2.1 behind the
        // journal records it does not have.
        card.files.borrow_mut().remove("/one.bin");
        let error = Split::open(card.clone(), &FILES).err().unwrap();
        assert_eq!(error.diagnostic(), 7);

        let card = self::card();
        *card.archive_fails.borrow_mut() = true;
        assert_eq!(
            Split::open(card.clone(), &FILES).err(),
            Some(native(Operation::OpenArchive, CORRUPTED))
        );
        *card.archive_fails.borrow_mut() = false;
        let mut split = open(&card);
        split.close().unwrap();
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        // Closed: nothing is opened again behind the caller's back, and
        // nothing reads as zeros.
        let mut all = [0xff; 15];
        assert_eq!(split.read(0, &mut all), Err(Error::Closed));
        assert_eq!(all, [0xff; 15]);
        assert_eq!(split.write(0, b"x"), Err(Error::Closed));
        assert_eq!(split.sync(), Err(Error::Closed));
        assert_eq!(split.recreate(0, 4), Err(Error::Closed));
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
    }

    #[test]
    fn a_container_holds_one_archive_handle_and_never_more_than_one_open_file() {
        // The console gives out only so many handles: with every unit file
        // of two containers open, creating a file failed there.
        let card = Card::default();
        let mut split = Split::create(card.clone(), &FILES).unwrap();
        assert_eq!((card.archives.now(), card.handles.now()), (1, 0));
        split.write(0, b"abcd").unwrap();
        split.write(4, b"efghij").unwrap();
        split.write(10, b"klmno").unwrap();
        split.sync().unwrap();
        let mut all = [0; 15];
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"abcdefghijklmno");
        split.recreate(4, 6).unwrap();
        split.write(4, b"EFGHIJ").unwrap();
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"abcdEFGHIJklmno");
        assert_eq!((card.archives.now(), card.handles.now()), (1, 1));
        // A second container beside it: two and two, as a Bank of one file
        // per container had.
        let mut other = open(&card);
        other.read(0, &mut all).unwrap();
        assert_eq!((card.archives.now(), card.handles.now()), (2, 2));
        drop(other);
        drop(split);
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        assert_eq!((card.archives.most(), card.handles.most()), (2, 2));
        // Opening looks at every unit file, one at a time.
        let card = self::card();
        let before = card.handles.most();
        drop(open(&card));
        assert_eq!(
            (card.archives.most(), card.handles.most().max(before)),
            (1, 1)
        );
    }

    #[test]
    fn a_container_that_rests_holds_nothing_and_opens_again_when_used() {
        let card = card();
        let mut split = open(&card);
        split.write(4, b"EF").unwrap();
        split.rest().unwrap();
        // Nothing open, and what was written is flushed.
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        assert_eq!(card.files.borrow()["/two.bin"].flushed, b"EFghij");
        // A second rest has nothing to let go of.
        split.rest().unwrap();
        let mut all = [0; 15];
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"abcdEFghijklmno");
        assert_eq!((card.archives.now(), card.handles.now()), (1, 1));
        drop(split);
        // What it knew of the units stays: an absent unit reads as zeros
        // after a rest and is created by its first write.
        card.files.borrow_mut().remove("/one.bin");
        let mut split = open(&card);
        split.rest().unwrap();
        split.read(0, &mut all).unwrap();
        assert_eq!(&all, b"\0\0\0\0EFghijklmno");
        split.rest().unwrap();
        split.write(0, b"ab").unwrap();
        split.rest().unwrap();
        assert_eq!(card.files.borrow()["/one.bin"].flushed, b"ab\0\0");
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        // One that is closed does not open again.
        split.close().unwrap();
        assert_eq!(split.read(0, &mut all), Err(Error::Closed));
        assert_eq!(split.write(0, b"x"), Err(Error::Closed));
        drop(split);
        assert_eq!((card.archives.now(), card.handles.now()), (0, 0));
        assert_eq!((card.archives.most(), card.handles.most()), (1, 1));
    }

    #[test]
    fn what_was_written_to_a_unit_is_flushed_before_another_unit_is_opened() {
        let card = card();
        let mut split = open(&card);
        split.write(0, b"WXYZ").unwrap();
        assert_eq!(card.files.borrow()["/one.bin"].flushed, b"abcd");
        // Moving on to another unit closes the first one, flushed.
        split.write(4, b"123456").unwrap();
        assert_eq!(card.files.borrow()["/one.bin"].flushed, b"WXYZ");
        assert_eq!(card.files.borrow()["/two.bin"].flushed, b"efghij");
        // A read moves on as well.
        let mut part = [0; 2];
        split.read(10, &mut part).unwrap();
        assert_eq!(card.files.borrow()["/two.bin"].flushed, b"123456");
        // And closing does.
        split.write(10, b"K").unwrap();
        split.close().unwrap();
        assert_eq!(card.files.borrow()["/three.bin"].flushed, b"Klmno");
    }

    #[test]
    fn a_flush_that_fails_when_a_unit_is_left_is_not_taken_for_durable() {
        let card = card();
        let mut split = open(&card);
        split.write(0, b"WXYZ").unwrap();
        *card.flush_fails.borrow_mut() = true;
        // The operation that had to leave the unit fails...
        assert_eq!(
            split.write(4, b"123456"),
            Err(native(Operation::Sync, CORRUPTED))
        );
        *card.flush_fails.borrow_mut() = false;
        // ...and so does the next sync, once.
        assert_eq!(split.sync(), Err(native(Operation::Sync, CORRUPTED)));
        split.sync().unwrap();
        split.write(4, b"123456").unwrap();
        split.sync().unwrap();
        assert_eq!(card.files.borrow()["/two.bin"].flushed, b"123456");
        assert_eq!((card.archives.most(), card.handles.most()), (1, 1));
    }
}
