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
//! with the folder's two (`side`; `settle` applies it). A transfer save the
//! journal names as made is finished in the cells; one it does not name
//! never touched them. A cell that holds a box of neither side changed
//! outside the save: it stays, and both sides are kept. Of a save in
//! progress, the side the Bank does not hold is kept. What is kept is written
//! to the SD card, into a folder of the same name, as single box files
//! (`bank30.new.box`), each read back and all compared with the folder once
//! more. Only then is the folder deleted, and the one on the SD card renamed
//! to a folder of the date, `YYYYMMDD-HHMMSS-backup/`. A cut at any step is
//! finished by the next opening, and leaves one such folder.

use crate::{
    bank_files::{BankFiles, Files},
    sdfiles::{self, Date, Error, Extdata, Failed, Found, Path, Piece, Sd, Store, ROOT},
};
use offline_core::{
    boxfile::{self, BoxFile, FILE_SIZE, NAME_TEXT},
    native_blob::BOX_COUNT,
    sections::{BANK_SIZE, RECORD_SIZE, TRANSPORT_SLOTS},
    Crc32, Part, Phase, Save, Sha256,
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
    staging: &mut [u8],
    ext: &mut X,
) -> Result<(), Failed<F, X>> {
    let Phase::Prepared(pending) = files.phase().map_err(Error::Files)? else {
        return Ok(());
    };
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
    files.unmark().map_err(Error::Files)?;
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

/// Deletes `folder` with every file in it.
pub fn remove<X: Extdata>(ext: &mut X, folder: &Path) -> Result<(), X::Error> {
    while let Some(file) = next(ext, folder, &|_, is_folder| !is_folder)? {
        ext.delete(&file)?;
    }
    ext.remove_dir(folder)
}

/// START found the game with one of the save's two images, which decides the
/// save: the game and the Bank then hold every Pokémon, and nothing of a
/// backup is kept. A backup a cut left in part is finished first, so that a
/// folder beside a clean journal is always whole, also where a cut comes
/// before the caller's `remove`. The answer is the folder, if there is one.
pub fn finish<F: Files, X: Extdata>(
    files: &mut BankFiles<F>,
    staging: &mut [u8],
    ext: &mut X,
) -> Result<Option<Path>, Failed<F, X>> {
    let Phase::Prepared(pending) = files.phase().map_err(Error::Files)? else {
        return Ok(None);
    };
    let folder = folder(State::of(&pending.before), State::of(&pending.after));
    let name = &folder.text()[1..];
    let found = next(ext, &Path::new(b"/"), &|entry, is_folder| {
        is_folder && entry.eq_ignore_ascii_case(name)
    })
    .map_err(Error::Sd)?;
    if found.is_none() {
        return Ok(None);
    }
    back_up(files, staging, ext)?;
    ext.rest();
    Ok(Some(folder))
}

/// Whenever the journal is clean, once the Bank it names was read whole, so
/// that nothing of a folder goes beside a Bank that cannot be read: at every
/// opening, and after START settled a save in progress. Every folder of
/// Bank's extdata named by two states is settled by the state the journal
/// names (`side`), so that a save of the transfer screen, or a save in
/// progress, makes its own only after this. `work` holds two box files;
/// `date` names the folder what is kept goes to.
///
/// | The journal names | A transfer save's cells | A save in progress keeps |
/// | --- | --- | --- |
/// | the folder's state before | stay | the `.new` boxes |
/// | its state after | finished from `.new` | the `.old` boxes |
/// | neither (`Side::Unknown`) | stay, both sides kept | both sides |
///
/// A cell that holds a whole box of neither side changed outside the save:
/// it stays, and both of its sides are kept. What the journal depends on was
/// proven before the journal was written, so a place of it that does not
/// read is damage: the error screen, with nothing deleted. Each step can be
/// made again.
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
    while let Some(folder) = next(ext, &Path::new(b"/"), &of_a_write).map_err(Error::Sd)? {
        let side = side(&folder.text()[1..], now);
        settle_folder(sd, ext, &folder, side, first, second)?;
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

/// Settles `folder` as `side`: a transfer save's cells, what is kept on the
/// SD card in a folder of the same name, proven, and then the folder gone,
/// with every file in it. Nothing goes before all that is kept is on the
/// card.
fn settle_folder<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    side: Side,
    first: &mut [u8; FILE_SIZE],
    second: &mut [u8; FILE_SIZE],
) -> Result<(), Error<E, S::Error>> {
    let found = entries(ext, folder).map_err(Error::Sd)?;
    let transfer = side != Side::Unknown && of_a_transfer(ext, folder, &found, first);
    let cells = match (transfer, side) {
        (true, Side::After) => finish_cells(sd, ext, folder, first, second)?,
        (true, _) => check_cells(sd, ext, folder, first, second).map_err(Error::Sd)?,
        _ => [false; GROUP],
    };
    let kinds = match side {
        Side::Before => NEW,
        Side::After => OLD,
        Side::Unknown => OLD | NEW,
    };
    let wanted = |group: usize, position: usize, kind: u8| {
        if transfer {
            group == MOVER_GROUP && cells[position]
        } else {
            kinds & kind != 0
        }
    };
    let must_read = side != Side::Unknown && !transfer;
    let card = on_card(&folder.text()[1..]);
    for proving in [false, true] {
        keep(
            sd, ext, folder, &card, &found, &wanted, must_read, proving, first, second,
        )?;
    }
    remove(ext, folder).map_err(Error::Sd)
}

/// Whether the folder is a transfer save's, by its transport box's place: a
/// transfer save writes the cells, from position 1, so `mover0.old` has zeros
/// at position 0, and it is written and proven before `mover0.new`. One that
/// does not read was being written, if no `mover0.new` follows it.
fn of_a_transfer<X: Extdata>(
    ext: &mut X,
    folder: &Path,
    found: &[u8; GROUPS],
    work: &mut [u8; FILE_SIZE],
) -> bool {
    if found[..BANK_GROUPS].iter().any(|kinds| *kinds != 0) {
        return false;
    }
    match ext.read_at(&cells_file(folder, false), 0, &mut work[..]) {
        Ok(Piece::Read) => work.iter().all(|byte| *byte == 0),
        Ok(Piece::Missing) => true,
        Err(_) => found[MOVER_GROUP] & NEW == 0,
    }
}

/// The file of cell `position` (1 to 3).
fn cell_path(position: usize) -> Path {
    sdfiles::box_file(&Path::new(ROOT), BOX_COUNT + position)
}

/// The journal names the transfer save as made: each cell gets its `.new`
/// box, if it holds its `.old` one, no file, or a file a cut left. A cell
/// that holds another whole box changed outside the save and stays; the
/// answer marks it. What the journal depends on was proven, so a place that
/// does not read is damage, and nothing more is done.
fn finish_cells<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    new: &mut [u8; FILE_SIZE],
    cell: &mut [u8; FILE_SIZE],
) -> Result<[bool; GROUP], Error<E, S::Error>> {
    let (old_file, new_file) = (cells_file(folder, false), cells_file(folder, true));
    let mut other = [false; GROUP];
    for (position, outside) in other.iter_mut().enumerate().skip(1) {
        let at = (position * FILE_SIZE) as u32;
        let piece = ext
            .read_at(&new_file, at, &mut new[..])
            .map_err(Error::Sd)?;
        if piece != Piece::Read || new.iter().all(|byte| *byte == 0) {
            continue;
        }
        if BoxFile::parse(&new[..]).is_err() {
            return Err(Error::NotAsWritten);
        }
        let path = cell_path(position);
        let found = sd.get(&path, &mut cell[..]).map_err(Error::Sd)?;
        if found == Found::Read && cell == new {
            continue;
        }
        if found == Found::Read && BoxFile::parse(&cell[..]).is_ok() {
            let held = digest(&cell[..]);
            let was = ext
                .read_at(&old_file, at, &mut cell[..])
                .map_err(Error::Sd)?;
            if was != Piece::Read || digest(&cell[..]) != held {
                *outside = true;
                continue;
            }
        }
        sdfiles::create_root(sd).map_err(Error::Sd)?;
        sd.put(&path, &new[..]).map_err(Error::Sd)?;
    }
    Ok(other)
}

/// The journal does not name the transfer save as made, so it wrote no box
/// into a cell. A file a cut left of the empty box a cell without a file gets
/// goes again. A cell that holds anything but its `.old` box changed outside
/// the save and stays; the answer marks it. Its `.old` may be what a cut left,
/// which says nothing.
fn check_cells<S: Sd, X: Extdata<Error = S::Error>>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    old: &mut [u8; FILE_SIZE],
    cell: &mut [u8; FILE_SIZE],
) -> Result<[bool; GROUP], S::Error> {
    let old_file = cells_file(folder, false);
    let mut other = [false; GROUP];
    for (position, outside) in other.iter_mut().enumerate().skip(1) {
        let at = (position * FILE_SIZE) as u32;
        let piece = ext.read_at(&old_file, at, &mut old[..]);
        if !matches!(piece, Ok(Piece::Read)) || BoxFile::parse(&old[..]).is_err() {
            continue;
        }
        let path = cell_path(position);
        let found = sd.get(&path, &mut cell[..])?;
        let whole = found == Found::Read && BoxFile::parse(&cell[..]).is_ok();
        if !whole {
            crate::transfer_screen::empty_box(cell, &[0; NAME_TEXT]);
        }
        if cell != old {
            *outside = true;
        } else if !whole && found != Found::Absent {
            sd.delete(&path)?;
        }
    }
    Ok(other)
}

/// Every whole box that is `wanted` in `folder` as a file of `card`: written
/// where it is not there as it should be, or with `proving` read again and
/// compared. With `must_read`, a place that neither reads nor is zeros is
/// damage, and nothing more is done.
#[allow(clippy::too_many_arguments)]
fn keep<S: Sd, X: Extdata<Error = S::Error>, E>(
    sd: &mut S,
    ext: &mut X,
    folder: &Path,
    card: &Path,
    found: &[u8; GROUPS],
    wanted: &dyn Fn(usize, usize, u8) -> bool,
    must_read: bool,
    proving: bool,
    from: &mut [u8; FILE_SIZE],
    to: &mut [u8; FILE_SIZE],
) -> Result<(), Error<E, S::Error>> {
    let mut made = false;
    for (group, kinds) in found.iter().enumerate() {
        for (kind, _) in KINDS.iter().filter(|(kind, _)| kinds & kind != 0) {
            let path = group_file(folder, group, *kind);
            for position in (0..GROUP).filter(|position| wanted(group, *position, *kind)) {
                let piece = match ext.read_at(&path, (position * FILE_SIZE) as u32, &mut from[..]) {
                    Ok(piece) => piece,
                    Err(error) if must_read => return Err(Error::Sd(error)),
                    Err(_) => continue,
                };
                // A place behind the file's end.
                if piece != Piece::Read {
                    continue;
                }
                if BoxFile::parse(&from[..]).is_err() {
                    if must_read && from.iter().any(|byte| *byte != 0) {
                        return Err(Error::NotAsWritten);
                    }
                    continue;
                }
                let target = kept_file(card, place_at(group, position), *kind);
                let same =
                    sd.get(&target, &mut to[..]).map_err(Error::Sd)? == Found::Read && to == from;
                if same {
                    continue;
                }
                if proving {
                    return Err(Error::NotAsWritten);
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
    Ok(())
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
    use crate::testing::{ExtCard, Refused, Shared};
    use offline_core::Slot;
    use std::vec::Vec;

    type Failure = Error<(), Refused>;

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

    /// A whole box file that `id` tells apart.
    fn a_box(id: u8) -> [u8; FILE_SIZE] {
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

    fn settled(disk: &Shared, folder: &Path, side: Side) -> Result<(), Failure> {
        let (mut first, mut second) = ([0; FILE_SIZE], [0; FILE_SIZE]);
        settle_folder(
            &mut disk.clone(),
            &mut ExtCard(disk.clone()),
            folder,
            side,
            &mut first,
            &mut second,
        )?;
        retire(&mut disk.clone(), [2026, 11, 16, 14, 3, 7]).map_err(Error::Sd)
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

    #[test]
    fn a_transfer_save_is_finished_or_never_touched_the_cells_and_keeps_nothing() {
        for (side, one, three) in [(Side::Before, 10, 11), (Side::After, 20, 21)] {
            let (disk, folder) = transfer_card();
            settled(&disk, &folder, side).unwrap();
            assert_eq!(cell_id(&disk, 1), Some(one), "{side:?}");
            assert_eq!(cell_id(&disk, 2), None, "{side:?}");
            assert_eq!(cell_id(&disk, 3), Some(three), "{side:?}");
            assert_eq!(dated(&disk), 0, "{side:?}");
            no_folder_left(&disk, "transfer");
        }
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
            assert_eq!(
                kept_names(&disk),
                names(&[(b"mover1.old.box", 10), (b"mover1.new.box", 20)]),
                "{side:?}"
            );
            // The other cell is settled as ever.
            let three = if side == Side::After { 21 } else { 11 };
            assert_eq!(cell_id(&disk, 3), Some(three), "{side:?}");
            no_folder_left(&disk, "neither");
        }
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
            assert_eq!(dated(&disk), 0, "torn {torn}");
        }
        // Before: the empty box a cell without a file got, cut short, goes.
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
        settled(&disk, &folder, Side::Before).unwrap();
        assert_eq!(cell_id(&disk, 2), None);
        assert_eq!(dated(&disk), 0);
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
    fn what_the_journal_depends_on_is_never_skipped() {
        // A backup, and a transfer save named as made: a file that does not
        // read, and a place whose checksum fails, stop with nothing deleted.
        type Card = fn() -> (Shared, Path);
        type Damaged = fn(&Path) -> Path;
        let cases: [(Card, Side, Damaged); 3] = [
            (backup_card, Side::Before, |f| file(f, 29, true).0),
            (backup_card, Side::After, |f| file(f, 29, false).0),
            (transfer_card, Side::After, |f| cells_file(f, true)),
        ];
        for (card, side, damaged) in cases {
            for unreadable in [true, false] {
                let (disk, folder) = card();
                let path = damaged(&folder);
                if unreadable {
                    disk.0.borrow_mut().ext_damaged.insert(path.text().to_vec());
                } else {
                    let cells = path.text().ends_with(b"mover0.new");
                    let at = if cells { FILE_SIZE } else { 2 * FILE_SIZE };
                    disk.0.borrow_mut().ext.get_mut(path.text()).unwrap()[at] ^= 1;
                }
                let before = disk.0.borrow().ext.clone();
                let cell = cell_id(&disk, 1);
                assert!(settled(&disk, &folder, side).is_err(), "{side:?}");
                assert!(disk.0.borrow().ext == before, "{side:?}");
                assert_eq!(cell_id(&disk, 1), cell, "{side:?}");
                assert_eq!(dated(&disk), 0, "{side:?}");
            }
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
            settled(&disk, &folder, Side::Before).unwrap();
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
