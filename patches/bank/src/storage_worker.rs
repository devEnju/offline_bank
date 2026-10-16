//! Storage operations executed only by the dedicated native SDK worker.
//! No job calls UI functions or accesses a live NativeBank object.

use crate::{
    native_game::{GameIoDescriptor, PreparedImage, SecureValues},
    transaction::Trainer,
};
use bank_common::bank_files::Loaded;
use offline_core::{Fingerprint, Phase, RecoveryDecision};

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
        /// Whose game it is, read when the game was loaded.
        trainer: Trainer,
    },
    Finalize {
        game: GameIoDescriptor,
    },
    Close,
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
    /// The game's save could not be read, and the original's test finds no
    /// Game Card. Nothing was written.
    CardRemoved,
    ReadyToWrite,
    /// The save in progress moved Pokémon both ways and its game is not
    /// there: it waits for the game of this title and trainer. Nothing was
    /// written, and a later opening settles it.
    Waiting {
        title: u64,
        trainer: Trainer,
    },
    Closed,
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::{
        native_game::{PlatformSecureValue, MAX_PREPARED_BLOCKS},
        transaction::{self, Found, Match, Medium},
    };
    #[cfg(feature = "test-build")]
    use bank_common::rehearsal::{self, Rehearsal};
    use bank_common::{
        bank_files::{self, BankFiles, ExtFiles},
        fault::Fault,
        fs::GameMainReader,
        worker::{Service, Shared, Worker, WorkerError, WorkerErrorKind},
    };
    use core::{cell::UnsafeCell, mem::transmute};
    use offline_core::{
        game_image::{fingerprint_image, fingerprint_images, Overlay},
        moved,
        native_blob::NativeBlobView,
        sections::{Identity, HELD},
        PendingTransfer, Storage,
    };

    /// A game's secure value on the console, slot 0x1000:
    /// `0016578c(session, first, second, value, archive, slot)` reads it
    /// (FS command 0x0876), `0012146c(session, archive, slot, value, 0)`
    /// sets it.
    const GET_SECURE_VALUE: usize = 0x0016_578c;
    const SET_SECURE_VALUE: usize = 0x0012_146c;

    /// Working room for comparing the two snapshots of a save in progress:
    /// everything the Bank held before it and after it.
    struct Held(UnsafeCell<[[Identity; HELD]; 2]>);
    // SAFETY: only the one storage worker thread uses it, inside one job.
    unsafe impl Sync for Held {}
    static HELD_LISTS: Held = Held(UnsafeCell::new([[[0; 3]; HELD]; 2]));

    static SHARED: Shared<StorageWorker> = Shared::new();
    /// The worker of the offline patch.
    pub(crate) type BankWorker = Worker<StorageWorker>;

    pub(crate) struct StorageWorker {
        files: BankFiles<ExtFiles>,
        fs_session: u32,
        missing: bool,
        poisoned: bool,
        /// The secure pair of the game image the last `Prepare` allowed to
        /// be written, until `Finalize` has looked for it.
        written: Option<SecureValues>,
    }
    impl Service for StorageWorker {
        type Job = Job;
        type Reply = Reply;
        // This worker plus the retained native game writer.
        const THREADS: i64 = 2;
        fn shared() -> &'static Shared<Self> {
            &SHARED
        }
        fn start() -> Self {
            Self {
                files: BankFiles::new(ExtFiles { session: 0 }),
                fs_session: 0,
                missing: false,
                poisoned: false,
                written: None,
            }
        }
        fn run(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            // A session has ended: nothing of the Bank stays open. This is
            // done whatever state an error left, and the thread goes on
            // waiting for the next session.
            if matches!(job, Job::Close) {
                if let Some(mut storage) = self.files.close() {
                    storage.close().map_err(|e| Fault::StorageOpen.io(e))?;
                }
                self.fs_session = 0;
                self.missing = false;
                self.written = None;
                return Ok(Reply::Closed);
            }
            if self.poisoned {
                return Err(Fault::StorageInvalid.with(0));
            }
            let result = self.execute(job, staging);
            if result.is_err() {
                self.poisoned = true;
            }
            result
        }
    }
    impl StorageWorker {
        fn phase(&mut self) -> Result<Phase, WorkerError> {
            self.files
                .phase()
                .map_err(|e| Fault::StorageInvalid.files(e))
        }
        /// Leaves the assembled native body in staging.
        fn read_bank(&mut self, staging: &mut [u8]) -> Result<bank_files::Loaded, WorkerError> {
            self.files
                .read(staging)
                .map_err(|e| Fault::StorageLoad.files(e))
        }
        /// Resolves the save in progress from a game save that is one of
        /// its two images.
        fn reconcile(&mut self, evidence: &GameEvidence) -> Result<RecoveryDecision, WorkerError> {
            self.files
                .reconcile(evidence.fingerprint)
                .map_err(|e| Fault::JournalRecovery.files(e))
        }
        /// Resolves the save in progress from what it moved, toward the
        /// snapshot that cannot lose a Pokémon. `seen` says that a game of
        /// its title is there, with a save that is neither image. Without
        /// one only a save that moved Pokémon one way, or none, is
        /// resolved; one that moved them both ways waits for its game:
        /// `None`, and nothing is written.
        fn settle_by_moves(
            &mut self,
            staging: &mut [u8],
            seen: bool,
        ) -> Result<Option<RecoveryDecision>, WorkerError> {
            // SAFETY: see `Held`; no other reference exists.
            let [before, after] = unsafe { &mut *HELD_LISTS.0.get() };
            let moved = self
                .files
                .pending_moves(staging, before, after)
                .map_err(|e| Fault::JournalRecovery.files(e))?;
            let decision = if seen {
                moved::choose(moved)
            } else {
                match moved::choose_unseen(moved) {
                    Some(decision) => decision,
                    None => return Ok(None),
                }
            };
            self.files
                .reconcile_as(decision)
                .map(Some)
                .map_err(|e| Fault::JournalRecovery.files(e))
        }
        /// The end of a recovery or a Save and Quit, once the save in
        /// progress is resolved.
        fn settled(
            &mut self,
            staging: &mut [u8],
            evidence: Option<GameEvidence>,
            decision: RecoveryDecision,
        ) -> Result<Reply, WorkerError> {
            let loaded = self.read_bank(staging)?;
            Ok(Reply::BankReady {
                evidence,
                decision: Some(decision),
                loaded: Some(loaded),
            })
        }
        fn execute(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            match job {
                Job::Open { session } => {
                    if !self.files.is_open() {
                        if self.missing {
                            return Err(Fault::StorageInvalid.with(0));
                        }
                        if session == 0 {
                            // Bank's own FS session is not initialized yet.
                            return Err(Fault::StorageOpen.with(0x7800_0000));
                        }
                        self.fs_session = session;
                        self.files.files_mut().session = session;
                    } else if self.fs_session != session {
                        // Bank's FS session changed while our file was open.
                        return Err(Fault::StorageOpen.with(0x7700_0000));
                    }
                    let opened = self.files.open().map_err(|error| match error {
                        bank_files::Error::Io(error) => Fault::StorageOpen.io(error),
                        error => Fault::StorageInvalid.files(error),
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
                        return Err(Fault::StorageCreate.with(0));
                    }
                    // Refuse an invalid native body before creating any file.
                    NativeBlobView::parse_layout(staging)
                        .map_err(|_| Fault::NativeObject.with(0))?;
                    self.files
                        .initialize(staging)
                        .map_err(|e| Fault::StorageCreate.files(e))?;
                    self.missing = false;
                    Ok(Reply::BankReady {
                        evidence: None,
                        decision: None,
                        loaded: None,
                    })
                }
                Job::Inspect { game } => {
                    // A reply, not an error: the worker stays usable for
                    // the next session.
                    let evidence = match inspect(game) {
                        Err(_) if !detected(game) => return Ok(Reply::CardRemoved),
                        evidence => evidence?,
                    };
                    require_secure(game, evidence.secure.current)?;
                    Ok(Reply::Inspected(evidence))
                }
                Job::InspectAndLoad { game } => {
                    // An opening settles a save in progress or ends there;
                    // no game is loaded beside one.
                    if !matches!(self.phase()?, Phase::Clean(_)) {
                        return Err(Fault::JournalRecovery.with(0));
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
                        return Err(Fault::JournalRecovery.with(0));
                    };
                    let waiting = Reply::Waiting {
                        title: pending.game.title_id,
                        trainer: Trainer::of(&pending.game),
                    };
                    let Some(game) = game else {
                        // No game shows which kind of copy the save was
                        // made with; its binding must still be this
                        // version's, on one kind or the other.
                        transaction::bound(&pending).map_err(|_| Fault::GameChanged.with(0))?;
                        return match self.settle_by_moves(staging, false)? {
                            Some(decision) => self.settled(staging, None, decision),
                            None => Ok(waiting),
                        };
                    };
                    let evidence = inspect(game)?;
                    let matched = matched(&pending, &evidence)?;
                    let decision = match matched {
                        Match::Before => {
                            require_secure(game, evidence.secure.current)?;
                            Some(self.reconcile(&evidence)?)
                        }
                        Match::After => {
                            finish_secure(game, evidence.secure)?;
                            Some(self.reconcile(&evidence)?)
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
                    match decision {
                        Some(decision) => self.settled(staging, Some(evidence), decision),
                        None => Ok(waiting),
                    }
                }
                Job::Finalize { game } => {
                    let Phase::Prepared(pending) = self.phase()? else {
                        return Err(Fault::JournalRecovery.with(0));
                    };
                    #[cfg(feature = "test-build")]
                    if rehearsal::chosen() == Rehearsal::StopAfterGame {
                        return Err(Fault::TestStop.with(2));
                    }
                    // The save is resolved as soon as the game is known to
                    // hold the image this worker prepared, so that a cut
                    // after the game's write rarely finds it in progress.
                    // The secure pair tells: it is new with every prepared
                    // image, and the game's commit replaces the file whole.
                    let written = self.written.take();
                    let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
                        .map_err(|e| Fault::GameRead.io(e))?;
                    let secure = file_secure(game, &mut main)?;
                    main.close().map_err(|e| Fault::GameRead.io(e))?;
                    if written != Some(secure) || secure.current == secure.previous {
                        return Err(Fault::TransferRolledBack.with(0));
                    }
                    finish_secure(game, secure)?;
                    let decision = self
                        .files
                        .reconcile_as(RecoveryDecision::CommitAfter)
                        .map_err(|e| Fault::JournalRecovery.files(e))?;
                    // The complete image is still checked; it is also what
                    // the next save of this session starts from.
                    let evidence = inspect(game)?;
                    if matched(&pending, &evidence)? != Match::After {
                        return Err(Fault::TransferRolledBack.with(0));
                    }
                    self.settled(staging, Some(evidence), decision)
                }
                Job::Prepare {
                    game,
                    prepared,
                    baseline,
                    trainer,
                } => {
                    if game.kind != prepared.kind
                        || game.kind.title_id() != baseline.title
                        || prepared.secure.previous != baseline.secure.current
                        || prepared.count != game.kind.data_block_count() as usize + 1
                        || prepared.count > MAX_PREPARED_BLOCKS
                    {
                        return Err(Fault::GamePrepare.with(0));
                    }
                    let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
                        .map_err(|e| Fault::GameRead.io(e))?;
                    let length = main.len();
                    if prepared
                        .validate_geometry(length)
                        .map_err(|_| Fault::GamePrepare.with(0))?
                        != game.metadata_offset
                    {
                        return Err(Fault::GamePrepare.with(0));
                    }
                    let secure = file_secure(game, &mut main)?;
                    if secure != baseline.secure {
                        return Err(Fault::GameChanged.with(0));
                    }
                    require_secure(game, secure.current)?;
                    // The unsafe submit contract keeps every frozen allocation
                    // immutable and live until this worker publishes completion.
                    let blocks = prepared.blocks[..prepared.count].iter().map(|block| {
                        let end = block
                            .offset
                            .checked_add(block.length as u64)
                            .ok_or(Fault::GamePrepare.with(0))?;
                        if end > length
                            || (block.length != 0 && block.address == 0)
                            || block.address.checked_add(block.length).is_none()
                        {
                            return Err(Fault::GamePrepare.with(0));
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
                            offline_core::game_image::ImageError::Read(e) => Fault::GameRead.io(e),
                            offline_core::game_image::ImageError::Block(e) => e,
                            _ => Fault::GamePrepare.with(0),
                        },
                    )?;
                    main.close().map_err(|e| Fault::GameRead.io(e))?;
                    if images.before != baseline.fingerprint {
                        return Err(Fault::GameChanged.with(0));
                    }
                    // Bound to the kind of copy, so that a recovery can
                    // tell this copy from one that stands in for it, and
                    // naming its trainer, for a recovery that has to wait
                    // for it.
                    let identity = transaction::identity(
                        baseline.title,
                        images.before,
                        images.after,
                        Medium::of(baseline.cartridge),
                        trainer,
                    )
                    .map_err(|_| Fault::GamePrepare.with(0))?;
                    #[cfg(feature = "test-build")]
                    self.files.rehearse(rehearsal::chosen());
                    // Side files first, then the Bank journal.
                    self.files
                        .prepare(staging, identity, images.before, images.after)
                        .map_err(|e| Fault::JournalPrepare.files(e))?;
                    // No game write is allowed until this post-barrier check ends.
                    let unchanged = inspect(game)?;
                    if unchanged != baseline {
                        return Err(Fault::GameChanged.with(0));
                    }
                    require_secure(game, unchanged.secure.current)?;
                    #[cfg(feature = "test-build")]
                    if rehearsal::chosen() == Rehearsal::StopBeforeGame {
                        return Err(Fault::TestStop.with(1));
                    }
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
        .map_err(|_| Fault::GameChanged.with(0))
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
            return Err(Fault::GameRead.with(0));
        }
        let mut bytes = [0u8; 16];
        main.read(game.metadata_offset, &mut bytes)
            .map_err(|e| Fault::GameRead.io(e))?;
        Ok(SecureValues {
            current: u64::from_le_bytes(
                bytes[..8].try_into().map_err(|_| Fault::GameRead.with(0))?,
            ),
            previous: u64::from_le_bytes(
                bytes[8..].try_into().map_err(|_| Fault::GameRead.with(0))?,
            ),
        })
    }
    fn inspect(game: GameIoDescriptor) -> Result<GameEvidence, WorkerError> {
        let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
            .map_err(|e| Fault::GameRead.io(e))?;
        let length = main.len();
        let secure = file_secure(game, &mut main)?;
        let fingerprint = fingerprint_image(&mut main, length).map_err(|error| match error {
            offline_core::game_image::ImageError::Read(e) => Fault::GameRead.io(e),
            _ => Fault::GameRead.with(0),
        })?;
        main.close().map_err(|e| Fault::GameRead.io(e))?;
        Ok(GameEvidence {
            title: game.kind.title_id(),
            fingerprint,
            secure,
            cartridge: platform(game)?.gamecard,
        })
    }
    fn platform(game: GameIoDescriptor) -> Result<PlatformSecureValue, WorkerError> {
        type Get =
            unsafe extern "aapcs" fn(*const u32, *mut u8, *mut u8, *mut u64, u64, u32) -> i32;
        let get: Get = unsafe { transmute(GET_SECURE_VALUE) };
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
            return Err(Fault::GameSecureValue.with(code as u32));
        }
        if first > 1 || second > 1 {
            return Err(Fault::GameSecureValue.with(0));
        }
        Ok(PlatformSecureValue {
            value_present: first != 0,
            gamecard: second != 0,
            value,
        })
    }
    /// Whether the game's Game Card is there, by the original's own test:
    /// its game selection asks for the game's secure value (00292d64,
    /// 001643dc, 00164370 → 0016578c) and shows message 0x1F, "No Pokémon
    /// Game Card detected", when the query fails. An answer that is not
    /// what the query gives is not taken for a missing card.
    fn detected(game: GameIoDescriptor) -> bool {
        !matches!(platform(game), Err(error) if error.native_result() != 0)
    }
    fn require_secure(game: GameIoDescriptor, expected: u64) -> Result<(), WorkerError> {
        if platform(game)?.matches_native_rule(expected) {
            Ok(())
        } else {
            Err(Fault::GameSecureValue.with(0))
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
            return Err(Fault::GameSecureValue.with(0));
        }
        type Set = unsafe extern "aapcs" fn(*const u32, u64, u32, u64, u8) -> i32;
        let set: Set = unsafe { transmute(SET_SECURE_VALUE) };
        let code = unsafe { set(&game.session, game.archive, 0x1000, verified.current, 0) };
        if code < 0 {
            return Err(Fault::GameSecureValue.with(code as u32));
        }
        let after = platform(game)?;
        if after.value_present != observed.value_present
            || after.gamecard != observed.gamecard
            || !after.matches_native_rule(verified.current)
        {
            return Err(Fault::GameSecureValue.with(0));
        }
        Ok(())
    }
}
#[cfg(target_arch = "arm")]
pub(crate) use arm::BankWorker;
