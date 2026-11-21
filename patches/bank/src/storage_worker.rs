//! Storage operations executed only by the dedicated native SDK worker.
//! No job calls UI functions or accesses a live NativeBank object.
//!
//! What Save and Quit checks and writes, and in which order (`prepare`,
//! `finalize`), and how a save in progress is settled (`settle`), is plain
//! code over Bank's files and a `Game`, tested on the simulated card with a
//! simulated game; the jobs on the console run it with the original's own
//! calls (`arm::Console`).
//!
//! A job that cannot finish says what it leaves (`Ending`) where the screen
//! can say it with a message of the original: a Game Card that is gone, by
//! the original's own test, or a card without room for a file. Every other
//! failure is an error, which shows the two numbers, a read or write of
//! Bank's files that the SD card refused among them.

use crate::{
    native_game::{GameIoDescriptor, GameKind, PlatformSecureValue, PreparedImage, SecureValues},
    transaction::{self, Found, Match, Medium, Trainer},
};
use bank_common::{
    bank_files::{self, BankFiles, Cleaned, Files, Loaded},
    fault::Fault,
    fs::NoRoom,
    interrupted,
    sdfiles::{self, Date, Extdata, Store},
    transfer_screen,
    worker::WorkerError,
};
use offline_core::{
    crc32,
    game_image::ImageFingerprints,
    moved,
    sections::{Identity, HELD},
    Fingerprint, PendingTransfer, Phase, RecoveryDecision, Storage,
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
    /// `date` is the console's when the job was handed over, read on the
    /// main thread with the original's clock (`hooks::date`); it names a
    /// folder of the SD card, here and in `Recover` and `TransferCommit`.
    Open {
        session: u32,
        date: Date,
    },
    Initialize,
    /// `game` is `None` when the game of the save in progress is not there;
    /// with it, whose game it is, read on the main thread.
    Recover {
        game: Option<(GameIoDescriptor, Trainer)>,
        date: Date,
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
    /// The save in progress was unlocked on the original's unlock screen:
    /// Transporter's question settles it whatever game is found from then on
    /// (`BankFiles::unlock`).
    Unlock,
    /// Save and Quit once the original's writer has ended, whether it
    /// wrote the game or failed: the save is settled from what the game
    /// holds (`finalize`). `status` is the writer's result, 0 when it
    /// reported none.
    Finalize {
        game: GameIoDescriptor,
        status: u32,
    },
    /// The transfer screen's offline boxes are read into `ROOM`
    /// (`transfer_screen::load_offline`).
    LoadOffline,
    /// What the transfer screen changed is written (`transfer_screen::commit`): the body
    /// in staging, the offline boxes in `ROOM`. `date` names the session's
    /// folder.
    TransferCommit {
        checked: [bool; transfer_screen::PLACES],
        date: Date,
    },
}

/// What the transfer screen keeps while it is up: the extended layout the
/// original builds the screen from (`layout::extend`), which must stay while
/// that layout lives, first and aligned (`layout::Room`); room for one box
/// file, aligned too, as the preview hands a record from it to the
/// original; the offline boxes, as the bytes of their files.
#[repr(C)]
pub struct TransferRoom {
    pub layout: crate::layout::Room,
    pub scratch: [u8; offline_core::boxfile::FILE_SIZE],
    pub offline: transfer_screen::Offline,
}

/// Memory that two phases share, as they never overlap: the lists that
/// compare a save's two snapshots (the worker, inside `Recover` and
/// `Finalize`, which need a game) and the transfer screen's room (from
/// `LoadOffline` until the screen's task ends, with no game). The payload
/// has to end below the next whole megabyte (`docs/internals.md`,
/// "Placement"), so the two do not get a block each.
#[repr(C)]
union RoomBytes {
    held: [[Identity; HELD]; 2],
    transfer: core::mem::ManuallyDrop<TransferRoom>,
}
const _: () =
    assert!(core::mem::size_of::<TransferRoom>() <= core::mem::size_of::<[[Identity; HELD]; 2]>());
// The opening's settling of the SD card's folders works in two box files.
const _: () = assert!(core::mem::size_of::<RoomBytes>() >= 2 * offline_core::boxfile::FILE_SIZE);
const _: () = assert!(
    core::mem::align_of::<RoomBytes>() >= 128
        && core::mem::offset_of!(TransferRoom, layout) == 0
        && core::mem::offset_of!(TransferRoom, scratch) % 8 == 0
);

pub struct Room(core::cell::UnsafeCell<RoomBytes>);
// SAFETY: one thread at a time, by the job protocol: the transfer screen's
// room is the main thread's while the screen is up and the worker's while
// `LoadOffline` or `TransferCommit` is out, as staging is; the held lists
// are the worker's inside one job.
unsafe impl Sync for Room {}
/// All zero, which is a valid value of either use.
pub static ROOM: Room = Room(core::cell::UnsafeCell::new(RoomBytes {
    held: [[[0; 3]; HELD]; 2],
}));
impl Room {
    fn transfer(&self) -> *mut TransferRoom {
        // A union field's place, without a reference to the whole.
        unsafe { core::ptr::addr_of_mut!((*self.0.get()).transfer).cast() }
    }
    /// The offline boxes and the room for one box file.
    /// # Safety
    /// Only by the thread whose turn it is, while the transfer screen's
    /// phase lasts (see `RoomBytes`).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn boxes(
        &self,
    ) -> (
        &mut transfer_screen::Offline,
        &mut [u8; offline_core::boxfile::FILE_SIZE],
    ) {
        let room = self.transfer();
        unsafe {
            (
                &mut *core::ptr::addr_of_mut!((*room).offline),
                &mut *core::ptr::addr_of_mut!((*room).scratch),
            )
        }
    }
    /// The extended layout: the main thread's, from its build until the
    /// screen's task ends. The worker never touches it.
    /// # Safety
    /// Only by the main thread, while the transfer screen's phase lasts.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn layout(&self) -> &mut [u8; crate::layout::EXTENDED_ROOM] {
        unsafe { &mut *core::ptr::addr_of_mut!((*self.transfer()).layout.0) }
    }
    /// # Safety
    /// Only by the worker, inside a job that needs a game (see `RoomBytes`).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn held(&self) -> &mut [[Identity; HELD]; 2] {
        unsafe { &mut (*self.0.get()).held }
    }
    /// The whole room as bytes, for the opening's settling of the SD card's
    /// folders (`settle_sd`), while staging holds the Bank just read.
    /// # Safety
    /// Only by the worker, inside `Open` or `Recover` once the held lists
    /// are done with; the transfer screen's phase has not begun.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn work(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.0.get().cast::<u8>(),
                core::mem::size_of::<RoomBytes>(),
            )
        }
    }
}

/// What a job that could not finish leaves, and so which message of the
/// original the screen shows (docs/internals.md, "When a job cannot
/// finish"). The session ends at that message and the start screen follows;
/// the worker stays usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// A cartridge: its Game Card is gone, by the original's own test, and
    /// nothing changed, or what its write left is settled best by the next
    /// START with the game, whose selection then finds a damaged save (the
    /// original's check, `002cbea0`): the original's advice to check that
    /// the Game Card is inserted and try again. Never for an installed copy,
    /// which has no card to take out: its failure shows the numbers.
    Retry,
    /// A file could not be made for lack of space on the SD card, and
    /// nothing changed.
    NoSpace,
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
    ReadyToWrite,
    /// The save in progress moved Pokémon both ways and its game is not
    /// there: it waits for the game of this title and trainer. Nothing was
    /// written, and a later opening settles it.
    Waiting {
        title: u64,
        trainer: Trainer,
    },
    /// The save in progress moved Pokémon both ways and its game, there and
    /// saved on since, no longer tells which snapshot loses none: it is left
    /// to Transporter's question (`Settled::Asks`); `unlocked` when it was
    /// unlocked, so whatever game was found.
    AskTransporter {
        unlocked: bool,
    },
    /// The unlock is written.
    Unlocked,
    /// The offline boxes are in `ROOM`.
    OfflineLoaded,
    /// What the transfer screen changed is written.
    Committed,
    /// The job could not finish, and this is what it leaves.
    Ended(Ending),
}

/// A game's save and its secure value on the console, as the worker reaches
/// them: `arm::Console` on the console, a simulated game in the tests.
pub trait Game {
    /// The original's check of the prepared game in memory (`002cbea0`),
    /// which its writer thread makes after the write. Nothing is read from
    /// the card.
    fn check(&mut self) -> Result<(), WorkerError>;
    /// The game's save as it is.
    fn inspect(&mut self) -> Result<GameEvidence, WorkerError>;
    /// One read of the game's save: its secure pair, and its image as it is
    /// and with the prepared image laid over it.
    fn images(&mut self) -> Result<(SecureValues, ImageFingerprints), WorkerError>;
    /// The console's secure value of the game (`0016578c`). A failed call
    /// is an error with the console's result as its second number.
    fn platform(&mut self) -> Result<PlatformSecureValue, WorkerError>;
    /// Sets the console's secure value of the game (`0012146c`), with the
    /// same errors.
    fn set_secure(&mut self, value: u64) -> Result<(), WorkerError>;
    /// Whose game it is, as the main thread read it with the original's
    /// calls before the job (`native_game::trainer`); `None` for a job that
    /// was given none.
    fn trainer(&mut self) -> Option<Trainer>;
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
type FilesError<F> = bank_files::Error<<<F as Files>::Storage as Storage>::Error>;
fn files_failed<E>(fault: Fault) -> impl Fn(bank_files::Error<E>) -> Stop<E> {
    move |error| Stop::Files(fault, error)
}
/// Whether the SD card refused a read or write of Bank's files for lack of
/// space. Any other refusal of the card is an error with its numbers: the
/// card is not where a Game Card's advice helps.
fn no_room<E: NoRoom>(error: &bank_files::Error<E>) -> bool {
    error.card().is_some_and(NoRoom::no_room)
}

/// A journal record that settles or marks a save in progress, written or
/// not. At START no room to write it again, where a cut left one of its two
/// copies unusable, leaves the save as it was, for the next START, as "The
/// SD card is full." (`Settled::NoSpace`). After Save and Quit's own write
/// the game has changed, so it is the error screen there.
fn settled<E: NoRoom>(
    rule: Rule,
    result: Result<Settled, bank_files::Error<E>>,
) -> Result<Settled, Stop<E>> {
    match result {
        Err(error) if rule == Rule::Start && no_room(&error) => Ok(Settled::NoSpace),
        result => result.map_err(files_failed(Fault::JournalRecovery)),
    }
}

/// The end of creating a Bank: what `BankFiles::initialize` answered, with a
/// card without room for the files as an ending. A creation that stopped is
/// finished in place by the next one, so nothing is lost by it.
pub fn created<E: NoRoom>(
    result: Result<(), bank_files::Error<E>>,
) -> Result<Option<Ending>, bank_files::Error<E>> {
    match result {
        Ok(()) => Ok(None),
        Err(error) if no_room(&error) => Ok(Some(Ending::NoSpace)),
        Err(error) => Err(error),
    }
}

/// What a failure of the box files of the SD card leaves (`sdfiles::Error`,
/// docs/errors.md): no room, on the SD card or for Bank's files, is "The SD
/// card is full." (`Ending::NoSpace`); any other failure of the card is
/// fault `15` with the console's result, a cell that is not a whole box
/// file `15` with `7B0000pp`; a failure of
/// Bank's files is `files` with its numbers.
pub fn sd_ended<E: NoRoom, S: Store>(
    error: sdfiles::Error<E, S::Error>,
    files: Fault,
) -> Result<Ending, Stop<E>> {
    match error {
        sdfiles::Error::Sd(error) if S::no_room(&error) => Ok(Ending::NoSpace),
        sdfiles::Error::Files(error) if no_room(&error) => Ok(Ending::NoSpace),
        sdfiles::Error::Sd(error) => Err(Fault::SdFiles.with(S::code(&error)).into()),
        sdfiles::Error::Files(error) => Err(Stop::Files(files, error)),
        sdfiles::Error::BadFile(place) => {
            Err(Fault::SdFiles.with(0x7b00_0000 | place as u32).into())
        }
        sdfiles::Error::Staging | sdfiles::Error::NotAsWritten => {
            Err(Fault::SdFiles.with(0).into())
        }
    }
}

/// What Save and Quit's preparation found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// The save did not begin, and nothing of it is left: the journal holds
    /// no save in progress, or one the next start keeps as it was before
    /// (the game was not written).
    Ended(Ending),
    /// The journal holds the save in progress, and the game may be written.
    /// `staged` is the checksum of the body in staging before the files took
    /// it apart, for `finalize` to find in the Bank again.
    Ready { staged: u32 },
}

/// Save and Quit's preparation, with the Bank staged: the prepared game is
/// checked first, then the game's save is read once, before anything is
/// written, for the image as it was loaded and the one about to be written.
/// Only then the journal takes the save in progress, and after it the game is
/// checked again: nothing may have changed it in between.
///
/// Whatever stops it once the journal may hold the save undoes that save
/// again (`undo`): the game is not written before `Ready`, so the save before
/// is the one the game holds. A missing Game Card and a card without room
/// for a file are endings; every other failure is an error.
pub fn prepare<F: Files, G: Game>(
    files: &mut BankFiles<F>,
    game: &mut G,
    staging: &mut [u8],
    baseline: GameEvidence,
    trainer: Trainer,
) -> Stopped<Prepared, F>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    game.check()?;
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
        Err(_) if card_gone(game, baseline.cartridge) => return Ok(Prepared::Ended(Ending::Retry)),
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
    // Clean as the files will take it, so that `finalize` finds it again.
    let body = Cleaned::new(staging).ok_or(Fault::StorageLoad.with(0))?;
    let staged = crc32(&body);
    // Side files first, then the Bank journal. A file is created only
    // before the journal is written, so a card without room for it leaves
    // no save in progress.
    if let Err(error) = files.prepare(body, identity, images.before, images.after) {
        // An undo that cannot be written either leaves the save to the next
        // start with the game, which keeps the save before.
        drop(undo(files));
        return if no_room(&error) {
            Ok(Prepared::Ended(Ending::NoSpace))
        } else {
            Err(Stop::Files(Fault::JournalPrepare, error))
        };
    }
    // No game write is allowed until this post-barrier check ends.
    let mut check = || {
        let unchanged = game.inspect()?;
        if unchanged != baseline {
            return Err(Fault::GameChanged.with(0));
        }
        require_secure(game, unchanged.secure.current)
    };
    if let Err(error) = check() {
        let missing = card_gone(game, baseline.cartridge);
        drop(undo(files));
        return if missing {
            Ok(Prepared::Ended(Ending::Retry))
        } else {
            Err(error.into())
        };
    }
    Ok(Prepared::Ready { staged })
}

/// Undoes a save in progress whose game was not written, as the next start
/// would settle it with that game: the Bank is opened again, since a
/// session that met an error is not used again (`bank_common::session`),
/// and if its journal holds the save, the save before is kept. An error
/// leaves it to the next start.
fn undo<F: Files>(files: &mut BankFiles<F>) -> Result<(), FilesError<F>> {
    drop(files.close());
    if let Some(Phase::Prepared(_)) = files.open()? {
        files.reconcile_as(RecoveryDecision::KeepBefore)?;
    }
    Ok(())
}

/// Which way a save in progress that is neither image is settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// At START: by what it moved (`moved::choose`); one that moved Pokémon
    /// both ways is left to Transporter's question when its game is there
    /// on the kind of copy it was made with (`Settled::Asks`), and waits
    /// for its game otherwise.
    Start,
    /// After its own write: by what it moved; one that moved Pokémon both
    /// ways is left for the next START with its game, and the image written
    /// counts only with the secure pair it was prepared with.
    AfterWrite,
}

/// How `settle` left a save in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    /// The game holds one of its two images in full: undone (`KeepBefore`)
    /// or made current (`CommitAfter`).
    Exact(RecoveryDecision),
    /// The game holds neither, or is not there: settled by what it moved.
    ByMoves(RecoveryDecision),
    /// Not settled: it moved Pokémon both ways, and its game is not there,
    /// or only on the other kind of copy. It waits for its game. At START
    /// its backup is written, and a save left to Transporter's question is
    /// taken back.
    Waits,
    /// Not settled: it moved Pokémon both ways, and its game is there on the
    /// kind of copy it was made with, saved on since, so that it no longer
    /// tells which snapshot loses none. It is left to Transporter's
    /// question, which asks the user (`Metadata::ask`).
    Asks,
    /// Not settled: the game cannot be read, as its Game Card is gone. The
    /// journal still holds it, for the next start.
    Open,
    /// Not settled: it moved Pokémon both ways, and the SD card has no room
    /// for the backup of the boxes it changed (`backup`). It stays in
    /// progress, and the next START tries again.
    NoSpace,
}

/// Settles the save in progress `pending` from the game of its title, or
/// without one (`game` is `None`: the scan did not load it). The table is
/// docs/internals.md's "An interrupted save"; `rule` says how a save that is
/// neither image is settled. `written` is the secure pair the save prepared,
/// for `Rule::AfterWrite`. `held` is the room for comparing the two
/// snapshots of the Bank.
pub fn settle<F: Files, G: Game>(
    files: &mut BankFiles<F>,
    game: Option<&mut G>,
    staging: &mut [u8],
    pending: &PendingTransfer,
    rule: Rule,
    written: Option<SecureValues>,
    held: &mut [[Identity; HELD]; 2],
) -> Stopped<Settled, F>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    settle_backed(files, game, staging, pending, rule, written, held, &mut ())
}

/// What a settle does with the backup of a save that moved Pokémon both
/// ways. Save and Quit's own settle makes none (`()`); START's is `Kept`.
pub trait Backup<F: Files> {
    /// Writes it, before the journal leaves the save to Transporter's
    /// question or to its game. `false` for no room.
    fn write(&mut self, _: &mut BankFiles<F>, _: &mut [u8]) -> Stopped<bool, F> {
        Ok(true)
    }
    /// The game decides the save: the folder goes, before the journal is
    /// settled. `false` for no room for the journal's record.
    fn discard(&mut self, _: &mut BankFiles<F>) -> Stopped<bool, F> {
        Ok(true)
    }
}
impl<F: Files> Backup<F> for () {}

/// START's backup, in the folders of Bank's extdata (`interrupted`).
pub struct Kept<'a, X> {
    ext: &'a mut X,
}
impl<'a, X> Kept<'a, X> {
    pub fn new(ext: &'a mut X) -> Self {
        Self { ext }
    }
}
impl<F: Files, X: Extdata> Backup<F> for Kept<'_, X>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    fn write(&mut self, files: &mut BankFiles<F>, staging: &mut [u8]) -> Stopped<bool, F> {
        match interrupted::back_up(files, staging, self.ext) {
            Ok(()) => {
                self.ext.rest();
                Ok(true)
            }
            Err(error) => sd_ended::<_, X>(error, Fault::JournalRecovery).map(|_| false),
        }
    }
    fn discard(&mut self, files: &mut BankFiles<F>) -> Stopped<bool, F> {
        match interrupted::discard(files, self.ext) {
            Ok(()) => {
                self.ext.rest();
                Ok(true)
            }
            Err(error) => sd_ended::<_, X>(error, Fault::JournalRecovery).map(|_| false),
        }
    }
}

/// `settle`, with what it does with a backup (`Backup`).
#[allow(clippy::too_many_arguments)]
pub fn settle_backed<F: Files, G: Game, B: Backup<F>>(
    files: &mut BankFiles<F>,
    game: Option<&mut G>,
    staging: &mut [u8],
    pending: &PendingTransfer,
    rule: Rule,
    written: Option<SecureValues>,
    held: &mut [[Identity; HELD]; 2],
    backup: &mut B,
) -> Stopped<Settled, F>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    let Some(game) = game else {
        return by_moves(files, staging, rule, false, held, backup);
    };
    let cartridge = made_on_cartridge(pending)?;
    let evidence = match game.inspect() {
        Ok(evidence) => evidence,
        Err(_) if card_gone(game, cartridge) => return Ok(Settled::Open),
        Err(error) => return Err(error.into()),
    };
    let mut matched = matched(pending, &evidence)?;
    // After its own write, the image counts as written only with the new
    // pair the save prepared, which a completed write always has.
    let secure = evidence.secure;
    if rule == Rule::AfterWrite
        && matched == Match::After
        && (written != Some(secure) || secure.current == secure.previous)
    {
        matched = Match::Other(Found::SameKind);
    }
    let secured = match matched {
        Match::Before => require_secure(game, secure.current),
        Match::After => finish_secure(game, secure),
        Match::Other(_) => Ok(()),
    };
    match secured {
        // The card was taken out meanwhile: nothing is decided now.
        Err(_) if card_gone(game, cartridge) => return Ok(Settled::Open),
        secured => secured?,
    }
    match matched {
        // The game decides the save, and nothing of a backup is kept: its
        // folder goes first, so that none stands beside the clean journal.
        Match::Before | Match::After => {
            if !backup.discard(files)? {
                return Ok(Settled::NoSpace);
            }
            settled(
                rule,
                files.reconcile(evidence.fingerprint).map(Settled::Exact),
            )
        }
        // A save that is neither image, on the kind of copy the save in
        // progress was made with: the game was played on, a new game was
        // started on it, or it is another cartridge. It no longer shows
        // whether Bank's write went through. Only the trainer the save was
        // made with is that game played on; another trainer's, and the
        // other kind of copy, which stands in for one that is gone, say
        // nothing about this save: as for a game that is not there. Its
        // secure value is not this save's to judge; a load of that game
        // does.
        Match::Other(found) => {
            let gen7 = GameKind::of_title(pending.game.title_id).is_some_and(GameKind::gen7);
            let seen = rule == Rule::Start
                && found == Found::SameKind
                && game
                    .trainer()
                    .is_some_and(|inserted| Trainer::of(&pending.game).same(inserted, gen7));
            by_moves(files, staging, rule, seen, held, backup)
        }
    }
}

/// Settles the save in progress by what it moved, toward the snapshot that
/// cannot lose a Pokémon: a save that moved Pokémon one way, or none. One
/// that moved them both ways is not settled here. `seen` says that its
/// game is there on the kind of copy it was made with, at START, with a
/// save that is neither image: then it is left to Transporter's question
/// (`Settled::Asks`). Otherwise it waits for its game.
fn by_moves<F: Files, B: Backup<F>>(
    files: &mut BankFiles<F>,
    staging: &mut [u8],
    rule: Rule,
    seen: bool,
    [before, after]: &mut [[Identity; HELD]; 2],
    backup: &mut B,
) -> Stopped<Settled, F>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    let moved = files
        .pending_moves(staging, before, after)
        .map_err(files_failed(Fault::JournalRecovery))?;
    let Some(decision) = moved::choose(moved) else {
        // The boxes it changed go into Bank's extdata, both ways, before
        // the journal says anything about it. A file written for it takes
        // the marks off the journal first (`interrupted::back_up`).
        let unlocked = files
            .unlocked()
            .map_err(files_failed(Fault::JournalRecovery))?;
        if !backup.write(files, staging)? {
            return Ok(Settled::NoSpace);
        }
        // The backup is whole: one record has an unlock stand again and bit
        // 2 go by what was seen; an unlocked save is Transporter's too. No
        // room to write it leaves the save waiting for the next START, as no
        // room for the backup does.
        return settled(
            rule,
            files.mark(seen, unlocked).map(
                |asks| {
                    if asks {
                        Settled::Asks
                    } else {
                        Settled::Waits
                    }
                },
            ),
        );
    };
    settled(rule, files.reconcile_as(decision).map(Settled::ByMoves))
}

/// How Save and Quit ended once the original's writer had ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finalized {
    /// The game holds the new save in full, it is current, and the Bank
    /// read back is the body that was staged.
    Saved,
    /// It is not: what it leaves.
    Ended(Ending),
}

/// Save and Quit after the original's writer ended, whether it wrote the
/// game or failed: the save is settled from what the game holds, at once,
/// as the next start would settle it (`settle`, `Rule::AfterWrite`). Only
/// the game's whole new image, with the secure pair `written`, which is new
/// with every prepared image, makes the save current; the Bank is then read
/// back and must be the body with checksum `staged`, every byte. The old
/// image undoes it; a save that is neither is settled by its moves, and one
/// that moved Pokémon both ways, or a game that cannot be read, is left for
/// the next start.
///
/// What is not saved is the advice to try again for a cartridge: the next
/// START with it settles what is left, and its selection finds a save that
/// the write damaged. An installed copy has no card to take out; its
/// failure is an error with `status`, the writer's result (0 when it
/// reported none).
#[allow(clippy::too_many_arguments)]
pub fn finalize<F: Files, G: Game>(
    files: &mut BankFiles<F>,
    game: &mut G,
    staging: &mut [u8],
    pending: &PendingTransfer,
    written: Option<SecureValues>,
    staged: Option<u32>,
    status: u32,
    held: &mut [[Identity; HELD]; 2],
) -> Stopped<Finalized, F>
where
    <F::Storage as Storage>::Error: NoRoom,
{
    let cartridge = made_on_cartridge(pending)?;
    match settle(
        files,
        Some(game),
        staging,
        pending,
        Rule::AfterWrite,
        written,
        held,
    )? {
        Settled::Exact(RecoveryDecision::CommitAfter) => {
            let staged = staged.ok_or(Fault::StorageLoad.with(0))?;
            files
                .read(staging)
                .map_err(files_failed(Fault::StorageLoad))?;
            if crc32(staging) != staged {
                return Err(Fault::StorageLoad.with(0).into());
            }
            Ok(Finalized::Saved)
        }
        // A card that is gone is only ever a cartridge's (`card_gone`).
        Settled::Open => Ok(Finalized::Ended(Ending::Retry)),
        Settled::Exact(RecoveryDecision::KeepBefore)
        | Settled::ByMoves(_)
        | Settled::Waits
        | Settled::Asks
            if cartridge =>
        {
            Ok(Finalized::Ended(Ending::Retry))
        }
        // `NoSpace` is START's alone: no backup is written after a save's own
        // write, and no room for the journal is an error there (`settled`).
        Settled::Exact(RecoveryDecision::KeepBefore)
        | Settled::ByMoves(_)
        | Settled::Waits
        | Settled::Asks
        | Settled::NoSpace => Err(Fault::GameWrite.with(status).into()),
    }
}

/// Whether the save in progress was made with a cartridge, by the kind of
/// copy its record is bound to (`transaction::bound`). A record bound to
/// neither is fault `B`, as when it is matched with a game.
fn made_on_cartridge(pending: &PendingTransfer) -> Result<bool, WorkerError> {
    transaction::bound(pending)
        .map(|medium| medium == Medium::Cartridge)
        .map_err(|_| Fault::GameChanged.with(0))
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
/// Whether the console cannot find the game, by the original's own test: its
/// game selection asks for the game's secure value (00292d64, 001643dc,
/// 00164370 → 0016578c) and shows message 0x1F, "No Pokémon Game Card
/// detected", when the query fails. An answer that is not what the query
/// gives is not taken for a missing card.
fn query_fails<G: Game>(game: &mut G) -> bool {
    matches!(game.platform(), Err(error) if error.native_result() != 0)
}
/// Whether the Game Card of a `cartridge` is gone (`query_fails`). An
/// installed copy has no card to take out: a failure of it is an error.
fn card_gone<G: Game>(game: &mut G, cartridge: bool) -> bool {
    cartridge && query_fails(game)
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
/// The game a session is loaded with, as it is: its save, with the
/// console's secure value advanced if a save in progress that was settled
/// without the game left it one step behind. A Game Card that is gone is
/// `Ending::Retry`: nothing of the Bank is written here.
///
/// The console's answer about the game comes first, before anything is
/// read: it says whether the copy is a cartridge. A query that fails then
/// is the original's missing card, whatever the copy, as its game selection
/// shows message 0x1F for it; a later failure counts as a card that is gone
/// only for a cartridge.
pub fn load_game<G: Game>(game: &mut G) -> Result<Result<GameEvidence, Ending>, WorkerError> {
    let cartridge = match game.platform() {
        Ok(console) => console.gamecard,
        Err(error) if error.native_result() != 0 => return Ok(Err(Ending::Retry)),
        Err(error) => return Err(error),
    };
    let evidence = match game.inspect() {
        Ok(evidence) => evidence,
        Err(_) if card_gone(game, cartridge) => return Ok(Err(Ending::Retry)),
        Err(error) => return Err(error),
    };
    match finish_secure(game, evidence.secure) {
        Err(_) if card_gone(game, cartridge) => Ok(Err(Ending::Retry)),
        finished => finished.map(|()| Ok(evidence)),
    }
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::native_game::MAX_PREPARED_BLOCKS;
    #[cfg(feature = "test-build")]
    use bank_common::rehearsal::{self, Rehearsal};
    use bank_common::{
        bank_files::ExtFiles,
        fs::{self, GameMainReader},
        program::Bank15,
        sdfiles::{ConsoleExtdata, ConsoleSd},
    };
    use core::mem::transmute;
    use offline_core::{
        game_image::{fingerprint_image, fingerprint_images, Overlay},
        native_blob::{NativeBlobView, BLOB_SIZE},
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
    fn held() -> &'static mut [[Identity; HELD]; 2] {
        // SAFETY: the worker, inside `Recover` or `Finalize`; no other
        // reference exists.
        unsafe { super::ROOM.held() }
    }

    static SHARED: Shared<StorageWorker> = Shared::new([0; BLOB_SIZE]);
    /// The worker of the offline patch.
    pub(crate) type BankWorker = Worker<StorageWorker>;
    /// The worker and the job out, for the task that submitted it.
    pub(crate) type BankRequests = patch_common::worker::Requests<StorageWorker, usize>;

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
            if self.poisoned {
                return Err(Fault::StorageInvalid.with(0));
            }
            // Between two jobs nothing of the Bank is open either: the
            // screens between them let HOME through, and the HOME Menu
            // fails while Bank holds its archive open after a game ran
            // before it. The next job opens what it needs again.
            let result = self.execute(job, staging);
            let rested = self.files.rest().map_err(|e| Fault::StorageOpen.files(e));
            let result = result.and_then(|reply| rested.map(|()| reply));
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
        /// Every folder of an interrupted write in Bank's extdata is settled
        /// by the state the journal names (`interrupted::settle`) whenever
        /// the journal has become clean, once the Bank it names was read
        /// whole: at an opening, and after START settled a save in progress.
        /// So every session that reaches the transfer screen or a save starts
        /// with none, and a Bank that cannot be read stops the opening before
        /// any file of a folder goes. It works in `ROOM`, as staging holds
        /// the Bank. No room on the SD card to finish a cell or for what
        /// is kept is an ending, "The SD card is full.": the folder stays,
        /// and the next START settles it.
        fn settle_sd(&mut self, date: Date) -> Result<Option<Ending>, WorkerError> {
            let mut sd = unsafe { ConsoleSd::<Bank15>::new() };
            let mut ext = unsafe { ConsoleExtdata::<Bank15>::new() };
            // SAFETY: inside `Open` or `Recover`, the held lists done with.
            let work = unsafe { ROOM.work() };
            match interrupted::settle(&mut self.files, &mut sd, &mut ext, work, date) {
                Ok(()) => Ok(None),
                Err(error) => sd_ended::<_, ConsoleSd<Bank15>>(error, Fault::JournalPrepare)
                    .map(Some)
                    .map_err(stopped),
            }
        }
        /// Leaves the assembled native body in staging.
        fn read_bank(&mut self, staging: &mut [u8]) -> Result<bank_files::Loaded, WorkerError> {
            self.files
                .read(staging)
                .map_err(|e| Fault::StorageLoad.files(e))
        }
        fn execute(&mut self, job: Job, staging: &mut [u8]) -> Result<Reply, WorkerError> {
            match job {
                Job::Open { session, date } => {
                    // Every session opens the Bank as a first one does:
                    // what the session before read of it is dropped. None
                    // of its files is open (`rest`).
                    drop(self.files.close());
                    self.missing = false;
                    self.written = None;
                    self.staged = None;
                    if session == 0 {
                        // Bank's own FS session is not initialized yet.
                        return Err(Fault::StorageOpen.with(0x7800_0000));
                    }
                    self.fs_session = session;
                    self.files.files_mut().session = session;
                    let opened = self.files.open().map_err(|error| match error {
                        bank_files::Error::Io(error) => Fault::StorageOpen.io(error),
                        error => Fault::StorageInvalid.files(error),
                    })?;
                    let Some(phase) = opened else {
                        self.missing = true;
                        return Ok(Reply::Missing);
                    };
                    // The Bank first, read whole, then the SD card's folders:
                    // the SD card's archive is never open while it is read.
                    let loaded = match phase {
                        Phase::Clean(_) => Some(self.read_bank(staging)?),
                        Phase::Prepared(_) => None,
                    };
                    if loaded.is_some() {
                        if let Some(ending) = self.settle_sd(date)? {
                            return Ok(Reply::Ended(ending));
                        }
                    }
                    // Test builds: every call of the SD card's box files
                    // once, on a file of its own (docs/testing.md).
                    #[cfg(feature = "test-build")]
                    {
                        // A card without room for the probe's file is no
                        // fault of the card's calls: the probe is skipped,
                        // so the checks of a full card can be made with
                        // this build too.
                        let mut sd = unsafe { ConsoleSd::<Bank15>::new() };
                        match bank_common::sdfiles::probe(&mut sd) {
                            Ok(true) => {}
                            Ok(false) => return Err(Fault::SdFiles.with(0)),
                            Err(error) if ConsoleSd::<Bank15>::no_room(&error) => {}
                            Err(error) => {
                                return Err(Fault::SdFiles.with(ConsoleSd::<Bank15>::code(&error)))
                            }
                        }
                        // The same for the folders of Bank's extdata.
                        let mut ext = unsafe { ConsoleExtdata::<Bank15>::new() };
                        match bank_common::sdfiles::probe_extdata(&mut ext) {
                            Ok(true) => {}
                            Ok(false) => return Err(Fault::SdFiles.with(0)),
                            Err(error) if ConsoleExtdata::<Bank15>::no_room(&error) => {}
                            Err(error) => {
                                return Err(
                                    Fault::SdFiles.with(ConsoleExtdata::<Bank15>::code(&error))
                                )
                            }
                        }
                    }
                    Ok(Reply::Opened { phase, loaded })
                }
                Job::Initialize => {
                    if !self.missing || self.files.is_open() || self.fs_session == 0 {
                        return Err(Fault::StorageCreate.with(0));
                    }
                    // Refuse an invalid native body before creating any file.
                    NativeBlobView::parse_layout(staging)
                        .map_err(|_| Fault::NativeObject.with(0))?;
                    let body = Cleaned::new(staging).ok_or(Fault::NativeObject.with(0))?;
                    if let Some(ending) = created(self.files.initialize(body))
                        .map_err(|e| Fault::StorageCreate.files(e))?
                    {
                        return Ok(Reply::Ended(ending));
                    }
                    self.missing = false;
                    Ok(Reply::BankReady { loaded: None })
                }
                Job::Detect { game } => {
                    // A reply, not an error: the worker stays usable for
                    // the next session. Nothing of the save is read yet.
                    // Only a cartridge's is an ending (`save_steps`).
                    if query_fails(&mut Console::of(game)) {
                        Ok(Reply::Ended(Ending::Retry))
                    } else {
                        Ok(Reply::Detected)
                    }
                }
                Job::LoadGame { game } => {
                    // An opening settles a save in progress or ends there;
                    // no game is loaded beside one.
                    if !matches!(self.phase()?, Phase::Clean(_)) {
                        return Err(Fault::JournalRecovery.with(0));
                    }
                    Ok(match load_game(&mut Console::of(game))? {
                        Ok(evidence) => Reply::Inspected(evidence),
                        Err(ending) => Reply::Ended(ending),
                    })
                }
                Job::Recover { game, date } => {
                    let Phase::Prepared(pending) = self.phase()? else {
                        return Err(Fault::JournalRecovery.with(0));
                    };
                    if game.is_none() {
                        // No game shows which kind of copy the save was
                        // made with; its binding must still be this
                        // version's, on one kind or the other.
                        transaction::bound(&pending).map_err(|_| Fault::GameChanged.with(0))?;
                    }
                    let mut game = game.map(|(game, trainer)| Console {
                        trainer: Some(trainer),
                        ..Console::of(game)
                    });
                    // The worker owns the program's file handles, so it
                    // reaches the folder of the backup too.
                    let mut ext = unsafe { ConsoleExtdata::<Bank15>::new() };
                    let settled = settle_backed(
                        &mut self.files,
                        game.as_mut(),
                        staging,
                        &pending,
                        Rule::Start,
                        None,
                        held(),
                        &mut Kept::new(&mut ext),
                    )
                    .map_err(stopped)?;
                    match settled {
                        Settled::Exact(_) | Settled::ByMoves(_) => {
                            drop(ext);
                            let loaded = self.read_bank(staging)?;
                            if let Some(ending) = self.settle_sd(date)? {
                                return Ok(Reply::Ended(ending));
                            }
                            Ok(Reply::BankReady {
                                loaded: Some(loaded),
                            })
                        }
                        Settled::Waits => Ok(Reply::Waiting {
                            title: pending.game.title_id,
                            trainer: Trainer::of(&pending.game),
                        }),
                        Settled::Asks => Ok(Reply::AskTransporter {
                            unlocked: self
                                .files
                                .unlocked()
                                .map_err(|e| Fault::JournalRecovery.files(e))?,
                        }),
                        Settled::Open => Ok(Reply::Ended(Ending::Retry)),
                        Settled::NoSpace => Ok(Reply::Ended(Ending::NoSpace)),
                    }
                }
                Job::LoadOffline => {
                    let mut sd = unsafe { ConsoleSd::<Bank15>::new() };
                    let (offline, _) = unsafe { ROOM.boxes() };
                    match transfer_screen::load_offline(&mut sd, offline) {
                        Ok(()) => Ok(Reply::OfflineLoaded),
                        Err(error) => {
                            sd_ended::<_, ConsoleSd<Bank15>>(error, Fault::JournalPrepare)
                                .map(Reply::Ended)
                                .map_err(stopped)
                        }
                    }
                }
                Job::TransferCommit { checked, date } => {
                    let mut sd = unsafe { ConsoleSd::<Bank15>::new() };
                    let mut ext = unsafe { ConsoleExtdata::<Bank15>::new() };
                    let (offline, scratch) = unsafe { ROOM.boxes() };
                    let request = transfer_screen::Commit {
                        checked: &checked,
                        date,
                    };
                    match transfer_screen::commit(
                        &mut self.files,
                        &mut sd,
                        &mut ext,
                        staging,
                        offline,
                        &request,
                        scratch,
                    ) {
                        Ok(()) => Ok(Reply::Committed),
                        Err(error) => {
                            sd_ended::<_, ConsoleSd<Bank15>>(error, Fault::JournalPrepare)
                                .map(Reply::Ended)
                                .map_err(stopped)
                        }
                    }
                }
                Job::Unlock => {
                    let Phase::Prepared(_) = self.phase()? else {
                        return Err(Fault::JournalRecovery.with(0));
                    };
                    match self.files.unlock() {
                        Ok(()) => Ok(Reply::Unlocked),
                        Err(error) if no_room(&error) => Ok(Reply::Ended(Ending::NoSpace)),
                        Err(error) => Err(Fault::JournalRecovery.files(error)),
                    }
                }
                Job::Finalize { game, status } => {
                    let Phase::Prepared(pending) = self.phase()? else {
                        return Err(Fault::JournalRecovery.with(0));
                    };
                    #[cfg(feature = "test-build")]
                    if rehearsal::chosen() == Rehearsal::StopAfterGame {
                        return Err(Fault::TestStop.with(2));
                    }
                    let (written, staged) = (self.written.take(), self.staged.take());
                    let finalized = finalize(
                        &mut self.files,
                        &mut Console::of(game),
                        staging,
                        &pending,
                        written,
                        staged,
                        status,
                        held(),
                    )
                    .map_err(stopped)?;
                    Ok(match finalized {
                        Finalized::Saved => Reply::Saved,
                        Finalized::Ended(ending) => Reply::Ended(ending),
                    })
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
                        prepared: Some(&prepared),
                        ..Console::of(game)
                    };
                    let prepared_game =
                        prepare(&mut self.files, &mut console, staging, baseline, trainer)
                            .map_err(stopped)?;
                    let staged = match prepared_game {
                        Prepared::Ready { staged } => staged,
                        Prepared::Ended(ending) => return Ok(Reply::Ended(ending)),
                    };
                    #[cfg(feature = "test-build")]
                    if rehearsal::chosen() == Rehearsal::StopBeforeGame {
                        return Err(Fault::TestStop.with(1));
                    }
                    self.staged = Some(staged);
                    self.written = Some(prepared.secure);
                    Ok(Reply::ReadyToWrite)
                }
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
        /// Whose game it is, for `Job::Recover`.
        trainer: Option<Trainer>,
    }
    impl Console<'_> {
        fn of(game: GameIoDescriptor) -> Self {
            Self {
                game,
                prepared: None,
                trainer: None,
            }
        }
    }
    impl Game for Console<'_> {
        fn trainer(&mut self) -> Option<Trainer> {
            self.trainer
        }
        fn check(&mut self) -> Result<(), WorkerError> {
            let prepared = self.prepared.ok_or(Fault::GamePrepare.with(0))?;
            // The unsafe submit contract keeps the game frozen and live, and
            // read by nothing else, until this worker publishes completion.
            unsafe { prepared.check() }.map_err(|_| Fault::GamePrepare.with(0))
        }
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
pub(crate) use arm::{BankRequests, BankWorker};

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use bank_common::{
        bank_files::FileName,
        testing::{
            asked, body, fresh, load, opened, pokemon, transport_check, unlocked, with_boxes,
            ExtCard, Refused, Shared,
        },
    };
    use offline_core::sections::RECORD_SIZE;
    use std::{boxed::Box, collections::BTreeMap, vec::Vec};

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
        /// The prepared game passes the original's check.
        valid: bool,
        /// Whose game it is.
        trainer: Trainer,
    }
    impl Simulated {
        fn loaded(cartridge: bool) -> Self {
            Self {
                save: Some((BEFORE, LOADED)),
                after: AFTER,
                cartridge,
                value: LOADED.current,
                out_after: None,
                valid: true,
                trainer: trainer(),
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
        fn check(&mut self) -> Result<(), WorkerError> {
            if self.valid {
                Ok(())
            } else {
                Err(Fault::GamePrepare.with(0))
            }
        }
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
        fn trainer(&mut self) -> Option<Trainer> {
            Some(self.trainer)
        }
    }

    /// Trainer ID 12345 and secret ID 7, of X.
    fn trainer() -> Trainer {
        Trainer {
            name: [0x53; crate::transaction::NAME],
            id: 7 << 16 | 12345,
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
    fn held() -> Box<[[Identity; HELD]; 2]> {
        Box::new([[[0; 3]; HELD]; 2])
    }
    /// A Bank on the card, and the body Save and Quit stages for it.
    fn bank() -> (Shared, BankFiles<Shared>, Vec<u8>) {
        let disk = fresh(&body(0, 0, 0));
        let mut files = opened(&disk);
        load(&mut files);
        (disk, files, body(0x52, 0x66, 1))
    }
    /// A Bank of `old` on the card, loaded, and the session's `new` staged
    /// and prepared with `game`: the save is in progress.
    fn prepared_save(
        old: &[u8],
        new: &[u8],
        game: &mut Simulated,
    ) -> (Shared, BankFiles<Shared>, PendingTransfer, u32) {
        let disk = fresh(old);
        let mut files = opened(&disk);
        load(&mut files);
        let baseline = game.evidence();
        let mut staging = new.to_vec();
        let Ok(Prepared::Ready { staged }) =
            prepare(&mut files, game, &mut staging, baseline, trainer())
        else {
            panic!("not prepared");
        };
        let pending = pending(&mut files).unwrap();
        (disk, files, pending, staged)
    }
    /// The Bank the next opening finds once the save in progress is settled.
    fn settled_bank(disk: &Shared) -> Vec<u8> {
        let mut files = opened(disk);
        assert_eq!(pending(&mut files), None);
        load(&mut files).0
    }
    fn fault<T: core::fmt::Debug, E: core::fmt::Debug>(result: Result<T, Stop<E>>) -> Fault {
        match result.unwrap_err() {
            Stop::Worker(error) => error.fault(),
            Stop::Files(fault, _) => fault,
        }
    }
    /// The writer's result `finalize` is handed for a write that failed.
    const STATUS: u32 = 0xc8a0_4554;
    /// Save and Quit's end after a writer that failed with `STATUS`: what it
    /// left, or the fault and second number of its error.
    fn finish(
        files: &mut BankFiles<Shared>,
        game: &mut Simulated,
        pending: &PendingTransfer,
        staged: u32,
    ) -> Result<Finalized, (Fault, u32)> {
        let mut staging = std::vec![0; offline_core::native_blob::BLOB_SIZE];
        finalize(
            files,
            game,
            &mut staging,
            pending,
            Some(WRITTEN),
            Some(staged),
            STATUS,
            &mut held(),
        )
        .map_err(|stop| match stop {
            Stop::Worker(error) => (error.fault(), error.native_result()),
            Stop::Files(fault, _) => (fault, 0),
        })
    }

    #[test]
    fn a_save_commits_and_the_bank_read_back_is_the_body_it_staged() {
        for cartridge in [true, false] {
            let (_, mut files, mut body) = bank();
            // A byte a write clears, as the box screen's moves leave some:
            // the save cleans before it counts, and the Bank read back is
            // the cleaned body.
            body[0x17C + 30 * 0xE8 + 0x22] = 0x77;
            let mut staging = body.clone();
            offline_core::sections::clean(&mut body).unwrap();
            let mut game = Simulated::loaded(cartridge);
            let baseline = game.evidence();
            let staged = match prepare(&mut files, &mut game, &mut staging, baseline, trainer()) {
                Ok(Prepared::Ready { staged }) => staged,
                other => panic!("{other:?}"),
            };
            let in_progress = pending(&mut files).unwrap();
            game.write(WRITTEN);
            let finalized = finalize(
                &mut files,
                &mut game,
                &mut staging,
                &in_progress,
                Some(WRITTEN),
                Some(staged),
                0,
                &mut held(),
            );
            assert_eq!(finalized.unwrap(), Finalized::Saved);
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
            assert_eq!(result.unwrap(), Prepared::Ended(Ending::Retry), "{calls}");
            assert!(stored(&disk) == before, "{calls}");
            assert!(pending(&mut files).is_none());
        }
        // An installed copy has no card to take out: its failure is an
        // error, with nothing written.
        let mut game = Simulated::loaded(false);
        let baseline = game.evidence();
        game.out_after = Some(0);
        let before = stored(&disk);
        let mut staging = body.clone();
        let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
        assert_eq!(fault(result), Fault::GameRead);
        assert!(stored(&disk) == before);
        assert!(pending(&mut files).is_none());
    }

    #[test]
    fn a_game_card_taken_out_after_the_journal_undoes_the_save_it_did_not_write() {
        let (disk, mut files, body) = bank();
        let old = load(&mut files).0;
        let mut game = Simulated::loaded(true);
        let baseline = game.evidence();
        // The read and its secure value, then out for the check after it.
        game.out_after = Some(2);
        let mut staging = body.clone();
        let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
        assert_eq!(result.unwrap(), Prepared::Ended(Ending::Retry));
        // The game was not written: the save before is kept at once.
        assert!(pending(&mut files).is_none());
        drop(files);
        assert!(settled_bank(&disk) == old);
    }

    #[test]
    fn a_card_without_room_for_a_file_ends_the_save_with_nothing_in_progress() {
        let (disk, mut files, body) = bank();
        let old = load(&mut files).0;
        // The transport box's files are gone, and the card has no room to
        // make them again.
        {
            let mut state = disk.0.borrow_mut();
            for unit in 0..FileName::Transport.units().len() {
                state.absent.insert((FileName::Transport as u8, unit));
            }
            state.full = true;
        }
        let mut game = Simulated::loaded(true);
        let baseline = game.evidence();
        let mut staging = body.clone();
        let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
        assert_eq!(result.unwrap(), Prepared::Ended(Ending::NoSpace));
        assert!(pending(&mut files).is_none());
        drop(files);
        disk.0.borrow_mut().full = false;
        assert!(settled_bank(&disk) == old);
    }

    #[test]
    fn a_failed_write_before_the_game_is_written_leaves_the_save_before() {
        let (base, mut files, body) = bank();
        let old = load(&mut files).0;
        drop(files);
        let mut budget = 0;
        loop {
            let disk = base.cut_after(budget);
            let mut files = opened(&disk);
            load(&mut files);
            let mut game = Simulated::loaded(true);
            let baseline = game.evidence();
            let mut staging = body.clone();
            let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
            // The card refuses the write: an error, with its numbers.
            match result {
                Ok(Prepared::Ready { .. }) => break,
                Err(Stop::Files(Fault::JournalPrepare, _)) => {}
                other => panic!("cut after {budget}: {other:?}"),
            }
            // The next start with the game, which was not written, keeps
            // the save before, whether or not the undo could be written.
            drop(files);
            let disk = disk.reboot();
            let mut files = opened(&disk);
            if let Some(in_progress) = pending(&mut files) {
                let settled = settle(
                    &mut files,
                    Some(&mut game),
                    &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                    &in_progress,
                    Rule::Start,
                    None,
                    &mut held(),
                );
                assert_eq!(
                    settled.unwrap(),
                    Settled::Exact(RecoveryDecision::KeepBefore)
                );
            }
            drop(files);
            assert!(settled_bank(&disk) == old, "cut after {budget}");
            budget += 1;
        }
        assert!(budget > 4);
    }

    #[test]
    fn a_prepared_game_that_fails_its_check_is_refused_before_anything_is_written() {
        let (disk, mut files, body) = bank();
        let mut game = Simulated::loaded(true);
        let baseline = game.evidence();
        game.valid = false;
        let before = stored(&disk);
        let mut staging = body.clone();
        let result = prepare(&mut files, &mut game, &mut staging, baseline, trainer());
        assert_eq!(fault(result), Fault::GamePrepare);
        assert!(stored(&disk) == before);
        assert!(pending(&mut files).is_none());
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

    /// A Bank with two Pokémon in its first box, and what a session made
    /// of it: deposited one, withdrew one, both, or moved none between the
    /// game and the Bank.
    fn sessions() -> (Vec<u8>, [(&'static str, Vec<u8>); 4]) {
        let one = |id| (0, id as usize, pokemon(id));
        let old = with_boxes(0x31, &[one(1), one(2)], 0);
        let moved = |boxed: &[(usize, usize, [u8; RECORD_SIZE])]| with_boxes(0x52, boxed, 0);
        (
            old,
            [
                ("deposited", moved(&[one(1), one(2), one(3)])),
                ("withdrawn", moved(&[one(1)])),
                ("none", moved(&[one(2), one(1)])),
                ("both ways", moved(&[one(1), one(3)])),
            ],
        )
    }

    #[test]
    fn after_its_write_a_save_is_settled_at_once_by_what_the_game_holds() {
        let (old, sessions) = sessions();
        for cartridge in [true, false] {
            // What is not saved: the advice to try again for a cartridge,
            // whose next START settles what is left; an installed copy's
            // error with the writer's result.
            let unsaved = if cartridge {
                Ok(Finalized::Ended(Ending::Retry))
            } else {
                Err((Fault::GameWrite, STATUS))
            };
            for (moved, new) in &sessions {
                let case = std::format!("{moved}, cartridge {cartridge}");
                // The writer failed and left the game as it was: undone,
                // and nothing changed.
                let mut game = Simulated::loaded(cartridge);
                let (disk, mut files, in_progress, staged) = prepared_save(&old, new, &mut game);
                let ending = finish(&mut files, &mut game, &in_progress, staged);
                assert_eq!(ending, unsaved, "{case}");
                drop(files);
                assert!(settled_bank(&disk) == old, "{case}");

                // The writer reported a failure but wrote the whole new
                // image: the save is current.
                let mut game = Simulated::loaded(cartridge);
                let (disk, mut files, in_progress, staged) = prepared_save(&old, new, &mut game);
                game.write(WRITTEN);
                let ending = finish(&mut files, &mut game, &in_progress, staged);
                assert_eq!(ending, Ok(Finalized::Saved), "{case}");
                drop(files);
                assert!(settled_bank(&disk) == *new, "{case}");

                // Neither image: by what the session moved, at once, unless
                // it moved Pokémon both ways, which waits for the next START
                // with the game.
                let mut game = Simulated::loaded(cartridge);
                let (disk, mut files, in_progress, staged) = prepared_save(&old, new, &mut game);
                game.save = Some((OTHER, WRITTEN));
                let ending = finish(&mut files, &mut game, &in_progress, staged);
                assert_eq!(ending, unsaved, "{case}");
                if *moved == "both ways" {
                    assert_eq!(pending(&mut files), Some(in_progress), "{case}");
                } else {
                    drop(files);
                    let kept = if *moved == "deposited" { new } else { &old };
                    assert!(settled_bank(&disk) == *kept, "{case}");
                }
            }
        }
        // A cartridge whose card is out: nothing is decided, for the next
        // START.
        for (moved, new) in &sessions {
            let mut game = Simulated::loaded(true);
            let (_, mut files, in_progress, staged) = prepared_save(&old, new, &mut game);
            game.write(WRITTEN);
            game.save = None;
            let ending = finish(&mut files, &mut game, &in_progress, staged);
            assert_eq!(ending, Ok(Finalized::Ended(Ending::Retry)), "{moved}");
            assert_eq!(pending(&mut files), Some(in_progress), "{moved}");
        }
        // An installed copy that cannot be read is an error, with the
        // save left for the next START.
        let mut game = Simulated::loaded(false);
        let (_, mut files, in_progress, staged) = prepared_save(&old, &sessions[0].1, &mut game);
        game.save = None;
        let ending = finish(&mut files, &mut game, &in_progress, staged);
        assert_eq!(ending, Err((Fault::GameRead, NO_CARD)));
        assert_eq!(pending(&mut files), Some(in_progress));
    }

    #[test]
    fn the_image_written_counts_only_with_the_pair_its_save_prepared() {
        let (old, sessions) = sessions();
        let new = &sessions[0].1;
        // The image, but not the pair this save prepared, is neither image.
        let mut game = Simulated::loaded(true);
        let (disk, mut files, in_progress, staged) = prepared_save(&old, new, &mut game);
        game.save = Some((AFTER, LOADED));
        let ending = finish(&mut files, &mut game, &in_progress, staged);
        assert_eq!(ending, Ok(Finalized::Ended(Ending::Retry)));
        drop(files);
        // It deposited: the new boxes, which cannot lose the Pokémon.
        assert!(settled_bank(&disk) == *new);
    }

    #[test]
    fn a_write_that_fails_while_the_save_is_settled_is_an_error_and_leaves_it_for_the_next_start() {
        let (old, sessions) = sessions();
        let new = &sessions[0].1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, staged) = prepared_save(&old, new, &mut game);
        drop(files);
        game.write(WRITTEN);
        // A cut at every step of settling it: done, or an error with the
        // save as it was, for the next start.
        for budget in 0.. {
            let card = disk.cut_after(budget);
            let mut files = opened(&card);
            let mut game = Simulated::loaded(true);
            game.write(WRITTEN);
            let finalized = finalize(
                &mut files,
                &mut game,
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Some(WRITTEN),
                Some(staged),
                0,
                &mut held(),
            );
            drop(files);
            let card = card.reboot();
            let mut files = opened(&card);
            match finalized {
                Ok(Finalized::Saved) => {
                    assert_eq!(pending(&mut files), None);
                    break;
                }
                Err(Stop::Files(Fault::JournalRecovery | Fault::StorageLoad, _)) => {
                    // The next start finds the new image and commits.
                    if let Some(in_progress) = pending(&mut files) {
                        let settled = settle(
                            &mut files,
                            Some(&mut game),
                            &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                            &in_progress,
                            Rule::Start,
                            None,
                            &mut held(),
                        );
                        assert_eq!(
                            settled.unwrap(),
                            Settled::Exact(RecoveryDecision::CommitAfter)
                        );
                    }
                    drop(files);
                    assert!(settled_bank(&card) == *new, "cut after {budget}");
                }
                other => panic!("cut after {budget}: {other:?}"),
            }
        }
    }

    #[test]
    fn at_start_a_game_saved_on_since_leaves_a_save_that_moved_both_ways_to_transporter() {
        let (old, sessions) = sessions();
        fn start(
            files: &mut BankFiles<Shared>,
            game: Option<&mut Simulated>,
            pending: &PendingTransfer,
        ) -> Settled {
            settle(
                files,
                game,
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                pending,
                Rule::Start,
                None,
                &mut held(),
            )
            .unwrap()
        }
        for cartridge in [true, false] {
            for (moved, new) in &sessions {
                let case = std::format!("{moved}, cartridge {cartridge}");
                // The game of the kind of copy the save was made with,
                // saved on since: neither image.
                let mut game = Simulated::loaded(cartridge);
                let (disk, mut files, in_progress, _) = prepared_save(&old, new, &mut game);
                game.save = Some((OTHER, LOADED));
                let settled = start(&mut files, Some(&mut game), &in_progress);
                if *moved != "both ways" {
                    // One way, or none: the snapshot that cannot lose.
                    let kept = if *moved == "deposited" { new } else { &old };
                    drop(files);
                    assert!(settled_bank(&disk) == *kept, "{case}");
                    assert!(!asked(&disk), "{case}");
                    continue;
                }
                // Both ways: left to Transporter's question, nothing else
                // written.
                assert_eq!(settled, Settled::Asks, "{case}");
                assert_eq!(pending(&mut files), Some(in_progress), "{case}");
                drop(files);
                assert!(asked(&disk), "{case}");
                // The next START with the same game asks again, and writes
                // nothing more.
                let mut files = opened(&disk);
                let before = stored(&disk);
                assert_eq!(
                    start(&mut files, Some(&mut game), &in_progress),
                    Settled::Asks,
                    "{case}"
                );
                assert_eq!(pending(&mut files), Some(in_progress), "{case}");
                drop(files);
                assert!(stored(&disk) == before, "{case}");
                // Without its game, with only the other kind of copy, or with
                // another trainer's game of the same kind, also one that
                // differs in its secret ID alone: it waits for its game
                // again, and Transporter's question is taken back.
                let with_id = |id| Simulated {
                    trainer: Trainer { id, ..trainer() },
                    ..Simulated::loaded(cartridge)
                };
                for other in [
                    None,
                    Some(Simulated::loaded(!cartridge)),
                    Some(with_id(trainer().id + 1)),
                    Some(with_id(trainer().id ^ 1 << 16)),
                ] {
                    let mut other = other.map(|mut game| {
                        game.save = Some((OTHER, LOADED));
                        game
                    });
                    let mut files = opened(&disk);
                    assert_eq!(
                        start(&mut files, other.as_mut(), &in_progress),
                        Settled::Waits,
                        "{case}"
                    );
                    assert_eq!(pending(&mut files), Some(in_progress), "{case}");
                    drop(files);
                    assert!(!asked(&disk), "{case}");
                    // Asked again by the next START with the game.
                    let mut files = opened(&disk);
                    start(&mut files, Some(&mut game), &in_progress);
                    drop(files);
                    assert!(asked(&disk), "{case}");
                }
                // The game back with the image it was saved from: settled.
                game.save = Some((BEFORE, LOADED));
                let mut files = opened(&disk);
                assert_eq!(
                    start(&mut files, Some(&mut game), &in_progress),
                    Settled::Exact(RecoveryDecision::KeepBefore),
                    "{case}"
                );
                drop(files);
                assert!(settled_bank(&disk) == old, "{case}");
                assert!(!asked(&disk), "{case}");
            }
        }
    }

    #[test]
    fn the_backup_keeps_a_box_whose_pokemon_changed_and_no_other() {
        // Both ways in box 1. Box 8 is renamed, box 9 only moved to another
        // category and position, each with the same Pokémon on both sides:
        // no change. Box 1 is renamed too, and its file carries the name.
        let one = |id| (0, id as usize, pokemon(id));
        let old = &mut with_boxes(
            0x31,
            &[one(1), one(2), (7, 0, pokemon(4)), (8, 0, pokemon(5))],
            0,
        );
        let new = &mut with_boxes(
            0x31,
            &[one(1), one(3), (7, 0, pokemon(4)), (8, 0, pokemon(5))],
            0,
        );
        let label = |index: usize| 0x17C + index * 0x1B56 + 0x1B30;
        new[label(7)] = b'Z';
        new[label(0)] = b'Y';
        new[label(8) + 0x23] ^= 1;
        new[label(8) + 0x24] ^= 1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        let card = disk.like(true).copy();
        assert_eq!(
            start_backed(&card, &mut game, &in_progress).unwrap(),
            Settled::Asks
        );
        let expected = [(0, false), (0, true)];
        assert!(backup_complete(&card, &in_progress, &expected));
        // Of the label only the name's text.
        let folder = backup_folder(&in_progress);
        let file = backup_box(&card, &folder, 0, true).unwrap();
        let file = offline_core::boxfile::BoxFile::parse(&file).unwrap();
        assert_eq!(
            file.name(),
            &new[label(0)..label(0) + offline_core::boxfile::NAME_TEXT]
        );
    }

    /// START with the SD card, for the backup (`settle_backed`).
    fn start_backed(
        card: &Shared,
        game: &mut Simulated,
        pending: &PendingTransfer,
    ) -> Result<Settled, Stop<bank_common::testing::Refused>> {
        let mut files = opened(card);
        let mut sd = ExtCard(card.clone());
        settle_backed(
            &mut files,
            Some(game),
            &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
            pending,
            Rule::Start,
            None,
            &mut held(),
            &mut Kept::new(&mut sd),
        )
    }

    /// The records of box `bit` of a native body, bit 100 the transport box.
    fn records_of(body: &[u8], bit: usize) -> Vec<u8> {
        if bit == 100 {
            return body[offline_core::sections::TRANSPORT_RECORDS].to_vec();
        }
        let at = 0x17C + bit * 0x1B56;
        body[at..at + 30 * 0xE8].to_vec()
    }

    /// The files of the backup of a save from `old` to `new`, as bit and
    /// half (`true` the save after): both sides of each box that differs.
    fn backup_files(old: &[u8], new: &[u8]) -> Vec<(usize, bool)> {
        (0..=100)
            .filter(|bit| records_of(old, *bit) != records_of(new, *bit))
            .flat_map(|bit| [(bit, false), (bit, true)])
            .collect()
    }

    /// The place `bit` of the backup in `folder`, of the save after or
    /// before, if the file reaches it and the console reads it.
    fn backup_box(
        card: &Shared,
        folder: &bank_common::sdfiles::Path,
        bit: usize,
        after: bool,
    ) -> Option<std::vec::Vec<u8>> {
        let (path, offset) = bank_common::interrupted::file(folder, bit, after);
        let disk = card.0.borrow();
        if disk.ext_damaged.contains(path.text()) {
            return None;
        }
        let start = offset as usize;
        disk.ext
            .get(path.text())?
            .get(start..start + offline_core::boxfile::FILE_SIZE)
            .map(<[u8]>::to_vec)
    }

    /// Whether the backup folder of `pending` is whole: it holds a box, and
    /// exactly `files`, each a box file that reads.
    fn backup_complete(card: &Shared, pending: &PendingTransfer, files: &[(usize, bool)]) -> bool {
        let folder = backup_folder(pending);
        let whole = |bit, after| {
            backup_box(card, &folder, bit, after)
                .is_some_and(|file| offline_core::boxfile::BoxFile::parse(&file).is_ok())
        };
        !files.is_empty()
            && (0..=100).all(|bit| {
                [false, true]
                    .into_iter()
                    .all(|after| whole(bit, after) == files.contains(&(bit, after)))
            })
    }

    #[test]
    fn the_boxes_a_save_changed_both_ways_are_kept_before_transporter_is_asked() {
        use bank_common::interrupted;
        let (_, sessions) = sessions();
        let (moved, _) = &sessions[3];
        assert_eq!(*moved, "both ways");
        // Both ways, and the transport box changed too. The transport box
        // held no Pokémon before, and box 6 holds none after: each side is
        // written all the same.
        let one = |id| (0, id as usize, pokemon(id));
        let old = &with_boxes(0x31, &[one(1), one(2), (5, 0, pokemon(9))], 0);
        let new = &with_boxes(0x52, &[one(1), one(3)], 1);
        let expected = backup_files(old, new);
        assert_eq!(
            expected,
            [
                (0, false),
                (0, true),
                (5, false),
                (5, true),
                (100, false),
                (100, true)
            ]
        );
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        let disk = disk.like(true);
        // A cut at every write of START: whenever the journal leaves the
        // save to the question, its backup is whole.
        for budget in 0.. {
            let card = disk.cut_after(budget);
            let result = start_backed(&card, &mut game, &in_progress);
            let card = card.reboot();
            if asked(&card) {
                assert!(
                    backup_complete(&card, &in_progress, &expected),
                    "cut after {budget}"
                );
            }
            // And the next START finishes what the cut left.
            assert_eq!(
                start_backed(&card, &mut game, &in_progress).unwrap(),
                Settled::Asks,
                "cut after {budget}"
            );
            assert!(asked(&card) && backup_complete(&card, &in_progress, &expected));
            if result.is_ok() {
                break;
            }
        }
        let card = disk.copy();
        assert_eq!(
            start_backed(&card, &mut game, &in_progress).unwrap(),
            Settled::Asks
        );
        // Each changed box as it was and as it became: the boxes of the two
        // bodies.
        let folder = backup_folder(&in_progress);
        assert!(backup_complete(&card, &in_progress, &expected));
        for (bit, after) in &expected {
            let file = backup_box(&card, &folder, *bit, *after).unwrap();
            let file = offline_core::boxfile::BoxFile::parse(&file).unwrap();
            let body = if *after { new } else { old };
            assert!(file.records() == records_of(body, *bit), "box {bit}");
        }
        // The transport box (bit 100) carries no name.
        let file = backup_box(&card, &folder, 100, true).unwrap();
        let file = offline_core::boxfile::BoxFile::parse(&file).unwrap();
        assert_eq!(file.name(), [0; offline_core::boxfile::NAME_TEXT]);
        // A second START writes nothing more, and nothing on the SD card.
        let before = card.0.borrow().ext.clone();
        assert!(card.0.borrow().sd.is_empty());
        let operations = card.0.borrow().operations;
        start_backed(&card, &mut game, &in_progress).unwrap();
        assert!(card.0.borrow().ext == before);
        let again = card.0.borrow().operations - operations;
        // A file of it lost, and another altered under a CRC that fits:
        // the next START writes both again.
        let (lost, _) = interrupted::file(&folder, 0, true);
        let (altered, at) = interrupted::file(&folder, 5, false);
        {
            let mut disk = card.0.borrow_mut();
            disk.ext.remove(lost.text());
            disk.ext_written.remove(lost.text());
            let file = disk.ext.get_mut(altered.text()).unwrap();
            let place = &mut file[at as usize..at as usize + offline_core::boxfile::FILE_SIZE];
            place[0x1B30] ^= 1;
            let mut bytes: [u8; offline_core::boxfile::FILE_SIZE] = (&*place).try_into().unwrap();
            offline_core::boxfile::seal(&mut bytes);
            place.copy_from_slice(&bytes);
        }
        let operations = card.0.borrow().operations;
        start_backed(&card, &mut game, &in_progress).unwrap();
        assert!(card.0.borrow().ext == before);
        assert!(card.0.borrow().operations - operations > again);
    }

    /// The folder of the backup of `pending`.
    fn backup_folder(pending: &PendingTransfer) -> bank_common::sdfiles::Path {
        use bank_common::interrupted::{folder, State};
        folder(State::of(&pending.before), State::of(&pending.after))
    }

    /// How a save left to the question is settled.
    #[derive(Clone, Copy, Debug)]
    enum Settling {
        /// START finds the game with this image.
        Bank(Fingerprint),
        /// Transporter's answer.
        Transporter(RecoveryDecision),
        /// Transporter's No, then a delivery.
        NoThenDelivery,
        /// Transporter's Yes, then a Bank put there from outside: a save that
        /// knows nothing of the backup.
        Outside,
    }

    #[test]
    fn a_settled_save_s_backup_keeps_only_the_side_the_bank_does_not_hold() {
        use bank_common::{interrupted, sdfiles, testing::Visit};
        use offline_core::transport;
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let date = [2026, 11, 16, 14, 3, 7];
        let mut kept = sdfiles::dated_folder(date);
        kept.push(interrupted::RETIRED);
        // The opening's step once the journal is clean (`settle_sd`).
        // As the opening does: the Bank read, then the SD card's folders.
        let opening = |card: &Shared| {
            let mut files = opened(card);
            files
                .read(&mut std::vec![0; offline_core::native_blob::BLOB_SIZE])
                .unwrap();
            interrupted::settle(
                &mut files,
                &mut card.clone(),
                &mut ExtCard(card.clone()),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                date,
            )
        };
        let names_in = |card: &Shared, folder: &[u8]| -> Vec<Vec<u8>> {
            card.0
                .borrow()
                .sd
                .keys()
                .filter_map(|path| Some(path.strip_prefix(folder)?.to_vec()))
                .collect()
        };
        // A save left to the question, its backup written, settled `how`.
        let settled = |how: Settling, console: bool| {
            let mut game = Simulated::loaded(true);
            let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
            drop(files);
            game.save = Some((OTHER, LOADED));
            let card = disk.like(console).copy();
            assert_eq!(
                start_backed(&card, &mut game, &in_progress).unwrap(),
                Settled::Asks
            );
            let answer = |decision| {
                transport::settle(&mut Visit(card.clone()), decision).unwrap();
            };
            match how {
                Settling::Bank(image) => {
                    game.save = Some((image, LOADED));
                    assert!(matches!(
                        start_backed(&card, &mut game, &in_progress).unwrap(),
                        Settled::Exact(_)
                    ));
                }
                Settling::Transporter(decision) => answer(decision),
                Settling::NoThenDelivery => {
                    answer(RecoveryDecision::CommitAfter);
                    assert!(bank_common::testing::try_deliver(&card, 2).is_some());
                }
                Settling::Outside => {
                    answer(RecoveryDecision::KeepBefore);
                    let mut files = opened(&card);
                    let mut body = std::vec![0; offline_core::native_blob::BLOB_SIZE];
                    files.read(&mut body).unwrap();
                    files.commit(Cleaned::new(&mut body).unwrap()).unwrap();
                }
            }
            (card, backup_folder(&in_progress))
        };
        // Every file a settled backup can keep: those of a Bank replaced from
        // outside, which keeps both sides, as they were written.
        let (card, _) = settled(Settling::Outside, true);
        opening(&card).unwrap();
        let all = names_in(&card, kept.text());
        let written: BTreeMap<_, _> = all
            .iter()
            .map(|name| {
                let path = [kept.text(), name].concat();
                (name.clone(), card.0.borrow().sd[&path].clone())
            })
            .collect();
        let half = |suffix: &[u8]| -> Vec<Vec<u8>> {
            all.iter()
                .filter(|name| name.ends_with(suffix))
                .cloned()
                .collect()
        };
        let (olds, news) = (half(b".old.box"), half(b".new.box"));
        assert!(!olds.is_empty() && !news.is_empty());
        let nothing = std::vec::Vec::new();
        for (how, left) in [
            (Settling::Bank(BEFORE), &nothing),
            (Settling::Bank(AFTER), &nothing),
            (Settling::Transporter(RecoveryDecision::KeepBefore), &news),
            (Settling::Transporter(RecoveryDecision::CommitAfter), &olds),
            (Settling::NoThenDelivery, &olds),
            (Settling::Outside, &all),
        ] {
            for console in [true, false] {
                let (card, folder) = settled(how, console);
                let what = std::format!("{how:?}, console {console}");
                // Whatever a cut leaves, the next opening ends with the side
                // kept, in one folder of the date.
                let counting = card.cut_after(usize::MAX);
                opening(&counting).unwrap();
                let total = counting.0.borrow().operations;
                assert_eq!(names_in(&counting, kept.text()), **left, "{what}");
                for cut in 0..=total {
                    let card = card.cut_after(cut);
                    let _ = opening(&card);
                    let card = card.reboot();
                    opening(&card).unwrap();
                    let sd = card.0.borrow();
                    assert!(!sd.ext_dirs.contains(folder.text()), "{what}, cut {cut}");
                    assert!(sd.ext.is_empty(), "{what}, cut {cut}");
                    let names = names_in(&card, kept.text());
                    assert_eq!(names, **left, "{what}, cut {cut}");
                    let dated = sd
                        .sd_dirs
                        .iter()
                        .filter(|dir| dir.ends_with(interrupted::RETIRED))
                        .count();
                    assert_eq!(dated, usize::from(!left.is_empty()), "{what}, cut {cut}");
                    // Each file as it was written.
                    for name in &names {
                        let path = [kept.text(), name].concat();
                        assert!(sd.sd[&path] == written[name], "{what}, cut {cut}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_game_found_again_decides_the_save_and_loses_nothing_wherever_it_is_cut() {
        use bank_common::interrupted;
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let date = [2026, 11, 16, 14, 3, 7];
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        let asked_card = disk.like(true).copy();
        start_backed(&asked_card, &mut game, &in_progress).unwrap();
        for image in [BEFORE, AFTER] {
            game.save = Some((image, LOADED));
            let counting = asked_card.cut_after(usize::MAX);
            start_backed(&counting, &mut game, &in_progress).unwrap();
            for cut in 0..counting.0.borrow().operations {
                let card = asked_card.cut_after(cut);
                let _ = start_backed(&card, &mut game, &in_progress);
                let card = card.reboot();
                if pending(&mut opened(&card)).is_some() {
                    start_backed(&card, &mut game, &in_progress).unwrap();
                }
                let mut files = opened(&card);
                files
                    .read(&mut std::vec![0; offline_core::native_blob::BLOB_SIZE])
                    .unwrap();
                interrupted::settle(
                    &mut files,
                    &mut card.clone(),
                    &mut ExtCard(card.clone()),
                    &mut std::vec![0; 2 * offline_core::boxfile::FILE_SIZE],
                    date,
                )
                .unwrap();
                let state = card.0.borrow();
                assert!(state.ext.is_empty(), "cut {cut}");
                // The folder went before the journal: nothing is kept.
                assert!(state.sd.is_empty(), "cut {cut}");
            }
        }
    }

    #[test]
    fn a_game_found_again_settles_a_backup_in_part_on_a_full_sd_card() {
        use bank_common::interrupted;
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        let card = disk.like(true).copy();
        start_backed(&card, &mut game, &in_progress).unwrap();
        let (lost, _) = interrupted::file(&backup_folder(&in_progress), 0, true);
        {
            let mut state = card.0.borrow_mut();
            state.ext.remove(lost.text());
            state.ext_written.remove(lost.text());
            state.sd_full = true;
        }
        game.save = Some((BEFORE, LOADED));
        assert_eq!(
            start_backed(&card, &mut game, &in_progress).unwrap(),
            Settled::Exact(RecoveryDecision::KeepBefore)
        );
        assert!(card.0.borrow().ext.is_empty());
    }

    #[test]
    fn a_backup_written_for_a_marked_save_never_stands_torn_beside_transporter_s_answer() {
        use bank_common::{interrupted, testing::Visit};
        use offline_core::transport;
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let date = [2026, 11, 21, 12, 0, 0];
        let opening = |card: &Shared| {
            let mut files = opened(card);
            files
                .read(&mut std::vec![0; offline_core::native_blob::BLOB_SIZE])
                .unwrap();
            interrupted::settle(
                &mut files,
                &mut card.clone(),
                &mut ExtCard(card.clone()),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                date,
            )
        };
        // A record an earlier release left marked, with no folder, and a
        // folder that lost a file; each asked, and unlocked.
        for (has_folder, unlock) in [(false, false), (false, true), (true, false), (true, true)] {
            let what = std::format!("folder {has_folder}, unlocked {unlock}");
            let mut game = Simulated::loaded(true);
            let (disk, mut files, in_progress, _) = prepared_save(&old, new, &mut game);
            game.save = Some((OTHER, LOADED));
            if !has_folder {
                files.mark(true, false).unwrap();
            }
            if unlock {
                files.unlock().unwrap();
            }
            drop(files);
            let disk = disk.like(true).copy();
            let expected = backup_files(&old, new);
            if has_folder {
                start_backed(&disk, &mut game, &in_progress).unwrap();
                let (lost, _) = interrupted::file(&backup_folder(&in_progress), 0, true);
                let mut state = disk.0.borrow_mut();
                state.ext.remove(lost.text());
                state.ext_written.remove(lost.text());
            }
            let counting = disk.cut_after(usize::MAX);
            assert_eq!(
                start_backed(&counting, &mut game, &in_progress).unwrap(),
                Settled::Asks,
                "{what}"
            );
            // Whole, and marked as it was.
            assert!(
                backup_complete(&counting, &in_progress, &expected),
                "{what}"
            );
            assert!(asked(&counting), "{what}");
            assert_eq!(unlocked(&counting), unlock, "{what}");
            let total = counting.0.borrow().operations;
            let mut answers = 0;
            for cut in 0..total {
                let card = disk.cut_after(cut);
                let _ = start_backed(&card, &mut game, &in_progress);
                let card = card.reboot();
                // Whenever Transporter answers, no file is being written:
                // before the marks came off, the folder is as it was, and the
                // next opening settles it. A replica the cut left unreadable
                // is refused by Transporter, and nothing changes.
                if asked(&card) || unlocked(&card) {
                    let answered = card.copy();
                    let decision = RecoveryDecision::KeepBefore;
                    if transport::settle(&mut Visit(answered.clone()), decision).is_ok() {
                        answers += 1;
                        opening(&answered).unwrap();
                        assert!(answered.0.borrow().ext.is_empty(), "{what}, cut {cut}");
                    }
                }
                // And the next START finishes what the cut left.
                assert_eq!(
                    start_backed(&card, &mut game, &in_progress).unwrap(),
                    Settled::Asks,
                    "{what}, cut {cut}"
                );
                assert!(
                    asked(&card) && backup_complete(&card, &in_progress, &expected),
                    "{what}, cut {cut}"
                );
            }
            assert!(answers > 0, "{what}");
        }
    }

    #[test]
    fn a_full_sd_card_leaves_a_save_that_moved_both_ways_for_the_next_start() {
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        disk.0.borrow_mut().sd_full = true;
        assert_eq!(
            start_backed(&disk, &mut game, &in_progress).unwrap(),
            Settled::NoSpace
        );
        assert!(!asked(&disk));
        disk.0.borrow_mut().sd_full = false;
        assert_eq!(
            start_backed(&disk, &mut game, &in_progress).unwrap(),
            Settled::Asks
        );
        assert!(backup_complete(
            &disk,
            &in_progress,
            &backup_files(&old, new)
        ));
    }

    #[test]
    fn a_backup_cut_short_leaves_no_file_that_does_not_read() {
        // START without the game writes the backup of a save that moved
        // Pokémon both ways, cut at every write; then the game is back with
        // its save before, START settles the save, and the opening its
        // folder. Every file left on the card is a box file that reads.
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        drop(files);
        let disk = disk.like(true);
        let without = |card: &Shared| {
            settle_backed(
                &mut opened(card),
                None::<&mut Simulated>,
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
                &mut Kept::new(&mut ExtCard(card.clone())),
            )
        };
        let counting = disk.cut_after(usize::MAX);
        without(&counting).unwrap();
        for cut in 0..counting.0.borrow().operations {
            let card = disk.cut_after(cut);
            let _ = without(&card);
            let card = card.reboot();
            let mut game = Simulated::loaded(true);
            game.save = Some((BEFORE, LOADED));
            start_backed(&card, &mut game, &in_progress).unwrap();
            let mut files = opened(&card);
            files
                .read(&mut std::vec![0; offline_core::native_blob::BLOB_SIZE])
                .unwrap();
            bank_common::interrupted::settle(
                &mut files,
                &mut card.clone(),
                &mut ExtCard(card.clone()),
                &mut std::vec![0; 2 * offline_core::boxfile::FILE_SIZE],
                [2026, 11, 16, 14, 3, 7],
            )
            .unwrap();
            for (path, bytes) in card.0.borrow().sd.iter() {
                assert!(
                    offline_core::boxfile::BoxFile::parse(bytes).is_ok(),
                    "cut {cut}: {path:?}"
                );
            }
        }
    }

    #[test]
    fn a_card_full_for_the_journal_after_a_cut_start_says_so() {
        // START cut at every write, then a START on a card with no room for
        // a unit file of Bank's: "The SD card is full.", never the error
        // screen, and the save still in progress. For a save that moved
        // Pokémon both ways, left to Transporter's question (its backup and
        // the record that marks it), and for one that moved them one way,
        // settled by the game's own image and by what it moved.
        let (old, sessions) = sessions();
        for (session, image) in [(3, OTHER), (0, BEFORE), (0, OTHER)] {
            let new = &sessions[session].1;
            let mut game = Simulated::loaded(true);
            let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
            drop(files);
            game.save = Some((image, LOADED));
            let disk = disk.like(true);
            let counting = disk.cut_after(usize::MAX);
            start_backed(&counting, &mut game, &in_progress).unwrap();
            for cut in 0..counting.0.borrow().operations {
                let card = disk.cut_after(cut);
                let _ = start_backed(&card, &mut game, &in_progress);
                let card = card.reboot();
                // Settled already: the next START opens the Bank.
                if !matches!(opened(&card).phase().unwrap(), Phase::Prepared(_)) {
                    continue;
                }
                card.0.borrow_mut().full = true;
                let what = std::format!("session {session}, cut {cut}");
                let settled = start_backed(&card, &mut game, &in_progress)
                    .unwrap_or_else(|error| panic!("{what}: the error screen: {error:?}"));
                if settled == Settled::NoSpace {
                    assert!(
                        matches!(opened(&card).phase().unwrap(), Phase::Prepared(_)),
                        "{what}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_unlocked_save_is_transporters_whatever_game_is_there_until_one_of_its_images_is() {
        let (old, sessions) = sessions();
        let (moved, new) = &sessions[3];
        assert_eq!(*moved, "both ways");
        let mut game = Simulated::loaded(true);
        let (disk, mut files, in_progress, _) = prepared_save(&old, new, &mut game);
        files.unlock().unwrap();
        drop(files);
        let mut another = Simulated {
            trainer: Trainer {
                id: trainer().id + 1,
                ..trainer()
            },
            save: Some((OTHER, LOADED)),
            ..Simulated::loaded(true)
        };
        game.save = Some((OTHER, LOADED));
        let mut other_kind = Simulated {
            save: Some((OTHER, LOADED)),
            ..Simulated::loaded(false)
        };
        let start = |found: Option<&mut Simulated>| {
            let mut files = opened(&disk);
            let settled = settle(
                &mut files,
                found,
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
            )
            .unwrap();
            (settled, pending(&mut files))
        };
        // Absent, another trainer's, the other kind of copy, its own:
        // Transporter's question each time; bit 2 still by what was seen.
        for (found, seen) in [
            (None, false),
            (Some(&mut another), false),
            (Some(&mut other_kind), false),
            (Some(&mut game), true),
        ] {
            assert_eq!(start(found), (Settled::Asks, Some(in_progress)));
            assert!(transport_check(&disk) == Err(offline_core::transport::Refusal::Asked));
            assert!(unlocked(&disk));
            assert_eq!(asked(&disk), seen);
        }
        // The game back with the image it was saved from: Bank settles the
        // save itself, unlocked or not.
        game.save = Some((BEFORE, LOADED));
        assert_eq!(
            start(Some(&mut game)),
            (Settled::Exact(RecoveryDecision::KeepBefore), None)
        );
        assert!(settled_bank(&disk) == old);
        assert!(!unlocked(&disk));
    }

    #[test]
    fn after_a_cut_while_bank_leaves_a_save_to_transporter_transporter_asks() {
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        assert_eq!(sessions[3].0, "both ways");
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        drop(files);
        game.save = Some((OTHER, LOADED));
        let disk = disk.like(true);
        // A cut at every step of leaving it to the question, as the console
        // cuts: a file with writes not yet synced does not read any more.
        for budget in 0.. {
            let card = disk.cut_after(budget);
            let mut files = opened(&card);
            let settled = settle(
                &mut files,
                Some(&mut game),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
            );
            drop(files);
            let card = card.reboot();
            // The next START of Bank sends the user to Transporter, and
            // Transporter asks: neither sends the user back.
            let mut files = opened(&card);
            let again = settle(
                &mut files,
                Some(&mut game),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
            );
            assert_eq!(again.unwrap(), Settled::Asks, "cut after {budget}");
            drop(files);
            assert_eq!(
                transport_check(&card),
                Err(offline_core::transport::Refusal::Asked),
                "cut after {budget}"
            );
            if settled.is_ok() {
                break;
            }
        }
    }

    #[test]
    fn after_a_cut_while_bank_repairs_a_journal_record_transporter_still_asks() {
        let (old, sessions) = sessions();
        let new = &sessions[3].1;
        let mut game = Simulated::loaded(true);
        let (disk, files, in_progress, _) = prepared_save(&old, new, &mut game);
        game.save = Some((OTHER, LOADED));
        let start = |card: &Shared, game: &mut Simulated| {
            let mut files = opened(card);
            let settled = settle(
                &mut files,
                Some(game),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
            );
            drop(files);
            settled
        };
        drop(files);
        assert_eq!(start(&disk, &mut game).unwrap(), Settled::Asks);
        // Each journal record in turn left unreadable by a cut, so that
        // Bank's next write recreates it; that START is cut at every step,
        // the recreation's deletion among them, which leaves the record
        // missing unless the console makes it unreadable. Then Bank's START
        // once more: it sends the user to Transporter, and Transporter asks.
        for (console, record) in [(true, 0), (true, 1), (false, 0), (false, 1)] {
            let spoiled = disk.like(console);
            spoiled.0.borrow_mut().unreadable.insert((0, record));
            for budget in 0.. {
                let card = spoiled.cut_after(budget);
                let cut = start(&card, &mut game);
                let card = card.reboot();
                assert_eq!(
                    start(&card, &mut game).unwrap(),
                    Settled::Asks,
                    "console {console}, record {record}, cut after {budget}"
                );
                assert_eq!(
                    transport_check(&card),
                    Err(offline_core::transport::Refusal::Asked),
                    "console {console}, record {record}, cut after {budget}"
                );
                if cut.is_ok() {
                    break;
                }
            }
        }
    }

    #[test]
    fn at_start_a_game_whose_card_is_gone_leaves_the_save_for_the_next_one() {
        let (old, sessions) = sessions();
        for (moved, new) in &sessions {
            let mut game = Simulated::loaded(true);
            let (_, mut files, in_progress, _) = prepared_save(&old, new, &mut game);
            game.save = None;
            let settled = settle(
                &mut files,
                Some(&mut game),
                &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
                &in_progress,
                Rule::Start,
                None,
                &mut held(),
            );
            assert_eq!(settled.unwrap(), Settled::Open, "{moved}");
            assert_eq!(pending(&mut files), Some(in_progress), "{moved}");
        }
        // A save made with an installed copy that cannot be read: an
        // error, with the save left as it was.
        let new = &sessions[0].1;
        let mut game = Simulated::loaded(false);
        let (_, mut files, in_progress, _) = prepared_save(&old, new, &mut game);
        game.save = None;
        let settled = settle(
            &mut files,
            Some(&mut game),
            &mut std::vec![0; offline_core::native_blob::BLOB_SIZE],
            &in_progress,
            Rule::Start,
            None,
            &mut held(),
        );
        assert_eq!(fault(settled), Fault::GameRead);
        assert_eq!(pending(&mut files), Some(in_progress));
    }

    #[test]
    fn a_game_whose_card_is_gone_is_not_loaded() {
        for cartridge in [true, false] {
            let mut game = Simulated::loaded(cartridge);
            assert_eq!(load_game(&mut game).unwrap(), Ok(game.evidence()));
        }
        // Gone at the first question, before anything is read: the
        // original's missing card, whatever the copy.
        for cartridge in [true, false] {
            let mut game = Simulated::loaded(cartridge);
            game.out_after = Some(0);
            assert_eq!(load_game(&mut game).unwrap(), Err(Ending::Retry));
        }
        // Gone later, before the read or between the read and its secure
        // value: a cartridge's card that is gone; an installed copy has no
        // card to take out, and its failure is an error.
        for calls in [1, 2] {
            let mut game = Simulated::loaded(true);
            game.out_after = Some(calls);
            assert_eq!(load_game(&mut game).unwrap(), Err(Ending::Retry), "{calls}");
            let mut game = Simulated::loaded(false);
            game.out_after = Some(calls);
            assert!(load_game(&mut game).is_err(), "{calls}");
        }
    }

    #[test]
    fn a_card_without_room_for_a_new_bank_ends_its_creation_and_the_next_one_finishes_it() {
        let disk = Shared::default();
        disk.0.borrow_mut().full = true;
        let new = body(0x31, 0x44, 0);
        let mut files = BankFiles::new(disk.clone());
        assert_eq!(files.open(), Ok(None));
        let mut staging = new.clone();
        assert_eq!(
            created(files.initialize(Cleaned::new(&mut staging).unwrap())),
            Ok(Some(Ending::NoSpace))
        );
        drop(files);
        // Room again: the next START creates it.
        disk.0.borrow_mut().full = false;
        let mut files = BankFiles::new(disk.clone());
        assert_eq!(files.open(), Ok(None));
        let mut staging = new.clone();
        assert_eq!(
            created(files.initialize(Cleaned::new(&mut staging).unwrap())),
            Ok(None)
        );
        drop(files);
        assert!(settled_bank(&disk) == new);
        // Any other refusal of the card is an error, with its numbers.
        assert_eq!(
            created::<Refused>(Err(bank_files::Error::Io(Refused::FAILED))),
            Err(bank_files::Error::Io(Refused::FAILED))
        );
    }

    #[test]
    fn two_saves_in_a_row_each_commit_their_own_image() {
        let (disk, mut files, _) = bank();
        // The worker lets the Bank file rest after every job.
        let between_jobs = |files: &mut BankFiles<Shared>| {
            files.rest().unwrap();
            assert_eq!(disk.0.borrow().open, 0);
        };
        between_jobs(&mut files);
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
            between_jobs(&mut files);
            game.write(secure);
            let finalized = finalize(
                &mut files,
                &mut game,
                &mut staging,
                &in_progress,
                Some(secure),
                Some(staged),
                0,
                &mut held(),
            );
            assert_eq!(finalized.unwrap(), Finalized::Saved);
            between_jobs(&mut files);
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
            .prepare(
                Cleaned::new(&mut staging).unwrap(),
                bank_common::testing::GAME,
                [1; 32],
                [2; 32],
            )
            .unwrap();
        assert_ne!(crc32(&staging), staged);
        // `Finalize` commits and reads the Bank back into the same buffer.
        files.reconcile_as(RecoveryDecision::CommitAfter).unwrap();
        files.read(&mut staging).unwrap();
        assert_eq!(crc32(&staging), staged);
    }
}
