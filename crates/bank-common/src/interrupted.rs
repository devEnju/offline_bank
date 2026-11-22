//! What an interrupted write keeps, and the one rule that settles it
//! (docs/internals.md, "Box files"). It lies in Bank's extdata, beside the
//! journal it belongs to, in a folder named by the two states of the Bank
//! the write goes between, the one before and the one after (`folder`):
//!
//! ```text
//! /AAAAAAAABBBBBBBB/bank00.old   boxes 100, 1, 2, 3 as they were before the write
//! /AAAAAAAABBBBBBBB/bank28.new   boxes 28 to 31 as they are after it
//! /AAAAAAAABBBBBBBB/mover0.old   the transport box or the cells before it
//! /AAAAAAAABBBBBBBB/mover0.new   the transport box or the cells after it
//! ```
//!
//! A file is a group of four places, box files one behind the other with no
//! header: a place's position is its number modulo 4 (box 100 counts as 0),
//! and `mover0` is the transport box and the three cells. A place not
//! included is a box file's size of zeros, which never reads as a box, and
//! places behind the last one included are not written. `.old` is always a
//! box before the write and `.new` a box after it.
//!
//! A save in progress that moved Pokémon both ways is left to Transporter's
//! question, or waits for its game; before Bank writes that into the journal,
//! every box whose Pokémon differ between the save before and the save after
//! (`boxfile::bank_box_contents`) is written twice, as it was and as it
//! became (`back_up`). The transfer screen writes the cells it changes, as
//! they are and as they will be, and proves them before the journal record;
//! it touches a cell only after that record (`transfer_screen::commit`).
//!
//! Once the journal is clean, Bank's opening compares the state it names
//! with the folder's two (`side`; `settle` applies it). Each place says by
//! itself whose it is: positions 1 to 3 of `mover0` are the cells, written
//! only by a transfer save; every other place is a Save and Quit's. Of a Save
//! and Quit, the side the Bank does not hold is kept. A transfer save the
//! journal names as made is finished in the cells, and one it does not name
//! never touched them. Its boxes with Pokémon are kept on both sides, unless
//! the journal names the state before and every cell holds its `.old` box:
//! that is the state before the save, and nothing can be lost.
//!
//! A file that does not read cannot be recovered by keeping it: what reads is
//! copied, and the file goes with the folder. Where the save had happened,
//! the opening then ends once with the error screen (`settle`). What is kept
//! is written to the SD card, into a folder of the same name, as single box
//! files (`bank30.new.box`), each read back and all compared with the folder
//! once more. Only then is the folder deleted, and the one on the SD card
//! renamed to a folder of the date, `YYYYMMDD-HHMMSS-backup/`. A cut at any
//! step is finished by the next opening, and leaves one such folder.

use crate::{
    bank_files::{BankFiles, Files},
    sdfiles::{self, Date, Error, Extdata, Failed, Found, Path, Piece, Sd, Store, ROOT},
    transfer_screen,
};
use offline_core::{
    boxfile::{self, BoxFile, FILE_SIZE, NAME_TEXT},
    native_blob::BOX_COUNT,
    sections::{self, BANK_SIZE, RECORD_SIZE, TRANSPORT_SLOTS},
    Crc32, Part, PendingTransfer, Phase, Save, Sha256,
};

/// 100 boxes and the transport box.
const BITS: usize = BOX_COUNT + 1;
const TRANSPORT_BIT: usize = BOX_COUNT;
/// One box's `boxfile::Contents`.
const PRINT: usize = 32;
/// The places of one file.
pub const GROUP: usize = 4;
/// The files of the boxes of one side.
const BANK_GROUPS: usize = BOX_COUNT / GROUP;
/// The file of the transport box and the three cells.
const MOVER_GROUP: usize = BANK_GROUPS;
const GROUPS: usize = BANK_GROUPS + 1;
/// The characters of a folder's name: two halves of eight.
const NAME: usize = 16;

/// A state of the Bank as the journal names it: the copy of the Bank file
/// and of the transport box in use, each with its checksum. Every save
/// writes the Bank file into its other copy, so the states before and after
/// a save always differ. A delivery of Transporter's has a checksum of its
/// own in the record, and leaves the state as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub bank: Part,
    pub mover: Part,
}

impl State {
    pub fn of(save: &Save) -> Self {
        Self {
            bank: save.bank,
            mover: save.mover,
        }
    }

    /// The state's half of a folder's name (`folder`): in its top bit the
    /// copy of the Bank file (0 or 1), in the other 31 bits those of a
    /// CRC-32 over the copy (0 or 1, as a 32-bit word) and the checksum of
    /// the Bank file, then of the transport box, each little endian. Every
    /// save writes the Bank file into its other copy, so the halves of a
    /// write's two states always differ, in their top bit.
    pub fn half(&self) -> u32 {
        let mut crc = Crc32::new();
        for part in [self.bank, self.mover] {
            crc.update(&(part.slot.index() as u32).to_le_bytes());
            crc.update(&part.crc.to_le_bytes());
        }
        (self.bank.slot.index() as u32) << 31 | crc.finish() & 0x7fff_ffff
    }
}

/// The folder in Bank's extdata of a write between `before` and `after`:
/// `/AAAAAAAABBBBBBBB`, the two states' halves (`State::half`).
pub fn folder(before: State, after: State) -> Path {
    let mut path = Path::new(b"/");
    path.push_hex(before.half()).push_hex(after.half());
    path
}

/// The two halves of a folder's name, if it is of `folder`'s form.
fn halves(name: &[u8]) -> Option<(u32, u32)> {
    let hex = |digits: &[u8]| {
        digits.iter().try_fold(0u32, |value, digit| {
            Some(value << 4 | char::from(*digit).to_digit(16)?)
        })
    };
    (name.len() == NAME).then_some(())?;
    Some((hex(&name[..NAME / 2])?, hex(&name[NAME / 2..])?))
}

/// Which of a folder's two states the journal names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
    /// Neither: the Bank is a version the folder does not know, as one put
    /// there from outside the patches is. Also a name with two equal halves,
    /// which no write gives.
    Unknown,
}

/// The side of the folder `name` that `now`, the state the clean journal
/// names, is.
pub fn side(name: &[u8], now: State) -> Side {
    match halves(name) {
        Some((before, after)) if before != after && now.half() == before => Side::Before,
        Some((before, after)) if before != after && now.half() == after => Side::After,
        _ => Side::Unknown,
    }
}

/// A file of boxes as they were before the write (`.old`).
pub const OLD: u8 = 1;
/// A file of boxes as they are after it (`.new`).
pub const NEW: u8 = 2;
const KINDS: [(u8, &[u8]); 2] = [(OLD, b".old"), (NEW, b".new")];

fn ending(kind: u8) -> &'static [u8] {
    KINDS
        .iter()
        .find_map(|(each, ending)| (*each == kind).then_some(*ending))
        .unwrap_or(b"")
}

/// The file and position of a place: box 1 is position 1 of `bank00`, box
/// 100 its position 0, the transport box position 0 of `mover0` and the
/// cells its positions 1 to 3.
#[cfg(any(test, feature = "test-support"))]
pub fn locate(place: usize) -> (usize, usize) {
    if place < BOX_COUNT {
        let number = (place + 1) % BOX_COUNT;
        (number / GROUP, number % GROUP)
    } else {
        (MOVER_GROUP, place - BOX_COUNT)
    }
}

/// The place at `position` of file `group`.
fn place_at(group: usize, position: usize) -> usize {
    if group == MOVER_GROUP {
        BOX_COUNT + position
    } else {
        (group * GROUP + position + BOX_COUNT - 1) % BOX_COUNT
    }
}

/// `folder`'s file of `group` and `kind`.
fn group_file(folder: &Path, group: usize, kind: u8) -> Path {
    let mut path = *folder;
    path.push(b"/");
    if group == MOVER_GROUP {
        path.push(b"mover0");
    } else {
        path.push(b"bank").push_decimal((group * GROUP) as u32, 2);
    }
    path.push(ending(kind));
    path
}

/// The file of `folder` that holds `place` as it is after the write
/// (`.new`) or was before it (`.old`), and where in it the place starts.
#[cfg(any(test, feature = "test-support"))]
pub fn file(folder: &Path, place: usize, after: bool) -> (Path, u32) {
    let (group, position) = locate(place);
    (
        group_file(folder, group, if after { NEW } else { OLD }),
        (position * FILE_SIZE) as u32,
    )
}

/// `folder`'s file of the cells as they will be (`after`) or are now.
pub fn cells_file(folder: &Path, after: bool) -> Path {
    group_file(folder, MOVER_GROUP, if after { NEW } else { OLD })
}

/// A kept box in a folder of the SD card: `bank30.new.box`, `mover2.old.box`.
pub fn kept_file(folder: &Path, place: usize, kind: u8) -> Path {
    let mut path = *folder;
    path.push(b"/")
        .push_box(place)
        .push(ending(kind))
        .push(b".box");
    path
}

/// The file `name` of a folder: its group and kind.
fn parse(name: &[u8]) -> Option<(usize, u8)> {
    let (stem, kind) = KINDS.iter().find_map(|(kind, ending)| {
        Some((sdfiles::strip_suffix_ignoring_case(name, ending)?, *kind))
    })?;
    let group = (0..GROUPS).find(|group| {
        let path = group_file(&Path::new(b""), *group, kind);
        path.text()[1..path.text().len() - ending(kind).len()].eq_ignore_ascii_case(stem)
    })?;
    Some((group, kind))
}

/// The files of `folder` that are an interrupted write's, by group: the
/// kinds found of each, `OLD | NEW`.
fn entries<X: Extdata>(ext: &mut X, folder: &Path) -> Result<[u8; GROUPS], X::Error> {
    let mut found = [0u8; GROUPS];
    ext.list(folder, &mut |name, is_folder| {
        if let (false, Some((group, kind))) = (is_folder, parse(name)) {
            found[group] |= kind;
        }
    })?;
    Ok(found)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut sum = Sha256::new();
    // A box is far below the hash's length limit.
    let _ = sum.update(bytes);
    sum.finalize()
}

/// The bytes of a file that holds the positions `included`: the length up
/// to the last of them.
fn group_length(included: &[bool; GROUP]) -> u32 {
    let last = included
        .iter()
        .rposition(|place| *place)
        .map_or(0, |at| at + 1);
    (last * FILE_SIZE) as u32
}

/// What fills a position of a file with its box.
type Fill<'a, E, S> = &'a mut dyn FnMut(usize, &mut [u8; FILE_SIZE]) -> Result<(), Error<E, S>>;

/// Makes the file `path` hold the positions `included`, each from `fill`,
/// with zeros at a position before the last that is not included. It is
/// made anew, written whole and read back against what was meant
/// (`group_is`).
#[inline(never)]
pub(crate) fn write_group<X: Extdata, E>(
    ext: &mut X,
    path: &Path,
    included: &[bool; GROUP],
    fill: Fill<'_, E, X::Error>,
    scratch: &mut [u8; FILE_SIZE],
) -> Result<(), Error<E, X::Error>> {
    let length = group_length(included);
    ext.create(path, length).map_err(Error::Sd)?;
    let count = length as usize / FILE_SIZE;
    for (position, is_included) in included.iter().enumerate().take(count) {
        place_bytes(*is_included, position, fill, scratch)?;
        ext.write_at(path, (position * FILE_SIZE) as u32, &scratch[..])
            .map_err(Error::Sd)?;
    }
    if group_is(ext, path, included, fill, scratch)? {
        Ok(())
    } else {
        Err(Error::NotAsWritten)
    }
}

/// What a file holds at `position`: its box, or zeros.
fn place_bytes<E, S>(
    is_included: bool,
    position: usize,
    fill: Fill<'_, E, S>,
    out: &mut [u8; FILE_SIZE],
) -> Result<(), Error<E, S>> {
    if is_included {
        fill(position, out)
    } else {
        out.fill(0);
        Ok(())
    }
}

/// Whether the file `path` holds exactly what `write_group` writes, and ends
/// there; one that does not read does not. Each position is compared by its
/// digest, so that this needs room for one box alone.
#[inline(never)]
fn group_is<X: Extdata, E>(
    ext: &mut X,
    path: &Path,
    included: &[bool; GROUP],
    fill: Fill<'_, E, X::Error>,
    scratch: &mut [u8; FILE_SIZE],
) -> Result<bool, Error<E, X::Error>> {
    let length = group_length(included);
    let count = length as usize / FILE_SIZE;
    for (position, is_included) in included.iter().enumerate().take(count) {
        place_bytes(*is_included, position, fill, scratch)?;
        let meant = digest(&scratch[..]);
        let piece = ext.read_at(path, (position * FILE_SIZE) as u32, &mut scratch[..]);
        if !matches!(piece, Ok(Piece::Read)) || digest(&scratch[..]) != meant {
            return Ok(false);
        }
    }
    Ok(matches!(
        ext.read_at(path, length, &mut scratch[..1]),
        Ok(Piece::Missing)
    ))
}

/// Writes the backup of the save in progress: every file that is not there
/// as it should be, so a START after a cut, or after a file of it was lost,
/// writes what is missing and one that finds it whole writes nothing.
/// Before it writes a file, the journal stops leaving the save to
/// Transporter's question (`make_group`), so that no answer can make the
/// journal clean beside a file being written; the caller marks it again once
/// this has ended. `staging` holds one Bank file at a time and nothing of use
/// afterwards. Of Bank's own files only the journal is written, and only for
/// that.
pub fn back_up<F: Files, X: Extdata>(
    files: &mut BankFiles<F>,
    pending: &PendingTransfer,
    staging: &mut [u8],
    ext: &mut X,
) -> Result<(), Failed<F, X>> {
    let side = RECORD_SIZE * TRANSPORT_SLOTS + TRANSPORT_SLOTS;
    if staging.len() < BANK_SIZE + FILE_SIZE + 2 * side + BOX_COUNT * PRINT {
        return Err(Error::Staging);
    }
    let folder = folder(State::of(&pending.before), State::of(&pending.after));
    let (payload, rest) = staging.split_at_mut(BANK_SIZE);
    let (out, rest) = rest.split_at_mut(FILE_SIZE);
    let out: &mut [u8; FILE_SIZE] = out.try_into().map_err(|_| Error::Staging)?;
    let (old, rest) = rest.split_at_mut(side);
    let (new, rest) = rest.split_at_mut(side);
    let prints = &mut rest[..BOX_COUNT * PRINT];
    let (old_records, old_tags) = old.split_at_mut(RECORD_SIZE * TRANSPORT_SLOTS);
    let (new_records, new_tags) = new.split_at_mut(RECORD_SIZE * TRANSPORT_SLOTS);
    let contents = |payload: &[u8], index| {
        boxfile::bank_box_contents(payload, index).map_err(|_| Error::Staging)
    };

    // Which boxes differ: the save before's, then the save after's compared
    // with them. The transport boxes as `pending_moves` compares them.
    let mut changed = [false; BITS];
    files
        .pending_payload(false, payload)
        .map_err(Error::Files)?;
    for (index, print) in prints.chunks_exact_mut(PRINT).enumerate() {
        print.copy_from_slice(&contents(payload, index)?);
    }
    files.pending_payload(true, payload).map_err(Error::Files)?;
    for (index, print) in prints.chunks_exact(PRINT).enumerate() {
        changed[index] = contents(payload, index)?[..] != *print;
    }
    files
        .pending_transport(false, old_records, old_tags)
        .map_err(Error::Files)?;
    files
        .pending_transport(true, new_records, new_tags)
        .map_err(Error::Files)?;
    let transport_contents = |records: &[u8], tags: &[u8]| {
        boxfile::transport_contents(records, tags).map_err(|_| Error::Staging)
    };
    changed[TRANSPORT_BIT] =
        transport_contents(old_records, old_tags)? != transport_contents(new_records, new_tags)?;
    if !changed.contains(&true) {
        return Ok(());
    }
    ext.create_dir(&folder).map_err(Error::Sd)?;

    // The order: the boxes' `.new` files, their `.old` files, the transport
    // box's `.old` and `.new`; a file that is as it should be is not written.
    if changed[..BOX_COUNT].contains(&true) {
        for kind in [NEW, OLD] {
            if kind == OLD {
                files
                    .pending_payload(false, payload)
                    .map_err(Error::Files)?;
            }
            for group in 0..BANK_GROUPS {
                let included: [bool; GROUP] =
                    core::array::from_fn(|position| changed[place_at(group, position)]);
                if !included.contains(&true) {
                    continue;
                }
                let path = group_file(&folder, group, kind);
                let payload = &*payload;
                let mut fill = |position: usize, to: &mut [u8; FILE_SIZE]| {
                    boxfile::encode_bank_box(payload, place_at(group, position), to)
                        .map_err(|_| Error::Staging)
                };
                make_group(files, ext, &path, &included, &mut fill, out)?;
            }
        }
    }
    if changed[TRANSPORT_BIT] {
        let included = [true, false, false, false];
        for (records, tags, kind) in [
            (&*old_records, &*old_tags, OLD),
            (&*new_records, &*new_tags, NEW),
        ] {
            let path = group_file(&folder, MOVER_GROUP, kind);
            let mut fill = |_: usize, to: &mut [u8; FILE_SIZE]| {
                boxfile::encode_transport(records, tags, &[0; NAME_TEXT], to)
                    .map_err(|_| Error::Staging)
            };
            make_group(files, ext, &path, &included, &mut fill, out)?;
        }
    }
    Ok(())
}

/// Makes a file of the backup hold what it should, unless it does. A file is
/// written only with the save taken away from Transporter's question: the
/// extdata store is let go first, as for every write of the journal.
fn make_group<F: Files, X: Extdata>(
    files: &mut BankFiles<F>,
    ext: &mut X,
    path: &Path,
    included: &[bool; GROUP],
    fill: Fill<'_, <F::Storage as offline_core::Storage>::Error, X::Error>,
    out: &mut [u8; FILE_SIZE],
) -> Result<(), Failed<F, X>> {
    if group_is(ext, path, included, fill, out)? {
        return Ok(());
    }
    ext.rest();
    files.mark(false, false).map_err(Error::Files)?;
    write_group(ext, path, included, fill, out)
}

/// What follows the date in the name of the folder of the SD card that
/// keeps what a settled write leaves: `YYYYMMDD-HHMMSS-backup`.
pub const RETIRED: &[u8] = b"-backup";

/// The folder `name` in the root folder of the SD card.
fn on_card(name: &[u8]) -> Path {
    let mut path = Path::new(ROOT);
    path.push(b"/").push(name);
    path
}

/// Deletes `folder` with every file in it, the `.new` files first: a cut
/// leaves no `.new` box without its `.old`, which the opening would have to
/// take for a change of the cell (`check_cells`), and the side it keeps.
pub fn remove<X: Extdata>(ext: &mut X, folder: &Path) -> Result<(), X::Error> {
    let of_new = |name: &[u8], is_folder: bool| {
        !is_folder && sdfiles::strip_suffix_ignoring_case(name, b".new").is_some()
    };
    while let Some(file) = next(ext, folder, &of_new)? {
        ext.delete(&file)?;
    }
    while let Some(file) = next(ext, folder, &|_, is_folder| !is_folder)? {
        ext.delete(&file)?;
    }
    ext.remove_dir(folder)
}

/// START found the game with one of the save's two images, which decides the
/// save: the game and the Bank then hold every Pokémon, and nothing of a
/// backup is kept. Its folder goes before the journal is settled, with the
/// save taken away from Transporter's question first, so that no folder in
/// part ever stands beside a clean journal. A cut leaves the save in
/// progress: the next START goes on, or writes the backup again if the game
/// is gone by then (`back_up`).
pub fn discard<F: Files, X: Extdata>(
    files: &mut BankFiles<F>,
    pending: &PendingTransfer,
    ext: &mut X,
) -> Result<(), Failed<F, X>> {
    let folder = folder(State::of(&pending.before), State::of(&pending.after));
    let name = &folder.text()[1..];
    let found = next(ext, &Path::new(b"/"), &|entry, is_folder| {
        is_folder && entry.eq_ignore_ascii_case(name)
    })
    .map_err(Error::Sd)?;
    if found.is_none() {
        return Ok(());
    }
    ext.rest();
    files.mark(false, false).map_err(Error::Files)?;
    remove(ext, &folder).map_err(Error::Sd)
}

/// Whenever the journal is clean, once the Bank it names was read whole, so
/// that nothing of a folder goes beside a Bank that cannot be read: at every
/// opening, and after START settled a save in progress. Every folder of
/// Bank's extdata named by two states is settled by the state the journal
/// names (`side`), so that a save of the transfer screen, or a save in
/// progress, makes its own only after this. `work` holds two box files;
/// `date` names the folder what is kept goes to.
///
/// | The journal names | The cells | A transfer save keeps | A Save and Quit keeps |
/// | --- | --- | --- | --- |
/// | the folder's state before | stay | nothing if each holds its `.old`, otherwise both sides | the `.new` boxes |
/// | its state after | finished from `.new` | both sides | the `.old` boxes |
/// | neither (`Side::Unknown`) | stay | both sides | both sides |
///
/// A cell that holds a whole box of neither side changed outside the save and
/// stays. A box without Pokémon of a cell is never kept. A place that does
/// not read is not kept, and goes with the folder. Where the save had
/// happened (`Side::After`, or a Save and Quit's place the journal's state
/// needs kept) the answer is then `Error::NotAsWritten`, once what reads is
/// kept, also when deleting the folder or a later step stops the opening,
/// which the next START goes on with. Each step can be made again.
pub fn settle<F: Files, S: Sd, X: Extdata<Error = S::Error>>(
    files: &mut BankFiles<F>,
    sd: &mut S,
    ext: &mut X,
    work: &mut [u8],
    date: Date,
) -> Result<(), Failed<F, S>> {
    let Phase::Clean(current) = files.phase().map_err(Error::Files)? else {
        return Ok(());
    };
    let now = State::of(&current);
    let (first, second) = work
        .get_mut(..2 * FILE_SIZE)
        .ok_or(Error::Staging)?
        .split_at_mut(FILE_SIZE);
    let first: &mut [u8; FILE_SIZE] = first.try_into().map_err(|_| Error::Staging)?;
    let second: &mut [u8; FILE_SIZE] = second.try_into().map_err(|_| Error::Staging)?;
    let mut damaged = false;
    let ended = settle_folders(sd, ext, now, date, first, second, &mut damaged);
    if damaged {
        Err(Error::NotAsWritten)
    } else {
        ended
    }
}

/// Every folder of a write, then the folders of the SD card. `damaged` is
/// set once what reads of a folder is kept, before it is deleted, so that a
/// stop after that does not hide it.
fn settle_folders<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    now: State,
    date: Date,
    first: &mut [u8; FILE_SIZE],
    second: &mut [u8; FILE_SIZE],
    damaged: &mut bool,
) -> Result<(), Error<E, S::Error>> {
    while let Some(folder) = next(ext, &Path::new(b"/"), &of_a_write).map_err(Error::Sd)? {
        let side = side(&folder.text()[1..], now);
        settle_folder(sd, ext, &folder, side, first, second, damaged)?;
    }
    retire(sd, date).map_err(Error::Sd)
}

/// Whether an entry of a root folder is a folder named by two states.
fn of_a_write(name: &[u8], is_folder: bool) -> bool {
    is_folder && halves(name).is_some()
}

/// The first entry of `folder` that is `wanted`. Each step that settles a
/// folder removes what it found, so the list is read again for the next.
fn next<S: Store>(
    store: &mut S,
    folder: &Path,
    wanted: &dyn Fn(&[u8], bool) -> bool,
) -> Result<Option<Path>, S::Error> {
    let mut found = None;
    store.list(folder, &mut |name, is_folder| {
        if found.is_none() && wanted(name, is_folder) {
            let mut path = *folder;
            if !folder.text().ends_with(b"/") {
                path.push(b"/");
            }
            path.push(name);
            found = Some(path);
        }
    })?;
    Ok(found)
}

/// What a place of a folder's file holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    /// A whole box.
    Whole,
    /// Nothing: no file, a place wholly behind its end, or zeros.
    Nothing,
    /// It does not read, is no box, or the file ends inside it.
    Damaged,
}

/// Reads the place at `position` of `path` into `out`.
fn held_at<X: Extdata>(
    ext: &mut X,
    path: &Path,
    position: usize,
    out: &mut [u8; FILE_SIZE],
) -> Held {
    let at = (position * FILE_SIZE) as u32;
    match ext.read_at(path, at, &mut out[..]) {
        // Too short for a box: whether anything of it is there.
        Ok(Piece::Missing) => match ext.read_at(path, at, &mut out[..1]) {
            Ok(Piece::Missing) => Held::Nothing,
            _ => Held::Damaged,
        },
        Ok(Piece::Read) if BoxFile::parse(&out[..]).is_ok() => Held::Whole,
        Ok(Piece::Read) if out.iter().all(|byte| *byte == 0) => Held::Nothing,
        _ => Held::Damaged,
    }
}

/// `held_at`, with a place that does not read read once more, for a decision
/// that one failed read must not change.
fn held_twice<X: Extdata>(
    ext: &mut X,
    path: &Path,
    position: usize,
    out: &mut [u8; FILE_SIZE],
) -> Held {
    match held_at(ext, path, position, out) {
        Held::Damaged => held_at(ext, path, position, out),
        held => held,
    }
}

/// Whether the folder's `mover0` is a Save and Quit's, which holds the
/// transport box alone and no cells: a side is one box long, and no side
/// reads past that. The size is known without the content. It only tells
/// whether a place that does not read matters: a box that reads is always
/// handled.
fn no_cells<X: Extdata>(
    ext: &mut X,
    folder: &Path,
    found: &[u8; GROUPS],
    byte: &mut [u8; FILE_SIZE],
) -> bool {
    let (mut one_box, mut longer) = (false, false);
    for (kind, _) in KINDS
        .iter()
        .filter(|(kind, _)| found[MOVER_GROUP] & kind != 0)
    {
        let path = group_file(folder, MOVER_GROUP, *kind);
        match ext.read_at(&path, FILE_SIZE as u32, &mut byte[..1]) {
            Ok(Piece::Missing) => one_box = true,
            Ok(Piece::Read) => longer = true,
            Err(_) => {}
        }
    }
    one_box && !longer
}

/// Settles `folder` as `side`: the cells, what is kept on the SD card in a
/// folder of the same name, proven, and then the folder gone, with every file
/// in it. Once what reads is kept, before the delete, `damaged` is set if a
/// place that had to be there did not read. A stop before that leaves the
/// folder whole, and the next START finds the same.
fn settle_folder<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    side: Side,
    first: &mut [u8; FILE_SIZE],
    second: &mut [u8; FILE_SIZE],
    damaged: &mut bool,
) -> Result<(), Error<E, S::Error>> {
    let found = entries(ext, folder).map_err(Error::Sd)?;
    let no_cells = no_cells(ext, folder, &found, first);
    let mut lost = false;
    let keep_cells = match side {
        Side::After => {
            lost = finish_cells(sd, ext, folder, first, second)? && !no_cells;
            true
        }
        Side::Before => {
            let has_new = found[MOVER_GROUP] & NEW != 0;
            !check_cells(sd, ext, folder, has_new, first, second).map_err(Error::Sd)?
        }
        Side::Unknown => true,
    };
    let card = on_card(&folder.text()[1..]);
    // The second pass writes what the first could not read, and only its
    // answer counts.
    keep(
        sd, ext, folder, &card, &found, side, keep_cells, no_cells, first, second,
    )?;
    lost |= keep(
        sd, ext, folder, &card, &found, side, keep_cells, no_cells, first, second,
    )?;
    *damaged |= lost;
    remove(ext, folder).map_err(Error::Sd)
}

/// The journal names the transfer save as made: each cell gets its `.new`
/// box, if it holds its `.old` one, no file, or a file a cut left. A cell
/// that holds another whole box changed outside the save and stays. The
/// answer is whether a place this needed did not read: its cell stays too.
fn finish_cells<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    new: &mut [u8; FILE_SIZE],
    cell: &mut [u8; FILE_SIZE],
) -> Result<bool, Error<E, S::Error>> {
    let (old_file, new_file) = (cells_file(folder, false), cells_file(folder, true));
    let mut damaged = false;
    for position in 1..GROUP {
        match held_at(ext, &new_file, position, new) {
            Held::Nothing => continue,
            Held::Damaged => {
                damaged = true;
                continue;
            }
            Held::Whole => {}
        }
        let path = transfer_screen::cell_path(position - 1);
        let found = sd.get(&path, &mut cell[..]).map_err(Error::Sd)?;
        if found == Found::Read && cell == new {
            continue;
        }
        if found == Found::Read && BoxFile::parse(&cell[..]).is_ok() {
            let held = digest(&cell[..]);
            let was = ext.read_at(&old_file, (position * FILE_SIZE) as u32, &mut cell[..]);
            if !matches!(was, Ok(Piece::Read)) || digest(&cell[..]) != held {
                damaged |= was.is_err();
                continue;
            }
        }
        sdfiles::create_root(sd).map_err(Error::Sd)?;
        sd.put(&path, &new[..]).map_err(Error::Sd)?;
    }
    Ok(damaged)
}

/// The journal does not name the transfer save as made, so it wrote no box
/// into a cell. The answer is whether every cell is exactly as before the
/// save. A file a cut left of the empty box a cell without a file gets goes
/// again, only beside an `.old` place that reads as that box, which is
/// written first. An `.old` place that does not read, with no `.new` file,
/// is what a cut left while it was written: the save reached no cell, and a
/// broken file in one stays. A place that fails is read once more before any
/// of this is decided (`held_twice`).
fn check_cells<S: Sd, X: Extdata<Error = S::Error>>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    has_new: bool,
    old: &mut [u8; FILE_SIZE],
    cell: &mut [u8; FILE_SIZE],
) -> Result<bool, S::Error> {
    let (old_file, new_file) = (cells_file(folder, false), cells_file(folder, true));
    let mut exact = true;
    for position in 1..GROUP {
        let place = held_twice(ext, &old_file, position, old);
        if place == Held::Nothing {
            // A `.new` box without its `.old` shows no state before.
            exact &= held_twice(ext, &new_file, position, old) != Held::Whole;
            continue;
        }
        let path = transfer_screen::cell_path(position - 1);
        let found = sd.get(&path, &mut cell[..])?;
        let whole = found == Found::Read && BoxFile::parse(&cell[..]).is_ok();
        let same = if place == Held::Damaged {
            !has_new
        } else {
            if !whole {
                transfer_screen::empty_box(cell, &[0; NAME_TEXT]);
            }
            cell == old
        };
        if !same {
            exact = false;
        } else if place == Held::Whole && !whole && found != Found::Absent {
            sd.delete(&path)?;
        }
    }
    Ok(exact)
}

/// Every whole box that `side` keeps from `folder`, as a file of `card`,
/// written where it is not there as it should be (`Sd::put` reads it back).
/// The answer is whether a place that had to be there did not read
/// (`settle`).
#[allow(clippy::too_many_arguments)]
fn keep<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    card: &Path,
    found: &[u8; GROUPS],
    side: Side,
    keep_cells: bool,
    no_cells: bool,
    from: &mut [u8; FILE_SIZE],
    to: &mut [u8; FILE_SIZE],
) -> Result<bool, Error<E, S::Error>> {
    let kept_side = match side {
        Side::Before => NEW,
        Side::After => OLD,
        Side::Unknown => OLD | NEW,
    };
    let mut made = false;
    let mut damaged = false;
    for (group, kinds) in found.iter().enumerate() {
        for (kind, _) in KINDS.iter().filter(|(kind, _)| kinds & kind != 0) {
            let path = group_file(folder, group, *kind);
            for position in 0..GROUP {
                let of_a_cell = group == MOVER_GROUP && position > 0;
                let wanted = if of_a_cell {
                    keep_cells
                } else {
                    kept_side & kind != 0
                };
                if !wanted {
                    continue;
                }
                match held_at(ext, &path, position, from) {
                    Held::Nothing => continue,
                    Held::Damaged => {
                        damaged |= match side {
                            Side::After => !(of_a_cell && no_cells),
                            Side::Before => !of_a_cell && (group != MOVER_GROUP || no_cells),
                            Side::Unknown => false,
                        };
                        continue;
                    }
                    Held::Whole => {}
                }
                if of_a_cell && sections::occupied(&from[boxfile::RECORDS]) == 0 {
                    continue;
                }
                let target = kept_file(card, place_at(group, position), *kind);
                let same =
                    sd.get(&target, &mut to[..]).map_err(Error::Sd)? == Found::Read && to == from;
                if same {
                    continue;
                }
                if !made {
                    sdfiles::create_root(sd).map_err(Error::Sd)?;
                    sd.create_dir(card).map_err(Error::Sd)?;
                    made = true;
                }
                sd.put(&target, &from[..]).map_err(Error::Sd)?;
            }
        }
    }
    Ok(damaged)
}

/// Each folder of the SD card's root named by two states holds what a
/// settled write keeps, whole: an empty one goes, any other is renamed, in
/// one step, to a new folder of `date` (`sdfiles::free_dated_folder`).
fn retire<S: Sd>(sd: &mut S, date: Date) -> Result<(), S::Error> {
    while let Some(folder) = next(sd, &Path::new(ROOT), &of_a_write)? {
        if sd.remove_dir(&folder).is_err() {
            let kept = sdfiles::free_dated_folder(sd, date, RETIRED)?;
            sd.rename_dir(&folder, &kept)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use crate::testing::{pokemon, ExtCard, Refused, Shared};
    use offline_core::Slot;
    use std::vec::Vec;

    type Failure = Error<(), Refused>;

    /// The file of cell `position` (1 to 3).
    fn cell_path(position: usize) -> Path {
        transfer_screen::cell_path(position - 1)
    }

    fn state(bank: (Slot, u32), mover: (Slot, u32)) -> State {
        State {
            bank: Part {
                slot: bank.0,
                crc: bank.1,
            },
            mover: Part {
                slot: mover.0,
                crc: mover.1,
            },
        }
    }

    /// A whole box file with a Pokémon, that `id` tells apart.
    fn a_box(id: u8) -> [u8; FILE_SIZE] {
        let mut name = [0u8; NAME_TEXT];
        name[0] = id;
        let mut out = [0u8; FILE_SIZE];
        crate::transfer_screen::empty_box(&mut out, &name);
        out[..RECORD_SIZE].copy_from_slice(&pokemon(u32::from(id)));
        boxfile::seal(&mut out);
        out
    }

    /// A whole box file without Pokémon, that `id` tells apart.
    fn an_empty_box(id: u8) -> [u8; FILE_SIZE] {
        let mut name = [0u8; NAME_TEXT];
        name[0] = id;
        let mut out = [0u8; FILE_SIZE];
        crate::transfer_screen::empty_box(&mut out, &name);
        out
    }

    fn put_group(ext: &mut ExtCard, path: &Path, boxes: [Option<u8>; GROUP]) {
        let included = boxes.map(|one| one.is_some());
        let mut fill = |position: usize, to: &mut [u8; FILE_SIZE]| -> Result<(), Failure> {
            *to = a_box(boxes[position].unwrap());
            Ok(())
        };
        write_group(ext, path, &included, &mut fill, &mut [0; FILE_SIZE]).unwrap();
    }

    fn name_of(folder: &Path) -> Vec<u8> {
        folder.text()[1..].to_vec()
    }

    #[test]
    fn the_names_are_those_of_the_docs() {
        let before = state((Slot::A, 1), (Slot::B, 2));
        let after = state((Slot::B, 3), (Slot::A, 4));
        // The copy and the checksum of each, one after the other.
        let mut bytes = [0u8; 16];
        for (index, value) in [0u32, 1, 1, 2].iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        // The copy in the top bit, over the CRC's other bits.
        assert_eq!(before.half(), offline_core::crc32(&bytes) & 0x7fff_ffff);
        assert_eq!(after.half() >> 31, 1);
        let folder = folder(before, after);
        let mut same = Path::new(b"/");
        same.push_hex(before.half()).push_hex(after.half());
        assert_eq!(folder, same);
        // The longest name the archive stores.
        assert_eq!(
            name_of(&folder).len(),
            crate::bank_files::UnitFile::MAX_NAME
        );
        let name = |place, after| {
            let (path, offset) = file(&folder, place, after);
            (path.text()[folder.text().len()..].to_vec(), offset as usize)
        };
        assert_eq!(name(29, false), (b"/bank28.old".to_vec(), 2 * FILE_SIZE));
        assert_eq!(name(99, true), (b"/bank00.new".to_vec(), 0));
        assert_eq!(name(0, true), (b"/bank00.new".to_vec(), FILE_SIZE));
        assert_eq!(name(TRANSPORT_BIT, true), (b"/mover0.new".to_vec(), 0));
        assert_eq!(
            name(BOX_COUNT + 2, true),
            (b"/mover0.new".to_vec(), 2 * FILE_SIZE)
        );
        assert_eq!(
            cells_file(&folder, false),
            file(&folder, BOX_COUNT + 1, false).0
        );
        assert!(file(&folder, 0, true).0.valid());
        // Each name reads back as its group and kind.
        assert_eq!(parse(b"bank28.old"), Some((7, OLD)));
        assert_eq!(parse(b"bank96.new"), Some((24, NEW)));
        assert_eq!(parse(b"mover0.new"), Some((MOVER_GROUP, NEW)));
        assert_eq!(parse(b"mover0.old"), Some((MOVER_GROUP, OLD)));
        // Names Bank never writes.
        for name in [
            &b"bank01.old"[..],
            b"bank00.bak",
            b"mover0.bak",
            b"mover1.old",
            b"bank00.box",
            b"notes.txt",
        ] {
            assert_eq!(parse(name), None, "{name:?}");
        }
        // What the SD card keeps.
        let mut retired = sdfiles::dated_folder([2026, 11, 16, 14, 3, 7]);
        retired.push(RETIRED);
        assert_eq!(retired.text(), [ROOT, b"/20261116-140307-backup"].concat());
        let kept = |place, kind| {
            let path = kept_file(&retired, place, kind);
            assert!(path.valid());
            path.text()[retired.text().len()..].to_vec()
        };
        assert_eq!(kept(29, NEW), b"/bank30.new.box");
        assert_eq!(kept(99, OLD), b"/bank00.old.box");
        assert_eq!(kept(TRANSPORT_BIT, OLD), b"/mover0.old.box");
        assert_eq!(kept(BOX_COUNT + 2, NEW), b"/mover2.new.box");
    }

    #[test]
    fn every_place_has_one_file_and_position() {
        let mut seen = std::collections::BTreeSet::new();
        for place in 0..BOX_COUNT + 4 {
            let (group, position) = locate(place);
            assert!(group < GROUPS && position < GROUP, "{place}");
            assert_eq!(place_at(group, position), place);
            assert!(seen.insert((group, position)), "{place}");
        }
        assert_eq!(seen.len(), GROUPS * GROUP);
        // Box 100 at the start of the first file, as its number ends in 00.
        assert_eq!(locate(99), (0, 0));
        assert_eq!(locate(2), (0, 3));
        assert_eq!(locate(3), (1, 0));
        assert_eq!(locate(98), (24, 3));
        assert_eq!(locate(TRANSPORT_BIT), (MOVER_GROUP, 0));
        assert_eq!(locate(BOX_COUNT + 3), (MOVER_GROUP, 3));
    }

    #[test]
    fn a_place_of_zeros_is_never_a_box() {
        assert!(BoxFile::parse(&[0u8; FILE_SIZE]).is_err());
        assert_eq!(offline_core::crc32(&[0u8; FILE_SIZE - 4]), 0x37fa_c18f);
    }

    #[test]
    fn a_file_is_its_boxes_one_behind_the_other_up_to_the_last() {
        let disk = Shared::default();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(b"/0000000180000002");
        ext.create_dir(&folder).unwrap();
        // Only box 2: boxes 100 and 1 are zeros, box 3 is not written.
        let (path, offset) = file(&folder, 1, true);
        assert_eq!(offset as usize, 2 * FILE_SIZE);
        put_group(&mut ext, &path, [None, None, Some(7), None]);
        let bytes = disk.0.borrow().ext[path.text()].clone();
        assert_eq!(bytes.len(), 3 * FILE_SIZE);
        assert!(bytes[..2 * FILE_SIZE].iter().all(|byte| *byte == 0));
        assert_eq!(bytes[2 * FILE_SIZE..], a_box(7));
        // All four.
        let (path, _) = file(&folder, 3, false);
        put_group(&mut ext, &path, [Some(1), Some(2), Some(3), Some(4)]);
        let bytes = disk.0.borrow().ext[path.text()].clone();
        assert_eq!(bytes.len(), GROUP * FILE_SIZE);
        for (index, part) in bytes.chunks_exact(FILE_SIZE).enumerate() {
            assert_eq!(part, a_box(index as u8 + 1));
        }
    }

    #[test]
    fn the_worst_case_fits_the_archive() {
        // Every box and the transport box, both sides: 52 files. The
        // simulated archive refuses more than its 100 with the Bank's 8.
        let disk = Shared::default();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(b"/0000000180000002");
        ext.create_dir(&folder).unwrap();
        for kind in [OLD, NEW] {
            for group in 0..BANK_GROUPS {
                put_group(
                    &mut ext,
                    &group_file(&folder, group, kind),
                    [Some(1), Some(2), Some(3), Some(4)],
                );
            }
            put_group(
                &mut ext,
                &group_file(&folder, MOVER_GROUP, kind),
                [Some(1), None, None, None],
            );
        }
        assert_eq!(disk.0.borrow().ext.len(), 52);
    }

    #[test]
    fn the_journal_s_state_tells_the_side_of_a_folder() {
        let before = state((Slot::A, 1), (Slot::B, 2));
        let after = state((Slot::B, 1), (Slot::B, 2));
        // The copy alone tells the two apart, the same checksums: their
        // halves differ in the top bit, whatever their CRCs.
        assert_eq!((before.half() ^ after.half()) >> 31, 1);
        let name = name_of(&folder(before, after));
        assert_eq!(side(&name, before), Side::Before);
        assert_eq!(side(&name, after), Side::After);
        // A delivery leaves the state as it is; any other state is neither.
        assert_eq!(
            side(&name, state((Slot::A, 1), (Slot::A, 2))),
            Side::Unknown
        );
        assert_eq!(
            side(&name, state((Slot::A, 9), (Slot::B, 2))),
            Side::Unknown
        );
        // Lower-case digits read as a list may give them.
        assert_eq!(side(&name.to_ascii_lowercase(), after), Side::After);
        // Two equal halves tell no side.
        let same = name_of(&folder(before, before));
        assert_eq!(side(&same, before), Side::Unknown);
        // Names not of the form.
        assert_eq!(halves(&name[..15]), None);
        assert_eq!(halves(&[name.as_slice(), b"0"].concat()), None);
        assert_eq!(halves(b"G000000000000000"), None);
        assert_eq!(halves(b"0000000-00000000"), None);
    }

    const FOLDER: &[u8] = b"/0000000180000002";

    /// A card with the backup of a save in progress: box 30 and the
    /// transport box, both sides.
    fn backup_card() -> (Shared, Path) {
        let disk = Shared::default();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(FOLDER);
        ext.create_dir(&folder).unwrap();
        let box_30 = |id| [None, None, Some(id), None];
        put_group(&mut ext, &file(&folder, 29, false).0, box_30(1));
        put_group(&mut ext, &file(&folder, 29, true).0, box_30(2));
        let transport = |id| [Some(id), None, None, None];
        put_group(&mut ext, &cells_file(&folder, false), transport(6));
        put_group(&mut ext, &cells_file(&folder, true), transport(3));
        (disk, folder)
    }

    /// A card with a transfer save of cells 1 and 3, before it touched them.
    fn transfer_card() -> (Shared, Path) {
        let disk = Shared::default();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        sd.put(&cell_path(1), &a_box(10)).unwrap();
        sd.put(&cell_path(3), &a_box(11)).unwrap();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(FOLDER);
        ext.create_dir(&folder).unwrap();
        put_group(
            &mut ext,
            &cells_file(&folder, false),
            [None, Some(10), None, Some(11)],
        );
        put_group(
            &mut ext,
            &cells_file(&folder, true),
            [None, Some(20), None, Some(21)],
        );
        (disk, folder)
    }

    /// Settles the folder; the answer is whether damage is reported.
    fn settled(disk: &Shared, folder: &Path, side: Side) -> Result<bool, Failure> {
        let (mut first, mut second) = ([0; FILE_SIZE], [0; FILE_SIZE]);
        let mut damaged = false;
        settle_folder(
            &mut disk.clone(),
            &mut ExtCard(disk.clone()),
            folder,
            side,
            &mut first,
            &mut second,
            &mut damaged,
        )?;
        retire(&mut disk.clone(), [2026, 11, 16, 14, 3, 7]).map_err(Error::Sd)?;
        Ok(damaged)
    }

    const KEPT_AT: &[u8] = b"/20261116-140307-backup";

    /// The names of the folder of the date, with what each holds.
    fn kept_names(disk: &Shared) -> Vec<(Vec<u8>, u8)> {
        let start = [ROOT, KEPT_AT, b"/"].concat();
        disk.0
            .borrow()
            .sd
            .iter()
            .filter_map(|(path, bytes)| {
                Some((path.strip_prefix(start.as_slice())?.to_vec(), bytes[0x1b30]))
            })
            .collect()
    }

    fn names(list: &[(&[u8], u8)]) -> Vec<(Vec<u8>, u8)> {
        let mut names: Vec<_> = list.iter().map(|(n, id)| (n.to_vec(), *id)).collect();
        names.sort();
        names
    }

    fn kept_of_a_backup(side: Side) -> Vec<(Vec<u8>, u8)> {
        names(match side {
            Side::Before => &[(b"bank30.new.box", 2), (b"mover0.new.box", 3)],
            Side::After => &[(b"bank30.old.box", 1), (b"mover0.old.box", 6)],
            Side::Unknown => &[
                (b"bank30.old.box", 1),
                (b"bank30.new.box", 2),
                (b"mover0.old.box", 6),
                (b"mover0.new.box", 3),
            ],
        })
    }

    fn cell_id(disk: &Shared, position: usize) -> Option<u8> {
        let state = disk.0.borrow();
        state.sd.get(cell_path(position).text()).map(|b| b[0x1b30])
    }

    fn no_folder_left(disk: &Shared, what: &str) {
        let state = disk.0.borrow();
        assert!(state.ext.is_empty(), "{what}");
        assert!(state.ext_dirs.is_empty(), "{what}");
        assert!(
            !state.sd_dirs.iter().any(|dir| {
                halves(&dir[dir.iter().rposition(|b| *b == b'/').unwrap() + 1..]).is_some()
            }),
            "{what}"
        );
    }

    fn dated(disk: &Shared) -> usize {
        let state = disk.0.borrow();
        state
            .sd_dirs
            .iter()
            .filter(|dir| dir.ends_with(RETIRED))
            .count()
    }

    #[test]
    fn a_backup_keeps_the_side_the_bank_does_not_hold() {
        for side in [Side::Before, Side::After, Side::Unknown] {
            let (disk, folder) = backup_card();
            settled(&disk, &folder, side).unwrap();
            assert_eq!(kept_names(&disk), kept_of_a_backup(side), "{side:?}");
            no_folder_left(&disk, "backup");
        }
    }

    /// Both sides of cells 1 and 3 of `transfer_card`.
    fn both_sides_of_the_transfer() -> Vec<(Vec<u8>, u8)> {
        names(&[
            (b"mover1.old.box", 10),
            (b"mover1.new.box", 20),
            (b"mover3.old.box", 11),
            (b"mover3.new.box", 21),
        ])
    }

    #[test]
    fn a_transfer_save_never_touched_the_cells_or_is_finished_and_keeps_both_sides() {
        let (disk, folder) = transfer_card();
        assert!(!settled(&disk, &folder, Side::Before).unwrap());
        assert_eq!((cell_id(&disk, 1), cell_id(&disk, 3)), (Some(10), Some(11)));
        assert_eq!(dated(&disk), 0);
        no_folder_left(&disk, "before");

        let (disk, folder) = transfer_card();
        assert!(!settled(&disk, &folder, Side::After).unwrap());
        assert_eq!((cell_id(&disk, 1), cell_id(&disk, 3)), (Some(20), Some(21)));
        assert_eq!(cell_id(&disk, 2), None);
        assert_eq!(kept_names(&disk), both_sides_of_the_transfer());
        no_folder_left(&disk, "after");
    }

    #[test]
    fn a_transfer_save_the_journal_does_not_know_touches_no_cell_and_keeps_both_sides() {
        let (disk, folder) = transfer_card();
        settled(&disk, &folder, Side::Unknown).unwrap();
        assert_eq!(cell_id(&disk, 1), Some(10));
        assert_eq!(cell_id(&disk, 3), Some(11));
        assert_eq!(
            kept_names(&disk),
            names(&[
                (b"mover1.old.box", 10),
                (b"mover1.new.box", 20),
                (b"mover3.old.box", 11),
                (b"mover3.new.box", 21),
            ])
        );
    }

    #[test]
    fn a_cell_of_neither_side_stays_and_both_sides_are_kept() {
        for side in [Side::Before, Side::After] {
            let (disk, folder) = transfer_card();
            disk.clone().put(&cell_path(1), &a_box(99)).unwrap();
            settled(&disk, &folder, side).unwrap();
            assert_eq!(cell_id(&disk, 1), Some(99), "{side:?}");
            assert_eq!(kept_names(&disk), both_sides_of_the_transfer(), "{side:?}");
            // The other cell is settled as ever.
            let three = if side == Side::After { 21 } else { 11 };
            assert_eq!(cell_id(&disk, 3), Some(three), "{side:?}");
            no_folder_left(&disk, "neither");
        }
    }

    /// What cell 1 holds when an opening finds a transfer save of it, from
    /// box 10 to box 20.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Cell {
        Old,
        New,
        NoFile,
        Torn,
        Other,
    }

    impl Cell {
        fn bytes(self) -> Option<std::vec::Vec<u8>> {
            let mut torn = a_box(10);
            torn[0] ^= 1;
            match self {
                Cell::Old => Some(a_box(10).to_vec()),
                Cell::New => Some(a_box(20).to_vec()),
                Cell::NoFile => None,
                Cell::Torn => Some(torn.to_vec()),
                Cell::Other => Some(a_box(99).to_vec()),
            }
        }
    }

    #[test]
    fn every_case_of_the_rule_for_a_transfer_save() {
        use Cell::*;
        let both = names(&[(b"mover1.old.box", 10), (b"mover1.new.box", 20)]);
        let nothing = Vec::new();
        for (side, holds, ends, kept) in [
            (Side::Before, Old, Old, &nothing),
            (Side::Before, New, New, &both),
            (Side::Before, NoFile, NoFile, &both),
            (Side::Before, Torn, Torn, &both),
            (Side::Before, Other, Other, &both),
            (Side::After, Old, New, &both),
            (Side::After, New, New, &both),
            (Side::After, NoFile, New, &both),
            (Side::After, Torn, New, &both),
            (Side::After, Other, Other, &both),
            (Side::Unknown, Old, Old, &both),
            (Side::Unknown, New, New, &both),
            (Side::Unknown, NoFile, NoFile, &both),
            (Side::Unknown, Torn, Torn, &both),
            (Side::Unknown, Other, Other, &both),
        ] {
            let what = std::format!("{side:?}, {holds:?}");
            let disk = Shared::default();
            let mut sd = disk.clone();
            sdfiles::create_root(&mut sd).unwrap();
            if let Some(bytes) = holds.bytes() {
                sd.put(&cell_path(1), &bytes).unwrap();
            }
            let mut ext = ExtCard(disk.clone());
            let folder = Path::new(FOLDER);
            ext.create_dir(&folder).unwrap();
            let one = |id| [None, Some(id), None, None];
            put_group(&mut ext, &cells_file(&folder, false), one(10));
            put_group(&mut ext, &cells_file(&folder, true), one(20));
            assert!(!settled(&disk, &folder, side).unwrap(), "{what}");
            let cell = disk.0.borrow().sd.get(cell_path(1).text()).cloned();
            assert_eq!(cell, ends.bytes(), "{what}");
            assert_eq!(kept_names(&disk), *kept, "{what}");
            no_folder_left(&disk, &what);
        }
    }

    #[test]
    fn a_box_without_pokemon_of_a_cell_is_never_kept() {
        // A Pokémon goes into an empty cell, then the journal names the save:
        // only the side with the Pokémon is kept.
        let disk = Shared::default();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        sd.put(&cell_path(1), &an_empty_box(7)).unwrap();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(FOLDER);
        ext.create_dir(&folder).unwrap();
        let mut fill = |_: usize, to: &mut [u8; FILE_SIZE]| -> Result<(), Failure> {
            *to = an_empty_box(7);
            Ok(())
        };
        write_group(
            &mut ext,
            &cells_file(&folder, false),
            &[false, true, false, false],
            &mut fill,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        put_group(
            &mut ext,
            &cells_file(&folder, true),
            [None, Some(20), None, None],
        );
        settled(&disk, &folder, Side::After).unwrap();
        assert_eq!(cell_id(&disk, 1), Some(20));
        assert_eq!(kept_names(&disk), names(&[(b"mover1.new.box", 20)]));
    }

    #[test]
    fn a_cell_a_cut_left_is_finished_or_cleared() {
        // After: no file, or a file that is no box, gets its `.new`.
        for torn in [false, true] {
            let (disk, folder) = transfer_card();
            if torn {
                disk.0.borrow_mut().sd.get_mut(cell_path(1).text()).unwrap()[0] ^= 1;
            } else {
                disk.clone().delete(&cell_path(1)).unwrap();
            }
            settled(&disk, &folder, Side::After).unwrap();
            assert_eq!(cell_id(&disk, 1), Some(20), "torn {torn}");
            assert_eq!(kept_names(&disk), both_sides_of_the_transfer());
        }
        // Before: the empty box a cell without a file got, cut short, goes.
        let (disk, folder) = placeholder_card();
        settled(&disk, &folder, Side::Before).unwrap();
        assert_eq!(cell_id(&disk, 2), None);
        assert_eq!(dated(&disk), 0);
    }

    /// A transfer save of cell 2, which had no file, cut while its empty box
    /// was written: the file is torn, and no `.new` follows.
    fn placeholder_card() -> (Shared, Path) {
        let disk = Shared::default();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        let mut empty = [0u8; FILE_SIZE];
        crate::transfer_screen::empty_box(&mut empty, &[0; NAME_TEXT]);
        let mut torn = empty;
        torn[0] ^= 1;
        sd.put(&cell_path(2), &torn).unwrap();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(FOLDER);
        ext.create_dir(&folder).unwrap();
        let mut fill = |_: usize, to: &mut [u8; FILE_SIZE]| -> Result<(), Failure> {
            *to = empty;
            Ok(())
        };
        write_group(
            &mut ext,
            &cells_file(&folder, false),
            &[false, false, true, false],
            &mut fill,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        (disk, folder)
    }

    #[test]
    fn a_cut_at_every_step_of_settling_ends_the_same_with_at_most_one_folder_of_the_date() {
        for (card, of_a_backup) in [
            (backup_card as fn() -> (Shared, Path), true),
            (transfer_card, false),
        ] {
            for side in [Side::Before, Side::After, Side::Unknown] {
                let (base, folder) = card();
                let counting = base.cut_after(usize::MAX);
                settled(&counting, &folder, side).unwrap();
                let total = counting.0.borrow().operations;
                let ended = kept_names(&counting);
                let cells = [cell_id(&counting, 1), cell_id(&counting, 3)];
                if of_a_backup {
                    assert_eq!(ended, kept_of_a_backup(side));
                }
                for cut in 0..total {
                    let what = std::format!("{of_a_backup} {side:?}, cut {cut}");
                    let disk = base.cut_after(cut);
                    assert!(settled(&disk, &folder, side).is_err(), "{what}");
                    let disk = disk.reboot();
                    settled(&disk, &folder, side).unwrap();
                    assert_eq!(kept_names(&disk), ended, "{what}");
                    assert_eq!([cell_id(&disk, 1), cell_id(&disk, 3)], cells, "{what}");
                    assert_eq!(dated(&disk), usize::from(!ended.is_empty()), "{what}");
                    no_folder_left(&disk, &what);
                }
            }
        }
    }

    #[test]
    fn a_file_that_does_not_read_goes_with_its_folder_and_is_reported_where_the_save_happened() {
        type Card = fn() -> (Shared, Path);
        type Damaged = fn(&Path) -> Path;
        type Kept = &'static [(&'static [u8], u8)];
        let cases: [(Card, Side, Damaged, bool, Kept); 9] = [
            // A Save and Quit whose journal is settled: the side kept.
            (
                backup_card,
                Side::Before,
                |f| file(f, 29, true).0,
                true,
                &[(b"mover0.new.box", 3)],
            ),
            (
                backup_card,
                Side::After,
                |f| file(f, 29, false).0,
                true,
                &[(b"mover0.old.box", 6)],
            ),
            (
                backup_card,
                Side::Unknown,
                |f| file(f, 29, false).0,
                false,
                &[
                    (b"bank30.new.box", 2),
                    (b"mover0.old.box", 6),
                    (b"mover0.new.box", 3),
                ],
            ),
            // Its transport box, in a `mover0` one box long.
            (
                backup_card,
                Side::Before,
                |f| cells_file(f, true),
                true,
                &[(b"bank30.new.box", 2)],
            ),
            // A transfer save named as made.
            (
                transfer_card,
                Side::After,
                |f| cells_file(f, true),
                true,
                &[(b"mover1.old.box", 10), (b"mover3.old.box", 11)],
            ),
            (
                transfer_card,
                Side::After,
                |f| cells_file(f, false),
                true,
                &[(b"mover1.new.box", 20), (b"mover3.new.box", 21)],
            ),
            // A transfer save that did not happen.
            (
                transfer_card,
                Side::Before,
                |f| cells_file(f, false),
                false,
                &[(b"mover1.new.box", 20), (b"mover3.new.box", 21)],
            ),
            (
                transfer_card,
                Side::Before,
                |f| cells_file(f, true),
                false,
                &[],
            ),
            (
                transfer_card,
                Side::Unknown,
                |f| cells_file(f, true),
                false,
                &[(b"mover1.old.box", 10), (b"mover3.old.box", 11)],
            ),
        ];
        for (index, (card, side, damaged, reported, kept)) in cases.into_iter().enumerate() {
            let what = std::format!("case {index}, {side:?}");
            let (disk, folder) = card();
            let path = damaged(&folder);
            disk.0.borrow_mut().ext_damaged.insert(path.text().to_vec());
            let cells = (cell_id(&disk, 1), cell_id(&disk, 3));
            assert_eq!(settled(&disk, &folder, side).unwrap(), reported, "{what}");
            assert_eq!(kept_names(&disk), names(kept), "{what}");
            // No cell gets anything that was not read.
            assert_eq!((cell_id(&disk, 1), cell_id(&disk, 3)), cells, "{what}");
            no_folder_left(&disk, &what);
        }
    }

    #[test]
    fn a_place_that_is_no_box_is_reported_and_the_other_cell_is_finished() {
        let (disk, folder) = transfer_card();
        let new = cells_file(&folder, true);
        disk.0.borrow_mut().ext.get_mut(new.text()).unwrap()[FILE_SIZE] ^= 1;
        assert!(settled(&disk, &folder, Side::After).unwrap());
        assert_eq!(cell_id(&disk, 1), Some(10));
        assert_eq!(cell_id(&disk, 3), Some(21));
        assert_eq!(
            kept_names(&disk),
            names(&[
                (b"mover1.old.box", 10),
                (b"mover3.old.box", 11),
                (b"mover3.new.box", 21),
            ])
        );
        no_folder_left(&disk, "no box");
    }

    /// How many reads settling the card makes.
    fn reads_of(card: fn() -> (Shared, Path), side: Side) -> usize {
        let (disk, folder) = card();
        settled(&disk, &folder, side).unwrap();
        let reads = disk.0.borrow().reads;
        reads
    }

    /// Settles until it ends, as the next START does after a stop.
    fn settled_to_the_end(disk: &Shared, folder: &Path, side: Side) -> bool {
        match settled(disk, folder, side) {
            Ok(reported) => reported,
            Err(_) => {
                disk.0.borrow_mut().fail_reads.clear();
                settled(disk, folder, side).unwrap()
            }
        }
    }

    fn only_pokemon_into_an_empty_cell() -> (Shared, Path) {
        let disk = Shared::default();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        sd.put(&cell_path(1), &an_empty_box(7)).unwrap();
        let mut ext = ExtCard(disk.clone());
        let folder = Path::new(FOLDER);
        ext.create_dir(&folder).unwrap();
        let mut fill = |_: usize, to: &mut [u8; FILE_SIZE]| -> Result<(), Failure> {
            *to = an_empty_box(7);
            Ok(())
        };
        write_group(
            &mut ext,
            &cells_file(&folder, false),
            &[false, true, false, false],
            &mut fill,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        put_group(
            &mut ext,
            &cells_file(&folder, true),
            [None, Some(20), None, None],
        );
        (disk, folder)
    }

    #[test]
    fn defect_1_a_read_that_fails_once_never_loses_the_only_copy_of_a_transfer() {
        let reads = reads_of(only_pokemon_into_an_empty_cell, Side::After);
        assert!(reads > 0);
        for read in 0..reads {
            let (disk, folder) = only_pokemon_into_an_empty_cell();
            disk.0.borrow_mut().fail_reads.insert(read);
            settled_to_the_end(&disk, &folder, Side::After);
            let kept = kept_names(&disk).iter().any(|(_, id)| *id == 20);
            assert!(
                cell_id(&disk, 1) == Some(20) || kept,
                "read {read}: the Pokémon is in neither"
            );
            no_folder_left(&disk, "defect 1");
        }
    }

    #[test]
    fn defect_2_a_failed_read_leaves_no_torn_placeholder_behind() {
        let reads = reads_of(placeholder_card, Side::Before);
        assert!(reads > 0);
        for read in 0..reads {
            let (disk, folder) = placeholder_card();
            disk.0.borrow_mut().fail_reads.insert(read);
            settled_to_the_end(&disk, &folder, Side::Before);
            assert_eq!(cell_id(&disk, 2), None, "read {read}");
            no_folder_left(&disk, "defect 2");
        }
    }

    #[test]
    fn defect_3_a_box_that_stops_reading_while_it_is_kept_is_reported() {
        let third = || {
            let (disk, folder) = transfer_card();
            disk.clone().put(&cell_path(1), &a_box(99)).unwrap();
            (disk, folder)
        };
        let reads = reads_of(third, Side::After);
        for first in 0..reads {
            for second in first..reads {
                let (disk, folder) = third();
                disk.0.borrow_mut().fail_reads.extend([first, second]);
                let reported = settled_to_the_end(&disk, &folder, Side::After);
                let kept = kept_names(&disk);
                let whole = both_sides_of_the_transfer()
                    .iter()
                    .all(|name| kept.contains(name));
                assert_eq!(cell_id(&disk, 1), Some(99), "reads {first}, {second}");
                assert!(whole || reported, "reads {first}, {second}: {kept:?}");
                no_folder_left(&disk, "defect 3");
            }
        }
    }

    /// A transfer save whose `.old` does not read, with a broken cell 1.
    fn broken_cell_card() -> (Shared, Path) {
        let (disk, folder) = transfer_card();
        disk.clone().put(&cell_path(1), &[0x5a; FILE_SIZE]).unwrap();
        let old = cells_file(&folder, false);
        disk.0.borrow_mut().ext_damaged.insert(old.text().to_vec());
        (disk, folder)
    }

    fn holds_the_broken_cell(disk: &Shared) -> bool {
        let state = disk.0.borrow();
        state
            .sd
            .get(cell_path(1).text())
            .is_some_and(|file| file[..] == [0x5a; FILE_SIZE])
    }

    #[test]
    fn a_broken_cell_beside_an_old_place_that_does_not_read_stays() {
        let (disk, folder) = broken_cell_card();
        ExtCard(disk.clone())
            .delete(&cells_file(&folder, true))
            .unwrap();
        assert!(!settled(&disk, &folder, Side::Before).unwrap());
        assert!(holds_the_broken_cell(&disk));
        no_folder_left(&disk, "broken cell");
    }

    #[test]
    fn a_cut_in_the_cleanup_does_not_change_what_happens_to_a_broken_cell() {
        let mut between = 0;
        for budget in 0..400 {
            let (disk, folder) = broken_cell_card();
            disk.0.borrow_mut().budget = Some(budget);
            let ended = settled(&disk, &folder, Side::Before);
            disk.0.borrow_mut().budget = None;
            let (old, new) = (cells_file(&folder, false), cells_file(&folder, true));
            let left = {
                let state = disk.0.borrow();
                state.ext.contains_key(old.text()) && !state.ext.contains_key(new.text())
            };
            if ended.is_err() && left {
                between += 1;
                settled(&disk, &folder, Side::Before).unwrap();
                assert!(holds_the_broken_cell(&disk), "budget {budget}");
            }
        }
        assert!(between > 0);
    }

    #[test]
    fn one_failed_read_of_the_comparison_keeps_the_boxes_that_read() {
        // Cell 1 holds its `.old` box; `.old` ends at the start of cell 3's.
        fn card() -> (Shared, Path) {
            let (disk, folder) = transfer_card();
            let old = cells_file(&folder, false);
            let mut state = disk.0.borrow_mut();
            state
                .ext
                .get_mut(old.text())
                .unwrap()
                .truncate(3 * FILE_SIZE);
            state.ext_written.insert(old.text().to_vec(), 3 * FILE_SIZE);
            drop(state);
            (disk, folder)
        }
        for read in 0..reads_of(card, Side::Before) {
            let (disk, folder) = card();
            disk.0.borrow_mut().fail_reads.insert(read);
            settled_to_the_end(&disk, &folder, Side::Before);
            let kept = kept_names(&disk);
            for name in [(&b"mover1.old.box"[..], 10), (b"mover1.new.box", 20)] {
                assert!(kept.contains(&(name.0.to_vec(), name.1)), "read {read}");
            }
        }
    }

    #[test]
    fn a_new_box_without_its_old_one_is_never_taken_for_no_change() {
        // The cell holds another box, and `.old` is gone or ends one byte
        // short of the box: the readable `.new` is kept.
        for short in [false, true] {
            let (disk, folder) = transfer_card();
            disk.clone().put(&cell_path(1), &a_box(99)).unwrap();
            disk.clone().put(&cell_path(3), &a_box(98)).unwrap();
            let old = cells_file(&folder, false);
            if short {
                let mut state = disk.0.borrow_mut();
                let file = state.ext.get_mut(old.text()).unwrap();
                file.truncate(4 * FILE_SIZE - 1);
                let length = file.len();
                state.ext_written.insert(old.text().to_vec(), length);
            } else {
                ExtCard(disk.clone()).delete(&old).unwrap();
            }
            settled(&disk, &folder, Side::Before).unwrap();
            let kept = kept_names(&disk);
            assert!(kept.contains(&(b"mover3.new.box".to_vec(), 21)), "{kept:?}");
            no_folder_left(&disk, "no old");
        }
    }

    #[test]
    fn a_new_file_that_ends_inside_a_box_is_reported() {
        let (disk, folder) = transfer_card();
        let new = cells_file(&folder, true);
        {
            let mut state = disk.0.borrow_mut();
            let file = state.ext.get_mut(new.text()).unwrap();
            file.truncate(4 * FILE_SIZE - 1);
            let length = file.len();
            state.ext_written.insert(new.text().to_vec(), length);
        }
        assert!(settled(&disk, &folder, Side::After).unwrap());
        assert_eq!(cell_id(&disk, 1), Some(20));
        assert_eq!(cell_id(&disk, 3), Some(11));
        no_folder_left(&disk, "short");
    }

    #[test]
    fn a_stop_after_a_lost_place_does_not_hide_it() {
        // Wherever the card stops once the damaged file is gone, in the rest
        // of the delete or at the rename of the folder on the SD card, the
        // answer is the damage.
        let before = state((Slot::A, 1), (Slot::B, 2));
        let after = state((Slot::B, 3), (Slot::A, 4));
        let card = || {
            let disk = Shared::default();
            let mut ext = ExtCard(disk.clone());
            let name = folder(before, after);
            ext.create_dir(&name).unwrap();
            let box_30 = |id| [None, None, Some(id), None];
            put_group(&mut ext, &file(&name, 29, false).0, box_30(1));
            put_group(&mut ext, &file(&name, 29, true).0, box_30(2));
            put_group(
                &mut ext,
                &file(&name, 5, false).0,
                [None, None, Some(3), None],
            );
            put_group(
                &mut ext,
                &file(&name, 5, true).0,
                [None, None, Some(4), None],
            );
            let lost = file(&name, 29, false).0;
            disk.0.borrow_mut().ext_damaged.insert(lost.text().to_vec());
            disk
        };
        let run = |disk: &Shared| {
            let (mut first, mut second) = ([0; FILE_SIZE], [0; FILE_SIZE]);
            let mut damaged = false;
            let ended: Result<(), Failure> = settle_folders(
                &mut disk.clone(),
                &mut ExtCard(disk.clone()),
                after,
                [2026, 11, 16, 14, 3, 7],
                &mut first,
                &mut second,
                &mut damaged,
            );
            (ended.is_err(), damaged)
        };
        let counting = card().cut_after(usize::MAX);
        assert_eq!(run(&counting), (false, true));
        let total = counting.0.borrow().operations;
        let lost = file(&folder(before, after), 29, false).0;
        let mut stopped_after_the_file = false;
        for cut in 0..total {
            let disk = card().cut_after(cut);
            let (stopped, damaged) = run(&disk);
            assert!(stopped, "cut {cut}");
            if !disk.0.borrow().ext.contains_key(lost.text()) {
                stopped_after_the_file = true;
                assert!(damaged, "cut {cut}");
            }
        }
        assert!(stopped_after_the_file);
    }

    #[test]
    fn a_short_old_file_never_hides_a_readable_new_box() {
        // The cell was deleted for its write, and `.old` lost all but its
        // first box of zeros: `.new` still goes into the cell.
        let (disk, folder) = transfer_card();
        disk.clone().delete(&cell_path(1)).unwrap();
        let old = cells_file(&folder, false);
        {
            let mut state = disk.0.borrow_mut();
            state.ext.get_mut(old.text()).unwrap().truncate(FILE_SIZE);
            state.ext_written.insert(old.text().to_vec(), FILE_SIZE);
        }
        settled(&disk, &folder, Side::After).unwrap();
        assert_eq!(cell_id(&disk, 1), Some(20));
        assert!(kept_names(&disk).contains(&(b"mover1.new.box".to_vec(), 20)));
        no_folder_left(&disk, "short old");
    }

    #[test]
    fn a_full_card_beside_a_lost_place_says_so_and_the_loss_is_reported_once() {
        let before = state((Slot::A, 1), (Slot::B, 2));
        let after = state((Slot::B, 3), (Slot::A, 4));
        let disk = Shared::default();
        let name = folder(before, after);
        let mut ext = ExtCard(disk.clone());
        ext.create_dir(&name).unwrap();
        put_group(
            &mut ext,
            &cells_file(&name, false),
            [None, Some(10), None, Some(11)],
        );
        put_group(
            &mut ext,
            &cells_file(&name, true),
            [None, Some(20), None, Some(21)],
        );
        let new = cells_file(&name, true);
        disk.0.borrow_mut().ext_damaged.insert(new.text().to_vec());
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        sd.put(&cell_path(1), &a_box(10)).unwrap();
        sd.put(&cell_path(3), &a_box(11)).unwrap();
        let run = |disk: &Shared| {
            let (mut first, mut second) = ([0; FILE_SIZE], [0; FILE_SIZE]);
            let mut damaged = false;
            let ended: Result<(), Failure> = settle_folders(
                &mut disk.clone(),
                &mut ExtCard(disk.clone()),
                after,
                [2026, 11, 16, 14, 3, 7],
                &mut first,
                &mut second,
                &mut damaged,
            );
            (ended, damaged)
        };
        disk.0.borrow_mut().sd_full = true;
        let (ended, damaged) = run(&disk);
        assert!(matches!(ended, Err(Error::Sd(error)) if error.full));
        assert!(!damaged);
        assert!(!disk.0.borrow().ext_dirs.is_empty());
        disk.0.borrow_mut().sd_full = false;
        let (ended, damaged) = run(&disk);
        assert!(ended.is_ok() && damaged);
        let (ended, damaged) = run(&disk);
        assert!(ended.is_ok() && !damaged);
    }

    #[test]
    fn an_earlier_copy_on_the_card_is_no_proof_for_a_place_that_does_not_read() {
        let (disk, folder) = backup_card();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        let held = on_card(&folder.text()[1..]);
        sd.create_dir(&held).unwrap();
        sd.put(&kept_file(&held, 29, OLD), &a_box(77)).unwrap();
        let old = file(&folder, 29, false).0;
        disk.0.borrow_mut().ext_damaged.insert(old.text().to_vec());
        assert!(settled(&disk, &folder, Side::After).unwrap());
    }

    #[test]
    fn the_probe_s_folder_is_never_taken_for_an_interrupted_write() {
        let counting = ExtCard(Shared::default().cut_after(usize::MAX));
        let mut ext = ExtCard(counting.0.clone());
        assert!(sdfiles::probe_extdata(&mut ext).unwrap());
        let total = counting.0 .0.borrow().operations;
        let mut seen = false;
        for cut in 0..total {
            let disk = Shared::default().cut_after(cut);
            assert!(sdfiles::probe_extdata(&mut ExtCard(disk.clone())).is_err());
            for dir in &disk.0.borrow().ext_dirs {
                seen = true;
                assert_eq!(dir.len(), NAME + 1, "cut {cut}");
                assert!(!of_a_write(&dir[1..], true), "cut {cut}");
            }
        }
        assert!(seen);
    }

    #[test]
    fn a_read_that_fails_in_the_first_pass_is_written_by_the_second() {
        // Each read in turn, on a folder the journal does not name: a read
        // that fails once ends quietly, with everything kept.
        let reads = reads_of(backup_card, Side::Unknown);
        for read in 0..reads {
            let (disk, folder) = backup_card();
            disk.0.borrow_mut().fail_reads.insert(read);
            match settled(&disk, &folder, Side::Unknown) {
                Ok(reported) => {
                    assert!(!reported, "read {read}");
                    assert_eq!(
                        kept_names(&disk),
                        kept_of_a_backup(Side::Unknown),
                        "read {read}"
                    );
                    no_folder_left(&disk, "second pass");
                }
                // Only a read of the SD card itself can stop it, with the
                // folder there for the next START.
                Err(_) => assert!(!disk.0.borrow().ext_dirs.is_empty(), "read {read}"),
            }
        }
    }

    #[test]
    fn an_unneeded_file_of_a_save_and_quit_that_does_not_open_is_not_reported() {
        let (disk, folder) = backup_card();
        let new = cells_file(&folder, true);
        disk.0
            .borrow_mut()
            .ext_unopenable
            .insert(new.text().to_vec());
        assert!(!settled(&disk, &folder, Side::After).unwrap());
        assert_eq!(kept_names(&disk), kept_of_a_backup(Side::After));
        no_folder_left(&disk, "unneeded");
    }

    #[test]
    fn a_transport_box_of_a_save_and_quit_that_does_not_open_is_reported() {
        // The file's own length is unknown: `.old` shows whose it is.
        let (disk, folder) = backup_card();
        let new = cells_file(&folder, true);
        disk.0
            .borrow_mut()
            .ext_unopenable
            .insert(new.text().to_vec());
        assert!(settled(&disk, &folder, Side::Before).unwrap());
        assert_eq!(kept_names(&disk), names(&[(b"bank30.new.box", 2)]));
    }

    #[test]
    fn every_delete_of_a_folder_takes_new_before_old() {
        let (base, folder) = transfer_card();
        let counting = base.cut_after(usize::MAX);
        remove(&mut ExtCard(counting.clone()), &folder).unwrap();
        let (old, new) = (cells_file(&folder, false), cells_file(&folder, true));
        for cut in 0..counting.0.borrow().operations {
            let disk = base.cut_after(cut);
            assert!(remove(&mut ExtCard(disk.clone()), &folder).is_err());
            let state = disk.0.borrow();
            assert!(
                state.ext.contains_key(old.text()) || !state.ext.contains_key(new.text()),
                "cut {cut}"
            );
        }
    }

    #[test]
    fn what_a_cut_left_of_a_transfer_save_the_journal_does_not_name_is_never_read() {
        // `mover0.new` torn, or `mover0.old` torn with no `mover0.new` yet.
        for old in [false, true] {
            let (disk, folder) = transfer_card();
            if old {
                ExtCard(disk.clone())
                    .delete(&cells_file(&folder, true))
                    .unwrap();
            }
            let path = cells_file(&folder, !old);
            disk.0.borrow_mut().ext_damaged.insert(path.text().to_vec());
            assert!(!settled(&disk, &folder, Side::Before).unwrap());
            assert_eq!(cell_id(&disk, 1), Some(10), "old {old}");
            assert_eq!(cell_id(&disk, 3), Some(11), "old {old}");
            assert_eq!(dated(&disk), 0, "old {old}");
            no_folder_left(&disk, "torn");
        }
    }

    #[test]
    fn nothing_goes_while_the_card_has_no_room_for_what_is_kept() {
        let (disk, folder) = backup_card();
        let before = disk.0.borrow().ext.clone();
        disk.0.borrow_mut().sd_full = true;
        let failed = settled(&disk, &folder, Side::After);
        assert!(matches!(failed, Err(Error::Sd(error)) if error.full));
        assert!(disk.0.borrow().ext == before);
        disk.0.borrow_mut().sd_full = false;
        settled(&disk, &folder, Side::After).unwrap();
        assert_eq!(kept_names(&disk), kept_of_a_backup(Side::After));
    }

    #[test]
    fn a_file_of_no_patch_goes_with_its_folder() {
        let (disk, folder) = backup_card();
        let mut foreign = folder;
        foreign.push(b"/notes.txt");
        let mut ext = ExtCard(disk.clone());
        crate::sdfiles::Extdata::create(&mut ext, &foreign, 4).unwrap();
        settled(&disk, &folder, Side::After).unwrap();
        assert_eq!(kept_names(&disk), kept_of_a_backup(Side::After));
        no_folder_left(&disk, "foreign");
    }

    #[test]
    fn a_folder_on_the_card_without_its_folder_in_extdata_is_only_renamed() {
        // A cut after the folder in extdata went, before the rename.
        let disk = Shared::default();
        let mut sd = disk.clone();
        sdfiles::create_root(&mut sd).unwrap();
        let held = on_card(b"0000000180000002");
        sd.create_dir(&held).unwrap();
        sd.put(&kept_file(&held, 29, OLD), &a_box(1)).unwrap();
        // An empty one goes, and no folder of the date is made for it.
        let empty = on_card(b"0000000380000004");
        sd.create_dir(&empty).unwrap();
        retire(&mut sd, [2026, 11, 16, 14, 3, 7]).unwrap();
        let state = disk.0.borrow();
        assert!(!state.sd_dirs.contains(held.text()));
        assert!(!state.sd_dirs.contains(empty.text()));
        drop(state);
        assert_eq!(kept_names(&disk), [(b"bank30.old.box".to_vec(), 1)]);
    }
}
