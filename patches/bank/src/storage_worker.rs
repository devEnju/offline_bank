//! Storage operations executed only by the dedicated native SDK worker.
//! No job calls UI functions or accesses a live NativeBank object.
//!
//! What Save and Quit checks and writes, and in which order (`prepare`,
//! `finalize`), is plain code over Bank's files and a `Game`, tested on the
//! simulated card with a simulated game; the jobs on the console run it with
//! the original's own calls (`arm::Console`).

use crate::{
    native_game::{GameIoDescriptor, PlatformSecureValue, PreparedImage, SecureValues},
    transaction::{self, Match, Medium, Trainer},
};
use bank_common::{
    bank_files::{self, BankFiles, Files, Loaded},
    fault::Fault,
    worker::WorkerError,
};
use offline_core::{
    crc32, game_image::ImageFingerprints, Fingerprint, PendingTransfer, Phase, RecoveryDecision,
    Storage,
};

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
    /// The game a session is loaded with. The Bank is the one opening
    /// restored; it is not read again.
    LoadGame {
        game: GameIoDescriptor,
    },
    /// Save and Quit's first question: is the game's Game Card there?
    Detect {
        game: GameIoDescriptor,
    },
    Prepare {
        game: GameIoDescriptor,
        prepared: PreparedImage,
        /// The game as it was loaded.
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
        loaded: Option<Loaded>,
    },
    /// Save and Quit has committed, and the Bank read back is the body it
    /// staged. The live object already is the saved body.
    Saved,
    Inspected(GameEvidence),
    /// The original's test finds the game's Game Card, or the installed
    /// copy, there.
    Detected,
    /// The original's test finds no Game Card, before the save wrote
    /// anything.
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

/// A game's save and its secure value on the console, as the worker reaches
/// them: `arm::Console` on the console, a simulated game in the tests.
pub trait Game {
    /// The game's save as it is.
    fn inspect(&mut self) -> Result<GameEvidence, WorkerError>;
    /// One read of the game's save: its secure pair, and its image as it is
    /// and with the prepared image laid over it.
    fn images(&mut self) -> Result<(SecureValues, ImageFingerprints), WorkerError>;
    /// The console's secure value of the game (`0016578c`).
    fn platform(&mut self) -> Result<PlatformSecureValue, WorkerError>;
    /// Sets the console's secure value of the game (`0012146c`).
    fn set_secure(&mut self, value: u64) -> Result<(), WorkerError>;
}

/// Why a job stopped: a game's or the worker's error, or one of Bank's
/// files, with the fault it is shown as (`Fault::files` on the console).
#[derive(Debug)]
pub enum Stop<E> {
    Worker(WorkerError),
    Files(Fault, bank_files::Error<E>),
}
impl<E> From<WorkerError> for Stop<E> {
    fn from(error: WorkerError) -> Self {
        Self::Worker(error)
    }
}
type Stopped<T, F> = Result<T, Stop<<<F as Files>::Storage as Storage>::Error>>;
fn files_failed<E>(fault: Fault) -> impl Fn(bank_files::Error<E>) -> Stop<E> {
    move |error| Stop::Files(fault, error)
}

/// What Save and Quit's preparation found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// The original's test finds no Game Card, and nothing was written.
    CardRemoved,
    /// The journal holds the save in progress, and the game may be written.
    /// `staged` is the checksum of the body in staging before the files took
    /// it apart, for `finalize` to find in the Bank again.
    Ready { staged: u32 },
}

/// Save and Quit's preparation, with the Bank staged: the game's save is read
/// once, before anything is written, for the image as it was loaded and the
/// one about to be written. Only then the journal takes the save in
/// progress, and after it the game is checked again: nothing may have
/// changed it in between.
pub fn prepare<F: Files, G: Game>(
    files: &mut BankFiles<F>,
    game: &mut G,
    staging: &mut [u8],
    baseline: GameEvidence,
    trainer: Trainer,
) -> Stopped<Prepared, F> {
    let mut read = || {
        let (secure, images) = game.images()?;
        if secure != baseline.secure {
            return Err(Fault::GameChanged.with(0));
        }
        let console = game.platform()?;
        if !console.matches_native_rule(secure.current) {
            return Err(Fault::GameSecureValue.with(0));
        }
        Ok((images, console.gamecard))
    };
    // A Game Card taken out by now ends the save as `Detect` does: nothing
    // is written yet.
    let (images, cartridge) = match read() {
        Err(_) if !detected(game) => return Ok(Prepared::CardRemoved),
        read => read?,
    };
    // The same save and the same kind of copy as the one loaded.
    if images.before != baseline.fingerprint || cartridge != baseline.cartridge {
        return Err(Fault::GameChanged.with(0).into());
    }
    // Bound to the kind of copy, so that a recovery can tell this copy from
    // one that stands in for it, and naming its trainer, for a recovery that
    // has to wait for it.
    let identity = transaction::identity(
        baseline.title,
        images.before,
        images.after,
        Medium::of(cartridge),
        trainer,
    )
    .map_err(|_| Fault::GamePrepare.with(0))?;
    let staged = crc32(staging);
    // Side files first, then the Bank journal.
    files
        .prepare(staging, identity, images.before, images.after)
        .map_err(files_failed(Fault::JournalPrepare))?;
    // No game write is allowed until this post-barrier check ends.
    let unchanged = game.inspect()?;
    if unchanged != baseline {
        return Err(Fault::GameChanged.with(0).into());
    }
    require_secure(game, unchanged.secure.current)?;
    Ok(Prepared::Ready { staged })
}

/// Save and Quit after the game's write. The save is resolved only once the
/// whole game is the image `prepare` allowed to be written, with the secure
/// pair `written`, which is new with every prepared image. Anything else
/// leaves the save in progress for the next start to settle. Then the Bank
/// is read back and must be the body with checksum `staged`, every byte.
pub fn finalize<F: Files, G: Game>(
    files: &mut BankFiles<F>,
    game: &mut G,
    staging: &mut [u8],
    pending: &PendingTransfer,
    written: Option<SecureValues>,
    staged: Option<u32>,
) -> Stopped<(), F> {
    let evidence = game.inspect()?;
    let secure = evidence.secure;
    if written != Some(secure)
        || secure.current == secure.previous
        || matched(pending, &evidence)? != Match::After
    {
        return Err(Fault::TransferRolledBack.with(0).into());
    }
    finish_secure(game, secure)?;
    files
        .reconcile_as(RecoveryDecision::CommitAfter)
        .map_err(files_failed(Fault::JournalRecovery))?;
    let staged = staged.ok_or(Fault::StorageLoad.with(0))?;
    files
        .read(staging)
        .map_err(files_failed(Fault::StorageLoad))?;
    if crc32(staging) != staged {
        return Err(Fault::StorageLoad.with(0).into());
    }
    Ok(())
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
/// Whether the game's Game Card is there, by the original's own test: its
/// game selection asks for the game's secure value (00292d64, 001643dc,
/// 00164370 → 0016578c) and shows message 0x1F, "No Pokémon Game Card
/// detected", when the query fails. An answer that is not what the query
/// gives is not taken for a missing card.
fn detected<G: Game>(game: &mut G) -> bool {
    !matches!(game.platform(), Err(error) if error.native_result() != 0)
}
fn require_secure<G: Game>(game: &mut G, expected: u64) -> Result<(), WorkerError> {
    if game.platform()?.matches_native_rule(expected) {
        Ok(())
    } else {
        Err(Fault::GameSecureValue.with(0))
    }
}
fn finish_secure<G: Game>(game: &mut G, verified: SecureValues) -> Result<(), WorkerError> {
    // `verified` is read from the game's actual save. Only the one step a
    // completed save takes is made: from that save's own previous value to
    // its current one.
    let observed = game.platform()?;
    if observed.matches_native_rule(verified.current) {
        return Ok(());
    }
    if observed.value != verified.previous || verified.current == verified.previous {
        return Err(Fault::GameSecureValue.with(0));
    }
    game.set_secure(verified.current)?;
    let after = game.platform()?;
    if after.value_present != observed.value_present
        || after.gamecard != observed.gamecard
        || !after.matches_native_rule(verified.current)
    {
        return Err(Fault::GameSecureValue.with(0));
    }
    Ok(())
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::{native_game::MAX_PREPARED_BLOCKS, transaction::Found};
    #[cfg(feature = "test-build")]
    use bank_common::rehearsal::{self, Rehearsal};
    use bank_common::{
        bank_files::ExtFiles,
        fs::{self, GameMainReader},
        program::Bank15,
        worker::WorkerErrorKind,
    };
    use core::{cell::UnsafeCell, mem::transmute};
    use offline_core::{
        game_image::{fingerprint_image, fingerprint_images, Overlay},
        moved,
        native_blob::{NativeBlobView, BLOB_SIZE},
        sections::{Identity, HELD},
    };
    use patch_common::worker::{Service, Shared, Worker};

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

    static SHARED: Shared<StorageWorker> = Shared::new([0; BLOB_SIZE]);
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
        /// The checksum of the body that `Prepare` staged, for `Finalize`
        /// to find in the Bank it reads back.
        staged: Option<u32>,
    }
    impl Service for StorageWorker {
        type Program = Bank15;
        type Job = Job;
        type Reply = Reply;
        type Error = WorkerError;
        type Staging = [u8; BLOB_SIZE];
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
                staged: None,
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
                self.staged = None;
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
        /// The end of a recovery, once the save in progress is resolved.
        fn settled(&mut self, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            let loaded = self.read_bank(staging)?;
            Ok(Reply::BankReady {
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
                    Ok(Reply::BankReady { loaded: None })
                }
                Job::Detect { game } => {
                    // A reply, not an error: the worker stays usable for
                    // the next session. Nothing of the save is read yet.
                    if detected(&mut Console::of(game)) {
                        Ok(Reply::Detected)
                    } else {
                        Ok(Reply::CardRemoved)
                    }
                }
                Job::LoadGame { game } => {
                    // An opening settles a save in progress or ends there;
                    // no game is loaded beside one.
                    if !matches!(self.phase()?, Phase::Clean(_)) {
                        return Err(Fault::JournalRecovery.with(0));
                    }
                    let mut game = Console::of(game);
                    let evidence = game.inspect()?;
                    // A save in progress that was settled without its game
                    // may have left the platform value one step behind.
                    finish_secure(&mut game, evidence.secure)?;
                    Ok(Reply::Inspected(evidence))
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
                            Some(_) => self.settled(staging),
                            None => Ok(waiting),
                        };
                    };
                    let mut game = Console::of(game);
                    let evidence = game.inspect()?;
                    let matched = matched(&pending, &evidence)?;
                    let decision = match matched {
                        Match::Before => {
                            require_secure(&mut game, evidence.secure.current)?;
                            Some(self.reconcile(&evidence)?)
                        }
                        Match::After => {
                            finish_secure(&mut game, evidence.secure)?;
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
                        Some(_) => self.settled(staging),
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
                    let (written, staged) = (self.written.take(), self.staged.take());
                    let mut game = Console::of(game);
                    finalize(
                        &mut self.files,
                        &mut game,
                        staging,
                        &pending,
                        written,
                        staged,
                    )
                    .map_err(stopped)?;
                    Ok(Reply::Saved)
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
                    #[cfg(feature = "test-build")]
                    self.files.rehearse(rehearsal::chosen());
                    let mut console = Console {
                        game,
                        prepared: Some(&prepared),
                    };
                    let prepared_game =
                        prepare(&mut self.files, &mut console, staging, baseline, trainer)
                            .map_err(stopped)?;
                    let Prepared::Ready { staged } = prepared_game else {
                        return Ok(Reply::CardRemoved);
                    };
                    #[cfg(feature = "test-build")]
                    if rehearsal::chosen() == Rehearsal::StopBeforeGame {
                        return Err(Fault::TestStop.with(1));
                    }
                    self.staged = Some(staged);
                    self.written = Some(prepared.secure);
                    Ok(Reply::ReadyToWrite)
                }
                Job::Close => Err(WorkerError::control(WorkerErrorKind::Stopped)),
            }
        }
    }
    /// The error a job stopped with, as the error screen shows it.
    fn stopped(stop: Stop<fs::Error>) -> WorkerError {
        match stop {
            Stop::Worker(error) => error,
            Stop::Files(fault, error) => fault.files(error),
        }
    }

    /// The game of a job, through the original's own calls.
    struct Console<'a> {
        game: GameIoDescriptor,
        /// The image Save and Quit prepared, for `images`.
        prepared: Option<&'a PreparedImage>,
    }
    impl Console<'_> {
        fn of(game: GameIoDescriptor) -> Self {
            Self {
                game,
                prepared: None,
            }
        }
    }
    impl Game for Console<'_> {
        fn inspect(&mut self) -> Result<GameEvidence, WorkerError> {
            let game = self.game;
            let mut main = unsafe { GameMainReader::open(game.session, game.archive) }
                .map_err(|e| Fault::GameRead.io(e))?;
            let length = main.len();
            let secure = file_secure(game, &mut main)?;
            let fingerprint =
                fingerprint_image(&mut main, length).map_err(|error| match error {
                    offline_core::game_image::ImageError::Read(e) => Fault::GameRead.io(e),
                    _ => Fault::GameRead.with(0),
                })?;
            main.close().map_err(|e| Fault::GameRead.io(e))?;
            Ok(GameEvidence {
                title: game.kind.title_id(),
                fingerprint,
                secure,
                cartridge: self.platform()?.gamecard,
            })
        }
        fn images(&mut self) -> Result<(SecureValues, ImageFingerprints), WorkerError> {
            let game = self.game;
            let prepared = self.prepared.ok_or(Fault::GamePrepare.with(0))?;
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
            let images =
                fingerprint_images(&mut main, length, blocks).map_err(|error| match error {
                    offline_core::game_image::ImageError::Read(e) => Fault::GameRead.io(e),
                    offline_core::game_image::ImageError::Block(e) => e,
                    _ => Fault::GamePrepare.with(0),
                })?;
            main.close().map_err(|e| Fault::GameRead.io(e))?;
            Ok((secure, images))
        }
        fn platform(&mut self) -> Result<PlatformSecureValue, WorkerError> {
            type Get =
                unsafe extern "aapcs" fn(*const u32, *mut u8, *mut u8, *mut u64, u64, u32) -> i32;
            let get: Get = unsafe { transmute(GET_SECURE_VALUE) };
            let (mut first, mut second, mut value) = (0, 0, 0);
            let code = unsafe {
                get(
                    &self.game.session,
                    &mut first,
                    &mut second,
                    &mut value,
                    self.game.archive,
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
        fn set_secure(&mut self, value: u64) -> Result<(), WorkerError> {
            type Set = unsafe extern "aapcs" fn(*const u32, u64, u32, u64, u8) -> i32;
            let set: Set = unsafe { transmute(SET_SECURE_VALUE) };
            let code = unsafe { set(&self.game.session, self.game.archive, 0x1000, value, 0) };
            if code < 0 {
                return Err(Fault::GameSecureValue.with(code as u32));
            }
            Ok(())
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
}
#[cfg(target_arch = "arm")]
pub(crate) use arm::BankWorker;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use bank_common::testing::{body, fresh, load, opened, Shared};
    use std::collections::BTreeMap;
    use std::vec::Vec;

    /// X, on a cartridge.
    const TITLE: u64 = 0x0004_0000_0005_5d00;
    /// The console's error for a card that is not there.
    const NO_CARD: u32 = 0xc880_4478;
    const LOADED: SecureValues = SecureValues {
        current: 7,
        previous: 6,
    };
    const WRITTEN: SecureValues = SecureValues {
        current: 8,
        previous: 7,
    };
    const BEFORE: Fingerprint = [1; 32];
    const AFTER: Fingerprint = [2; 32];
    const OTHER: Fingerprint = [3; 32];

    /// A game as the worker reaches it.
    struct Simulated {
        /// Its save, or `None` once its Game Card is out.
        save: Option<(Fingerprint, SecureValues)>,
        /// The image the prepared one makes of it.
        after: Fingerprint,
        cartridge: bool,
        /// The console's secure value of it; a cartridge keeps its own.
        value: u64,
        /// The card is taken out after this many calls more.
        out_after: Option<usize>,
    }
    impl Simulated {
        fn loaded(cartridge: bool) -> Self {
            Self {
                save: Some((BEFORE, LOADED)),
                after: AFTER,
                cartridge,
                value: LOADED.current,
                out_after: None,
            }
        }
        /// The game as it was loaded, as `LoadGame` saw it.
        fn evidence(&self) -> GameEvidence {
            let (fingerprint, secure) = self.save.unwrap();
            GameEvidence {
                title: TITLE,
                fingerprint,
                secure,
                cartridge: self.cartridge,
            }
        }
        /// The original's writer: the prepared image, with its new pair.
        fn write(&mut self, secure: SecureValues) {
            self.save = Some((self.after, secure));
        }
        fn save(&mut self) -> Result<(Fingerprint, SecureValues), WorkerError> {
            if let Some(calls) = self.out_after.as_mut() {
                if *calls == 0 {
                    self.save = None;
                } else {
                    *calls -= 1;
                }
            }
            self.save.ok_or(Fault::GameRead.with(NO_CARD))
        }
    }
    impl Game for Simulated {
        fn inspect(&mut self) -> Result<GameEvidence, WorkerError> {
            let (fingerprint, secure) = self.save()?;
            Ok(GameEvidence {
                title: TITLE,
                fingerprint,
                secure,
                cartridge: self.cartridge,
            })
        }
        fn images(&mut self) -> Result<(SecureValues, ImageFingerprints), WorkerError> {
            let (before, secure) = self.save()?;
            let after = self.after;
            Ok((secure, ImageFingerprints { before, after }))
        }
        fn platform(&mut self) -> Result<PlatformSecureValue, WorkerError> {
            self.save()
                .map_err(|_| Fault::GameSecureValue.with(NO_CARD))?;
            Ok(PlatformSecureValue {
                value_present: true,
                gamecard: self.cartridge,
                value: self.value,
            })
        }
        fn set_secure(&mut self, value: u64) -> Result<(), WorkerError> {
            self.save()?;
            self.value = value;
            Ok(())
        }
    }

    fn trainer() -> Trainer {
        Trainer {
            name: [0x53; crate::transaction::NAME],
            id: 12345,
        }
    }
    fn stored(disk: &Shared) -> BTreeMap<u8, Vec<u8>> {
        disk.0.borrow().files.clone()
    }
    fn pending(files: &mut BankFiles<Shared>) -> Option<PendingTransfer> {
        match files.phase().unwrap() {
            Phase::Prepared(pending) => Some(pending),
            Phase::Clean(_) => None,
        }
    }
    /// A Bank on the card, and the body Save and Quit stages for it.
    fn bank() -> (Shared, BankFiles<Shared>, Vec<u8>) {
        let disk = fresh(&body(0, 0, 0));
        let mut files = opened(&disk);
        load(&mut files);
        (disk, files, body(0x52, 0x66, 1))
    }
    fn fault<T: core::fmt::Debug, E: core::fmt::Debug>(result: Result<T, Stop<E>>) -> Fault {
        match result.unwrap_err() {
            Stop::Worker(error) => error.fault(),
            Stop::Files(fault, _) => fault,
        }
    }

    #[test]
    fn a_save_commits_and_the_bank_read_back_is_the_body_it_staged() {
        for cartridge in [true, false] {
            let (_, mut files, body) = bank();
            let mut game = Simulated::loaded(cartridge);
            let baseline = game.evidence();
            let mut staging = body.clone();
            let staged = match prepare(&mut files, &mut game, &mut staging, baseline, trainer()) {
                Ok(Prepared::Ready { staged }) => staged,
                other => panic!("{other:?}"),
            };
            let in_progress = pending(&mut files).unwrap();
            game.write(WRITTEN);
            finalize(
                &mut files,
                &mut game,
                &mut staging,
                &in_progress,
                Some(WRITTEN),
                Some(staged),
            )
            .unwrap();
            assert!(staging == body);
            assert!(pending(&mut files).is_none());
            assert!(load(&mut files).0 == body);
            // An installed copy's value on the console follows its save; a
            // cartridge keeps its own.
            let value = if cartridge { LOADED } else { WRITTEN };
            assert_eq!(game.value, value.current);
        }
    }

    #[test]
    fn a_game_card_taken_out_before_the_journal_ends_the_save_with_nothing_written() {
        let (disk, mut files, body) = bank();
        let mut game = Simulated::loaded(true);
        let baseline = game.evidence();
        // Out before the read, and between the read and its secure value.
        for calls in [0, 1] {
            game.out_after = Some(calls);
            game.save = Some((BEFORE, LOADED));
            let before = stored(&disk);
            let mut staging = body.clone();
            let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
            assert_eq!(result.unwrap(), Prepared::CardRemoved, "{calls}");
            assert!(stored(&disk) == before, "{calls}");
            assert!(pending(&mut files).is_none());
        }
    }

    #[test]
    fn a_game_card_taken_out_after_the_journal_leaves_the_save_in_progress() {
        let (_, mut files, body) = bank();
        let mut game = Simulated::loaded(true);
        let baseline = game.evidence();
        // The read and its secure value, then out for the check after it.
        game.out_after = Some(2);
        let mut staging = body.clone();
        let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
        assert_eq!(fault(result), Fault::GameRead);
        assert!(pending(&mut files).is_some());
    }

    #[test]
    fn a_game_that_is_not_the_one_loaded_is_refused_before_anything_is_written() {
        let changed: [&dyn Fn(&mut Simulated); 3] = [
            // Played on since it was loaded.
            &|game| game.save = Some((OTHER, LOADED)),
            // Another save with the same image.
            &|game| game.save = Some((BEFORE, WRITTEN)),
            // The other kind of copy.
            &|game| game.cartridge = !game.cartridge,
        ];
        for change in changed {
            let (disk, mut files, body) = bank();
            let mut game = Simulated::loaded(true);
            let baseline = game.evidence();
            change(&mut game);
            let before = stored(&disk);
            let mut staging = body.clone();
            let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
            assert_eq!(fault(result), Fault::GameChanged);
            assert!(stored(&disk) == before);
            assert!(pending(&mut files).is_none());
        }
    }

    #[test]
    fn a_game_that_is_not_the_written_image_leaves_the_save_in_progress() {
        let written: [&dyn Fn(&mut Simulated); 4] = [
            // The writer failed: the save before.
            &|_| {},
            // Its new pair, but not the whole image.
            &|game| game.save = Some((OTHER, WRITTEN)),
            // The image, but not the pair this save prepared.
            &|game| game.save = Some((AFTER, LOADED)),
            // The card is out.
            &|game| game.save = None,
        ];
        for write in written {
            let (_, mut files, body) = bank();
            let mut game = Simulated::loaded(true);
            let baseline = game.evidence();
            let mut staging = body.clone();
            let Ok(Prepared::Ready { staged }) =
                prepare(&mut files, &mut game, &mut staging, baseline, trainer())
            else {
                panic!("not prepared");
            };
            let in_progress = pending(&mut files).unwrap();
            write(&mut game);
            let result = finalize(
                &mut files,
                &mut game,
                &mut staging,
                &in_progress,
                Some(WRITTEN),
                Some(staged),
            );
            assert!(result.is_err());
            assert_eq!(pending(&mut files), Some(in_progress));
        }
    }

    #[test]
    fn two_saves_in_a_row_each_commit_their_own_image() {
        let (_, mut files, _) = bank();
        let mut game = Simulated::loaded(false);
        let mut secure = WRITTEN;
        for (round, after) in [(1u8, AFTER), (2, OTHER)] {
            game.after = after;
            let baseline = game.evidence();
            let body = body(0x50 + round, 0x66, 1);
            let mut staging = body.clone();
            let Ok(Prepared::Ready { staged }) =
                prepare(&mut files, &mut game, &mut staging, baseline, trainer())
            else {
                panic!("round {round} not prepared");
            };
            let in_progress = pending(&mut files).unwrap();
            game.write(secure);
            finalize(
                &mut files,
                &mut game,
                &mut staging,
                &in_progress,
                Some(secure),
                Some(staged),
            )
            .unwrap();
            assert!(load(&mut files).0 == body, "round {round}");
            assert_eq!(game.value, secure.current);
            secure = SecureValues {
                current: secure.current + 1,
                previous: secure.current,
            };
        }
    }

    #[test]
    fn the_bank_read_back_after_a_save_is_the_body_the_save_staged() {
        let (_, mut files, mut staging) = bank();
        // `Prepare` takes it before the files take staging apart.
        let staged = crc32(&staging);
        files
            .prepare(&mut staging, bank_common::testing::GAME, [1; 32], [2; 32])
            .unwrap();
        assert_ne!(crc32(&staging), staged);
        // `Finalize` commits and reads the Bank back into the same buffer.
        files.reconcile_as(RecoveryDecision::CommitAfter).unwrap();
        files.read(&mut staging).unwrap();
        assert_eq!(crc32(&staging), staged);
    }
}
