//! Storage operations executed only by the dedicated native SDK worker.
//! No job calls UI functions or accesses a live NativeBank object.

use crate::{
    bank_files::Loaded,
    native_game::{GameIoDescriptor, PreparedImage, SecureValues},
};
use offline_core::{rewards::Stored, Fingerprint, Phase, RecoveryDecision};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameEvidence {
    pub title: u64,
    pub fingerprint: Fingerprint,
    pub secure: SecureValues,
    /// Whether this copy of the game is a cartridge; otherwise it is the
    /// installed copy. Bank holds one copy per title, the cartridge first.
    pub cartridge: bool,
}

// Fixed descriptors live directly in the one static mailbox; no allocator or
// independently freed request pointer can outlive the owning native task.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug)]
pub enum Job {
    Open {
        session: u32,
    },
    Initialize,
    /// `game` is `None` when the game of the save in progress is not there.
    Recover {
        game: Option<GameIoDescriptor>,
    },
    InspectAndLoad {
        game: GameIoDescriptor,
    },
    Inspect {
        game: GameIoDescriptor,
    },
    Prepare {
        game: GameIoDescriptor,
        prepared: PreparedImage,
        baseline: GameEvidence,
        /// The Miles state that must commit with this save.
        rewards: Stored,
    },
    Finalize {
        game: GameIoDescriptor,
    },
    Close,
}
impl Job {
    /// Running mutation jobs finish their durable protocol even if cancellation
    /// was requested. Read-only jobs may discard their result at completion.
    pub const fn cancellable(&self) -> bool {
        matches!(
            self,
            Self::Open { .. } | Self::Inspect { .. } | Self::InspectAndLoad { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Missing,
    /// Clean state includes the assembled native body in staging and what was
    /// found beside it. Prepared state requires Recover before any staged
    /// body may be displayed.
    Opened {
        phase: Phase,
        loaded: Option<Loaded>,
    },
    /// `loaded` is `None` only for a newly created Bank, whose live object
    /// already is the saved body.
    BankReady {
        evidence: Option<GameEvidence>,
        decision: Option<RecoveryDecision>,
        loaded: Option<Loaded>,
    },
    Inspected(GameEvidence),
    ReadyToWrite,
    Closed,
}

/// Test builds: the fault of a Save and Quit stopped on purpose. Its second
/// number is the stop point: 1 after the journal is prepared and before the
/// game is written, 2 after the game is written and before the journal is
/// resolved. The files are left as a power cut at that point leaves them.
#[cfg(feature = "test-build")]
pub const TEST_STOP: u32 = 0x7e57;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerErrorKind {
    Operation,
    Busy,
    StaleJob,
    Cancelled,
    Resource,
    Stopped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerError {
    pub kind: WorkerErrorKind,
    fault: u32,
    native: u32,
}
impl WorkerError {
    pub const fn fault_code(self) -> u32 {
        self.fault
    }
    pub const fn native_result(self) -> u32 {
        self.native
    }
    #[cfg(target_arch = "arm")]
    pub(crate) const fn operation(fault: u32, native: u32) -> Self {
        Self {
            kind: WorkerErrorKind::Operation,
            fault,
            native,
        }
    }
    #[cfg(any(test, target_arch = "arm"))]
    pub(crate) const fn control(kind: WorkerErrorKind) -> Self {
        Self {
            kind,
            fault: 2,
            native: 0,
        }
    }
    #[cfg(target_arch = "arm")]
    pub(crate) const fn resource(native: u32) -> Self {
        Self {
            kind: WorkerErrorKind::Resource,
            fault: 2,
            native,
        }
    }
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::{
        bank_files::{self, BankFiles, FileName, Files},
        fs::{self, ExtdataStorage, GameMainReader},
        native_game::{PlatformSecureValue, MAX_PREPARED_BLOCKS},
        session,
        transaction::{self, Found, Match, Medium},
    };
    use core::{cell::UnsafeCell, convert::Infallible, mem::transmute};
    use offline_core::{
        game_image::{fingerprint_images, Overlay},
        moved,
        native_blob::NativeBlobView,
        sections::{Identity, HELD},
        GameObservation, PendingTransfer, Storage, StoreError,
    };

    /// Working room for comparing the two snapshots of a save in progress:
    /// everything the Bank held before it and after it.
    struct Held(UnsafeCell<[[Identity; HELD]; 2]>);
    // SAFETY: only the one storage worker thread uses it, inside one job.
    unsafe impl Sync for Held {}
    static HELD_LISTS: Held = Held(UnsafeCell::new([[[0; 3]; HELD]; 2]));

    /// The files in Bank's extdata, opened through the borrowed session.
    struct ExtFiles {
        session: u32,
    }
    impl Files for ExtFiles {
        type Storage = ExtdataStorage;
        fn open(&mut self, file: FileName) -> Result<Option<ExtdataStorage>, fs::Error> {
            unsafe { ExtdataStorage::open_existing(self.session, file.units()) }
        }
        fn create(&mut self, file: FileName) -> Result<ExtdataStorage, fs::Error> {
            unsafe { ExtdataStorage::create_new(self.session, file.units()) }
        }
    }

    pub(crate) struct StorageWorker {
        files: BankFiles<ExtFiles>,
        fs_session: u32,
        missing: bool,
        poisoned: bool,
        /// The secure pair of the game image the last `Prepare` allowed to
        /// be written, until `Finalize` has looked for it.
        written: Option<SecureValues>,
    }
    impl StorageWorker {
        pub const fn new() -> Self {
            Self {
                files: BankFiles::new(ExtFiles { session: 0 }),
                fs_session: 0,
                missing: false,
                poisoned: false,
                written: None,
            }
        }
        pub fn run(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            if matches!(job, Job::Close) {
                if let Some(mut storage) = self.files.close() {
                    storage.close().map_err(|e| fs_error(e, 3))?;
                }
                self.fs_session = 0;
                self.missing = false;
                self.written = None;
                return Ok(Reply::Closed);
            }
            if self.poisoned {
                return Err(WorkerError::operation(5, 0));
            }
            let result = self.execute(job, staging);
            if result.is_err() {
                self.poisoned = true;
            }
            result
        }
        fn phase(&mut self) -> Result<Phase, WorkerError> {
            self.files.phase().map_err(|e| files_error(e, 5))
        }
        /// Leaves the assembled native body in staging.
        fn read_bank(&mut self, staging: &mut [u8]) -> Result<bank_files::Loaded, WorkerError> {
            self.files.read(staging).map_err(|e| files_error(e, 9))
        }
        /// Resolves the save in progress from a game save that is one of
        /// its two images.
        fn reconcile(
            &mut self,
            pending: &PendingTransfer,
            evidence: &GameEvidence,
        ) -> Result<RecoveryDecision, WorkerError> {
            self.files
                .reconcile(GameObservation::Present {
                    game: pending.game,
                    fingerprint: evidence.fingerprint,
                })
                .map_err(|e| files_error(e, 16))
        }
        /// Resolves the save in progress from what it moved, toward the
        /// snapshot that cannot lose a Pokémon. `seen` says that a game of
        /// its title is there, with a save that is neither image. Without
        /// one only a save that moved Pokémon one way, or none, is
        /// resolved; one that moved them both ways waits for its game, and
        /// nothing is written.
        fn settle_by_moves(
            &mut self,
            staging: &mut [u8],
            seen: bool,
        ) -> Result<RecoveryDecision, WorkerError> {
            // SAFETY: see `Held`; no other reference exists.
            let [before, after] = unsafe { &mut *HELD_LISTS.0.get() };
            let moved = self
                .files
                .pending_moves(staging, before, after)
                .map_err(|e| files_error(e, 16))?;
            let decision = if seen {
                moved::choose(moved)
            } else {
                moved::choose_unseen(moved).ok_or(WorkerError::operation(7, 0))?
            };
            self.files
                .reconcile_as(decision)
                .map_err(|e| files_error(e, 16))
        }
        /// The end of a recovery or a Save and Quit, once the save in
        /// progress is resolved.
        fn settled(
            &mut self,
            staging: &mut [u8],
            evidence: Option<GameEvidence>,
            decision: RecoveryDecision,
        ) -> Result<Reply, WorkerError> {
            if !matches!(
                decision,
                RecoveryDecision::KeepBefore | RecoveryDecision::CommitAfter
            ) {
                return Err(WorkerError::operation(16, 0));
            }
            // Remove transport slots the published save made obsolete. A
            // failure here is retried by the next save and must not undo
            // this one.
            let _ = self.files.tidy_transport();
            let loaded = self.read_bank(staging)?;
            Ok(Reply::BankReady {
                evidence,
                decision: Some(decision),
                loaded: Some(loaded),
            })
        }
        // A test build's stop point leaves the rest of its job unreached.
        #[cfg_attr(feature = "test-build", allow(unreachable_code, unused_variables))]
        fn execute(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            match job {
                Job::Open { session } => {
                    if !self.files.is_open() {
                        if self.missing {
                            return Err(WorkerError::operation(5, 0));
                        }
                        if session == 0 {
                            // Bank's own FS session is not initialized yet.
                            return Err(WorkerError::operation(3, 0x7800_0000));
                        }
                        self.fs_session = session;
                        self.files.files_mut().session = session;
                    } else if self.fs_session != session {
                        // Bank's FS session changed while our file was open.
                        return Err(WorkerError::operation(3, 0x7700_0000));
                    }
                    let opened = self.files.open().map_err(|error| match error {
                        bank_files::Error::Io(error) => fs_error(error, 3),
                        error => files_error(error, 5),
                    })?;
                    let Some(phase) = opened else {
                        self.missing = true;
                        return Ok(Reply::Missing);
                    };
                    let loaded = match phase {
                        Phase::Clean(_) => Some(self.read_bank(staging)?),
                        Phase::Prepared(_) => None,
                    };
                    Ok(Reply::Opened { phase, loaded })
                }
                Job::Initialize => {
                    if !self.missing || self.files.is_open() || self.fs_session == 0 {
                        return Err(WorkerError::operation(4, 0));
                    }
                    // Refuse an invalid native body before creating any file.
                    NativeBlobView::parse_layout(staging)
                        .map_err(|_| WorkerError::operation(2, 0))?;
                    self.files
                        .initialize(staging)
                        .map_err(|e| files_error(e, 4))?;
                    self.missing = false;
                    Ok(Reply::BankReady {
                        evidence: None,
                        decision: None,
                        loaded: None,
                    })
                }
                Job::Inspect { game } => {
                    let evidence = inspect(game)?;
                    require_secure(game, evidence.secure.current)?;
                    Ok(Reply::Inspected(evidence))
                }
                Job::InspectAndLoad { game } => {
                    if !matches!(self.phase()?, Phase::Clean(_)) {
                        return Err(WorkerError::operation(7, 0));
                    }
                    let evidence = inspect(game)?;
                    // A save in progress that was settled without its game
                    // may have left the platform value one step behind.
                    finish_secure(game, evidence.secure)?;
                    let loaded = self.read_bank(staging)?;
                    Ok(Reply::BankReady {
                        evidence: Some(evidence),
                        decision: None,
                        loaded: Some(loaded),
                    })
                }
                Job::Recover { game } => {
                    let Phase::Prepared(pending) = self.phase()? else {
                        return Err(WorkerError::operation(16, 0));
                    };
                    let Some(game) = game else {
                        let decision = self.settle_by_moves(staging, false)?;
                        return self.settled(staging, None, decision);
                    };
                    let evidence = inspect(game)?;
                    let matched = matched(&pending, &evidence)?;
                    let decision = match matched {
                        Match::Before => {
                            require_secure(game, evidence.secure.current)?;
                            self.reconcile(&pending, &evidence)?
                        }
                        Match::After => {
                            finish_secure(game, evidence.secure)?;
                            self.reconcile(&pending, &evidence)?
                        }
                        // A save that is neither image, on the kind of
                        // copy the save in progress was made with: the game
                        // was played on, a new game was started on it, or it
                        // is another cartridge. It no longer shows whether
                        // Bank's write went through. Its secure value is not
                        // this save's to judge; a load of that game does.
                        Match::Other(Found::SameKind) => self.settle_by_moves(staging, true)?,
                        // The other kind of copy stands in for one that is
                        // gone and says nothing about this save: as for a
                        // game that is not there.
                        Match::Other(Found::StandIn) => self.settle_by_moves(staging, false)?,
                    };
                    self.settled(staging, Some(evidence), decision)
                }
                Job::Finalize { game } => {
                    let Phase::Prepared(pending) = self.phase()? else {
                        return Err(WorkerError::operation(16, 0));
                    };
                    #[cfg(feature = "test-stop-after-game")]
                    return Err(WorkerError::operation(TEST_STOP, 2));
                    // The save is resolved as soon as the game is known to
                    // hold the image this worker prepared, so that a cut
                    // after the game's write rarely finds it in progress.
                    // The secure pair tells: it is new with every prepared
                    // image, and the game's commit replaces the file whole.
                    let written = self.written.take();
                    let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
                        .map_err(|e| fs_error(e, 10))?;
                    let secure = file_secure(game, &mut main)?;
                    main.close().map_err(|e| fs_error(e, 10))?;
                    if written != Some(secure) || secure.current == secure.previous {
                        return Err(WorkerError::operation(17, 0));
                    }
                    finish_secure(game, secure)?;
                    let decision = self
                        .files
                        .reconcile_as(RecoveryDecision::CommitAfter)
                        .map_err(|e| files_error(e, 16))?;
                    // The complete image is still checked; it is also what
                    // the next save of this session starts from.
                    let evidence = inspect(game)?;
                    if matched(&pending, &evidence)? != Match::After {
                        return Err(WorkerError::operation(17, 0));
                    }
                    self.settled(staging, Some(evidence), decision)
                }
                Job::Prepare {
                    game,
                    prepared,
                    baseline,
                    rewards,
                } => {
                    if game.kind != prepared.kind
                        || game.kind.title_id() != baseline.title
                        || prepared.secure.previous != baseline.secure.current
                        || prepared.count != game.kind.data_block_count() as usize + 1
                        || prepared.count > MAX_PREPARED_BLOCKS
                    {
                        return Err(WorkerError::operation(12, 0));
                    }
                    let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
                        .map_err(|e| fs_error(e, 10))?;
                    let length = main.len();
                    if prepared
                        .validate_geometry(length)
                        .map_err(|_| WorkerError::operation(12, 0))?
                        != game.metadata_offset
                    {
                        return Err(WorkerError::operation(12, 0));
                    }
                    let secure = file_secure(game, &mut main)?;
                    if secure != baseline.secure {
                        return Err(WorkerError::operation(11, 0));
                    }
                    require_secure(game, secure.current)?;
                    // The unsafe submit contract keeps every frozen allocation
                    // immutable and live until this worker publishes completion.
                    let blocks = prepared.blocks[..prepared.count].iter().map(|block| {
                        let end = block
                            .offset
                            .checked_add(block.length as u64)
                            .ok_or(WorkerError::operation(12, 0))?;
                        if end > length
                            || (block.length != 0 && block.address == 0)
                            || block.address.checked_add(block.length).is_none()
                        {
                            return Err(WorkerError::operation(12, 0));
                        }
                        let pointer = if block.length == 0 {
                            core::ptr::NonNull::<u8>::dangling().as_ptr()
                        } else {
                            block.address as *mut u8
                        };
                        Ok(Overlay {
                            offset: block.offset,
                            bytes: unsafe { core::slice::from_raw_parts(pointer, block.length) },
                        })
                    });
                    let images = fingerprint_images(&mut main, length, blocks).map_err(
                        |error| match error {
                            offline_core::game_image::ImageError::Read(e) => fs_error(e, 10),
                            offline_core::game_image::ImageError::Block(e) => e,
                            _ => WorkerError::operation(12, 0),
                        },
                    )?;
                    main.close().map_err(|e| fs_error(e, 10))?;
                    if images.before != baseline.fingerprint {
                        return Err(WorkerError::operation(11, 0));
                    }
                    // Bound to the kind of copy, so that a recovery can
                    // tell this copy from one that stands in for it.
                    let identity = transaction::identity(
                        baseline.title,
                        images.before,
                        images.after,
                        Medium::of(baseline.cartridge),
                    )
                    .map_err(|_| WorkerError::operation(12, 0))?;
                    // Side files first, then the Bank journal.
                    self.files
                        .prepare(staging, rewards, identity, images.before, images.after)
                        .map_err(|e| files_error(e, 15))?;
                    // No game write is allowed until this post-barrier check ends.
                    let unchanged = inspect(game)?;
                    if unchanged != baseline {
                        return Err(WorkerError::operation(11, 0));
                    }
                    require_secure(game, unchanged.secure.current)?;
                    #[cfg(feature = "test-stop-before-game")]
                    return Err(WorkerError::operation(TEST_STOP, 1));
                    self.written = Some(prepared.secure);
                    Ok(Reply::ReadyToWrite)
                }
                Job::Close => Err(WorkerError::control(WorkerErrorKind::Stopped)),
            }
        }
    }
    /// Which image of the save in progress the game's save is, if either.
    fn matched(pending: &PendingTransfer, evidence: &GameEvidence) -> Result<Match, WorkerError> {
        transaction::match_pending_image(
            pending,
            evidence.title,
            evidence.fingerprint,
            Medium::of(evidence.cartridge),
        )
        .map_err(|_| WorkerError::operation(11, 0))
    }
    fn fs_error(error: fs::Error, fault: u32) -> WorkerError {
        WorkerError::operation(fault, error.diagnostic())
    }
    /// Second number for side-file problems: `7A00ffpp`, with `ff` the file
    /// (0 Bank, 1 Pokédex, 2 transport, 3 rewards) and `pp` the
    /// `bank_files::Problem`.
    fn files_error(error: bank_files::Error<fs::Error>, fault: u32) -> WorkerError {
        match error {
            bank_files::Error::Io(error) => fs_error(error, fault),
            bank_files::Error::Bank(error) => session_error(error, fault),
            bank_files::Error::Side(file, problem) => {
                WorkerError::operation(fault, 0x7a00_0000 | (file as u32) << 8 | problem as u32)
            }
        }
    }
    fn session_error(error: session::Error<fs::Error>, fault: u32) -> WorkerError {
        match error {
            session::Error::Storage(StoreError::Io(error)) => fs_error(error, fault),
            _ => WorkerError::operation(fault, 0),
        }
    }
    fn file_secure(
        game: GameIoDescriptor,
        main: &mut GameMainReader<'_>,
    ) -> Result<SecureValues, WorkerError> {
        if main.len() > i32::MAX as u64
            || game
                .metadata_offset
                .checked_add(0x1e8)
                .is_none_or(|end| end > main.len())
        {
            return Err(WorkerError::operation(10, 0));
        }
        let mut bytes = [0u8; 16];
        main.read(game.metadata_offset, &mut bytes)
            .map_err(|e| fs_error(e, 10))?;
        Ok(SecureValues {
            current: u64::from_le_bytes(
                bytes[..8]
                    .try_into()
                    .map_err(|_| WorkerError::operation(10, 0))?,
            ),
            previous: u64::from_le_bytes(
                bytes[8..]
                    .try_into()
                    .map_err(|_| WorkerError::operation(10, 0))?,
            ),
        })
    }
    fn inspect(game: GameIoDescriptor) -> Result<GameEvidence, WorkerError> {
        let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
            .map_err(|e| fs_error(e, 10))?;
        let length = main.len();
        let secure = file_secure(game, &mut main)?;
        let images = fingerprint_images(
            &mut main,
            length,
            core::iter::empty::<Result<Overlay<'_>, Infallible>>(),
        )
        .map_err(|error| match error {
            offline_core::game_image::ImageError::Read(e) => fs_error(e, 10),
            _ => WorkerError::operation(10, 0),
        })?;
        main.close().map_err(|e| fs_error(e, 10))?;
        Ok(GameEvidence {
            title: game.kind.title_id(),
            fingerprint: images.before,
            secure,
            cartridge: platform(game)?.gamecard,
        })
    }
    fn platform(game: GameIoDescriptor) -> Result<PlatformSecureValue, WorkerError> {
        type Get =
            unsafe extern "aapcs" fn(*const u32, *mut u8, *mut u8, *mut u64, u64, u32) -> i32;
        let get: Get = unsafe { transmute(0x0016578cusize) };
        let (mut first, mut second, mut value) = (0, 0, 0);
        let code = unsafe {
            get(
                &game.session,
                &mut first,
                &mut second,
                &mut value,
                game.archive,
                0x1000,
            )
        };
        if code < 0 {
            return Err(WorkerError::operation(14, code as u32));
        }
        if first > 1 || second > 1 {
            return Err(WorkerError::operation(14, 0));
        }
        Ok(PlatformSecureValue {
            value_present: first != 0,
            gamecard: second != 0,
            value,
        })
    }
    fn require_secure(game: GameIoDescriptor, expected: u64) -> Result<(), WorkerError> {
        if platform(game)?.matches_native_rule(expected) {
            Ok(())
        } else {
            Err(WorkerError::operation(14, 0))
        }
    }
    fn finish_secure(game: GameIoDescriptor, verified: SecureValues) -> Result<(), WorkerError> {
        // `verified` is read from the game's actual save. Only the one step
        // a completed save takes is made: from that save's own previous
        // value to its current one.
        let observed = platform(game)?;
        if observed.matches_native_rule(verified.current) {
            return Ok(());
        }
        if observed.value != verified.previous || verified.current == verified.previous {
            return Err(WorkerError::operation(14, 0));
        }
        type Set = unsafe extern "aapcs" fn(*const u32, u64, u32, u64, u8) -> i32;
        let set: Set = unsafe { transmute(0x0012146cusize) };
        let code = unsafe { set(&game.session, game.archive, 0x1000, verified.current, 0) };
        if code < 0 {
            return Err(WorkerError::operation(14, code as u32));
        }
        let after = platform(game)?;
        if after.value_present != observed.value_present
            || after.gamecard != observed.gamecard
            || !after.matches_native_rule(verified.current)
        {
            return Err(WorkerError::operation(14, 0));
        }
        Ok(())
    }
}
#[cfg(target_arch = "arm")]
pub(crate) use arm::StorageWorker;
