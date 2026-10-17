//! One-time conversion of a Bank an earlier release stored.
//!
//! Earlier releases stored the Bank in other ways (`Source`), each read by a
//! folder of its own with the code of the release that wrote it. Whichever
//! is there is read into the one native body, which the target (`target`)
//! writes as a Bank of the current files through `BankFiles`, the way the
//! offline patch creates one. Some of the new files have the names of old
//! ones, which is why the new files are first written under temporary names.
//! This module is the run between the two, and knows neither.
//!
//! Only this patch knows the earlier files, and it does nothing else: it
//! never opens the Bank for use.
//!
//! The steps, each safe to interrupt:
//!
//! 1. *Stage.* The old Bank is loaded the way its release loads it. It must
//!    be at rest: a save in progress, or a Pokédex or transport box that
//!    release would repair at its next save, are that release's to settle.
//!    Its body is written as a new Bank under temporary names, which is then
//!    loaded and must be the same body. Only then a marker file is created,
//!    which names the release. Until the marker exists the old files are
//!    untouched, and a start begins again.
//! 2. *Finish.* With the marker, the temporary files are the Bank, whatever
//!    they were made from, and at every start they must load as one before
//!    anything is removed. The files of every earlier way are removed, the
//!    final files are written from the temporary ones (the journal records
//!    last) and compared, and must load as the same Bank; then the marker is
//!    removed, and then the temporary files.

use crate::{source::Source, target};
use bank_common::{bank_files::UnitFile, numbers::Label};
use offline_core::{native_blob::BLOB_SIZE, Storage};

/// First number of the result screen when nothing went wrong; the second
/// number is the `Done`.
pub const DONE: u32 = 0x600d;
/// First number when the old Bank is not at rest: the release that stored
/// it has to settle it first.
pub const REFUSED: u32 = 0xbad;
/// First number of a failed step, plus the `Step`.
pub const FAILED: u32 = 0xba0;

/// The files of the card, by path.
pub trait Card {
    type Error;
    type Storage: Storage<Error = Self::Error>;
    /// Whether every one of `files` is there with exactly its size.
    fn exists(&mut self, files: &'static [UnitFile]) -> Result<bool, Self::Error>;
    /// `None` when none of `files` is there.
    fn open(&mut self, files: &'static [UnitFile]) -> Result<Option<Self::Storage>, Self::Error>;
    /// Creates every one of `files` anew.
    fn create(&mut self, files: &'static [UnitFile]) -> Result<Self::Storage, Self::Error>;
    /// Removes those of `files` that are there.
    fn remove(&mut self, files: &'static [UnitFile]) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Done {
    /// Neither an old Bank nor a new one.
    NoBank = 0,
    Converted = 1,
    /// A Bank in the new files, and nothing to convert.
    AlreadyNew = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Step {
    ReadOld = 1,
    WriteTemporary = 2,
    RemoveOld = 3,
    WriteNew = 4,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cause<E> {
    Io(E),
    /// The files do not load as a Bank, or hold Banks of two earlier ways.
    Damaged,
    /// A copy does not read back as what was copied, or the copies do not
    /// load as the Bank the old files held.
    Mismatch,
    /// A file that has to be there is not.
    Missing,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stopped<E> {
    /// The old Bank has a save in progress. Nothing was changed.
    InProgress,
    /// The old Bank has no Pokédex or no transport box for its current
    /// save, which its release repairs at its next Save and Quit. Nothing
    /// was changed.
    Unsettled,
    Failed(Step, Cause<E>),
}

/// What a run did, and the way of storing it found, when it found one.
#[derive(Debug, PartialEq, Eq)]
pub struct Report<E> {
    pub source: Option<Source>,
    pub result: Result<Done, Stopped<E>>,
}

/// The two numbers of the result screen. `diagnostic` gives the console's
/// code for a file error.
pub fn numbers<E>(result: Result<Done, Stopped<E>>, diagnostic: impl Fn(E) -> u32) -> [u32; 2] {
    match result {
        Ok(done) => [DONE, done as u32],
        Err(Stopped::InProgress) => [REFUSED, 7],
        Err(Stopped::Unsettled) => [REFUSED, 8],
        Err(Stopped::Failed(step, cause)) => [
            FAILED + step as u32,
            match cause {
                Cause::Io(error) => diagnostic(error),
                Cause::Damaged => 5,
                Cause::Mismatch => 1,
                Cause::Missing => 2,
            },
        ],
    }
}

/// The patch's name: behind "Ver. 1.5" on the start screen, and behind the
/// version on the result screen when no earlier Bank was found.
pub const PACKAGE: &[u8] = b"-migration";

/// The line above the numbers. For a Bank of an earlier way, that way and
/// the release converting it, as "v0.2.1-0.6.0" (`version` without its "v");
/// otherwise the version and the package, as "v0.6.0-migration". `None` for
/// a version that makes no label.
pub fn label(source: Option<Source>, version: &[u8]) -> Option<Label> {
    let Some(source) = source else {
        return Label::new(version, PACKAGE);
    };
    let version = version.strip_prefix(b"v").unwrap_or(version);
    let mut suffix = [0; Label::ROOM];
    let part = suffix.get_mut(1..1 + version.len())?;
    part.copy_from_slice(version);
    suffix[0] = b'-';
    Label::new(source.tag(), &suffix[..1 + version.len()])
}

/// Says that the temporary files are a whole Bank, and which way it was
/// stored before: the tag of the `Source` in ASCII, zero after it. All
/// zero, as the release before this one left it, names none.
pub(crate) static MARKER: [UnitFile; 1] = [UnitFile::new("/migrate.ok", 16)];

type Outcome<T, C> = Result<T, Stopped<<C as Card>::Error>>;

fn io<E>(step: Step) -> impl Fn(E) -> Stopped<E> {
    move |error| Stopped::Failed(step, Cause::Io(error))
}

/// Converts what is there, or goes on where an interrupted run stopped.
/// `buffer` is working room of at least one native Bank body.
pub fn run<C: Card>(card: &mut C, buffer: &mut [u8]) -> Report<C::Error> {
    let mut source = None;
    let result = convert(card, buffer, &mut source);
    Report { source, result }
}

fn convert<C: Card>(
    card: &mut C,
    buffer: &mut [u8],
    source: &mut Option<Source>,
) -> Outcome<Done, C> {
    if card.exists(&MARKER).map_err(io(Step::ReadOld))? {
        *source = marked(card);
    } else {
        let Some(found) = find(card)? else {
            // What a finished run may have left when it was cut at its end.
            target::remove_temporary(card)?;
            return Ok(if target::exists(card)? {
                Done::AlreadyNew
            } else {
                Done::NoBank
            });
        };
        *source = Some(found);
        stage(card, buffer, found)?;
    }
    finish(card, buffer)?;
    Ok(Done::Converted)
}

/// The earlier way the Bank on the card is stored in, if any. Files of two
/// are no Bank either of them wrote, and nothing is done with them.
fn find<C: Card>(card: &mut C) -> Outcome<Option<Source>, C> {
    let mut found = None;
    for source in Source::ALL {
        if source.there(card).map_err(io(Step::ReadOld))? {
            if found.is_some() {
                return Err(Stopped::Failed(Step::ReadOld, Cause::Damaged));
            }
            found = Some(source);
        }
    }
    Ok(found)
}

/// The marker's content for `source`.
pub(crate) fn mark(source: Source) -> [u8; 16] {
    let mut out = [0; 16];
    out[..source.tag().len()].copy_from_slice(source.tag());
    out
}
/// The way the marker names, read for the result screen only.
fn marked<C: Card>(card: &mut C) -> Option<Source> {
    let mut marker = card.open(&MARKER).ok()??;
    let mut bytes = [0; 16];
    marker.read(0, &mut bytes).ok()?;
    Source::ALL
        .into_iter()
        .find(|&source| mark(source) == bytes)
}

/// Storage that is only read: every write is refused. A way reads its files
/// through it.
pub(crate) struct ReadOnly<S>(pub(crate) S);
pub(crate) enum Guarded<E> {
    Io(E),
    ReadOnly,
}
impl<S: Storage> Storage for ReadOnly<S> {
    type Error = Guarded<S::Error>;
    fn read(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read(offset, out).map_err(Guarded::Io)
    }
    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
    fn sync(&mut self) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
    fn recreate(&mut self, _: u64, _: u64) -> Result<(), Self::Error> {
        Err(Guarded::ReadOnly)
    }
}

/// Reads the old Bank into one native body, writes that body as a new Bank
/// under temporary names, and marks it complete. The old files are only read.
fn stage<C: Card>(card: &mut C, buffer: &mut [u8], source: Source) -> Outcome<(), C> {
    let write = Step::WriteTemporary;
    let Some(body) = buffer.get_mut(..BLOB_SIZE) else {
        return Err(Stopped::Failed(Step::ReadOld, Cause::Damaged));
    };
    source.read(card, body)?;
    target::write_temporary(card, body)?;
    card.remove(&MARKER).map_err(io(write))?;
    let mut marker = card.create(&MARKER).map_err(io(write))?;
    marker.write(0, &mark(source)).map_err(io(write))?;
    marker.sync().map_err(io(write))?;
    Ok(())
}

/// Puts the temporary files in place of the old ones.
fn finish<C: Card>(card: &mut C, buffer: &mut [u8]) -> Outcome<(), C> {
    // The temporary files are all there is of the Bank once the old files
    // go: they must still load as one, at every start that goes on here.
    let converted = target::temporary_bank(card, buffer)?;
    // Whichever way it was stored before: what is not there is not removed.
    for source in Source::ALL {
        source.remove(card).map_err(io(Step::RemoveOld))?;
    }
    target::put_in_place(card, buffer, converted)?;
    // The marker first: without it the temporary files mean nothing.
    card.remove(&MARKER).map_err(io(Step::WriteNew))?;
    target::remove_temporary(card)
}

/// The run, for every earlier way (`Source::fixtures`), and what belongs to
/// the run alone.
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::{
        testing::{convert, current, of_the_run, only_the_target_files, report, Final, Sd},
        v0_4_0,
    };
    use bank_common::{
        bank_files::{BankFiles, FileName},
        testing::body,
    };
    use std::{string::String, vec};

    #[test]
    fn a_bank_of_every_earlier_way_becomes_the_same_bank_in_the_target() {
        for source in Source::ALL {
            for console in [false, true] {
                let (card, body) = (source.fixtures().used)(console);
                assert_eq!(
                    report(&card),
                    Report {
                        source: Some(source),
                        result: Ok(Done::Converted)
                    },
                    "{source:?}"
                );
                assert!(only_the_target_files(&card), "{source:?}");
                assert!(card.0.borrow().unreadable.is_empty(), "{source:?}");
                assert!(current(&card).unwrap() == body, "{source:?}");
                // A second run finds nothing to do and changes nothing.
                let files = card.files();
                assert_eq!(
                    report(&card),
                    Report {
                        source: None,
                        result: Ok(Done::AlreadyNew)
                    }
                );
                assert!(card.files() == files, "{source:?}");
            }
        }
    }

    #[test]
    fn a_bank_that_is_not_at_rest_is_left_to_its_release() {
        for source in Source::ALL {
            let fixtures = source.fixtures();
            let mut cards = vec![((fixtures.in_progress)(), Stopped::InProgress)];
            for card in (fixtures.unsettled)() {
                cards.push((card, Stopped::Unsettled));
            }
            for (card, stopped) in cards {
                let files = card.files();
                assert_eq!(
                    report(&card),
                    Report {
                        source: Some(source),
                        result: Err(stopped)
                    }
                );
                assert!(card.files() == files, "{source:?}");
            }
        }
    }

    #[test]
    fn files_of_two_earlier_ways_are_left_as_they_are() {
        let mut pairs = 0;
        for first in Source::ALL {
            for second in Source::ALL.into_iter().filter(|&other| other != first) {
                // The files of the second that have no name of the first's.
                let (card, _) = (first.fixtures().used)(false);
                let (other, _) = (second.fixtures().used)(false);
                for (path, bytes) in other.files() {
                    card.0.borrow_mut().files.entry(path).or_insert(bytes);
                }
                let mut probe = card.clone();
                if !(first.there(&mut probe).unwrap() && second.there(&mut probe).unwrap()) {
                    continue;
                }
                pairs += 1;
                let files = card.files();
                assert_eq!(
                    report(&card),
                    Report {
                        source: None,
                        result: Err(Stopped::Failed(Step::ReadOld, Cause::Damaged))
                    },
                    "{first:?} {second:?}"
                );
                assert!(card.files() == files);
            }
        }
        // Every two ways are tried at least one way round.
        assert!(pairs >= Source::ALL.len() - 1);
    }

    /// Cuts the conversion of `card` at every operation in turn, and has
    /// the next start go on from there; some of those are cut again.
    fn cut_everywhere(card: &Sd, body: &[u8], source: Source) {
        let whole = card.restarted(Some(usize::MAX));
        assert_eq!(
            report(&whole),
            Report {
                source: Some(source),
                result: Ok(Done::Converted)
            }
        );
        let total = whole.0.borrow().operations;
        assert!(total > 40);
        let reference = whole.files();
        let before = card.files();
        let mut resumed = 0;
        for cut in 0..total {
            let cut_card = card.restarted(Some(cut));
            assert!(convert(&cut_card).is_err(), "cut {cut}");
            let after = cut_card.restarted(None);
            let marker = after.files().get(MARKER[0].path).cloned();
            let expected = match marker {
                Some(bytes) => {
                    resumed += 1;
                    let readable = !after.0.borrow().unreadable.contains(MARKER[0].path);
                    (readable && bytes[..] == mark(source)).then_some(source)
                }
                None if source.there(&mut after.clone()).unwrap() => {
                    // Until the marker exists the old files are untouched.
                    let mut files = after.files();
                    let mut was = before.clone();
                    files.retain(|path, _| !of_the_run(path));
                    was.retain(|path, _| !of_the_run(path));
                    assert!(files == was, "cut {cut}");
                    Some(source)
                }
                None => None,
            };
            // The next start finishes it, whatever the cut left.
            let done = report(&after);
            assert!(
                matches!(done.result, Ok(Done::Converted | Done::AlreadyNew)),
                "cut {cut}: {done:?}"
            );
            assert_eq!(done.source, expected, "cut {cut}");
            assert!(after.files() == reference, "cut {cut}: files differ");
            assert!(after.0.borrow().unreadable.is_empty(), "cut {cut}");
            assert!(current(&after).unwrap() == body, "cut {cut}");
        }
        // Some cuts fell after the marker: those went on, not again.
        assert!(resumed > 10);
        // A start that goes on is cut too, and the one after it finishes.
        for first in (0..total).step_by(7) {
            for second in (0..total).step_by(11) {
                let cut_card = card.restarted(Some(first));
                assert!(convert(&cut_card).is_err());
                let again = cut_card.restarted(Some(second));
                let _ = convert(&again);
                let last = again.restarted(None);
                let done = convert(&last);
                assert!(
                    matches!(done, Ok(Done::Converted | Done::AlreadyNew)),
                    "cuts {first}, {second}: {done:?}"
                );
                assert!(last.files() == reference, "cuts {first}, {second}");
                assert!(current(&last).unwrap() == body, "cuts {first}, {second}");
            }
        }
    }

    #[test]
    fn a_cut_at_every_step_of_the_conversion_is_finished_by_the_next_start() {
        for source in Source::ALL {
            for console in [false, true] {
                let (card, body) = (source.fixtures().used)(console);
                cut_everywhere(&card, &body, source);
            }
        }
    }

    #[test]
    fn a_run_that_goes_on_removes_nothing_unless_the_temporary_bank_still_loads() {
        for source in Source::ALL {
            let buffer = &mut vec![0; BLOB_SIZE];
            let (staged, body) = (source.fixtures().used)(false);
            stage(&mut staged.clone(), buffer, source).unwrap();
            let damage = |change: &dyn Fn(&mut crate::testing::State)| {
                let card = staged.restarted(None);
                change(&mut card.0.borrow_mut());
                card
            };
            let cases = [
                // One byte of the boxes, of the main file, of a journal record.
                (
                    damage(&|state| state.files.get_mut("/m2.tmp").unwrap()[77] ^= 1),
                    Cause::Damaged,
                ),
                (
                    damage(&|state| state.files.get_mut("/m4.tmp").unwrap()[77] ^= 1),
                    Cause::Damaged,
                ),
                (
                    damage(&|state| {
                        state.files.get_mut("/m0.tmp").unwrap()[77] ^= 1;
                        state.files.get_mut("/m1.tmp").unwrap()[77] ^= 1;
                    }),
                    Cause::Damaged,
                ),
                // The temporary Bank gone.
                (
                    damage(&|state| {
                        for unit in target::temporary(FileName::Bank) {
                            state.files.remove(unit.path);
                        }
                    }),
                    Cause::Missing,
                ),
            ];
            for (card, cause) in cases {
                let files = card.files();
                assert_eq!(
                    convert(&card),
                    Err(Stopped::Failed(Step::WriteTemporary, cause))
                );
                // The old Bank is still there, whole.
                assert!(card.files() == files);
                assert!(source.there(&mut card.clone()).unwrap());
            }
            // Undamaged, the same card is finished.
            assert_eq!(convert(&staged), Ok(Done::Converted));
            assert!(current(&staged).unwrap() == body);
        }
    }

    #[test]
    fn a_cut_run_of_the_migration_v0_4_0_shipped_is_left_as_it_is() {
        // That migration cut the files of v0.2.1 into those of v0.3.0, under
        // the same marker and temporary names, with the lengths of v0.3.0.
        let (card, _) = (Source::V0_2_1.fixtures().used)(false);
        let (split, _) = v0_4_0::tests::used(false);
        let units = [
            v0_4_0::bank_files::FileName::Bank,
            v0_4_0::bank_files::FileName::Dex,
            v0_4_0::bank_files::FileName::Transport,
            v0_4_0::bank_files::FileName::Rewards,
        ]
        .map(v0_4_0::bank_files::FileName::units)
        .concat();
        let names = [
            "/m0.tmp", "/m1.tmp", "/m2.tmp", "/m3.tmp", "/m4.tmp", "/m5.tmp", "/m6.tmp", "/m7.tmp",
            "/m8.tmp", "/m9.tmp",
        ];
        {
            let mut state = card.0.borrow_mut();
            state.files.insert(MARKER[0].path, vec![0; 16]);
            for (name, unit) in names.into_iter().zip(&units) {
                state.files.insert(name, split.files()[unit.path].clone());
            }
            // Its finish had begun.
            state.files.remove("/dex.bin");
        }
        let files = card.files();
        assert_eq!(
            report(&card),
            Report {
                source: None,
                result: Err(Stopped::Failed(Step::WriteTemporary, Cause::Io(())))
            }
        );
        assert!(card.files() == files);
    }

    #[test]
    fn a_cut_run_of_the_migration_v0_5_0_shipped_is_finished() {
        let (card, body) = v0_4_0::tests::used(false);
        stage(&mut card.clone(), &mut vec![0; BLOB_SIZE], Source::V0_4_0).unwrap();
        // That release's marker names no way of storing.
        card.0
            .borrow_mut()
            .files
            .insert(MARKER[0].path, vec![0; 16]);
        assert_eq!(
            report(&card),
            Report {
                source: None,
                result: Ok(Done::Converted)
            }
        );
        assert!(only_the_target_files(&card));
        assert!(current(&card).unwrap() == body);
    }

    #[test]
    fn every_result_has_its_two_numbers() {
        let code = |_: ()| 0xd900_458b;
        assert_eq!(numbers(Ok(Done::NoBank), code), [0x600d, 0]);
        assert_eq!(numbers(Ok(Done::Converted), code), [0x600d, 1]);
        assert_eq!(numbers(Ok(Done::AlreadyNew), code), [0x600d, 2]);
        assert_eq!(numbers(Err(Stopped::InProgress), code), [0xbad, 7]);
        assert_eq!(numbers(Err(Stopped::Unsettled), code), [0xbad, 8]);
        for (step, first) in [
            (Step::ReadOld, 0xba1),
            (Step::WriteTemporary, 0xba2),
            (Step::RemoveOld, 0xba3),
            (Step::WriteNew, 0xba4),
        ] {
            let failed = |cause| numbers(Err(Stopped::Failed(step, cause)), code);
            assert_eq!(failed(Cause::Io(())), [first, 0xd900_458b]);
            assert_eq!(failed(Cause::Mismatch), [first, 1]);
            assert_eq!(failed(Cause::Missing), [first, 2]);
            assert_eq!(failed(Cause::Damaged), [first, 5]);
        }
    }

    #[test]
    fn nothing_to_convert_changes_nothing() {
        let empty = Sd::default();
        assert_eq!(convert(&empty), Ok(Done::NoBank));
        assert!(empty.files().is_empty());
        // A Bank of the current release.
        let card = Sd::default();
        let mut bank = BankFiles::new(Final(card.clone()));
        assert!(matches!(bank.open(), Ok(None)));
        bank.initialize(&mut body(0x31, 0x44, 0)).unwrap();
        drop(bank);
        let files = card.files();
        assert_eq!(convert(&card), Ok(Done::AlreadyNew));
        assert!(card.files() == files);
    }

    #[test]
    fn the_line_names_the_way_converted_and_the_release() {
        let text = |source, version: &[u8]| {
            let label: Label = label(source, version)?;
            Some(String::from_utf16(label.units()).unwrap())
        };
        for source in Source::ALL {
            let tag = core::str::from_utf8(source.tag()).unwrap();
            let some = Some(source);
            assert_eq!(text(some, b"v0.6.0"), Some(std::format!("{tag}-0.6.0")));
            assert_eq!(text(some, b"draft"), Some(std::format!("{tag}-draft")));
            // The longest version that fits behind the tag, and what does not.
            let room = Label::ROOM - tag.len() - 1;
            assert!(text(some, &vec![b'1'; room]).is_some());
            assert_eq!(text(some, &vec![b'1'; room + 1]), None);
            assert_eq!(text(some, b"v0.6.0 x"), None);
            // Its marker names it, and no other.
            assert_eq!(
                Source::ALL
                    .iter()
                    .filter(|other| mark(**other) == mark(source))
                    .count(),
                1
            );
        }
        assert_eq!(text(None, b"v0.6.0").as_deref(), Some("v0.6.0-migration"));
        assert_eq!(text(None, b"draft").as_deref(), Some("draft-migration"));
    }
}
