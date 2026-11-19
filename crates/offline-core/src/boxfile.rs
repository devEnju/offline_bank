//! One box as a plain file of the SD card (docs/internals.md, "Box files"):
//! the bytes of one box of the Bank file in the Bank file's order, of its
//! label only the name's text, and a CRC-32. The transfer screen and the
//! backups of an interrupted save write it, for a box of the Bank, the
//! transport box and an offline box alike, and a user may put it back.
//! 7,298 bytes, little endian:
//!
//! | Offset | Size | Content |
//! | --- | --- | --- |
//! | `0` | `30 × E8` | the records, encrypted as the Bank stores them |
//! | `1B30` | `0x22` | the name's text, zeros behind its end; for the transport box "Transport Box" in the console's language, or zero |
//! | `1B52` | 30 | format tag per slot |
//! | `1B70` | 30 | flag byte per slot; zero for an empty slot and the transport box |
//! | `1B8E` | 240 | timestamp per slot, 8 bytes each; zero for an empty slot and the transport box |
//! | `1C7E` | 4 | CRC-32 of everything before it |
//!
//! The category and the position, behind the name in the Bank, are not
//! kept: they belong to the place in Bank.
//!
//! A file is taken only whole: its size and its CRC. Its records are taken
//! as they are, as the Bank takes them; a slot whose record decrypts to
//! species 0 is empty.

use crate::{
    crc32,
    native_blob::BLOB_SIZE,
    native_blob::BOX_COUNT,
    sections::{
        BANK_SIZE, BOXES_START, BOX_SLOTS, BOX_STRIDE, FLAGS_START, RECORD_SIZE, STAMPS_START,
        TAGS_START, TRANSPORT_SLOTS,
    },
};
use core::ops::Range;

/// The box's name as the Bank stores it.
pub const NAME_SIZE: usize = 0x24;
/// The records of one box.
pub const RECORDS_SIZE: usize = BOX_SLOTS * RECORD_SIZE;
/// The name and the word behind it, in the Bank.
pub const LABEL_SIZE: usize = NAME_SIZE + 2;
/// The name's text, with its end: what the original's name setter copies
/// (`002ba124`). In the Bank the byte after it is unused, then the category
/// (`001dac5c`), then the position within it (`001d9730`).
pub const NAME_TEXT: usize = 0x22;
/// The box's category in Bank: the last byte of the name.
pub const CATEGORY: usize = NAME_SIZE - 1;
pub const STAMP_SIZE: usize = 8;

pub const RECORDS: Range<usize> = 0..RECORDS_SIZE;
pub const NAME: Range<usize> = RECORDS.end..RECORDS.end + NAME_TEXT;
pub const TAGS: Range<usize> = NAME.end..NAME.end + BOX_SLOTS;
pub const FLAGS: Range<usize> = TAGS.end..TAGS.end + BOX_SLOTS;
pub const STAMPS: Range<usize> = FLAGS.end..FLAGS.end + BOX_SLOTS * STAMP_SIZE;
/// The CRC-32 of everything before it.
const CRC: usize = STAMPS.end;
pub const FILE_SIZE: usize = CRC + 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxFileError {
    Size,
    Checksum,
    /// A box index past the Bank's boxes.
    Index,
}

/// Where box `index` lies in a Bank file payload (`bank_box`) or in a
/// native body (`body_box`): its records and label (one range, the native
/// box), then its tags, flag bytes and timestamps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BankBox {
    pub records: Range<usize>,
    pub label: Range<usize>,
    pub tags: Range<usize>,
    pub flags: Range<usize>,
    pub stamps: Range<usize>,
}

const TAGS_AT: usize = BOX_COUNT * BOX_STRIDE;
const FLAGS_AT: usize = TAGS_AT + BOX_COUNT * BOX_SLOTS;
const STAMPS_AT: usize = FLAGS_AT + BOX_COUNT * BOX_SLOTS;
const _: () = assert!(STAMPS_AT + BOX_COUNT * BOX_SLOTS * STAMP_SIZE == BANK_SIZE);

pub fn bank_box(index: usize) -> Result<BankBox, BoxFileError> {
    if index >= BOX_COUNT {
        return Err(BoxFileError::Index);
    }
    let at = index * BOX_STRIDE;
    let slots = index * BOX_SLOTS;
    Ok(BankBox {
        records: at..at + RECORDS_SIZE,
        label: at + RECORDS_SIZE..at + BOX_STRIDE,
        tags: TAGS_AT + slots..TAGS_AT + slots + BOX_SLOTS,
        flags: FLAGS_AT + slots..FLAGS_AT + slots + BOX_SLOTS,
        stamps: STAMPS_AT + slots * STAMP_SIZE..STAMPS_AT + (slots + BOX_SLOTS) * STAMP_SIZE,
    })
}

pub fn body_box(index: usize) -> Result<BankBox, BoxFileError> {
    if index >= BOX_COUNT {
        return Err(BoxFileError::Index);
    }
    let at = BOXES_START + index * BOX_STRIDE;
    let slots = index * BOX_SLOTS;
    Ok(BankBox {
        records: at..at + RECORDS_SIZE,
        label: at + RECORDS_SIZE..at + BOX_STRIDE,
        tags: TAGS_START + slots..TAGS_START + slots + BOX_SLOTS,
        flags: FLAGS_START + slots..FLAGS_START + slots + BOX_SLOTS,
        stamps: STAMPS_START + slots * STAMP_SIZE..STAMPS_START + (slots + BOX_SLOTS) * STAMP_SIZE,
    })
}

/// The Pokémon a box holds, as a change counts them: the transfer screen
/// (`Changes`) and the backup of an interrupted save both compare boxes by
/// it. SHA-256 over the box slot by slot: whether the slot holds a Pokémon
/// and, if it does, its record, tag, flag byte and time. A slot without a
/// Pokémon counts only as empty; what it keeps besides (an empty record, a
/// flag, the time it was emptied) shows nowhere.
pub struct Contents(crate::Sha256);
impl Contents {
    pub fn new() -> Self {
        Self(crate::Sha256::new())
    }
    fn add(&mut self, bytes: &[u8]) {
        // A box is far below the hash's length limit.
        let _ = self.0.update(bytes);
    }
    /// The next slot.
    pub fn slot(&mut self, record: &[u8; RECORD_SIZE], tag: u8, flag: u8, stamp: &[u8]) {
        if crate::sections::stored_species(record) == 0 {
            self.add(&[0]);
            return;
        }
        self.add(&[1]);
        self.add(record);
        self.add(&[tag, flag]);
        self.add(stamp);
    }
    pub fn finish(self) -> [u8; 32] {
        self.0.finalize()
    }
}
impl Default for Contents {
    fn default() -> Self {
        Self::new()
    }
}

/// A name's text: its units up to its end, with zeros behind it. What an
/// earlier, longer text left behind the end is no part of it.
pub fn text_only(name: &[u8; NAME_TEXT]) -> [u8; NAME_TEXT] {
    let mut text = [0u8; NAME_TEXT];
    for (to, unit) in text
        .chunks_exact_mut(2)
        .zip(name.chunks_exact(2))
        .take_while(|(_, unit)| **unit != [0, 0])
    {
        to.copy_from_slice(unit);
    }
    text
}

/// What a label shows: the name's text, the category and the position. The
/// name's hidden bytes behind its end and the unused byte show nowhere.
pub fn shown_label(label: &[u8; LABEL_SIZE]) -> [u8; LABEL_SIZE] {
    let mut shown = [0u8; LABEL_SIZE];
    let mut name = [0u8; NAME_TEXT];
    name.copy_from_slice(&label[..NAME_TEXT]);
    shown[..NAME_TEXT].copy_from_slice(&text_only(&name));
    shown[CATEGORY..].copy_from_slice(&label[CATEGORY..]);
    shown
}

/// `Contents` of box `index` of a Bank file payload: its Pokémon. Its
/// name, category and position are no part of it.
pub fn bank_box_contents(payload: &[u8], index: usize) -> Result<[u8; 32], BoxFileError> {
    let place = bank_box(index)?;
    if payload.len() != BANK_SIZE {
        return Err(BoxFileError::Size);
    }
    let mut contents = Contents::new();
    for slot in 0..BOX_SLOTS {
        let record = place.records.start + slot * RECORD_SIZE;
        let record: &[u8; RECORD_SIZE] = payload[record..record + RECORD_SIZE]
            .try_into()
            .map_err(|_| BoxFileError::Size)?;
        let stamp = place.stamps.start + slot * STAMP_SIZE;
        contents.slot(
            record,
            payload[place.tags.start + slot],
            payload[place.flags.start + slot],
            &payload[stamp..stamp + STAMP_SIZE],
        );
    }
    Ok(contents.finish())
}

/// The file of box `index` of a Bank file payload: its pieces, of its label
/// the name's text, cleaned (`clean`).
pub fn encode_bank_box(
    payload: &[u8],
    index: usize,
    out: &mut [u8; FILE_SIZE],
) -> Result<(), BoxFileError> {
    if payload.len() != BANK_SIZE {
        return Err(BoxFileError::Size);
    }
    encode_box(payload, bank_box(index)?, out);
    Ok(())
}

/// `encode_bank_box` from a native body.
pub fn encode_body_box(
    body: &[u8],
    index: usize,
    out: &mut [u8; FILE_SIZE],
) -> Result<(), BoxFileError> {
    if body.len() != BLOB_SIZE {
        return Err(BoxFileError::Size);
    }
    encode_box(body, body_box(index)?, out);
    Ok(())
}

fn encode_box(source: &[u8], place: BankBox, out: &mut [u8; FILE_SIZE]) {
    out[RECORDS].copy_from_slice(&source[place.records]);
    out[NAME].copy_from_slice(&source[place.label][..NAME_TEXT]);
    out[TAGS].copy_from_slice(&source[place.tags]);
    out[FLAGS].copy_from_slice(&source[place.flags]);
    out[STAMPS].copy_from_slice(&source[place.stamps]);
    clean(out);
}

/// The file of the transport box: its records and tags as the transport
/// box file holds them, and `name`, the text of its name in the console's
/// language (the native transport box has none; all zero for none). It
/// keeps no flag bytes or times.
pub fn encode_transport(
    records: &[u8],
    tags: &[u8],
    name: &[u8; NAME_TEXT],
    out: &mut [u8; FILE_SIZE],
) -> Result<(), BoxFileError> {
    if records.len() != TRANSPORT_SLOTS * RECORD_SIZE || tags.len() != TRANSPORT_SLOTS {
        return Err(BoxFileError::Size);
    }
    out.fill(0);
    out[RECORDS].copy_from_slice(records);
    out[NAME].copy_from_slice(name);
    out[TAGS].copy_from_slice(tags);
    clean(out);
    Ok(())
}

/// A name's text as a box file holds it (`NAME_TEXT`): `units` up to their
/// end, little endian, with its end. `None` for a text with more units than
/// the field holds before its end; no name is cut.
pub fn name_text(units: &[u16]) -> Option<[u8; NAME_TEXT]> {
    let len = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    if len >= NAME_TEXT / 2 {
        return None;
    }
    let mut out = [0u8; NAME_TEXT];
    for (bytes, unit) in out.chunks_exact_mut(2).zip(&units[..len]) {
        bytes.copy_from_slice(&unit.to_le_bytes());
    }
    Some(out)
}

/// `Contents` of a transport box, its records and tags: it keeps no flag
/// bytes or times, and has no label.
pub fn transport_contents(records: &[u8], tags: &[u8]) -> Result<[u8; 32], BoxFileError> {
    if records.len() != TRANSPORT_SLOTS * RECORD_SIZE || tags.len() != TRANSPORT_SLOTS {
        return Err(BoxFileError::Size);
    }
    let mut contents = Contents::new();
    for (record, tag) in records.chunks_exact(RECORD_SIZE).zip(tags) {
        let record: &[u8; RECORD_SIZE] = record.try_into().map_err(|_| BoxFileError::Size)?;
        contents.slot(record, *tag, 0, &[0; STAMP_SIZE]);
    }
    Ok(contents.finish())
}

/// Writes the file's CRC-32.
pub fn seal(out: &mut [u8; FILE_SIZE]) {
    let crc = crc32(&out[..CRC]);
    out[CRC..].copy_from_slice(&crc.to_le_bytes());
}

/// Clears what a box file holds besides its box, as every write of it
/// leaves it (`sections::clean` for the Bank): a slot without a Pokémon
/// gets flag 0 and time 0, and the name zeros behind its end; then the file
/// is sealed. Every byte of a slot with a Pokémon and the empty records and
/// tags stay. Making it again changes nothing.
pub fn clean(file: &mut [u8; FILE_SIZE]) {
    for slot in 0..BOX_SLOTS {
        let record = RECORDS.start + slot * RECORD_SIZE;
        let empty = file[record..record + RECORD_SIZE]
            .try_into()
            .is_ok_and(|record| crate::sections::stored_species(record) == 0);
        if empty {
            file[FLAGS.start + slot] = 0;
            file[STAMPS.start + slot * STAMP_SIZE..][..STAMP_SIZE].fill(0);
        }
    }
    crate::sections::clean_name(&mut file[NAME]);
    seal(file);
}

/// A box file that passed every check.
#[derive(Clone, Copy, Debug)]
pub struct BoxFile<'a> {
    bytes: &'a [u8],
}

impl<'a> BoxFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, BoxFileError> {
        if bytes.len() != FILE_SIZE {
            return Err(BoxFileError::Size);
        }
        let stored =
            u32::from_le_bytes([bytes[CRC], bytes[CRC + 1], bytes[CRC + 2], bytes[CRC + 3]]);
        if crc32(&bytes[..CRC]) != stored {
            return Err(BoxFileError::Checksum);
        }
        Ok(Self { bytes })
    }
    pub fn records(&self) -> &'a [u8] {
        &self.bytes[RECORDS]
    }
    pub fn name(&self) -> &'a [u8] {
        &self.bytes[NAME]
    }
    pub fn tags(&self) -> &'a [u8] {
        &self.bytes[TAGS]
    }
    pub fn flags(&self) -> &'a [u8] {
        &self.bytes[FLAGS]
    }
    pub fn stamps(&self) -> &'a [u8] {
        &self.bytes[STAMPS]
    }
    /// Occupied slots.
    pub fn occupied(&self) -> u32 {
        crate::sections::occupied(self.records())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    /// A record with a valid checksum for `species`, encrypted with key
    /// `key`, as the games store one.
    pub fn record(key: u32, species: u16) -> [u8; RECORD_SIZE] {
        let mut plain = [0u16; 112];
        // Block A is wherever the shuffle puts it; the species is the first
        // word of block A. Put it in every block: the sum still works.
        for block in 0..4 {
            plain[block * 28] = species;
        }
        let sum = plain.iter().fold(0u16, |sum, word| sum.wrapping_add(*word));
        let mut out = [0u8; RECORD_SIZE];
        out[..4].copy_from_slice(&key.to_le_bytes());
        out[6..8].copy_from_slice(&sum.to_le_bytes());
        let mut seed = key;
        for (index, word) in plain.iter().enumerate() {
            seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
            let stored = word ^ (seed >> 16) as u16;
            out[8 + index * 2..10 + index * 2].copy_from_slice(&stored.to_le_bytes());
        }
        out
    }

    fn payload() -> Vec<u8> {
        let mut payload = vec![0u8; BANK_SIZE];
        for index in [0, 41, 99] {
            let place = bank_box(index).unwrap();
            for slot in 0..3 {
                let at = place.records.start + slot * RECORD_SIZE;
                payload[at..at + RECORD_SIZE]
                    .copy_from_slice(&record(0x1234_5678 + (index * 30 + slot) as u32, 25));
            }
            payload[place.label.clone()].fill(0x41 + index as u8);
            payload[place.tags.clone()].fill(7);
            payload[place.flags.clone()].fill(3);
            payload[place.stamps.clone()].fill(0x55);
        }
        payload
    }

    #[test]
    fn the_format_is_7298_bytes_with_its_fields_where_the_table_says() {
        assert_eq!(FILE_SIZE, 7298);
        assert_eq!(
            [
                RECORDS.start,
                NAME.start,
                TAGS.start,
                FLAGS.start,
                STAMPS.start,
                CRC
            ],
            [0, 0x1B30, 0x1B52, 0x1B70, 0x1B8E, 0x1C7E]
        );
    }

    #[test]
    fn the_boxes_of_the_bank_file_are_where_the_native_body_has_them() {
        let first = bank_box(0).unwrap();
        assert_eq!(first.records, 0..0x1B30);
        assert_eq!(first.label, 0x1B30..0x1B56);
        assert_eq!(first.tags.start, 0xAAD98);
        assert_eq!(first.flags.start, 0xAB950);
        assert_eq!(first.stamps.start, 0xAC508);
        let last = bank_box(99).unwrap();
        assert_eq!(last.stamps.end, BANK_SIZE);
        assert_eq!(bank_box(100), Err(BoxFileError::Index));
    }

    #[test]
    fn the_boxes_of_a_body_are_the_bank_files_boxes_at_the_native_offsets() {
        let first = body_box(0).unwrap();
        assert_eq!(first.records, 0x17C..0x1CAC);
        assert_eq!(first.label, 0x1CAC..0x1CD2);
        assert_eq!(first.tags.start, 0xACA44);
        assert_eq!(first.flags.start, 0xB4AA0);
        assert_eq!(first.stamps.start, 0xB5658);
        assert_eq!(body_box(99).unwrap().stamps.end, 0xBB418);
        assert_eq!(body_box(100), Err(BoxFileError::Index));
        // The payload spread into a body gives every box the same file.
        let payload = payload();
        let mut body = vec![0u8; BLOB_SIZE];
        body[..BANK_SIZE].copy_from_slice(&payload);
        crate::sections::expand(&mut body).unwrap();
        let (mut from_payload, mut from_body) = ([0u8; FILE_SIZE], [0u8; FILE_SIZE]);
        for index in [0, 41, 99] {
            encode_bank_box(&payload, index, &mut from_payload).unwrap();
            encode_body_box(&body, index, &mut from_body).unwrap();
            assert_eq!(from_body, from_payload, "box {index}");
        }
        assert_eq!(
            encode_body_box(&payload, 0, &mut from_body),
            Err(BoxFileError::Size)
        );
    }

    #[test]
    fn a_box_of_the_bank_is_its_pieces_of_the_bank_file_and_its_name() {
        // Box 41 of the payload: Pokémon in slots 0..3, the others empty with
        // flag 3 and a time; every byte of its label 0x6A: a name without
        // an end, an unused byte, a category and a position.
        let mut payload = payload();
        let place = bank_box(41).unwrap();
        payload[place.label.start..place.label.start + 6]
            .copy_from_slice(&[b'A', 0, b'B', 0, 0, 0]);
        let mut file = [0u8; FILE_SIZE];
        encode_bank_box(&payload, 41, &mut file).unwrap();
        let parsed = BoxFile::parse(&file).unwrap();
        assert_eq!(parsed.occupied(), 3);
        assert_eq!(parsed.records(), &payload[place.records.clone()]);
        assert_eq!(parsed.tags(), &payload[place.tags.clone()]);
        for slot in 0..BOX_SLOTS {
            let (flag, stamp) = (
                parsed.flags()[slot],
                &parsed.stamps()[slot * STAMP_SIZE..][..STAMP_SIZE],
            );
            if slot < 3 {
                assert_eq!((flag, stamp), (3, &[0x55; STAMP_SIZE][..]), "slot {slot}");
            } else {
                assert_eq!((flag, stamp), (0, &[0; STAMP_SIZE][..]), "slot {slot}");
            }
        }
        // The name's text alone: nothing behind its end, no category, no
        // position.
        let mut name = [0u8; NAME_TEXT];
        name[..4].copy_from_slice(&[b'A', 0, b'B', 0]);
        assert_eq!(parsed.name(), &name[..]);
        assert_eq!(&file[CRC..], &crc32(&file[..CRC]).to_le_bytes());
        // Cleaned already, and a box of the same contents.
        let mut again = file;
        clean(&mut again);
        assert_eq!(again, file);
        let mut other = vec![0u8; BANK_SIZE];
        let to = bank_box(7).unwrap();
        other[to.records.clone()].copy_from_slice(parsed.records());
        other[to.label.start..to.label.start + NAME_TEXT].copy_from_slice(parsed.name());
        other[to.tags].copy_from_slice(parsed.tags());
        other[to.flags].copy_from_slice(parsed.flags());
        other[to.stamps].copy_from_slice(parsed.stamps());
        assert_eq!(
            bank_box_contents(&other, 7).unwrap(),
            bank_box_contents(&payload, 41).unwrap()
        );
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused() {
        let payload = payload();
        let mut file = [0u8; FILE_SIZE];
        encode_bank_box(&payload, 0, &mut file).unwrap();
        let mut bad = file;
        bad[RECORDS.start + 20] ^= 1;
        assert_eq!(BoxFile::parse(&bad).unwrap_err(), BoxFileError::Checksum);
        assert_eq!(
            BoxFile::parse(&file[..100]).unwrap_err(),
            BoxFileError::Size
        );
        // A record whose own checksum fails, under a CRC that fits: taken
        // as it is, as the Bank holds one.
        let mut odd = file;
        odd[RECORDS.start + 6] ^= 1;
        seal(&mut odd);
        assert_eq!(BoxFile::parse(&odd).unwrap().records(), &odd[RECORDS]);
        // A file of the size before the unused bytes went.
        assert_eq!(
            BoxFile::parse(&[0u8; 7310]).unwrap_err(),
            BoxFileError::Size
        );
    }

    #[test]
    fn a_name_is_taken_whole_or_not_at_all() {
        let units: std::vec::Vec<u16> = "Caja de Traslado".encode_utf16().collect();
        let name = name_text(&units).unwrap();
        assert_eq!(name[..2], [b'C', 0]);
        assert_eq!(name[30..34], [b'o', 0, 0, 0]);
        // Its end, and what follows it, are not part of the name.
        let mut ended = units.clone();
        ended.extend([0, 0x41]);
        assert_eq!(name_text(&ended), Some(name));
        ended.insert(0, 0x41);
        assert_eq!(name_text(&ended[..17]), None);
        assert_eq!(name_text(&[]), Some([0; NAME_TEXT]));
    }

    #[test]
    fn the_transport_box_has_records_and_tags_only() {
        let mut records = vec![0u8; RECORDS_SIZE];
        records[..RECORD_SIZE].copy_from_slice(&record(9, 1));
        let tags = [4u8; TRANSPORT_SLOTS];
        let mut name = [0u8; NAME_TEXT];
        name[..4].copy_from_slice(&[b'T', 0, b'B', 0]);
        let mut file = [0u8; FILE_SIZE];
        encode_transport(&records, &tags, &name, &mut file).unwrap();
        assert_eq!(file[NAME], name);
        let parsed = BoxFile::parse(&file).unwrap();
        assert_eq!(parsed.occupied(), 1);
        assert!(file[FLAGS.start..STAMPS.end].iter().all(|byte| *byte == 0));
        assert_eq!(parsed.tags(), &tags);
        assert_eq!(
            transport_contents(&records, &tags).unwrap(),
            transport_contents(parsed.records(), parsed.tags()).unwrap()
        );
    }

    #[test]
    fn a_box_counts_by_its_pokemon_alone() {
        let payload = payload();
        let contents = |payload: &[u8]| bank_box_contents(payload, 41).unwrap();
        let before = contents(&payload);
        let place = bank_box(41).unwrap();
        // What an empty slot keeps besides is no change.
        let mut leftovers = payload.clone();
        leftovers[place.flags.start + 10] = 9;
        leftovers[place.stamps.start + 10 * STAMP_SIZE] = 9;
        assert_eq!(contents(&leftovers), before);
        // Nor is any byte of the label: the name, the unused byte, the
        // category and the position.
        for at in place.label.clone() {
            let mut changed = payload.clone();
            changed[at] ^= 0x5A;
            assert_eq!(contents(&changed), before, "byte {at:x}");
        }
        // A Pokémon's own bytes are.
        for (at, value) in [
            (place.flags.start, 9),
            (place.stamps.start, 9),
            (place.tags.start, 1),
        ] {
            let mut changed = payload.clone();
            changed[at] = value;
            assert_ne!(contents(&changed), before, "byte {at:x}");
        }
    }
}
