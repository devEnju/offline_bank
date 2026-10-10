//! Native game preparation, exact block views, and controlled save lifecycle.
//!
//! See docs/internals.md for the verified ABI and persistence boundaries.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GameKind {
    X = 1,
    Y = 2,
    OmegaRuby = 3,
    AlphaSapphire = 4,
    Sun = 5,
    Moon = 6,
    UltraSun = 7,
    UltraMoon = 8,
}

impl TryFrom<u8> for GameKind {
    type Error = NativeGameError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::X),
            2 => Ok(Self::Y),
            3 => Ok(Self::OmegaRuby),
            4 => Ok(Self::AlphaSapphire),
            5 => Ok(Self::Sun),
            6 => Ok(Self::Moon),
            7 => Ok(Self::UltraSun),
            8 => Ok(Self::UltraMoon),
            other => Err(NativeGameError::UnknownGame(other)),
        }
    }
}
impl GameKind {
    /// Identifies the game title, not a particular cartridge or trainer save.
    pub const fn title_id(self) -> u64 {
        let unique = match self {
            Self::X => 0x55d,
            Self::Y => 0x55e,
            Self::OmegaRuby => 0x11c4,
            Self::AlphaSapphire => 0x11c5,
            Self::Sun => 0x1648,
            Self::Moon => 0x175e,
            Self::UltraSun => 0x1b50,
            Self::UltraMoon => 0x1b51,
        };
        0x0004_0000_0000_0000 | (unique << 8)
    }
    /// Native constructor's data-block count, excluding the final metadata block.
    pub const fn data_block_count(self) -> u32 {
        match self {
            Self::X | Self::Y => 55,
            Self::OmegaRuby | Self::AlphaSapphire => 58,
            Self::Sun | Self::Moon => 37,
            Self::UltraSun | Self::UltraMoon => 39,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeGameError {
    NullPointer,
    UnalignedPointer,
    AddressOverflow,
    WrongVtable(u32),
    UnknownGame(u8),
    WrongBlockCount { expected: u32, actual: u32 },
    InvalidGetter(u32),
    InvalidFileLength,
    BlockOutsideFile,
    WrongMetadataVtable(u32),
    NotLoaded,
    Busy,
    InvalidPhase,
    MissingArchive,
    InvalidSecureFlags { first: u8, second: u8 },
    SecureValueMismatch,
    NativeResult { operation: GameOperation, code: u32 },
    NativeStatus(u32),
    Validation { status: u32, detail: u32 },
    Storage(bank_common::fs::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecureValues {
    /// Metadata value generated for this prepared image.
    pub current: u64,
    /// Metadata value retained from before native preparation.
    pub previous: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOperation {
    ReadSecureValue,
    WriteSecureValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformSecureValue {
    pub value_present: bool,
    /// The console's answer to whether the title is on a game card. The
    /// native predicate accepts a mismatch then: a cartridge keeps its value
    /// in its own save image. It is also what tells a cartridge copy of a
    /// game from an installed one.
    pub gamecard: bool,
    pub value: u64,
}
impl PlatformSecureValue {
    pub const fn matches_native_rule(self, expected: u64) -> bool {
        self.gamecard || !self.value_present || self.value == expected
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePoll {
    Pending,
    Complete,
}

/// Read-only values captured from a native game whose owners remain alive.
/// The worker borrows the existing archive and never closes it or the FS session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameIoDescriptor {
    pub kind: GameKind,
    pub(crate) session: u32,
    pub(crate) archive: u64,
    pub(crate) metadata_offset: u64,
}

pub const MAX_PREPARED_BLOCKS: usize = 59;
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrozenBlock {
    pub offset: u64,
    pub address: usize,
    pub length: usize,
}
impl FrozenBlock {
    #[cfg(target_arch = "arm")]
    const EMPTY: Self = Self {
        offset: 0,
        address: 0,
        length: 0,
    };
}

/// Raw immutable block descriptors, not ownership of their native allocations.
/// Submitting these to a worker is unsafe: the native game must remain frozen
/// and allocated until that job has completed. The main thread retains its owner.
#[derive(Clone, Copy, Debug)]
pub struct PreparedImage {
    pub kind: GameKind,
    pub secure: SecureValues,
    pub(crate) blocks: [FrozenBlock; MAX_PREPARED_BLOCKS],
    pub(crate) count: usize,
}

impl PreparedImage {
    /// Checks the frozen geometry without dereferencing block addresses. Returns
    /// the metadata offset; address validity/lifetime remains submit's contract.
    pub fn validate_geometry(&self, file_len: u64) -> Result<u64, NativeGameError> {
        if file_len == 0 || file_len > i32::MAX as u64 {
            return Err(NativeGameError::InvalidFileLength);
        }
        if self.count != self.kind.data_block_count() as usize + 1
            || self.count > MAX_PREPARED_BLOCKS
        {
            return Err(NativeGameError::BlockOutsideFile);
        }
        let mut expected = 0u64;
        let mut metadata_offset = 0;
        for (index, block) in self.blocks[..self.count].iter().enumerate() {
            if block.offset != expected
                || block.address.checked_add(block.length).is_none()
                || (block.length != 0 && block.address == 0)
                || block
                    .offset
                    .checked_add(block.length as u64)
                    .is_none_or(|end| end > file_len)
            {
                return Err(NativeGameError::BlockOutsideFile);
            }
            if index + 1 == self.count {
                if block.length != 0x1e8 {
                    return Err(NativeGameError::BlockOutsideFile);
                }
                metadata_offset = block.offset;
            }
            expected = expected
                .checked_add((block.length as u64 + 511) & !511)
                .ok_or(NativeGameError::BlockOutsideFile)?;
        }
        Ok(metadata_offset)
    }
}

#[cfg(test)]
mod frozen_tests {
    use super::*;
    fn fixture() -> PreparedImage {
        let mut blocks = [FrozenBlock {
            offset: 0,
            address: 0,
            length: 0,
        }; MAX_PREPARED_BLOCKS];
        for (index, block) in blocks[..56].iter_mut().enumerate() {
            *block = FrozenBlock {
                offset: index as u64 * 512,
                address: 0x1000,
                length: if index == 55 { 0x1e8 } else { 17 },
            };
        }
        PreparedImage {
            kind: GameKind::Y,
            secure: SecureValues {
                current: 2,
                previous: 1,
            },
            blocks,
            count: 56,
        }
    }
    #[test]
    fn frozen_geometry_rejects_truncation_overlap_and_wrong_metadata() {
        let valid = fixture();
        let end = 55 * 512 + 0x1e8;
        assert_eq!(valid.validate_geometry(end), Ok(55 * 512));
        assert!(valid.validate_geometry(end - 1).is_err());
        let mut overlap = valid;
        overlap.blocks[1].offset = 16;
        assert!(overlap.validate_geometry(end).is_err());
        let mut bad_metadata = valid;
        bad_metadata.blocks[55].length = 16;
        assert!(bad_metadata.validate_geometry(end).is_err());
        let mut short = valid;
        short.count = 55;
        assert!(short.validate_geometry(end).is_err());
    }
    #[test]
    fn frozen_geometry_rejects_bad_addresses_without_reading_them() {
        let mut image = fixture();
        image.blocks[0].address = usize::MAX - 3;
        assert!(image.validate_geometry(0x10000).is_err());
        image.blocks[0].address = 0;
        assert!(image.validate_geometry(0x10000).is_err());
    }
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::{
        FrozenBlock, GameIoDescriptor, GameKind, GameOperation, NativeGameError, NativePoll,
        PlatformSecureValue, PreparedImage, SecureValues, MAX_PREPARED_BLOCKS,
    };
    use bank_common::fs::GameMainReader;
    use core::{marker::PhantomData, mem::transmute, ptr::NonNull};
    use offline_core::game_image::Overlay;
    use offline_core::Storage;

    pub const OBJECT_SIZE: usize = 0xb49d0;
    const GAME_VTABLE: u32 = 0x0036_2f88;
    const TEXT_START: u32 = 0x0010_0000;
    const TEXT_END: u32 = 0x0031_3910;

    /// Immutably borrows an already prepared native object and its reachable
    /// block storage. Safe code cannot start a native writer through this view.
    pub struct NativePreparedGame<'a> {
        pointer: NonNull<u8>,
        metadata: NonNull<u8>,
        kind: GameKind,
        exclusive: PhantomData<&'a [u8; OBJECT_SIZE]>,
        thread: PhantomData<*mut ()>,
    }

    fn pointer(value: u32) -> Result<NonNull<u8>, NativeGameError> {
        if value & 3 != 0 {
            return Err(NativeGameError::UnalignedPointer);
        }
        NonNull::new(value as *mut u8).ok_or(NativeGameError::NullPointer)
    }
    unsafe fn word(pointer: NonNull<u8>, offset: usize) -> u32 {
        unsafe { pointer.as_ptr().add(offset).cast::<u32>().read() }
    }
    unsafe fn pair(pointer: NonNull<u8>, offset: usize) -> u64 {
        u64::from(unsafe { word(pointer, offset) })
            | (u64::from(unsafe { word(pointer, offset + 4) }) << 32)
    }
    fn getter(address: u32) -> Result<unsafe extern "aapcs" fn(*const u8) -> u32, NativeGameError> {
        if address & 3 != 0 || !(TEXT_START..TEXT_END).contains(&address) {
            return Err(NativeGameError::InvalidGetter(address));
        }
        Ok(unsafe {
            transmute::<usize, unsafe extern "aapcs" fn(*const u8) -> u32>(address as usize)
        })
    }

    impl<'a> NativePreparedGame<'a> {
        /// # Safety
        /// The matching Bank executable must be verified. `raw` must reference
        /// OBJECT_SIZE initialized bytes produced by native constructor00232360.
        /// Every reachable metadata/block object, vtable, getter, and returned
        /// byte range must remain valid throughout `'a`. They must be the native
        /// objects/getters for this game, not arbitrary readable addresses.
        /// Native RNG002bc7c4 and preparation002bc4bc must have completed and the
        /// prepared image must have passed native validation. No native or Rust mutation, mutable alias, callback, or worker may overlap this borrow. Shared read-only views are allowed.
        pub unsafe fn from_raw(raw: *mut u8) -> Result<Self, NativeGameError> {
            let base = raw as u32;
            let pointer = pointer(base)?;
            base.checked_add(OBJECT_SIZE as u32)
                .ok_or(NativeGameError::AddressOverflow)?;
            let vtable = unsafe { word(pointer, 0) };
            if vtable != GAME_VTABLE {
                return Err(NativeGameError::WrongVtable(vtable));
            }
            let kind = GameKind::try_from(unsafe { pointer.as_ptr().add(8).read() })?;
            let metadata = self::pointer(unsafe { word(pointer, 4) })?;
            (metadata.as_ptr() as u32)
                .checked_add(0x200)
                .ok_or(NativeGameError::AddressOverflow)?;
            let actual = unsafe { word(metadata, 0x1f8) };
            let expected = kind.data_block_count();
            if actual != expected {
                return Err(NativeGameError::WrongBlockCount { expected, actual });
            }
            Ok(Self {
                pointer,
                metadata,
                kind,
                exclusive: PhantomData,
                thread: PhantomData,
            })
        }
        pub fn kind(&self) -> GameKind {
            self.kind
        }
        pub fn secure_values(&self) -> SecureValues {
            // The constructor's graph lifetime covers the separately allocated
            // 0x200-byte native metadata object, including these two pairs.
            SecureValues {
                current: unsafe { pair(self.metadata, 8) },
                previous: unsafe { pair(self.metadata, 0x10) },
            }
        }
        /// Captures raw ranges without moving the native object across threads.
        /// The caller must keep the game frozen until any submitted worker job ends.
        pub fn freeze(&self) -> Result<PreparedImage, NativeGameError> {
            let mut image = PreparedImage {
                kind: self.kind,
                secure: self.secure_values(),
                blocks: [FrozenBlock::EMPTY; MAX_PREPARED_BLOCKS],
                count: 0,
            };
            for block in self.blocks(i32::MAX as u64)? {
                let block = block?;
                if image.count == MAX_PREPARED_BLOCKS {
                    return Err(NativeGameError::BlockOutsideFile);
                }
                image.blocks[image.count] = FrozenBlock {
                    offset: block.offset,
                    address: block.bytes.as_ptr() as usize,
                    length: block.bytes.len(),
                };
                image.count += 1;
            }
            Ok(image)
        }
        /// Iterates native data blocks followed by the native metadata block.
        /// `file_len` must come from the verified existing main file. Padding and
        /// tail bytes are supplied by game_image::fingerprint_images, not here.
        pub fn blocks(&self, file_len: u64) -> Result<PreparedBlocks<'_>, NativeGameError> {
            if file_len == 0 || file_len > i32::MAX as u64 {
                return Err(NativeGameError::InvalidFileLength);
            }
            Ok(PreparedBlocks {
                game: self.pointer,
                index: 0,
                count: self.kind.data_block_count() + 1,
                offset: 0,
                file_len,
                failed: false,
                borrow: PhantomData,
            })
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        Ready,
        Prepared,
        Running,
        Finished,
        Poisoned,
    }

    /// Owns the caller's exclusive native game-save lifecycle. Raw main-file
    /// handles and prepared views borrow this owner, so they cannot coexist with
    /// prepare/start/poll calls through safe Rust.
    pub struct NativeGameSession {
        manager: NonNull<u8>,
        writer: NonNull<u8>,
        game: NonNull<u8>,
        heap: u32,
        kind: GameKind,
        phase: Phase,
        thread: PhantomData<*mut ()>,
    }

    unsafe fn set_word(pointer: NonNull<u8>, offset: usize, value: u32) {
        unsafe {
            pointer.as_ptr().add(offset).cast::<u32>().write(value);
        }
    }
    unsafe fn block_length(game: NonNull<u8>, index: u32) -> Result<u32, NativeGameError> {
        type GetBlock = unsafe extern "aapcs" fn(*const u8, u32) -> u32;
        let get: GetBlock = unsafe { transmute(0x002b_c460usize) };
        let block = pointer(unsafe { get(game.as_ptr(), index) })?;
        let table = pointer(unsafe { word(block, 0) })?;
        let length = getter(unsafe { word(table, 0xc) })?;
        Ok(unsafe { length(block.as_ptr()) })
    }

    impl NativeGameSession {
        /// # Safety
        /// The exact executable and native state must be verified. `manager`
        /// must be the initialized 0x100-byte save manager from00106510 with a
        /// selected game whose native block validation has passed. Platform
        /// secure-value validation may still require recovery. Its reachable
        /// writer, stream, metadata, blocks, archive registration, and heap must
        /// stay live until this owner is dropped after a terminal result. The
        /// caller grants exclusive control of game-save mutation and
        /// prevents native task destruction, cancellation, unmount, other game
        /// writes, and conflicting Rust borrows throughout this session. Native
        /// worker access is permitted only while this session tracks Running.
        pub unsafe fn from_raw(raw: *mut u8, heap: u32) -> Result<Self, NativeGameError> {
            let manager = pointer(raw as u32)?;
            (raw as u32)
                .checked_add(0x100)
                .ok_or(NativeGameError::AddressOverflow)?;
            let selected = unsafe { manager.as_ptr().add(0xc8).read() };
            let kind = GameKind::try_from(selected)?;
            let game = pointer(unsafe { word(manager, usize::from(selected) * 12 + 4) })?;
            let writer = pointer(unsafe { word(manager, usize::from(selected) * 12 + 8) })?;
            (game.as_ptr() as u32)
                .checked_add(OBJECT_SIZE as u32)
                .ok_or(NativeGameError::AddressOverflow)?;
            let vtable = unsafe { word(game, 0) };
            if vtable != GAME_VTABLE {
                return Err(NativeGameError::WrongVtable(vtable));
            }
            if unsafe { game.as_ptr().add(8).read() } != selected {
                return Err(NativeGameError::UnknownGame(unsafe {
                    game.as_ptr().add(8).read()
                }));
            }
            let metadata = pointer(unsafe { word(game, 4) })?;
            let metadata_vtable = unsafe { word(metadata, 0) };
            if metadata_vtable != 0x0036_2d68 {
                return Err(NativeGameError::WrongMetadataVtable(metadata_vtable));
            }
            let actual = unsafe { word(metadata, 0x1f8) };
            if actual != kind.data_block_count() {
                return Err(NativeGameError::WrongBlockCount {
                    expected: kind.data_block_count(),
                    actual,
                });
            }
            let this = Self {
                manager,
                writer,
                game,
                heap,
                kind,
                phase: Phase::Ready,
                thread: PhantomData,
            };
            this.check_idle()?;
            this.mounted_archive()?;
            Ok(this)
        }
        pub fn kind(&self) -> GameKind {
            self.kind
        }
        fn check_idle(&self) -> Result<(), NativeGameError> {
            // Constructor00233840 does not initialize writer+1c, and load mode3
            // never uses it. Only initialized job/worker fields and native
            // activity flags can establish idle state before the first save.
            if unsafe { word(self.writer, 0x14) } != 0
                || unsafe { self.writer.as_ptr().add(0x11).read() } != 0
                || unsafe { self.manager.as_ptr().add(0xfc).read() } != 0
                || unsafe { (0x0037_298a as *const u8).read_volatile() } & 4 != 0
            {
                return Err(NativeGameError::Busy);
            }
            Ok(())
        }
        /// Returns the initialized FS session and borrowed selected archive.
        /// The lifetime remains owned by this session; callers must not close it.
        pub fn mounted_archive(&self) -> Result<(u32, u64), NativeGameError> {
            if self.phase == Phase::Running {
                return Err(NativeGameError::Busy);
            }
            let stream = pointer(unsafe { word(self.writer, 0) })?;
            let mount = pointer(unsafe { word(stream, 8) })?;
            type Resolve = unsafe extern "aapcs" fn(*const u8) -> u32;
            let resolve: Resolve = unsafe { transmute(0x0022_9488usize) };
            let archive_object = NonNull::new(unsafe { resolve(mount.as_ptr()) } as *mut u8)
                .ok_or(NativeGameError::MissingArchive)?;
            if archive_object.as_ptr() as usize & 3 != 0 {
                return Err(NativeGameError::UnalignedPointer);
            }
            let archive = unsafe { pair(archive_object, 8) };
            let session = unsafe { (0x0039_0100 as *const u32).read_volatile() };
            if archive == 0 || session == 0 {
                return Err(NativeGameError::MissingArchive);
            }
            Ok((session, archive))
        }
        /// Captures only archive identity and serialized metadata geometry.
        /// Keep this session and its native graph alive until the worker returns.
        pub fn io_descriptor(&self) -> Result<GameIoDescriptor, NativeGameError> {
            let (session, archive) = self.mounted_archive()?;
            let mut metadata_offset = 0u64;
            for index in 0..self.kind.data_block_count() {
                let length = unsafe { block_length(self.game, index) }?;
                metadata_offset = metadata_offset
                    .checked_add((u64::from(length) + 511) & !511)
                    .ok_or(NativeGameError::BlockOutsideFile)?;
            }
            if unsafe { block_length(self.game, self.kind.data_block_count()) }? != 0x1e8
                || metadata_offset + 0x1e8 > i32::MAX as u64
            {
                return Err(NativeGameError::BlockOutsideFile);
            }
            Ok(GameIoDescriptor {
                kind: self.kind,
                session,
                archive,
                metadata_offset,
            })
        }
        pub fn open_main(&self) -> Result<GameMainReader<'_>, NativeGameError> {
            let (session, archive) = self.mounted_archive()?;
            unsafe { GameMainReader::open(session, archive) }.map_err(NativeGameError::Storage)
        }
        /// Reads current/previous values from the actual main file. The final
        /// native metadata block serializes object+8, so these are its first16
        /// bytes. This never substitutes values from the edited live object.
        pub fn file_secure_values(
            &self,
            source: &mut GameMainReader<'_>,
        ) -> Result<SecureValues, NativeGameError> {
            if self.phase == Phase::Running {
                return Err(NativeGameError::Busy);
            }
            let mut offset = 0u64;
            for index in 0..self.kind.data_block_count() {
                let length = unsafe { block_length(self.game, index) }?;
                offset = offset
                    .checked_add((u64::from(length) + 511) & !511)
                    .ok_or(NativeGameError::BlockOutsideFile)?;
            }
            if unsafe { block_length(self.game, self.kind.data_block_count()) }? != 0x1e8
                || offset
                    .checked_add(0x1e8)
                    .is_none_or(|end| end > source.len())
            {
                return Err(NativeGameError::BlockOutsideFile);
            }
            let mut bytes = [0; 16];
            source
                .read(offset, &mut bytes)
                .map_err(NativeGameError::Storage)?;
            let mut first = [0; 8];
            let mut second = [0; 8];
            first.copy_from_slice(&bytes[..8]);
            second.copy_from_slice(&bytes[8..]);
            Ok(SecureValues {
                current: u64::from_le_bytes(first),
                previous: u64::from_le_bytes(second),
            })
        }
        pub fn read_platform_secure_value(&self) -> Result<PlatformSecureValue, NativeGameError> {
            let (session, archive) = self.mounted_archive()?;
            type Get =
                unsafe extern "aapcs" fn(*const u32, *mut u8, *mut u8, *mut u64, u64, u32) -> i32;
            let get: Get = unsafe { transmute(0x0016_578cusize) };
            let mut first = 0;
            let mut second = 0;
            let mut value = 0;
            let code = unsafe {
                get(
                    &session,
                    &mut first,
                    &mut second,
                    &mut value,
                    archive,
                    0x1000,
                )
            };
            if code < 0 {
                return Err(NativeGameError::NativeResult {
                    operation: GameOperation::ReadSecureValue,
                    code: code as u32,
                });
            }
            if first > 1 || second > 1 {
                return Err(NativeGameError::InvalidSecureFlags { first, second });
            }
            Ok(PlatformSecureValue {
                value_present: first != 0,
                gamecard: second != 0,
                value,
            })
        }
        /// Advances native save metadata and regenerates all checksums/signatures.
        /// Does not bind a writer, set activity flags, spawn a worker, or write FS.
        pub fn prepare(&mut self) -> Result<(), NativeGameError> {
            if self.phase != Phase::Ready {
                return Err(NativeGameError::InvalidPhase);
            }
            self.check_idle()?;
            type Rotate = unsafe extern "aapcs" fn(*mut u8) -> u64;
            type Prepare = unsafe extern "aapcs" fn(*mut u8) -> u32;
            type Validate = unsafe extern "aapcs" fn(*mut u8, *mut u32) -> u32;
            let rotate: Rotate = unsafe { transmute(0x002b_c7c4usize) };
            let prepare: Prepare = unsafe { transmute(0x002b_c4bcusize) };
            let validate: Validate = unsafe { transmute(0x002c_bea0usize) };
            self.phase = Phase::Poisoned;
            unsafe {
                rotate(self.game.as_ptr());
                prepare(self.game.as_ptr());
            }
            let mut detail = 0;
            let status = unsafe { validate(self.game.as_ptr(), &mut detail) };
            if status != 1 {
                return Err(NativeGameError::Validation { status, detail });
            }
            self.phase = Phase::Prepared;
            Ok(())
        }
        pub fn prepared(&self) -> Result<NativePreparedGame<'_>, NativeGameError> {
            if self.phase != Phase::Prepared {
                return Err(NativeGameError::InvalidPhase);
            }
            unsafe { NativePreparedGame::from_raw(self.game.as_ptr()) }
        }
        /// Starts exactly the native mode4 worker; preparation is not repeated.
        ///
        /// # Safety
        /// The caller must have durably prepared both journal replicas for this
        /// exact prepared image, reverified the unchanged original game file,
        /// and protected the native owning task against cancellation/destruction
        /// (0025c420 busy lifecycle or an equivalent verified hook). Keep this
        /// session and its native owners alive until poll reaches a terminal
        /// result. A launch failure/crash leaves the journal pending for recovery.
        pub unsafe fn start_after_journal(&mut self) -> Result<(), NativeGameError> {
            if self.phase != Phase::Prepared {
                return Err(NativeGameError::InvalidPhase);
            }
            self.check_idle()?;
            type Mark = unsafe extern "aapcs" fn(u8);
            type Start = unsafe extern "aapcs" fn(*mut u8, u32, u8, u32, *mut u8);
            let mark: Mark = unsafe { transmute(0x001d_4d90usize) };
            let start: Start = unsafe { transmute(0x001d_37ecusize) };
            unsafe {
                set_word(self.manager, 0xe8, self.heap);
                mark(4);
                set_word(self.writer, 0x1c, self.game.as_ptr() as u32);
            }
            self.phase = Phase::Running;
            unsafe {
                start(self.writer.as_ptr(), 15, 4, self.heap, self.game.as_ptr());
                self.manager.as_ptr().add(0xfc).write(1);
            }
            Ok(())
        }
        /// Polls native worker join, write and archive-commit results, cleanup,
        /// and the native platform secure-value update. Complete still requires
        /// independent file readback and journal reconciliation by the caller.
        pub fn poll(&mut self) -> Result<NativePoll, NativeGameError> {
            if self.phase != Phase::Running {
                return Err(NativeGameError::InvalidPhase);
            }
            type Poll = unsafe extern "aapcs" fn(*mut u8) -> u32;
            let poll: Poll = unsafe { transmute(0x0015_dc74usize) };
            let status = unsafe { poll(self.manager.as_ptr()) };
            if status == 1 {
                return Ok(NativePoll::Pending);
            }
            self.phase = Phase::Finished;
            if status == 0 {
                Ok(NativePoll::Complete)
            } else {
                Err(NativeGameError::NativeStatus(status))
            }
        }
        /// Completes only a pending forward secure-value transition.
        ///
        /// # Safety
        /// The selected title and the entire actual main file must have matched
        /// the trusted pending journal's AFTER fingerprint immediately before
        /// this call. `verified` must be read from that same authenticated file
        /// via file_secure_values, not from edited live object bytes. No archive,
        /// media, file, or platform secure value may change concurrently.
        pub unsafe fn finish_secure_value(
            &mut self,
            verified: SecureValues,
        ) -> Result<(), NativeGameError> {
            if self.phase == Phase::Running {
                return Err(NativeGameError::Busy);
            }
            let observed = self.read_platform_secure_value()?;
            if observed.matches_native_rule(verified.current) {
                return Ok(());
            }
            if observed.value != verified.previous || verified.current == verified.previous {
                return Err(NativeGameError::SecureValueMismatch);
            }
            let (session, archive) = self.mounted_archive()?;
            type Set = unsafe extern "aapcs" fn(*const u32, u64, u32, u64, u8) -> i32;
            let set: Set = unsafe { transmute(0x0012_146cusize) };
            let code = unsafe { set(&session, archive, 0x1000, verified.current, 0) };
            if code < 0 {
                return Err(NativeGameError::NativeResult {
                    operation: GameOperation::WriteSecureValue,
                    code: code as u32,
                });
            }
            let after = self.read_platform_secure_value()?;
            if after.value_present != observed.value_present
                || after.gamecard != observed.gamecard
                || !after.matches_native_rule(verified.current)
            {
                return Err(NativeGameError::SecureValueMismatch);
            }
            Ok(())
        }
    }
    impl GameIoDescriptor {
        /// Captures a validated game without changing the manager selection.
        ///
        /// # Safety
        /// Native Task3 must have completed on the reviewed executable. The
        /// manager and all loaded games/mounts stay alive and idle through the
        /// worker job. The caller prevents selection changes, unmounts, saves,
        /// teardown and conflicting mutation until completion is collected.
        pub unsafe fn from_loaded_kind(
            raw: *mut u8,
            kind: GameKind,
        ) -> Result<Self, NativeGameError> {
            let manager = pointer(raw as u32)?;
            (raw as u32)
                .checked_add(0x100)
                .ok_or(NativeGameError::AddressOverflow)?;
            let offset = kind as usize * 12;
            // The scan found no usable save of this game.
            if unsafe { raw.add(offset + 1).read() } == 0 {
                return Err(NativeGameError::NotLoaded);
            }
            if unsafe { raw.add(offset).read() } != kind as u8
                || unsafe { raw.add(offset + 1).read() } != 1
            {
                return Err(NativeGameError::Busy);
            }
            let game = pointer(unsafe { word(manager, offset + 4) })?;
            let writer = pointer(unsafe { word(manager, offset + 8) })?;
            (game.as_ptr() as u32)
                .checked_add(OBJECT_SIZE as u32)
                .ok_or(NativeGameError::AddressOverflow)?;
            let vtable = unsafe { word(game, 0) };
            if vtable != GAME_VTABLE {
                return Err(NativeGameError::WrongVtable(vtable));
            }
            if unsafe { game.as_ptr().add(8).read() } != kind as u8 {
                return Err(NativeGameError::UnknownGame(unsafe {
                    game.as_ptr().add(8).read()
                }));
            }
            let metadata = pointer(unsafe { word(game, 4) })?;
            let table = unsafe { word(metadata, 0) };
            if table != 0x00362d68 {
                return Err(NativeGameError::WrongMetadataVtable(table));
            }
            let actual = unsafe { word(metadata, 0x1f8) };
            if actual != kind.data_block_count() {
                return Err(NativeGameError::WrongBlockCount {
                    expected: kind.data_block_count(),
                    actual,
                });
            }
            let session = NativeGameSession {
                manager,
                writer,
                game,
                heap: 0,
                kind,
                phase: Phase::Ready,
                thread: PhantomData,
            };
            session.check_idle()?;
            session.io_descriptor()
        }
    }
    pub struct PreparedBlocks<'a> {
        game: NonNull<u8>,
        index: u32,
        count: u32,
        offset: u64,
        file_len: u64,
        failed: bool,
        borrow: PhantomData<&'a NativePreparedGame<'a>>,
    }
    impl<'a> PreparedBlocks<'a> {
        fn next_block(&mut self) -> Result<Overlay<'a>, NativeGameError> {
            type GetBlock = unsafe extern "aapcs" fn(*const u8, u32) -> u32;
            let get_block: GetBlock = unsafe { transmute(0x002b_c460usize) };
            let block = pointer(unsafe { get_block(self.game.as_ptr(), self.index) })?;
            let vtable = pointer(unsafe { word(block, 0) })?;
            let get_length = getter(unsafe { word(vtable, 0xc) })?;
            let get_data = getter(unsafe { word(vtable, 8) })?;
            let length = unsafe { get_length(block.as_ptr()) };
            let data = unsafe { get_data(block.as_ptr()) };
            let end = self
                .offset
                .checked_add(u64::from(length))
                .ok_or(NativeGameError::BlockOutsideFile)?;
            if end > self.file_len {
                return Err(NativeGameError::BlockOutsideFile);
            }
            let data = if length == 0 {
                NonNull::<u8>::dangling().as_ptr()
            } else {
                if data == 0 {
                    return Err(NativeGameError::NullPointer);
                }
                data.checked_add(length)
                    .ok_or(NativeGameError::AddressOverflow)?;
                data as *mut u8
            };
            // from_raw requires live immutable block bytes for the whole borrow.
            let bytes = unsafe { core::slice::from_raw_parts(data.cast_const(), length as usize) };
            let overlay = Overlay {
                offset: self.offset,
                bytes,
            };
            self.offset = self
                .offset
                .checked_add((u64::from(length) + 511) & !511)
                .ok_or(NativeGameError::BlockOutsideFile)?;
            self.index += 1;
            Ok(overlay)
        }
    }
    impl<'a> Iterator for PreparedBlocks<'a> {
        type Item = Result<Overlay<'a>, NativeGameError>;
        fn next(&mut self) -> Option<Self::Item> {
            if self.failed || self.index == self.count {
                return None;
            }
            let result = self.next_block();
            if result.is_err() {
                self.failed = true;
            }
            Some(result)
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            let remaining = if self.failed {
                0
            } else {
                (self.count - self.index) as usize
            };
            (0, Some(remaining))
        }
    }
}

#[cfg(target_arch = "arm")]
pub use arm::{NativeGameSession, NativePreparedGame, PreparedBlocks, OBJECT_SIZE};
