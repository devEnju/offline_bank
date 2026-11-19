//! The files an interrupted write leaves on the SD card, and the one rule
//! that settles them (docs/internals.md, "Box files"). Each lies in a
//! folder named by the two states of the Bank the write goes between, the
//! one before and the one after (`folder`):
//!
//! ```text
//! /3ds/Pokémon Bank/interrupted-AAAAAAAA-BBBBBBBB/mover1.bak   a cell before a save of the transfer screen
//! /3ds/Pokémon Bank/interrupted-AAAAAAAA-BBBBBBBB/bank30.old   box 30 of the save before an interrupted save
//! /3ds/Pokémon Bank/interrupted-AAAAAAAA-BBBBBBBB/bank30.new   box 30 of the save after it
//! /3ds/Pokémon Bank/interrupted-AAAAAAAA-BBBBBBBB/mover0.old   the transport box, if it differs
//! /3ds/Pokémon Bank/interrupted-AAAAAAAA-BBBBBBBB/mover0.new
//! ```
//!
//! A save in progress that moved Pokémon both ways is left to Transporter's
//! question, or waits for its game; before Bank writes that into the journal,
//! every box whose Pokémon differ between the save before and the save after
//! (`boxfile::bank_box_contents`) is written twice, as it was and as it
//! became (`back_up`). The transfer screen keeps each cell it
//! writes as a `.bak` (`transfer_screen::commit`).
//!
//! Nothing is ever deleted from a folder while it bears this name, so the
//! files of one are always the whole set its write made. Once the journal
//! is clean, Bank's opening compares the state it names with the folder's
//! two (`side`; `settle` applies it): the state before puts the `.bak`
//! files back. Then the folder is resolved (`resolve`): renamed whole, in
//! one step, to a folder of the date, `YYYYMMDD-HHMMSS-backup/`, and only
//! there are the files deleted whose Pokémon the Bank or the cells hold. A
//! cut there leaves in that folder files it would have deleted, never a set
//! that looks whole and is not.

use crate::{
    bank_files::{BankFiles, Files},
    sdfiles::{self, Date, Error, Failed, Found, Path, Sd, ROOT},
};
use offline_core::{
    boxfile::{self, BoxFile, FILE_SIZE, NAME_TEXT},
    native_blob::BOX_COUNT,
    sections::{BANK_SIZE, RECORD_SIZE, TRANSPORT_SLOTS},
    Crc32, Part, Phase, Save,
};

/// 100 boxes and the transport box.
const BITS: usize = BOX_COUNT + 1;
const TRANSPORT_BIT: usize = BOX_COUNT;
/// The boxes, the transport box and the three cells (`mover1`..`mover3`).
const PLACES: usize = BOX_COUNT + 4;
/// The places of the three cells.
const CELLS: core::ops::Range<usize> = BOX_COUNT + 1..PLACES;
/// The ending of a cell's old box that the transfer screen's write keeps.
const BAK: &[u8] = b".bak";
/// One box's `boxfile::Contents`.
const PRINT: usize = 32;
/// The start of the name of a folder of an interrupted write.
const PREFIX: &[u8] = b"interrupted-";

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

/// The folder of a write between `before` and `after`:
/// `interrupted-AAAAAAAA-BBBBBBBB`, the two states' halves (`State::half`).
pub fn folder(before: State, after: State) -> Path {
    let mut path = Path::new(ROOT);
    path.push(b"/")
        .push(PREFIX)
        .push_hex(before.half())
        .push(b"-")
        .push_hex(after.half());
    path
}

/// Which of a folder's two states the journal names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
    /// Neither: the Bank is a version the folder does not know, as one put
    /// there from outside the patches is. Also a name not of `folder`'s form,
    /// and one with two equal halves, which no write gives.
    Unknown,
}

/// The side of the folder `name` (its name in the root folder) that `now`,
/// the state the clean journal names, is.
pub fn side(name: &[u8], now: State) -> Side {
    let hex = |digits: &[u8]| {
        digits.iter().try_fold(0u32, |value, digit| {
            Some(value << 4 | char::from(*digit).to_digit(16)?)
        })
    };
    let halves = sdfiles::strip_prefix_ignoring_case(name, PREFIX)
        .filter(|rest| rest.len() == 17 && rest[8] == b'-')
        .and_then(|rest| Some((hex(&rest[..8])?, hex(&rest[9..])?)));
    match halves {
        Some((before, after)) if before != after && now.half() == before => Side::Before,
        Some((before, after)) if before != after && now.half() == after => Side::After,
        _ => Side::Unknown,
    }
}

/// A file of the box of the save before (`.old`).
const OLD: u8 = 1;
/// A file of the box of the save after (`.new`).
const NEW: u8 = 2;
/// A cell's old box (`.bak`).
const KEPT: u8 = 4;
const SUFFIXES: [(u8, &[u8]); 3] = [(OLD, b".old"), (NEW, b".new"), (KEPT, BAK)];

/// The files of `folder` that are an interrupted write's, by place
/// (`push_box`): the kinds found of each, `OLD | NEW | KEPT`.
fn entries<S: Sd>(sd: &mut S, folder: &Path) -> Result<[u8; PLACES], S::Error> {
    let mut found = [0u8; PLACES];
    sd.list(folder, &mut |name, is_folder| {
        if let (false, Some((place, kind))) = (is_folder, parse(name)) {
            found[place] |= kind;
        }
    })?;
    Ok(found)
}

/// The place and the kind of a file by its name: `bank30.old` is box 29
/// of the save before, `mover1.bak` the first cell's old box. The card may
/// list a name that fits 8.3 in capitals (`MOVER1.BAK`), so case does not
/// count.
fn parse(name: &[u8]) -> Option<(usize, u8)> {
    let (stem, kind) = SUFFIXES.iter().find_map(|(kind, suffix)| {
        Some((sdfiles::strip_suffix_ignoring_case(name, suffix)?, *kind))
    })?;
    let place = (0..PLACES).find(|place| {
        let mut path = Path::new(b"/");
        path.push_box(*place);
        path.text()[1..].eq_ignore_ascii_case(stem)
    })?;
    // Bank writes a `.bak` only for a cell, and `.old` and `.new` only for a
    // box of the Bank or the transport box: any other name is of no write.
    let ours = if kind == KEPT {
        CELLS.contains(&place)
    } else {
        place < BITS
    };
    ours.then_some((place, kind))
}

/// The `.bak` of the cell at `place` in `folder`.
pub fn bak(folder: &Path, place: usize) -> Path {
    let mut path = *folder;
    path.push(b"/").push_box(place).push(BAK);
    path
}

/// The file of `bit` (a box, or the transport box) in `folder`, of the
/// save after (`.new`) or before (`.old`).
pub fn file(folder: &Path, bit: usize, after: bool) -> Path {
    let mut path = *folder;
    path.push(b"/")
        .push_box(bit)
        .push(if after { b".new" } else { b".old" });
    path
}

/// Writes the backup of the save in progress: every file that is not on the
/// card as it should be, so a START after a cut, or after a file of it was
/// lost, writes what is missing and one that finds it whole writes nothing.
/// `staging` holds one Bank file at a time and nothing of use afterwards.
/// Nothing of Bank's files is written.
pub fn back_up<F: Files, S: Sd>(
    files: &mut BankFiles<F>,
    staging: &mut [u8],
    sd: &mut S,
) -> Result<(), Failed<F, S>> {
    let Phase::Prepared(pending) = files.phase().map_err(Error::Files)? else {
        return Ok(());
    };
    let side = RECORD_SIZE * TRANSPORT_SLOTS + TRANSPORT_SLOTS;
    if staging.len() < BANK_SIZE + 2 * FILE_SIZE + 2 * side + BOX_COUNT * PRINT {
        return Err(Error::Staging);
    }
    let folder = folder(State::of(&pending.before), State::of(&pending.after));
    let (payload, rest) = staging.split_at_mut(BANK_SIZE);
    let (out, rest) = rest.split_at_mut(FILE_SIZE);
    let out: &mut [u8; FILE_SIZE] = out.try_into().map_err(|_| Error::Staging)?;
    let (found, rest) = rest.split_at_mut(FILE_SIZE);
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

    // The order: each changed box's `.new`, its `.old`, the transport box's
    // `.old` and `.new`; a file that is as it should be is not written.
    if !changed.contains(&true) {
        return Ok(());
    }
    sdfiles::create_root(sd).map_err(Error::Sd)?;
    sd.create_dir(&folder).map_err(Error::Sd)?;
    let mut ensure = |sd: &mut S, path: &Path, out: &[u8; FILE_SIZE]| {
        if sd.get(path, found).map_err(Error::Sd)? == Found::Read && found[..] == out[..] {
            return Ok(());
        }
        sd.put(path, &out[..]).map_err(Error::Sd)
    };
    let boxes = (0..BOX_COUNT).filter(|index| changed[*index]);
    let mut write = |sd: &mut S, payload: &[u8], index: usize, after: bool| {
        boxfile::encode_bank_box(payload, index, out).map_err(|_| Error::Staging)?;
        ensure(sd, &file(&folder, index, after), out)
    };
    for index in boxes.clone() {
        write(sd, payload, index, true)?;
    }
    if boxes.clone().next().is_some() {
        files
            .pending_payload(false, payload)
            .map_err(Error::Files)?;
        for index in boxes {
            write(sd, payload, index, false)?;
        }
    }
    if changed[TRANSPORT_BIT] {
        for (records, tags, after) in [
            (&*old_records, &*old_tags, false),
            (&*new_records, &*new_tags, true),
        ] {
            boxfile::encode_transport(records, tags, &[0; NAME_TEXT], out)
                .map_err(|_| Error::Staging)?;
            ensure(sd, &file(&folder, TRANSPORT_BIT, after), out)?;
        }
    }
    Ok(())
}

/// What follows the date in the name of the folder an `interrupted-`
/// folder is renamed to: `YYYYMMDD-HHMMSS-backup`.
pub const RETIRED: &[u8] = b"-backup";

/// What a folder's write is settled as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// The state before: the `.bak` files are back in their cells, and the
    /// Bank holds the boxes of the `.old` ones.
    Before,
    /// The state after, as the cells show: the Bank or a cell holds what
    /// every `.bak` held, and the Bank the boxes of the `.new` ones.
    After,
    /// Neither: everything is kept.
    Neither,
}

/// The end of an `interrupted-` folder whose write is settled as
/// `resolved`. An empty one goes. Any other is renamed whole, in one step,
/// to a new folder of `date` (`sdfiles::free_dated_folder`), so that nothing
/// is put back from it again; there the files whose Pokémon the Bank or the
/// cells hold are deleted, and the folder if it is empty then.
///
/// | Settled as | Deleted | Kept |
/// | --- | --- | --- |
/// | before | `.bak` (back in its cell), `.old` | `.new` |
/// | after | `.bak`, `.new` | `.old` |
/// | neither | nothing | everything |
///
/// Of what is kept, a file that is not a whole box file goes too: a cut
/// left it half written, and no one can use it. So every file a folder of
/// the date keeps is a box file that reads. A cut after the rename leaves
/// some of the deleted ones in it beside what is kept; nothing comes back
/// to it. `work` holds one box file.
pub fn resolve<S: Sd>(
    sd: &mut S,
    folder: &Path,
    resolved: Resolved,
    date: Date,
    work: &mut [u8; FILE_SIZE],
) -> Result<(), S::Error> {
    if sd.remove_dir(folder).is_ok() {
        return Ok(());
    }
    let found = entries(sd, folder)?;
    let kept = sdfiles::free_dated_folder(sd, date, RETIRED)?;
    sd.rename_dir(folder, &kept)?;
    let held = match resolved {
        Resolved::Before => KEPT | OLD,
        Resolved::After => KEPT | NEW,
        Resolved::Neither => 0,
    };
    for (place, kinds) in found.iter().enumerate() {
        for (kind, path) in [
            (KEPT, bak(&kept, place)),
            (OLD, file(&kept, place, false)),
            (NEW, file(&kept, place, true)),
        ] {
            if kinds & kind == 0 {
                continue;
            }
            let goes = kinds & kind & held != 0;
            if goes || !(sd.get(&path, work)? == Found::Read && BoxFile::parse(work).is_ok()) {
                sd.delete(&path)?;
            }
        }
    }
    let _ = sd.remove_dir(&kept);
    Ok(())
}

/// Whenever the journal is clean, once the Bank it names was read whole,
/// so that no file of a folder goes beside a Bank that cannot be read: at
/// every opening, and after START settled a save in progress. Every
/// `interrupted-` folder is settled by the state the journal names
/// (`side`) and resolved (`resolve`), and none is left, so that a save of
/// the transfer screen, or a save in progress, makes its own only after
/// this. `work` holds two box files; `date` names the folder what is kept
/// goes to.
///
/// | The journal names | Done in the folder | Settled as |
/// | --- | --- | --- |
/// | the folder's state before | each whole `.bak` put back into its cell | before |
/// | its state after | nothing | after |
/// | neither (`Side::Unknown`) | nothing | neither |
///
/// The journal is the one word on a folder: Bank's data replaced, restored
/// or deleted while a folder lies there, and damaged files, are outside
/// what this settles (docs/internals.md, "What recovery covers"). Each step
/// can be made again: a cut before the rename leaves the folder whole under
/// its name, and the next opening decides the same.
pub fn settle<F: Files, S: Sd>(
    files: &mut BankFiles<F>,
    sd: &mut S,
    work: &mut [u8],
    date: Date,
) -> Result<(), Failed<F, S>> {
    let Phase::Clean(current) = files.phase().map_err(Error::Files)? else {
        return Ok(());
    };
    let now = State::of(&current);
    let (bak, rest) = work
        .get_mut(..2 * FILE_SIZE)
        .ok_or(Error::Staging)?
        .split_at_mut(FILE_SIZE);
    let bak: &mut [u8; FILE_SIZE] = bak.try_into().map_err(|_| Error::Staging)?;
    let cell: &mut [u8; FILE_SIZE] = rest.try_into().map_err(|_| Error::Staging)?;
    // The root's list is read again after each folder, as settling one
    // changes it; each one goes.
    loop {
        let mut next = None;
        sd.list(&Path::new(ROOT), &mut |name, is_folder| {
            let mut path = Path::new(ROOT);
            path.push(b"/").push(name);
            let ours = sdfiles::strip_prefix_ignoring_case(name, PREFIX).is_some();
            if is_folder && ours && path.valid() && next.is_none() {
                next = Some((path, side(name, now)));
            }
        })
        .map_err(Error::Sd)?;
        let Some((folder, side)) = next else {
            break;
        };
        let resolved = match side {
            Side::Before => {
                put_back(sd, &folder, bak, cell).map_err(Error::Sd)?;
                Resolved::Before
            }
            Side::After => Resolved::After,
            Side::Unknown => Resolved::Neither,
        };
        resolve(sd, &folder, resolved, date, bak).map_err(Error::Sd)?;
    }
    Ok(())
}

/// Puts each whole `.bak` of `folder` back into its cell, unless the cell
/// holds it already; a cell without a file holds an empty box without a
/// name, so it needs no `.bak` of one, and a full card is no matter then. A
/// `.bak` that is not whole was being written, before any cell was touched.
/// Nothing is deleted.
fn put_back<S: Sd>(
    sd: &mut S,
    folder: &Path,
    bak: &mut [u8; FILE_SIZE],
    cell: &mut [u8; FILE_SIZE],
) -> Result<(), S::Error> {
    let found = entries(sd, folder)?;
    for place in CELLS.filter(|place| found[*place] & KEPT != 0) {
        let cell_path = sdfiles::box_file(&Path::new(ROOT), place);
        if sd.get(&self::bak(folder, place), bak)? != Found::Read || BoxFile::parse(bak).is_err() {
            continue;
        }
        let holds = match sd.get(&cell_path, cell)? {
            Found::Read => cell[..] == bak[..],
            Found::Absent => {
                crate::transfer_screen::empty_box(cell, &[0; NAME_TEXT]);
                cell[..] == bak[..]
            }
            Found::OtherSize => false,
        };
        if !holds {
            sd.put(&cell_path, &bak[..])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use crate::testing::Shared;
    use offline_core::Slot;
    use std::vec::Vec;

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

    /// The name of `folder` in the root folder.
    fn name_of(folder: &Path) -> Vec<u8> {
        folder.text()[ROOT.len() + 1..].to_vec()
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
        let mut same = Path::new(ROOT);
        same.push(b"/interrupted-")
            .push_hex(before.half())
            .push(b"-")
            .push_hex(after.half());
        assert_eq!(folder, same);
        assert_eq!(name_of(&folder).len(), b"interrupted-".len() + 17);
        let name = |bit, after| {
            let path = file(&folder, bit, after);
            path.text()[folder.text().len()..].to_vec()
        };
        assert_eq!(name(29, false), b"/bank30.old");
        assert_eq!(name(99, true), b"/bank00.new");
        assert_eq!(name(TRANSPORT_BIT, true), b"/mover0.new");
        assert_eq!(
            bak(&folder, BOX_COUNT + 1).text()[folder.text().len()..],
            b"/mover1.bak"[..]
        );
        // The longest path of the folder fits the SD card's commands.
        assert!(file(&folder, 0, true).valid());
        assert!(bak(&folder, BOX_COUNT + 3).valid());
        // Each name reads back as its place and kind.
        for (place, kind) in [(29, OLD), (99, NEW), (TRANSPORT_BIT, NEW), (101, KEPT)] {
            let path = match kind {
                KEPT => bak(&folder, place),
                kind => file(&folder, place, kind == NEW),
            };
            assert_eq!(
                parse(&path.text()[folder.text().len() + 1..]),
                Some((place, kind))
            );
        }
        // As the card may list a name that fits 8.3.
        assert_eq!(parse(b"MOVER1.BAK"), Some((101, KEPT)));
        assert_eq!(parse(b"BANK30.OLD"), Some((29, OLD)));
        assert_eq!(parse(b"mover1.bin"), None);
        // Names Bank never writes.
        assert_eq!(parse(b"bank01.bak"), None);
        assert_eq!(parse(b"mover0.bak"), None);
        assert_eq!(parse(b"mover1.old"), None);
        assert_eq!(parse(b"notes.txt"), None);
        let mut retired = sdfiles::dated_folder([2026, 11, 16, 14, 3, 7]);
        retired.push(RETIRED);
        assert_eq!(retired.text(), [ROOT, b"/20261116-140307-backup"].concat());
        let mut longest = retired;
        longest.push(b"/mover1.bak");
        assert!(longest.valid());
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
        // Lower-case digits read as the card may list them.
        assert_eq!(side(&name.to_ascii_lowercase(), after), Side::After);
        assert_eq!(side(&name.to_ascii_uppercase(), after), Side::After);
        // Two equal halves tell no side.
        let same = name_of(&folder(before, before));
        assert_eq!(side(&same, before), Side::Unknown);
        // Names not of the form.
        let hex = |state: State| {
            let mut path = Path::new(b"");
            path.push_hex(state.half());
            path.text().to_vec()
        };
        for other in [
            [b"interrupted-".as_slice(), &hex(before)].concat(),
            [b"interrupted-".as_slice(), &hex(before), b"_", &hex(after)].concat(),
            [
                b"interrupted-".as_slice(),
                &hex(before),
                b"-",
                &hex(after),
                b"0",
            ]
            .concat(),
            [b"interrupted-".as_slice(), b"G0000000-", &hex(after)].concat(),
            [b"backup-".as_slice(), &hex(before), b"-", &hex(after)].concat(),
        ] {
            assert_eq!(side(&other, before), Side::Unknown, "{other:?}");
        }
    }

    #[test]
    fn resolve_renames_a_folder_whole_and_deletes_there_only_what_is_held() {
        let date = [2026, 11, 16, 14, 3, 7];
        let backup = folder(
            state((Slot::A, 1), (Slot::A, 2)),
            state((Slot::B, 3), (Slot::A, 4)),
        );
        // A folder of every kind of file, each a whole box file, on a card
        // whose second of `date` is taken already.
        let card = |files: &[(usize, u8)]| {
            let disk = Shared::default();
            let mut sd = disk.clone();
            sdfiles::create_root(&mut sd).unwrap();
            let mut taken = sdfiles::dated_folder(date);
            taken.push(RETIRED);
            sd.create_dir(&taken).unwrap();
            sd.create_dir(&backup).unwrap();
            for (place, kind) in files {
                let path = match *kind {
                    KEPT => bak(&backup, *place),
                    kind => file(&backup, *place, kind == NEW),
                };
                let mut name = [0u8; NAME_TEXT];
                name[0] = *place as u8;
                let mut out = [0u8; FILE_SIZE];
                crate::transfer_screen::empty_box(&mut out, &name);
                sd.put(&path, &out).unwrap();
            }
            disk
        };
        let all = [(29, OLD), (29, NEW), (TRANSPORT_BIT, NEW), (101, KEPT)];
        let kept = [ROOT, b"/20261116-140308-backup"].concat();
        for (resolved, left) in [
            (Resolved::Neither, &all[..]),
            (Resolved::Before, &[(29, NEW), (TRANSPORT_BIT, NEW)][..]),
            (Resolved::After, &[(29, OLD)][..]),
        ] {
            let disk = card(&all);
            resolve(
                &mut disk.clone(),
                &backup,
                resolved,
                date,
                &mut [0; FILE_SIZE],
            )
            .unwrap();
            let sd = disk.0.borrow();
            assert!(!sd.sd_dirs.contains(backup.text()), "{resolved:?}");
            let kept_path = Path::new(&kept);
            let mut names: Vec<Vec<u8>> = left
                .iter()
                .map(|(place, kind)| {
                    match *kind {
                        KEPT => bak(&kept_path, *place),
                        kind => file(&kept_path, *place, kind == NEW),
                    }
                    .text()
                    .to_vec()
                })
                .collect();
            names.sort();
            let found: Vec<Vec<u8>> = sd.sd.keys().cloned().collect();
            assert_eq!(found, names, "{resolved:?}");
        }
        // Nothing left: no folder of the date stays.
        let disk = card(&[(101, KEPT)]);
        resolve(
            &mut disk.clone(),
            &backup,
            Resolved::After,
            date,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        assert!(!disk.0.borrow().sd_dirs.contains(&kept));
        assert!(disk.0.borrow().sd.is_empty());
        // A cut after the rename: the folder of the date holds the rest, and
        // no `interrupted-` folder is left to put anything back from.
        let disk = card(&all).cut_after(2);
        assert!(resolve(
            &mut disk.clone(),
            &backup,
            Resolved::Before,
            date,
            &mut [0; FILE_SIZE]
        )
        .is_err());
        let sd = disk.0.borrow();
        assert!(!sd.sd_dirs.contains(backup.text()));
        assert!(sd.sd_dirs.contains(&kept));
        // An empty one goes, and no folder of the date is made for it.
        drop(sd);
        let disk = card(&[]);
        let before = disk.0.borrow().sd_dirs.len();
        resolve(
            &mut disk.clone(),
            &backup,
            Resolved::Neither,
            date,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        assert_eq!(disk.0.borrow().sd_dirs.len(), before - 1);
        // A file a cut left half written is not kept, whatever is settled:
        // no one could use it.
        let disk = card(&all);
        let torn = file(&backup, 29, true);
        disk.0.borrow_mut().sd.get_mut(torn.text()).unwrap()[0] ^= 1;
        resolve(
            &mut disk.clone(),
            &backup,
            Resolved::Neither,
            date,
            &mut [0; FILE_SIZE],
        )
        .unwrap();
        let names: Vec<Vec<u8>> = disk.0.borrow().sd.keys().cloned().collect();
        assert_eq!(names.len(), all.len() - 1);
        assert!(!names.iter().any(|name| name.ends_with(b"/bank30.new")));
    }
}
