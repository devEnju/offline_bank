//! The transfer screen as a box manager (docs/internals.md, "The transfer screen"):
//! 104 places, the 100 boxes of the Bank, its transport box and three
//! offline boxes kept as box files on the SD card, what the blue and the
//! green cursor do with them, and how what the screen changed is written:
//! the Bank in one journal record without a game (`BankFiles::commit`), the
//! cells behind it, from a file proven before it, so that the next opening
//! finds every Pokémon in the Bank or a cell, as before the save or as after
//! it; a cut may leave a spare copy, never a Pokémon without a place.
//!
//! ```text
//! /3ds/Pokémon Bank/mover1.box .. mover3.box       the offline boxes; absent is an empty one
//! Bank's extdata: /AAAAAAAABBBBBBBB/              the folder of the save's two states (`interrupted::folder`)
//!     mover0.old, mover0.new                       the cells before and after the save
//! /3ds/Pokémon Bank/YYYYMMDD-HHMMSS/bank01.box     a checked box of the Bank, copied
//! /3ds/Pokémon Bank/YYYYMMDD-HHMMSS/mover0.box     the checked transport box, copied
//! /3ds/Pokémon Bank/YYYYMMDD-HHMMSS/mover1.box     a checked offline box, copied;
//!                                                  its cell is an empty box afterwards
//! ```
//!
//! Everything here is plain code over the native body, the offline boxes
//! and the `Sd` and `Files` traits, tested on the simulated card.

use crate::{
    bank_files::{BankFiles, Cleaned, Files},
    interrupted,
    sdfiles::{self, Date, Error, Extdata, Failed, Found, Path, Sd, ROOT},
};
use offline_core::{
    boxfile::{self, BoxFile, CATEGORY, FILE_SIZE, LABEL_SIZE, NAME_SIZE, NAME_TEXT, STAMP_SIZE},
    native_blob::{BLOB_SIZE, BOX_COUNT, GROUP_COUNT, GROUP_NAMES_OFFSET, GROUP_NAME_SIZE},
    sections::{self, BOX_SLOTS, RECORD_SIZE, TRANSPORT_RECORDS, TRANSPORT_TAGS},
};

/// The places of the screen: the boxes, the transport box, the offline
/// boxes.
pub const TRANSPORT: usize = BOX_COUNT;
pub const OFFLINE: usize = BOX_COUNT + 1;
pub const OFFLINE_COUNT: usize = 3;
pub const PLACES: usize = OFFLINE + OFFLINE_COUNT;

/// What the screen keeps beside the Bank: the three offline boxes as box
/// files, and the names of the four cells that only the console's texts
/// give. Each cell always holds a box, with Pokémon or without, as each of
/// the 100 boxes does; a cell without a file on the SD card holds an empty
/// one.
pub struct Offline {
    pub files: [[u8; FILE_SIZE]; OFFLINE_COUNT],
    /// The names in the console's language, all zero while not known: each
    /// offline cell's default name, and the transport box's name, which the
    /// upper screen shows and its session copy carries
    /// (`boxfile::encode_transport`).
    pub defaults: [[u8; NAME_TEXT]; OFFLINE_COUNT],
    pub transport: [u8; NAME_TEXT],
}
impl Offline {
    /// Room for the three boxes, not yet boxes: `load_offline` or `empty`
    /// makes them.
    pub const fn new() -> Self {
        Self {
            files: [[0; FILE_SIZE]; OFFLINE_COUNT],
            defaults: [[0; NAME_TEXT]; OFFLINE_COUNT],
            transport: [0; NAME_TEXT],
        }
    }
    /// Three empty boxes without names.
    pub fn empty() -> Self {
        let mut offline = Self::new();
        for file in &mut offline.files {
            empty_box(file, &[0; NAME_TEXT]);
        }
        offline
    }
    /// Each box without a name gets its cell's default (`defaults`).
    pub fn name_defaults(&mut self) {
        for (file, default) in self.files.iter_mut().zip(&self.defaults) {
            let name = &mut file[boxfile::NAME];
            if name[..2] == [0, 0] {
                name.copy_from_slice(default);
                boxfile::seal(file);
            }
        }
    }
}
impl Default for Offline {
    fn default() -> Self {
        Self::empty()
    }
}

/// An offline box without Pokémon, named `name`. A cell without a file
/// holds the one without a name.
pub(crate) fn empty_box(out: &mut [u8; FILE_SIZE], name: &[u8; NAME_TEXT]) {
    out.fill(0);
    for slot in 0..BOX_SLOTS {
        out[file_slot(slot).record].copy_from_slice(&empty_record());
    }
    out[boxfile::NAME].copy_from_slice(name);
    boxfile::seal(out);
}

/// Why a move is refused, with the original's message for it where it has
/// one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing goes into the transport box: "Cannot be transported there."
    /// (38:0x23).
    IntoTransport,
    /// The target holds Pokémon where they would go.
    Occupied,
    /// The source holds no Pokémon.
    Empty,
    /// Source and target are one place, or one that is none.
    Place,
}

/// A record of an empty slot, as the original writes one: a cleared
/// record, encrypted with key 0 (`001d95c8`). Its species reads as 0.
pub fn empty_record() -> [u8; RECORD_SIZE] {
    let mut out = [0u8; RECORD_SIZE];
    let mut seed = 0u32;
    for word in out[8..].chunks_exact_mut(2) {
        seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x6073);
        word.copy_from_slice(&((seed >> 16) as u16).to_le_bytes());
    }
    out
}

/// Where one slot of a place lies.
struct Slot {
    record: core::ops::Range<usize>,
    tag: usize,
    /// Flag byte and timestamp: none for the transport box.
    data: Option<(usize, core::ops::Range<usize>)>,
}

fn body_slot(place: usize, slot: usize) -> Slot {
    if place == TRANSPORT {
        let at = TRANSPORT_RECORDS.start + slot * RECORD_SIZE;
        return Slot {
            record: at..at + RECORD_SIZE,
            tag: TRANSPORT_TAGS.start + slot,
            data: None,
        };
    }
    // A box of the Bank: its bytes as `boxfile` describes them.
    let bank_box = boxfile::native_box(place);
    let at = bank_box.records.start + slot * RECORD_SIZE;
    let stamp = bank_box.stamps.start + slot * STAMP_SIZE;
    Slot {
        record: at..at + RECORD_SIZE,
        tag: bank_box.tags.start + slot,
        data: Some((bank_box.flags.start + slot, stamp..stamp + STAMP_SIZE)),
    }
}

fn file_slot(slot: usize) -> Slot {
    let at = boxfile::RECORDS.start + slot * RECORD_SIZE;
    Slot {
        record: at..at + RECORD_SIZE,
        tag: boxfile::TAGS.start + slot,
        data: Some((
            boxfile::FLAGS.start + slot,
            boxfile::STAMPS.start + slot * STAMP_SIZE
                ..boxfile::STAMPS.start + (slot + 1) * STAMP_SIZE,
        )),
    }
}

/// The label of box `place` of the Bank: its name, unused byte, category
/// and the position word behind it (`boxfile::native_box`).
fn label_range(place: usize) -> core::ops::Range<usize> {
    boxfile::native_box(place).label
}

fn occupied_record(record: &[u8]) -> bool {
    record
        .try_into()
        .is_ok_and(|record| sections::stored_species(record) != 0)
}

/// The places and what they hold: the native body and the offline boxes.
pub struct Places<'a> {
    pub body: &'a mut [u8],
    pub offline: &'a mut Offline,
}

impl Places<'_> {
    fn valid(&self, place: usize) -> bool {
        place < PLACES && self.body.len() == BLOB_SIZE
    }

    /// Whether slot `slot` of `place` holds a Pokémon.
    pub fn occupied_slot(&self, place: usize, slot: usize) -> bool {
        if place >= OFFLINE {
            occupied_record(&self.offline.files[place - OFFLINE][file_slot(slot).record])
        } else {
            occupied_record(&self.body[body_slot(place, slot).record])
        }
    }

    /// Pokémon in `place`.
    pub fn count(&self, place: usize) -> u32 {
        if !self.valid(place) {
            return 0;
        }
        (0..BOX_SLOTS)
            .filter(|slot| self.occupied_slot(place, *slot))
            .count() as u32
    }

    /// Writes slot `slot` of `place` from `record`, `tag`, `flag` and
    /// `stamp`.
    fn put_slot(
        &mut self,
        place: usize,
        slot: usize,
        record: &[u8],
        tag: u8,
        flag: u8,
        stamp: &[u8],
    ) {
        let (bytes, at) = if place >= OFFLINE {
            (
                &mut self.offline.files[place - OFFLINE][..],
                file_slot(slot),
            )
        } else {
            (&mut self.body[..], body_slot(place, slot))
        };
        bytes[at.record].copy_from_slice(record);
        bytes[at.tag] = tag;
        if let Some((flag_at, stamp_at)) = at.data {
            bytes[flag_at] = flag;
            bytes[stamp_at].copy_from_slice(stamp);
        }
    }

    /// Slot `slot` of `place` as the upper screen shows it: its record and
    /// its tag.
    pub fn slot(&self, place: usize, slot: usize) -> ([u8; RECORD_SIZE], u8) {
        let (record, tag, ..) = self.get_slot(place, slot);
        (record, tag)
    }

    /// Slot `slot` of `place`: record, tag, flag byte, timestamp.
    fn get_slot(&self, place: usize, slot: usize) -> ([u8; RECORD_SIZE], u8, u8, [u8; STAMP_SIZE]) {
        let (bytes, at): (&[u8], Slot) = if place >= OFFLINE {
            (&self.offline.files[place - OFFLINE], file_slot(slot))
        } else {
            (self.body, body_slot(place, slot))
        };
        let mut record = [0u8; RECORD_SIZE];
        record.copy_from_slice(&bytes[at.record]);
        let (flag, stamp) = match at.data {
            Some((flag_at, stamp_at)) => {
                let mut stamp = [0u8; STAMP_SIZE];
                stamp.copy_from_slice(&bytes[stamp_at]);
                (bytes[flag_at], stamp)
            }
            None => (0, [0; STAMP_SIZE]),
        };
        (record, bytes[at.tag], flag, stamp)
    }

    /// The `NAME_TEXT` name bytes of `place`, a box of Bank or an offline
    /// box, as stored. The original's name setter copies the whole `0x22`
    /// bytes from a work string (`002ba124`), so a stored name can hold what
    /// an earlier text left behind its end: no part of the name, but bytes
    /// of the box that a move carries along unchanged. The category and the
    /// position behind them belong to the place in Bank, and a box moved
    /// there joins that category at that position.
    fn name_bytes(&self, place: usize) -> [u8; NAME_TEXT] {
        let label = if place >= OFFLINE {
            &self.offline.files[place - OFFLINE][boxfile::NAME]
        } else {
            &self.body[label_range(place)]
        };
        let mut name = [0u8; NAME_TEXT];
        name.copy_from_slice(&label[..NAME_TEXT]);
        name
    }

    /// The name of `place`, a box of Bank or an offline box: its text only
    /// (`name_text`), as the screen shows it.
    pub fn label(&self, place: usize) -> [u8; NAME_TEXT] {
        boxfile::text_only(&self.name_bytes(place))
    }

    /// The group of the box of Bank at `place` and its position in it (from
    /// 0): the label's category byte and the word behind the name. `None`
    /// for any other place, or a category the Bank has no group for.
    pub fn group(&self, place: usize) -> Option<(u8, u16)> {
        if place >= TRANSPORT || self.body.len() != BLOB_SIZE {
            return None;
        }
        let label = &self.body[label_range(place)];
        let group = label[CATEGORY];
        let position = u16::from_le_bytes([label[NAME_SIZE], label[NAME_SIZE + 1]]);
        (usize::from(group) < GROUP_COUNT).then_some((group, position))
    }

    /// How many boxes of Bank are in `group`.
    pub fn group_size(&self, group: u8) -> u16 {
        (0..TRANSPORT)
            .filter(|&place| self.group(place).is_some_and(|(of, _)| of == group))
            .count() as u16
    }

    /// The box of Bank at `position` in `group`.
    pub fn in_group(&self, group: u8, position: u16) -> Option<usize> {
        (0..TRANSPORT).find(|&place| self.group(place) == Some((group, position)))
    }

    /// The name of `group`: its text only.
    pub fn group_name(&self, group: u8) -> Option<[u8; NAME_TEXT]> {
        if usize::from(group) >= GROUP_COUNT || self.body.len() != BLOB_SIZE {
            return None;
        }
        let at = GROUP_NAMES_OFFSET + usize::from(group) * GROUP_NAME_SIZE;
        let mut name = [0u8; NAME_TEXT];
        name.copy_from_slice(&self.body[at..at + NAME_TEXT]);
        Some(boxfile::text_only(&name))
    }

    /// The box of Bank at `place` goes to `position` in its group, and the
    /// boxes between move one up or down to make room, so the group keeps
    /// every position once and without a gap; only position words change.
    /// The screen moves through `Changes`.
    fn insert_position(&mut self, place: usize, position: u16) -> Result<(), Refusal> {
        let (group, from) = self.group(place).ok_or(Refusal::Place)?;
        if position == from || position >= self.group_size(group) {
            return Err(Refusal::Place);
        }
        for other in (0..TRANSPORT).filter(|&other| other != place) {
            let Some((of, at)) = self.group(other) else {
                continue;
            };
            let moved = match () {
                _ if of != group => None,
                _ if position < from && (position..from).contains(&at) => Some(at + 1),
                _ if position > from && (from + 1..=position).contains(&at) => Some(at - 1),
                _ => None,
            };
            if let Some(at) = moved {
                self.set_position(other, at);
            }
        }
        self.set_position(place, position);
        Ok(())
    }

    fn set_position(&mut self, place: usize, position: u16) {
        let at = label_range(place).start + NAME_SIZE;
        self.body[at..at + 2].copy_from_slice(&position.to_le_bytes());
    }

    /// What the label of `place` shows (`boxfile::shown_label`): a box of
    /// Bank's name, category and position; an offline box's name, the rest
    /// zero, as its file holds no more; none for the transport box.
    fn shown_label(&self, place: usize) -> [u8; LABEL_SIZE] {
        let mut label = [0u8; LABEL_SIZE];
        if place >= OFFLINE {
            label[..NAME_TEXT].copy_from_slice(&self.offline.files[place - OFFLINE][boxfile::NAME]);
        } else if place < TRANSPORT {
            label.copy_from_slice(&self.body[label_range(place)]);
        }
        boxfile::shown_label(&label)
    }

    /// Names `place`, a box of Bank or an offline box, as the original's
    /// name setter does (`002ba124`: `NAME_TEXT` bytes).
    fn set_label(&mut self, place: usize, name: &[u8; NAME_TEXT]) {
        let label = if place >= OFFLINE {
            &mut self.offline.files[place - OFFLINE][boxfile::NAME]
        } else {
            &mut self.body[label_range(place)][..NAME_TEXT]
        };
        label.copy_from_slice(name);
    }

    /// After two boxes of Bank changed places: one of a different category
    /// keeps its category and its position in it, so each takes them along
    /// (the unused byte, the category, the position: `label` stops before
    /// them). Of the same category they stay with the places, so the two
    /// change positions: their order in the category changes. Either way
    /// every category keeps its positions unique and without gaps. An
    /// offline box holds no category: the place in Bank it leaves or fills
    /// keeps its own.
    fn keep_categories(&mut self, from: usize, to: usize) {
        if from >= TRANSPORT || to >= TRANSPORT {
            return;
        }
        let (a, b) = (label_range(from), label_range(to));
        if self.body[a.start + CATEGORY] == self.body[b.start + CATEGORY] {
            return;
        }
        let mut tail = [0u8; LABEL_SIZE - NAME_TEXT];
        tail.copy_from_slice(&self.body[a.start + NAME_TEXT..a.end]);
        self.body
            .copy_within(b.start + NAME_TEXT..b.end, a.start + NAME_TEXT);
        self.body[b.start + NAME_TEXT..b.end].copy_from_slice(&tail);
    }

    /// Seals the offline boxes a move changed, the places at its two ends
    /// that are offline boxes: their checksums. A move changes no other
    /// place, so the others keep theirs.
    fn seal(&mut self, places: [usize; 2]) {
        for place in places.into_iter().filter(|place| *place >= OFFLINE) {
            if let Some(file) = self.offline.files.get_mut(place - OFFLINE) {
                boxfile::seal(file);
            }
        }
    }

    /// The green cursor: every Pokémon of `from` into the same slots of
    /// `to`, with its record and tag, flag 0 and the time `now`, from every
    /// place as from the transport box, which keeps no flag bytes or times:
    /// as a Pokémon put into a slot gets them (`001d86e0`), with 0 for the
    /// game this screen does not have. So a Pokémon's flag and time say when
    /// it last moved here (nothing of the program reads them). Each slot it
    /// leaves gets an empty record and tag 0 and keeps its flag and time,
    /// which a write clears (`sections::clean`); a slot without a Pokémon
    /// is not touched on either side. Put down on `from` itself, they stay
    /// in their slots and get flag 0 and the time `now` all the same; the
    /// transport box, put down on itself, keeps what it holds, as it keeps
    /// no times. Names stay. Refused into the transport box from elsewhere,
    /// and if another `to` holds any Pokémon, even where they would fit.
    /// The screen moves through `Changes`.
    fn move_all(&mut self, from: usize, to: usize, now: [u8; STAMP_SIZE]) -> Result<(), Refusal> {
        if !self.valid(from) || !self.valid(to) {
            return Err(Refusal::Place);
        }
        if to == TRANSPORT && from != TRANSPORT {
            return Err(Refusal::IntoTransport);
        }
        if self.count(from) == 0 {
            return Err(Refusal::Empty);
        }
        if from != to && self.count(to) != 0 {
            return Err(Refusal::Occupied);
        }
        for slot in 0..BOX_SLOTS {
            if !self.occupied_slot(from, slot) {
                continue;
            }
            let (record, tag, flag, stamp) = self.get_slot(from, slot);
            if from != to {
                self.put_slot(from, slot, &empty_record(), 0, flag, &stamp);
            }
            self.put_slot(to, slot, &record, tag, 0, &now);
        }
        self.seal([from, to]);
        Ok(())
    }

    /// The blue cursor: `from` and `to` change places whole, every slot
    /// with its record, tag, flag byte and timestamp, and the name with all
    /// its `NAME_TEXT` bytes, also those behind its end. A default name
    /// belongs to its place (`defaults`, of `from` and of `to`: "Bank 12"
    /// for box 12, "Box 2" for offline box 2, in the console's language,
    /// as the bytes the place takes for it): a box whose name's text is the
    /// default of the place it is at, or that has no name, brings none, and
    /// the other place then takes its own default. A default away from its
    /// place is a name like any other. A place whose default is not known
    /// (no text) keeps its name where it would take the default. Nothing
    /// goes into or comes out of the transport box: its Pokémon leave it
    /// only with the green cursor. The screen swaps through `Changes`.
    fn swap(
        &mut self,
        from: usize,
        to: usize,
        defaults: [[u8; NAME_TEXT]; 2],
    ) -> Result<(), Refusal> {
        if !self.valid(from) || !self.valid(to) || from == to || from == TRANSPORT {
            return Err(Refusal::Place);
        }
        if to == TRANSPORT {
            return Err(Refusal::IntoTransport);
        }
        if self.count(from) == 0 && self.count(to) == 0 {
            return Err(Refusal::Empty);
        }
        // The names as they are before anything moves, and what each place
        // is named when the other brings none.
        let names = [self.name_bytes(from), self.name_bytes(to)];
        let brought = |side: usize| {
            let text = boxfile::text_only(&names[side]);
            (text[..2] != [0, 0] && text != boxfile::text_only(&defaults[side]))
                .then_some(names[side])
        };
        let own = |side: usize| {
            if defaults[side][..2] == [0, 0] {
                names[side]
            } else {
                defaults[side]
            }
        };
        for slot in 0..BOX_SLOTS {
            let a = self.get_slot(from, slot);
            let b = self.get_slot(to, slot);
            self.put_slot(to, slot, &a.0, a.1, a.2, &a.3);
            self.put_slot(from, slot, &b.0, b.1, b.2, &b.3);
        }
        self.set_label(to, &brought(0).unwrap_or_else(|| own(1)));
        self.set_label(from, &brought(1).unwrap_or_else(|| own(0)));
        self.keep_categories(from, to);
        self.seal([from, to]);
        Ok(())
    }
}

/// A SHA-256 fingerprint of a place's bytes (`Places::fingerprint`).
pub type Fingerprint = [u8; 32];

impl Places<'_> {
    /// What `place` holds but its label (`shown_label`), as a change counts
    /// it (`boxfile::Contents`, by which the backup of an interrupted save
    /// compares boxes too): its slots, the transport box's without flag
    /// bytes or times. So empty boxes that changed places hold what they
    /// held, while Pokémon the green cursor moved out and back hold the time
    /// of the move; leaving without saving keeps the files as they were. An
    /// offline box's file holds besides only its name, its label here, and
    /// the CRC the move's seal writes from the rest.
    fn fingerprint(&self, place: usize) -> Fingerprint {
        let mut contents = boxfile::Contents::new();
        for slot in 0..BOX_SLOTS {
            let (record, tag, flag, stamp) = self.get_slot(place, slot);
            contents.slot(&record, tag, flag, &stamp);
        }
        contents.finish()
    }
}

/// What the screen's moves changed, so that DONE asks to save only after a
/// change; what is written is decided by the bytes (`commit`). A move
/// writes only bytes of its two places, L only position words of one group,
/// and the screen changes the places only through this record. For each
/// place it keeps, from when the screen opened, the fingerprint of what it
/// holds (`fingerprint`: its slots with Pokémon, each with all its bytes,
/// and which are empty) and what its label shows (`boxfile::shown_label`:
/// the name's text, the category, the position), each taken before the
/// first change that can touch it, and after each change whether the place
/// now differs in either. So a place changed exactly when either differs: blue swaps back and forth,
/// or round and back, change nothing; empty boxes that ended up at each
/// other's places change nothing; L round its group back to the start
/// changes nothing. A put-down of the green cursor is a change whatever
/// follows (`moved`), also on its own box and when its Pokémon are moved
/// back, as each Pokémon it puts down gets the time of the move
/// (`Places::move_all`), even where the clock gives a time a Pokémon
/// already held; only the transport box put down on itself changes
/// nothing, as it keeps no times. L touches only labels, so it hashes
/// nothing, also in a group of 100 boxes.
pub struct Changes {
    /// Each place's fingerprint when the screen opened.
    before: [Fingerprint; PLACES],
    /// `before` is taken.
    seen: [bool; PLACES],
    /// What each place's label showed when the screen opened.
    labels: [[u8; LABEL_SIZE]; PLACES],
    /// `labels` is taken.
    labels_seen: [bool; PLACES],
    /// After the last change that touched the place: what it holds, and
    /// what its label shows, differ from when the screen opened.
    holds_other: [bool; PLACES],
    shows_other: [bool; PLACES],
    /// The green cursor put Pokémon down with the time of the move.
    moved: bool,
}

impl Changes {
    pub const fn new() -> Self {
        Changes {
            before: [[0; 32]; PLACES],
            seen: [false; PLACES],
            labels: [[0; LABEL_SIZE]; PLACES],
            labels_seen: [false; PLACES],
            holds_other: [false; PLACES],
            shows_other: [false; PLACES],
            moved: false,
        }
    }

    /// The blue cursor's swap (`Places::swap`).
    pub fn swap(
        &mut self,
        places: &mut Places,
        from: usize,
        to: usize,
        defaults: [[u8; NAME_TEXT]; 2],
    ) -> Result<(), Refusal> {
        self.around(places, from, to, |places| places.swap(from, to, defaults))
    }

    /// The green cursor's move (`Places::move_all`).
    pub fn move_all(
        &mut self,
        places: &mut Places,
        from: usize,
        to: usize,
        now: [u8; STAMP_SIZE],
    ) -> Result<(), Refusal> {
        self.around(places, from, to, |places| places.move_all(from, to, now))?;
        // Only into the transport box, from itself, is nothing stamped.
        self.moved |= to != TRANSPORT;
        Ok(())
    }

    /// L with the blue cursor: the box of Bank at `place` goes to `position`
    /// in its group (`Places::insert_position`).
    pub fn insert(
        &mut self,
        places: &mut Places,
        place: usize,
        position: u16,
    ) -> Result<(), Refusal> {
        let (group, _) = places.group(place).ok_or(Refusal::Place)?;
        // An insert changes no box's group: its members are the same after.
        let mut members = [false; TRANSPORT];
        for (other, member) in members.iter_mut().enumerate() {
            *member = places.group(other).is_some_and(|(of, _)| of == group);
            if *member {
                self.see_label(places, other);
            }
        }
        places.insert_position(place, position)?;
        for (other, _) in members.iter().enumerate().filter(|(_, member)| **member) {
            self.shows_other[other] = places.shown_label(other) != self.labels[other];
        }
        Ok(())
    }

    /// Something changed: the green cursor put Pokémon down, or some place
    /// holds or shows something else than when the screen opened.
    pub fn any(&self) -> bool {
        self.moved || (0..PLACES).any(|place| self.changed(place))
    }

    fn changed(&self, place: usize) -> bool {
        self.holds_other[place] || self.shows_other[place]
    }

    fn see_label(&mut self, places: &Places, place: usize) {
        if !self.labels_seen[place] {
            self.labels_seen[place] = true;
            self.labels[place] = places.shown_label(place);
        }
    }

    /// A move between `from` and `to`, with what each place was when the
    /// screen opened taken before it, and both after it.
    fn around(
        &mut self,
        places: &mut Places,
        from: usize,
        to: usize,
        change: impl FnOnce(&mut Places) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        if from >= PLACES || to >= PLACES {
            return Err(Refusal::Place);
        }
        for place in [from, to] {
            if !self.seen[place] {
                self.seen[place] = true;
                self.before[place] = places.fingerprint(place);
            }
            self.see_label(places, place);
        }
        change(places)?;
        for place in [from, to] {
            self.holds_other[place] = places.fingerprint(place) != self.before[place];
            self.shows_other[place] = places.shown_label(place) != self.labels[place];
        }
        Ok(())
    }
}

impl Default for Changes {
    fn default() -> Self {
        Self::new()
    }
}

/// The state of a cell: 0 a box without Pokémon, 1 one with Pokémon, 2 one
/// with Pokémon that is checked, as the original's cells are
/// (`box_check_00`, `01`, `02`).
pub fn cell_state(count: u32, checked: bool) -> u8 {
    match (count, checked) {
        (0, _) => 0,
        (_, false) => 1,
        (_, true) => 2,
    }
}

// ---------------------------------------------------------------- files

/// The file of cell `index`.
pub(crate) fn cell_path(index: usize) -> Path {
    sdfiles::box_file(&Path::new(ROOT), OFFLINE + index)
}

/// Reads the three offline boxes. A file that is there must be a whole box
/// file; one that is not is refused and stays as it is. Every box file is of
/// one kind, so a session's copy of a box of Bank or of the transport box
/// copied here is an offline box as it is. A cell without a file holds an
/// empty box without a name, until the screen names it
/// (`Offline::name_defaults`). The names of the cells are not known until
/// the screen's first frame loads them: they are cleared here, as the room
/// they lie in serves other work between two screens.
pub fn load_offline<E, S: Sd>(sd: &mut S, offline: &mut Offline) -> Result<(), Error<E, S::Error>> {
    offline.defaults = [[0; NAME_TEXT]; OFFLINE_COUNT];
    offline.transport = [0; NAME_TEXT];
    for (index, file) in offline.files.iter_mut().enumerate() {
        let found = sd.get(&cell_path(index), file).map_err(Error::Sd)?;
        match found {
            Found::Absent => empty_box(file, &[0; NAME_TEXT]),
            Found::OtherSize => return Err(Error::BadFile(OFFLINE + index)),
            Found::Read => {
                BoxFile::parse(file).map_err(|_| Error::BadFile(OFFLINE + index))?;
            }
        }
    }
    Ok(())
}

/// What the screen asks to be written.
pub struct Commit<'a> {
    /// The checked places, copied into the session's folder.
    pub checked: &'a [bool; PLACES],
    /// The console's date and time, which names the session's folder, a new
    /// one (`sdfiles::create_dated_folder`).
    pub date: Date,
}

/// Writes what the screen changed (docs/internals.md, "The transfer screen"):
/// `body` is the native body as the screen left it and is compacted when the
/// Bank is written; `offline` the cells as it left them, and as they are on
/// the card afterwards. The body and the cells are cleaned first
/// (`sections::clean`, `boxfile::clean`), before anything is compared or
/// counted, as every write leaves them. The Bank is always written, as Save
/// and Quit writes it; a cell only where its bytes differ from the card's.
/// `scratch` holds one box file.
///
/// | Step | What |
/// | --- | --- |
/// | 1 | the checked boxes are copied into the session's folder, one no folder was of before |
/// | 2 | `mover0.old` of the cells that change, proven; a cell among them without a file gets the empty box it holds, so that writing it later needs no new room; then `mover0.new`, proven; both in the folder in Bank's extdata of the save's two states (`interrupted::folder`): the journal's now and the one the record of step 3 names |
/// | 3 | the Bank, in one journal record without a game (`BankFiles::commit`) |
/// | 4 | the cells that change, a checked one as an empty box with its default name |
/// | 5 | `mover0.new` is deleted, then `mover0.old` and the folder (`interrupted::remove`), so that a cut leaves the side the opening keeps |
///
/// Every record writes the Bank file into its other copy, so the two states
/// differ and the next opening tells by the journal alone whether the save
/// went through (`interrupted::settle`). A step before the record that
/// fails ends here, with no cell changed. Once the record's first replica is
/// durable the save is made, and the answer says so: a failure of steps 4
/// and 5 ends nothing, as the next opening finishes them from the folder. The
/// opening before it left no such folder, so none of an earlier save can
/// count for the states of this one.
#[inline(never)]
pub fn commit<F: Files, S: Sd, X: Extdata<Error = S::Error>>(
    files: &mut BankFiles<F>,
    sd: &mut S,
    ext: &mut X,
    body: &mut [u8],
    offline: &mut Offline,
    request: &Commit,
    scratch: &mut [u8; FILE_SIZE],
) -> Result<(), Failed<F, S>> {
    let body = Cleaned::new(body).ok_or(Error::Staging)?;
    for file in &mut offline.files {
        boxfile::clean(file);
    }
    // Which cells change: one whose new box is not what it holds, a cell
    // without a file holding an empty box without a name; a checked one is
    // decided once its new box is known (step 2). Every cell's file is read
    // here, before anything is written, and one that is not a whole box is
    // refused: it could not be put back.
    let mut changed = [false; OFFLINE_COUNT];
    for (index, changes) in changed.iter_mut().enumerate() {
        *changes = differs(sd, index, &offline.files[index], scratch)?;
    }
    // The states the save goes between, and the folder of the cells.
    let [before, after] = files
        .commit_states(&body)
        .map_err(Error::Files)?
        .map(|(bank, mover)| interrupted::State { bank, mover });
    let checked = request.checked.iter().any(|checked| *checked);
    let folder = interrupted::folder(before, after);
    if checked || changed.contains(&true) {
        sdfiles::create_root(sd).map_err(Error::Sd)?;
    }
    // 1: the session's copies.
    if checked {
        let session = sdfiles::create_dated_folder(sd, request.date).map_err(Error::Sd)?;
        for place in (0..PLACES).filter(|place| request.checked[*place]) {
            if place < TRANSPORT {
                boxfile::encode_body_box(&body, place, scratch).map_err(|_| Error::Staging)?;
            } else if place == TRANSPORT {
                boxfile::encode_transport(
                    &body[TRANSPORT_RECORDS],
                    &body[TRANSPORT_TAGS],
                    &offline.transport,
                    scratch,
                )
                .map_err(|_| Error::Staging)?;
            } else {
                scratch.copy_from_slice(&offline.files[place - OFFLINE]);
            }
            sd.put(&sdfiles::box_file(&session, place), &scratch[..])
                .map_err(Error::Sd)?;
        }
    }
    // 2: a checked cell is decided here, by its new box. The folder only for
    // a cell that changes.
    for (index, changes) in changed.iter_mut().enumerate() {
        if request.checked[OFFLINE + index] {
            let default = offline.defaults[index];
            empty_box(&mut offline.files[index], &default);
            *changes = differs(sd, index, &offline.files[index], scratch)?;
        }
    }
    let made = changed.contains(&true);
    if made {
        ext.create_dir(&folder).map_err(Error::Sd)?;
        let included = [false, changed[0], changed[1], changed[2]];
        let mut as_it_is =
            |position: usize, to: &mut [u8; FILE_SIZE]| held(sd, position - 1, to).map(|_| ());
        let old = interrupted::cells_file(&folder, false);
        interrupted::write_group(ext, &old, &included, &mut as_it_is, scratch)?;
        for index in (0..OFFLINE_COUNT).filter(|index| changed[*index]) {
            if !held(sd, index, scratch)? {
                sd.put(&cell_path(index), &scratch[..]).map_err(Error::Sd)?;
            }
        }
        let mut as_it_will_be = |position: usize, to: &mut [u8; FILE_SIZE]| {
            to.copy_from_slice(&offline.files[position - 1]);
            Ok(())
        };
        let new = interrupted::cells_file(&folder, true);
        interrupted::write_group(ext, &new, &included, &mut as_it_will_be, scratch)?;
        ext.rest();
    }
    // 3: the Bank, as Save and Quit always writes it; its record decides the
    // save, and the cells follow it.
    files.commit(body).map_err(Error::Files)?;
    // 4 and 5: the save is made. A failure here ends nothing: the folder
    // stays, and the next opening finishes the cells from it.
    if made {
        let written = (0..OFFLINE_COUNT)
            .filter(|index| changed[*index])
            .try_for_each(|index| sd.put(&cell_path(index), &offline.files[index]));
        if written.is_ok() {
            let new = interrupted::cells_file(&folder, true);
            let _ = ext
                .delete(&new)
                .and_then(|()| interrupted::remove(ext, &folder));
        }
    }
    Ok(())
}

/// The box cell `index` holds, into `out`: its file's, which must be a whole
/// box, or an empty box without a name for a cell without a file. The answer
/// is whether it has a file.
fn held<E, S: Sd>(
    sd: &mut S,
    index: usize,
    out: &mut [u8; FILE_SIZE],
) -> Result<bool, Error<E, S::Error>> {
    match sd.get(&cell_path(index), out).map_err(Error::Sd)? {
        Found::Absent => {
            empty_box(out, &[0; NAME_TEXT]);
            Ok(false)
        }
        Found::Read if BoxFile::parse(out).is_ok() => Ok(true),
        _ => Err(Error::BadFile(OFFLINE + index)),
    }
}

/// Whether cell `index` holds another box than `new` (`held`); its box is
/// left in `scratch`.
fn differs<E, S: Sd>(
    sd: &mut S,
    index: usize,
    new: &[u8; FILE_SIZE],
    scratch: &mut [u8; FILE_SIZE],
) -> Result<bool, Error<E, S::Error>> {
    held(sd, index, scratch)?;
    Ok(scratch[..] != new[..])
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::bank_files::body_crcs;
    use crate::sdfiles::Store;
    use crate::testing::*;
    use offline_core::{
        sections::{Identity, BANK_RANGES},
        Phase,
    };
    use std::{collections::BTreeMap, vec, vec::Vec};

    const NOW: [u8; 8] = [0xea, 0x07, 11, 9, 12, 30, 0, 0];
    /// No default name known for either place.
    const UNKNOWN: [[u8; NAME_TEXT]; 2] = [[0; NAME_TEXT]; 2];

    fn put(body: &mut [u8], place: usize, slot: usize, id: u32) {
        let at = body_slot(place, slot);
        body[at.record].copy_from_slice(&pokemon(id));
        body[at.tag] = 7;
        if let Some((flag, stamp)) = at.data {
            body[flag] = 3;
            body[stamp].fill(id as u8);
        }
    }

    /// A body whose boxes are all empty, but for `filled`.
    fn bank_body(filled: &[(usize, usize, u32)]) -> Vec<u8> {
        let mut body = with_boxes(0, &[], 0);
        for place in 0..BOX_COUNT {
            body[label_range(place)].fill(0x40 + (place % 26) as u8);
        }
        for (place, slot, id) in filled {
            put(&mut body, *place, *slot, *id);
        }
        body
    }

    /// Every Pokémon the places hold, by identity, with where it is.
    fn everyone(body: &[u8], offline: &Offline) -> BTreeMap<Identity, usize> {
        let mut out = BTreeMap::new();
        let mut copy = body.to_vec();
        let mut offline_copy = Offline::empty();
        offline_copy.files = offline.files;
        let places = Places {
            body: &mut copy,
            offline: &mut offline_copy,
        };
        for place in 0..PLACES {
            for slot in 0..BOX_SLOTS {
                let (record, ..) = places.get_slot(place, slot);
                if let Some(identity) = sections::stored_identity(&record) {
                    if let Some(other) = out.insert(identity, place) {
                        panic!("twice: {identity:?} at {place} slot {slot} and {other}");
                    }
                }
            }
        }
        out
    }

    #[test]
    fn a_box_s_bytes_are_where_the_body_has_them() {
        // The ranges `boxfile::native_box` gives, as the native body lays out
        // a box of the Bank: records and label one after the other, then a
        // tag, a flag byte and a time per slot in their own tables.
        use offline_core::sections::{
            BOXES_START, BOX_STRIDE, FLAGS_START, STAMPS_START, TAGS_START,
        };
        for place in 0..TRANSPORT {
            let label = BOXES_START + place * BOX_STRIDE + BOX_SLOTS * RECORD_SIZE;
            assert_eq!(label_range(place), label..label + LABEL_SIZE);
            for slot in 0..BOX_SLOTS {
                let at = body_slot(place, slot);
                let record = BOXES_START + place * BOX_STRIDE + slot * RECORD_SIZE;
                let index = place * BOX_SLOTS + slot;
                assert_eq!(at.record, record..record + RECORD_SIZE);
                assert_eq!(at.tag, TAGS_START + index);
                let stamp = STAMPS_START + index * STAMP_SIZE;
                assert_eq!(
                    at.data,
                    Some((FLAGS_START + index, stamp..stamp + STAMP_SIZE))
                );
            }
        }
    }

    #[test]
    fn an_empty_slot_reads_as_empty() {
        assert_eq!(sections::stored_species(&empty_record()), 0);
    }

    #[test]
    fn blue_swaps_two_boxes_whole_with_their_names() {
        let mut body = bank_body(&[(3, 0, 1), (3, 5, 2), (7, 1, 3)]);
        let mut offline = Offline::empty();
        let names = (body[label_range(3)].to_vec(), body[label_range(7)].to_vec());
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        places.swap(3, 7, UNKNOWN).unwrap();
        assert_eq!((places.count(3), places.count(7)), (1, 2));
        // The names go with the boxes; the two are of different categories
        // (their last name bytes differ), so category and position go too.
        assert_ne!(names.0[CATEGORY], names.1[CATEGORY]);
        assert_eq!(places.body[label_range(7)], names.0[..]);
        assert_eq!(places.body[label_range(3)], names.1[..]);
        // Flag bytes and times went with them.
        assert_eq!(places.body[body_slot(7, 5).data.unwrap().0], 3);
        assert!(places.body[body_slot(7, 5).data.unwrap().1]
            .iter()
            .all(|b| *b == 2));
        // Nothing into the transport box.
        assert_eq!(
            places.swap(3, TRANSPORT, UNKNOWN),
            Err(Refusal::IntoTransport)
        );
    }

    #[test]
    fn a_box_goes_offline_with_its_name_and_comes_back() {
        let mut body = bank_body(&[(0, 0, 1), (0, 29, 2)]);
        let mut offline = Offline::empty();
        let name = body[label_range(0)].to_vec();
        let before = everyone(&body, &offline);
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        assert_eq!(places.swap(0, OFFLINE + 1, UNKNOWN), Ok(()));
        assert_eq!(places.count(0), 0);
        // The offline box takes the name's bytes, not its category.
        assert_eq!(places.offline.files[1][boxfile::NAME], name[..NAME_TEXT]);
        let file = BoxFile::parse(&places.offline.files[1]).unwrap();
        assert_eq!(file.occupied(), 2);
        let moved: Vec<_> = everyone(places.body, places.offline).into_keys().collect();
        assert_eq!(moved, before.into_keys().collect::<Vec<_>>());
        // Back onto another box, with its name; box 50 keeps its category
        // and position.
        let place = places.body[label_range(50)][NAME_TEXT..].to_vec();
        assert_eq!(places.swap(OFFLINE + 1, 50, UNKNOWN), Ok(()));
        // The cell keeps a box, now without Pokémon, with box 50's name.
        assert_eq!(places.count(OFFLINE + 1), 0);
        assert!(BoxFile::parse(&places.offline.files[1]).is_ok());
        assert_eq!(places.count(50), 2);
        assert_eq!(places.body[label_range(50)][..NAME_TEXT], name[..NAME_TEXT]);
        assert_eq!(places.body[label_range(50)][NAME_TEXT..], place[..]);
    }

    #[test]
    fn categories_go_with_their_boxes_and_positions_swap_within_one() {
        // Every box in a category of its own position: byte 0x23 the
        // category, the word behind it the position.
        let mut body = bank_body(&[(0, 0, 1), (1, 3, 2), (2, 5, 3), (3, 7, 4), (4, 9, 5)]);
        for place in 0..TRANSPORT {
            let label = &mut body[label_range(place)];
            label[..NAME_TEXT].fill(0);
            label[0] = b'A' + (place % 26) as u8;
            label[NAME_TEXT] = 0;
            label[NAME_TEXT + 1] = (place % 3) as u8;
            label[NAME_TEXT + 2..].copy_from_slice(&((place / 3) as u16).to_le_bytes());
        }
        let mut expected: Vec<Vec<u8>> = (0..TRANSPORT)
            .map(|place| body[label_range(place)][NAME_TEXT..].to_vec())
            .collect();
        let mut offline = Offline::empty();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        // Swaps among boxes of different categories (0 and 1) and of the
        // same (0 and 3), into and out of offline places, green moves.
        let moves: [(bool, usize, usize); 10] = [
            (true, 0, 1),
            (true, 0, 3),
            (true, 2, OFFLINE),
            (true, OFFLINE, 40),
            (true, 3, OFFLINE + 1),
            (false, 4, OFFLINE + 2),
            (true, OFFLINE + 2, 3),
            (true, OFFLINE + 1, 99),
            (false, 1, 60),
            (true, 60, 0),
        ];
        for (blue, from, to) in moves {
            let result = if blue {
                places.swap(from, to, UNKNOWN)
            } else {
                places.move_all(from, to, NOW)
            };
            assert_eq!(result, Ok(()), "{from} -> {to}");
            // Of different categories a blue swap takes them along; any
            // other move leaves them with the places.
            if blue
                && from < TRANSPORT
                && to < TRANSPORT
                && expected[from][CATEGORY - NAME_TEXT] != expected[to][CATEGORY - NAME_TEXT]
            {
                expected.swap(from, to);
            }
            for (place, tail) in expected.iter().enumerate() {
                assert_eq!(
                    places.body[label_range(place)][NAME_TEXT..],
                    tail[..],
                    "place {place} after {from} -> {to}"
                );
            }
            // Every category: positions 0.. unique and without gaps.
            for category in 0..3u8 {
                let mut positions: Vec<u16> = (0..TRANSPORT)
                    .map(|place| &places.body[label_range(place)])
                    .filter(|label| label[CATEGORY] == category)
                    .map(|label| u16::from_le_bytes([label[NAME_TEXT + 2], label[NAME_TEXT + 3]]))
                    .collect();
                positions.sort_unstable();
                assert!(
                    positions.iter().enumerate().all(|(i, p)| *p as usize == i),
                    "category {category} after {from} -> {to}"
                );
            }
        }
        // The swap of 0 and 1 (categories 0 and 1) moved them: place 1 now
        // has category 0; the swap of 0 and 3 (both category 0) swapped
        // their positions.
        assert_eq!(expected[1][CATEGORY - NAME_TEXT], 0);
    }

    /// A name's text from ASCII, as a box stores it.
    fn named(text: &str) -> [u8; NAME_TEXT] {
        let units: Vec<u16> = text.encode_utf16().collect();
        boxfile::name_text(&units).unwrap()
    }

    #[test]
    fn empty_boxes_at_each_other_s_places_are_no_change() {
        // Box 1 "Fire" with a Pokémon in group 0; boxes 10 and 11 empty,
        // with their defaults, emptied at different times; every other box
        // in no group.
        let run = |empty_group: u8, moves: &[(usize, usize)], lowered: &[usize]| {
            let mut body = bank_body(&[(1, 0, 1)]);
            for place in 0..BOX_COUNT {
                body[label_range(place).start + CATEGORY] = 0xff;
            }
            let fire_position = 0u16;
            for (place, name, group, position) in [
                (1, "Fire", 0u8, fire_position),
                (10, "Bank 10", empty_group, 1 - u16::from(empty_group)),
                (11, "Bank 11", empty_group, 2 - u16::from(empty_group)),
            ] {
                body[label_range(place)][..NAME_TEXT].copy_from_slice(&named(name));
                set_group(&mut body, place, group, position);
            }
            for (place, time) in [(10, 0x10u8), (11, 0x11)] {
                for slot in 0..BOX_SLOTS {
                    if let Some((_, stamp)) = body_slot(place, slot).data {
                        body[stamp].fill(time);
                    }
                }
            }
            let mut offline = Offline::empty();
            let default = |place: usize| named(&std::format!("Bank {place}"));
            let mut places = Places {
                body: &mut body,
                offline: &mut offline,
            };
            let seen = |places: &Places| {
                [1usize, 10, 11].map(|place| {
                    (
                        places.label(place),
                        places.group(place),
                        places.count(place),
                    )
                })
            };
            let before = seen(&places);
            let mut changes = Changes::new();
            for &(from, to) in moves {
                changes
                    .swap(&mut places, from, to, [default(from), default(to)])
                    .unwrap();
            }
            for &place in lowered {
                let (group, position) = places.group(place).unwrap();
                let to = match position {
                    0 => places.group_size(group) - 1,
                    _ => position - 1,
                };
                changes.insert(&mut places, place, to).unwrap();
            }
            (seen(&places) == before, changes.any())
        };
        // "Fire" into 10, on into 11, back to 1, all of one group: the empty
        // boxes are at each other's places, each with its place's name and
        // position, and nothing shows a change.
        let round = [(1, 10), (10, 11), (11, 1)];
        assert_eq!(run(0, &round, &[]), (true, false));
        // The empty boxes in another group take their positions along, so
        // they end in each other's order, which L puts back.
        assert!(!run(1, &round, &[]).0);
        assert_eq!(run(1, &round, &[10]), (true, false));
        // "Fire" left in 10: a change.
        assert_eq!(run(0, &[(1, 10)], &[]), (false, true));
    }

    #[test]
    fn a_default_name_stays_with_its_place_bank_and_offline_alike() {
        let mut body = bank_body(&[(12, 0, 1), (37, 0, 2), (5, 0, 3)]);
        // Box 12 bears its default, box 37 and box 40 names of their own.
        for (place, name) in [(12, "Bank 12"), (37, "Fire"), (40, "Water")] {
            body[label_range(place)][..NAME_TEXT].copy_from_slice(&named(name));
        }
        let mut offline = Offline::empty();
        for (index, default) in offline.defaults.iter_mut().enumerate() {
            *default = named(&std::format!("Box {}", index + 1));
        }
        // Opening: every cell is named its default.
        offline.name_defaults();
        let cells = offline.defaults;
        // The default of a place, as the screen hands it over.
        let default = |place: usize| {
            if place >= OFFLINE {
                cells[place - OFFLINE]
            } else {
                named(&std::format!("Bank {place}"))
            }
        };
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        let swap = |places: &mut Places, from: usize, to: usize| {
            places.swap(from, to, [default(from), default(to)]).unwrap();
        };
        let name = |places: &Places, place: usize| places.label(place);
        for cell in OFFLINE..PLACES {
            assert_eq!(name(&places, cell), default(cell));
        }
        // Box 12, its default, onto cell 2: neither default travels.
        swap(&mut places, 12, OFFLINE + 1);
        assert_eq!(name(&places, OFFLINE + 1), named("Box 2"));
        assert_eq!(name(&places, 12), named("Bank 12"));
        // Box 37 "Fire" onto cell 1: the cell takes "Fire", box 37 its
        // default.
        swap(&mut places, 37, OFFLINE);
        assert_eq!(name(&places, OFFLINE), named("Fire"));
        assert_eq!(name(&places, 37), named("Bank 37"));
        // Cell 1 onto box 40 "Water", without Pokémon: the names change
        // places, and the cell keeps "Water" without Pokémon.
        swap(&mut places, OFFLINE, 40);
        assert_eq!(name(&places, 40), named("Fire"));
        assert_eq!(name(&places, OFFLINE), named("Water"));
        assert_eq!(places.count(OFFLINE), 0);
        // Cell 2 onto cell 3: each keeps its own default.
        swap(&mut places, OFFLINE + 1, OFFLINE + 2);
        assert_eq!(name(&places, OFFLINE + 1), named("Box 2"));
        assert_eq!(name(&places, OFFLINE + 2), named("Box 3"));
        // A default away from its place is a name: "Box 3" brought into
        // cell 2 (as from a backup) goes onto box 40 and stays "Box 3"
        // there, and cell 2 takes box 40's "Fire".
        places.set_label(OFFLINE + 1, &named("Box 3"));
        swap(&mut places, 40, OFFLINE + 1);
        assert_eq!(name(&places, 40), named("Box 3"));
        assert_eq!(name(&places, OFFLINE + 1), named("Fire"));
        // The same for a box of Bank: "Bank 12" on box 5 goes onto box 12,
        // where it is the default again, and box 5 takes its own.
        places.set_label(5, &named("Bank 12"));
        swap(&mut places, 5, 12);
        assert_eq!(name(&places, 12), named("Bank 12"));
        assert_eq!(name(&places, 5), named("Bank 5"));
        swap(&mut places, 12, 37);
        assert_eq!(name(&places, 37), named("Bank 37"));
        assert_eq!(name(&places, 12), named("Bank 12"));
        // A box without a name brings none either.
        places.set_label(OFFLINE + 2, &[0; NAME_TEXT]);
        swap(&mut places, OFFLINE + 2, 60);
        assert_eq!(name(&places, 60), named("Bank 60"));
        // Green: names stay, the cell's too.
        places.move_all(37, OFFLINE, NOW).unwrap();
        assert_eq!(name(&places, OFFLINE), named("Water"));
        assert_eq!(name(&places, 37), named("Bank 37"));
        // Every box still a whole box file.
        for file in &places.offline.files {
            BoxFile::parse(file).unwrap();
        }
    }

    #[test]
    fn a_name_is_its_text_up_to_its_end_whatever_lies_behind_it() {
        // The original's name setter copies the name's 0x22 bytes from a
        // work string (`002ba124`), so a stored name can hold what an
        // earlier, longer text left behind its end.
        let mut body = bank_body(&[]);
        let mut left = named("Bank 91");
        left[20..26].copy_from_slice(&[b'x', 0, b'y', 0, b'z', 0]);
        body[label_range(90)][..NAME_TEXT].copy_from_slice(&left);
        let mut offline = Offline::empty();
        offline.defaults[2] = named("Box 3");
        offline.name_defaults();
        offline.files[2][boxfile::NAME].copy_from_slice(&named("Fire"));
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        put(places.body, 5, 0, 1);
        places.move_all(5, OFFLINE + 2, NOW).unwrap();
        // Cell 3 "Fire" onto box 91, which bears its default: box 91 takes
        // "Fire", and cell 3 its own default, not box 91's.
        places
            .swap(OFFLINE + 2, 90, [named("Box 3"), named("Bank 91")])
            .unwrap();
        assert_eq!(places.label(90), named("Fire"));
        assert_eq!(places.label(OFFLINE + 2), named("Box 3"));
        // Box 91 is written with the cell's name bytes as they were.
        assert_eq!(places.body[label_range(90)][..NAME_TEXT], named("Fire"));
    }

    #[test]
    fn a_place_whose_default_is_not_known_keeps_its_name() {
        let mut body = bank_body(&[(5, 0, 1)]);
        let names = (body[label_range(5)].to_vec(), body[label_range(6)].to_vec());
        let mut offline = Offline::empty();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        // Onto a cell without a name: the box's name goes there, and box 5
        // keeps its own, as nothing else is known for it.
        places.swap(5, OFFLINE, UNKNOWN).unwrap();
        assert_eq!(places.label(OFFLINE)[..], names.0[..NAME_TEXT]);
        assert_eq!(places.body[label_range(5)], names.0[..]);
        // Two names of their own change places.
        places.swap(OFFLINE, 6, UNKNOWN).unwrap();
        assert_eq!(
            places.body[label_range(6)][..NAME_TEXT],
            names.0[..NAME_TEXT]
        );
        assert_eq!(places.label(OFFLINE)[..], names.1[..NAME_TEXT]);
    }

    /// A name `text` with `hidden` behind its end, as the original's name
    /// setter can leave one.
    fn hidden(text: &str, hidden: u8) -> [u8; NAME_TEXT] {
        let mut name = named(text);
        let end = (text.encode_utf16().count() + 1) * 2;
        name[end..].fill(hidden);
        name
    }

    /// The default of a place, as the screen hands it over.
    fn default_of(place: usize) -> [u8; NAME_TEXT] {
        if place >= OFFLINE {
            named(&std::format!("Box {}", place - OFFLINE + 1))
        } else {
            named(&std::format!("Bank {}", place + 1))
        }
    }

    /// Every byte a move can write in `place`, from the layout of the body
    /// and the box file, for comparison byte by byte.
    /// What `place` holds, as a change counts it: what its label shows and,
    /// slot by slot, all bytes of a slot with a Pokémon; of an empty slot
    /// only that it is empty.
    fn place_bytes(body: &[u8], offline: &Offline, place: usize) -> Vec<u8> {
        let (bytes, label): (&[u8], core::ops::Range<usize>) = if place >= OFFLINE {
            (&offline.files[place - OFFLINE], boxfile::NAME)
        } else {
            (body, label_range(place))
        };
        let mut held = Vec::new();
        for slot in 0..BOX_SLOTS {
            let at = if place >= OFFLINE {
                file_slot(slot)
            } else {
                body_slot(place, slot)
            };
            if !occupied_record(&bytes[at.record.clone()]) {
                held.push(0);
                continue;
            }
            held.push(1);
            held.extend_from_slice(&bytes[at.record]);
            held.push(bytes[at.tag]);
            if let Some((flag, stamp)) = at.data {
                held.push(bytes[flag]);
                held.extend_from_slice(&bytes[stamp]);
            }
        }
        if place != TRANSPORT {
            let mut stored = [0u8; LABEL_SIZE];
            stored[..label.len()].copy_from_slice(&bytes[label]);
            held.extend_from_slice(&boxfile::shown_label(&stored));
        }
        held
    }

    /// The cells of `files` with the rest of the room empty.
    fn with_cells(files: [[u8; FILE_SIZE]; OFFLINE_COUNT]) -> Offline {
        Offline {
            files,
            ..Offline::empty()
        }
    }

    /// The body and the cells as a write takes them: cleaned
    /// (`sections::clean`, `boxfile::clean`).
    fn cleaned(body: &[u8], offline: &Offline) -> (Vec<u8>, Vec<u8>) {
        let mut body = body.to_vec();
        sections::clean(&mut body).unwrap();
        let mut cells = Vec::new();
        for file in offline.files {
            let mut file = file;
            boxfile::clean(&mut file);
            cells.extend_from_slice(&file);
        }
        (body, cells)
    }

    /// Whether `changes` holds `place` as changed.
    fn holds_changed(changes: &Changes, place: usize) -> bool {
        changes.changed(place)
    }

    /// Puts the box of Bank at `place` into `group` at `position`.
    fn set_group(body: &mut [u8], place: usize, group: u8, position: u16) {
        let at = label_range(place).start;
        body[at + CATEGORY] = group;
        body[at + NAME_SIZE..at + NAME_SIZE + 2].copy_from_slice(&position.to_le_bytes());
    }

    /// A Bank with Pokémon in boxes, the transport box and offline box 2,
    /// every name with hidden bytes behind its end, some names the
    /// default.
    fn varied() -> (Vec<u8>, Offline) {
        let mut body = bank_body(&[
            (0, 0, 1),
            (0, 3, 2),
            (1, 1, 3),
            (2, 0, 4),
            (5, 29, 5),
            (40, 7, 6),
            (11, 2, 7),
        ]);
        for slot in 0..2 {
            put(&mut body, TRANSPORT, slot, 20 + slot as u32);
        }
        for place in 0..BOX_COUNT {
            let name = if place % 3 == 0 {
                hidden(&std::format!("Bank {}", place + 1), place as u8)
            } else {
                hidden("Fire", 0x80 | place as u8)
            };
            body[label_range(place)][..NAME_TEXT].copy_from_slice(&name);
            // Three groups, every position once and without a gap.
            set_group(&mut body, place, (place % 3) as u8, (place / 3) as u16);
        }
        let mut offline = Offline::empty();
        offline.files[0][boxfile::NAME].copy_from_slice(&hidden("Box 1", 0x31));
        offline.files[1][boxfile::NAME].copy_from_slice(&hidden("Water", 0x32));
        offline.files[2][boxfile::NAME].copy_from_slice(&hidden("Box 3", 0x33));
        // A Pokémon in offline box 2.
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        places.put_slot(OFFLINE + 1, 4, &pokemon(40), 7, 3, &[9; STAMP_SIZE]);
        places.seal([OFFLINE, OFFLINE + 1]);
        places.seal([OFFLINE + 2, OFFLINE + 2]);
        (body, offline)
    }

    #[test]
    fn a_move_writes_only_its_two_places_and_the_fingerprints_tell_every_change() {
        let (mut body, mut offline) = varied();
        let start = (body.clone(), offline.files);
        let mut changes = Changes::new();
        // Blue (b) and green (g) from a place to another, and L (l) taking a
        // box to a position of its group, back and forth and round, between
        // the Bank, the transport box and the offline boxes, refusals
        // included.
        let moves = [
            ('b', 0, 1),
            ('b', 1, 0),
            ('l', 0, 1),
            ('b', 2, OFFLINE),
            ('b', OFFLINE, OFFLINE + 1),
            ('g', TRANSPORT, 3),
            ('l', 0, 33),
            ('l', 1, 0),
            ('l', 4, 0),
            ('l', 0, 40),
            ('g', 5, OFFLINE + 2),
            ('b', 40, 7),
            ('b', 7, 40),
            ('g', 0, 1),
            ('b', 3, TRANSPORT),
            ('g', 4, 6),
            ('b', OFFLINE + 1, OFFLINE),
            ('b', OFFLINE, 2),
            ('b', 11, 12),
            ('b', 12, 13),
            ('b', 13, 11),
            ('b', 11, 12),
            ('l', 12, 0),
            ('g', OFFLINE + 2, 5),
        ];
        for (kind, from, to) in moves {
            let before = (body.clone(), offline.files);
            let mut places = Places {
                body: &mut body,
                offline: &mut offline,
            };
            let result = match kind {
                'b' => changes.swap(&mut places, from, to, [default_of(from), default_of(to)]),
                'g' => changes.move_all(&mut places, from, to, NOW),
                _ => changes.insert(&mut places, from, to as u16),
            };
            // Nothing outside the move's two places changed; for L nothing
            // outside the box's group.
            let category = |body: &[u8], place: usize| body[label_range(place).start + CATEGORY];
            let touched = |place: usize| match kind {
                'l' => place < TRANSPORT && category(&before.0, place) == category(&before.0, from),
                _ => [from, to].contains(&place),
            };
            for place in (0..PLACES).filter(|&place| result.is_err() || !touched(place)) {
                assert_eq!(
                    place_bytes(&body, &offline, place),
                    place_bytes(
                        &before.0,
                        &Offline {
                            files: before.1,
                            ..Offline::empty()
                        },
                        place
                    ),
                    "place {place} after {from} -> {to}"
                );
            }
            let mut masked = (body.clone(), before.0.clone());
            for place in 0..TRANSPORT + 1 {
                for slot in 0..BOX_SLOTS {
                    let at = body_slot(place, slot);
                    for copy in [&mut masked.0, &mut masked.1] {
                        copy[at.record.clone()].fill(0);
                        copy[at.tag] = 0;
                        if let Some((flag, stamp)) = at.data.clone() {
                            copy[flag] = 0;
                            copy[stamp].fill(0);
                        }
                        if place < TRANSPORT {
                            copy[label_range(place)].fill(0);
                        }
                    }
                }
            }
            assert!(
                masked.0 == masked.1,
                "a byte of no place changed: {from} -> {to}"
            );
            // Every place: held as changed exactly when its bytes differ
            // from the start.
            let opened = Offline {
                files: start.1,
                ..Offline::empty()
            };
            for place in 0..PLACES {
                let differs =
                    place_bytes(&body, &offline, place) != place_bytes(&start.0, &opened, place);
                assert_eq!(
                    holds_changed(&changes, place),
                    differs,
                    "place {place} after {from} -> {to}"
                );
            }
            let differs = |places: core::ops::Range<usize>| {
                places.into_iter().any(|place| {
                    place_bytes(&body, &offline, place) != place_bytes(&start.0, &opened, place)
                })
            };
            assert_eq!(changes.any(), differs(0..PLACES), "after {from} -> {to}");
        }
    }

    #[test]
    fn after_any_sequence_of_moves_a_place_counts_as_changed_exactly_when_it_differs() {
        // Blue, green and L at random places, the blue and L moves often
        // undone in reverse, so that places come back exactly; green only in
        // the second half, as its Pokémon keep the time of their move. After
        // every move, `Changes` agrees with a comparison of every place with
        // the start, made anew: what it holds and what its label shows.
        let (mut body, mut offline) = varied();
        let start = with_cells(offline.files);
        let opened: Vec<Vec<u8>> = (0..PLACES)
            .map(|place| place_bytes(&body, &start, place))
            .collect();
        let mut changes = Changes::new();
        let mut seed = 0x2545_f491u32;
        let mut next = |below: usize| {
            seed = seed.wrapping_mul(0x41c6_4e6d).wrapping_add(0x6073);
            (seed >> 16) as usize % below
        };
        // The moves that went through and can be undone, latest last.
        let mut undo: Vec<(char, usize, usize)> = Vec::new();
        let mut undoing = false;
        let mut came_back = 0;
        for step in 0..2000 {
            if undo.is_empty() {
                undoing = false;
            } else if !undoing && next(8) == 0 {
                undoing = true;
            }
            let mut places = Places {
                body: &mut body,
                offline: &mut offline,
            };
            let (kind, from, to) = match undoing {
                true => undo.pop().unwrap(),
                false => {
                    let kinds = if step < 1000 { 2 } else { 3 };
                    let kind = ['b', 'l', 'g'][next(kinds)];
                    let to = if kind == 'l' { next(40) } else { next(PLACES) };
                    (kind, next(PLACES), to)
                }
            };
            let undone = places.group(from).map(|(_, position)| position);
            let result = match kind {
                'b' => changes.swap(&mut places, from, to, [default_of(from), default_of(to)]),
                'g' => changes.move_all(&mut places, from, to, NOW),
                _ => changes.insert(&mut places, from, to as u16),
            };
            match (kind, result, undoing) {
                ('b', Ok(()), false) => undo.push(('b', to, from)),
                ('l', Ok(()), false) => undo.push(('l', from, usize::from(undone.unwrap()))),
                (_, result, true) => assert_eq!(result, Ok(()), "undoing at step {step}"),
                _ => {}
            }
            let mut differs = false;
            for (place, at_start) in opened.iter().enumerate() {
                let now = place_bytes(&body, &offline, place) != *at_start;
                assert_eq!(
                    holds_changed(&changes, place),
                    now,
                    "place {place} at step {step}: {kind} {from} -> {to}"
                );
                differs |= now;
            }
            assert_eq!(changes.any(), differs, "step {step}");
            came_back += usize::from(!differs && step > 0);
            // Every offline box sealed: a move seals those it changed, and
            // the others keep the checksum they had.
            for (index, file) in offline.files.iter().enumerate() {
                assert!(
                    BoxFile::parse(file).is_ok(),
                    "offline box {index} at step {step}"
                );
            }
        }
        // The moves came back to where they began, so "no change" was
        // tried, not only "changed".
        assert!(came_back > 0);
    }

    #[test]
    fn l_inserts_a_box_one_place_before_and_round_again_is_no_change() {
        let (mut body, mut offline) = varied();
        let start = body.clone();
        let mut changes = Changes::new();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        // Group 0 holds boxes 0, 3, 6, ... at positions 0, 1, 2, ...
        assert_eq!(places.group(3), Some((0, 1)));
        assert_eq!(places.group(4), Some((1, 1)));
        assert_eq!(places.group(TRANSPORT), None);
        assert_eq!(places.group(OFFLINE), None);
        assert_eq!(places.group_size(0), 34);
        assert_eq!(places.group_size(2), 33);
        assert_eq!(places.in_group(0, 2), Some(6));
        let order = |places: &Places| {
            let mut order = [0usize; 34];
            for (position, place) in order.iter_mut().enumerate() {
                *place = places.in_group(0, position as u16).unwrap();
            }
            order
        };
        // Box 3 one place before: it and box 0 change positions; nothing else
        // of theirs changes.
        let records = (
            places.body[body_slot(0, 0).record].to_vec(),
            places.label(3),
        );
        changes.insert(&mut places, 3, 0).unwrap();
        assert_eq!(places.group(0), Some((0, 1)));
        assert_eq!(places.group(3), Some((0, 0)));
        assert_eq!(places.body[body_slot(0, 0).record], records.0[..]);
        assert_eq!(places.label(3), records.1);
        assert!(changes.any());
        // Box 3, now first, to the last: every other box of the group one
        // place up, the rest of the group's order kept.
        changes.insert(&mut places, 3, 33).unwrap();
        let expected: Vec<usize> = [0]
            .into_iter()
            .chain((2..34).map(|box_of_group| box_of_group * 3))
            .chain([3])
            .collect();
        assert_eq!(order(&places)[..], expected[..]);
        // Round the group to where it began: box 3 back down to 1, one place
        // at a time, as L takes it.
        for position in (1..33).rev() {
            changes.insert(&mut places, 3, position).unwrap();
        }
        assert!(places.body[..] == start[..]);
        assert!(!changes.any());
        // Its own position, past the group, or a place that is no box of
        // Bank: refused, nothing changed.
        for (place, position) in [(3, 1), (3, 34), (TRANSPORT, 0), (OFFLINE, 0)] {
            assert_eq!(
                changes.insert(&mut places, place, position),
                Err(Refusal::Place)
            );
        }
        assert!(places.body[..] == start[..]);
    }

    #[test]
    fn a_group_is_named_from_the_bank_and_a_category_beyond_the_ten_is_none() {
        let (mut body, mut offline) = varied();
        let mut name = named("Fire Group");
        name[24..].fill(0x55);
        let at = GROUP_NAMES_OFFSET + 3 * GROUP_NAME_SIZE;
        body[at..at + NAME_TEXT].copy_from_slice(&name);
        set_group(&mut body, 50, 10, 0);
        let places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        // The text up to its end.
        assert_eq!(places.group_name(3), Some(named("Fire Group")));
        assert_eq!(places.group_name(10), None);
        assert_eq!(places.group(50), None);
        assert_eq!(places.label(OFFLINE + 1), named("Water"));
    }

    #[test]
    fn only_a_change_that_stays_counts() {
        let run = |moves: &[(bool, usize, usize)]| {
            let (mut body, mut offline) = varied();
            let start = (body.clone(), offline.files);
            let mut changes = Changes::new();
            let mut places = Places {
                body: &mut body,
                offline: &mut offline,
            };
            for &(blue, from, to) in moves {
                let result = if blue {
                    changes.swap(&mut places, from, to, [default_of(from), default_of(to)])
                } else {
                    changes.move_all(&mut places, from, to, NOW)
                };
                assert_eq!(result, Ok(()), "{from} -> {to}");
            }
            (
                changes.any(),
                cleaned(&body, &offline) == cleaned(&start.0, &with_cells(start.1)),
            )
        };
        // A swap and its swap back; round three boxes and back: every byte
        // as a write takes it.
        assert_eq!(run(&[(true, 0, 1), (true, 1, 0)]), (false, true));
        assert_eq!(
            run(&[(true, 0, 1), (true, 1, 2), (true, 2, 1), (true, 1, 0)]),
            (false, true)
        );
        // Between a box of the Bank, which bears its default with hidden
        // bytes, and offline boxes with their own.
        assert_eq!(
            run(&[
                (true, 0, OFFLINE),
                (true, OFFLINE, OFFLINE + 1),
                (true, OFFLINE + 1, OFFLINE),
                (true, OFFLINE, 0)
            ]),
            (false, true)
        );
        // Left swapped: a change, in the Bank or between offline boxes.
        assert_eq!(run(&[(true, 0, 1)]), (true, false));
        assert_eq!(run(&[(true, OFFLINE, OFFLINE + 1)]), (true, false));
        // Green, and back: a change, as each Pokémon holds the time of its
        // last move.
        assert_eq!(run(&[(false, 1, 4), (false, 4, 1)]), (true, false));
        assert_eq!(
            run(&[(false, 0, OFFLINE + 2), (false, OFFLINE + 2, 0)]),
            (true, false)
        );
        assert_eq!(
            run(&[
                (false, OFFLINE + 1, OFFLINE + 2),
                (false, OFFLINE + 2, OFFLINE + 1)
            ]),
            (true, false)
        );
        // Green put down on its own box: a change, its Pokémon hold the time
        // of the put-down. The transport box on itself keeps no times: none.
        assert_eq!(run(&[(false, 1, 1)]), (true, false));
        assert_eq!(run(&[(false, OFFLINE + 1, OFFLINE + 1)]), (true, false));
        assert_eq!(run(&[(false, TRANSPORT, TRANSPORT)]), (false, true));
        // Green, and back, with a clock set back to the second the Pokémon
        // already held, with flag 0: every byte as before, still a change.
        let mut body = bank_body(&[]);
        let at = body_slot(1, 0);
        body[at.record.clone()].copy_from_slice(&pokemon(3));
        let (flag, stamp) = at.data.unwrap();
        body[flag] = 0;
        body[stamp].copy_from_slice(&NOW);
        let start = body.clone();
        let mut offline = Offline::empty();
        let mut changes = Changes::new();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        changes.move_all(&mut places, 1, 4, NOW).unwrap();
        changes.move_all(&mut places, 4, 1, NOW).unwrap();
        assert!(
            places.body[body_slot(1, 0).record.start..label_range(1).start]
                == start[body_slot(1, 0).record.start..label_range(1).start]
        );
        assert!(changes.any());
    }

    #[test]
    fn a_category_s_order_and_a_name_count_as_a_change() {
        // Boxes A at 1 and B at 2 of one category, an empty box E at 3 of
        // another: A onto E, B onto E, A onto E, B onto A. Every box is at
        // its place again, but A and B are in each other's position.
        let (mut body, mut offline) = varied();
        for (place, category, position) in [(1, 0u8, 0u16), (2, 0, 1), (3, 1, 0)] {
            let at = label_range(place).start;
            body[at + CATEGORY] = category;
            body[at + CATEGORY + 1..at + CATEGORY + 3].copy_from_slice(&position.to_le_bytes());
        }
        put(&mut body, 2, 4, 30);
        let start = body.clone();
        let mut changes = Changes::new();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        for (from, to) in [(1, 3), (2, 1), (3, 2), (1, 2)] {
            changes
                .swap(&mut places, from, to, [default_of(from), default_of(to)])
                .unwrap();
        }
        for place in 1..=3 {
            assert_eq!(
                places.body[body_slot(place, 0).record.start..label_range(place).start],
                start[body_slot(place, 0).record.start..label_range(place).start],
                "box {place} holds its own Pokémon again"
            );
        }
        assert!(changes.any());
        // A box named with another place's default: box 13 "Bank 41" and box
        // 41 bearing its default "Bank 41". Swapped and back, box 13 comes
        // back "Bank 13".
        let (mut body, mut offline) = varied();
        body[label_range(12)][..NAME_TEXT].copy_from_slice(&named("Bank 41"));
        body[label_range(40)][..NAME_TEXT].copy_from_slice(&named("Bank 41"));
        let mut changes = Changes::new();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        changes
            .swap(&mut places, 12, 40, [default_of(12), default_of(40)])
            .unwrap();
        changes
            .swap(&mut places, 40, 12, [default_of(40), default_of(12)])
            .unwrap();
        assert_eq!(places.label(12), named("Bank 13"));
        assert!(changes.any());
    }

    #[test]
    fn green_moves_the_pokemon_only_into_a_box_without_pokemon() {
        let mut body = bank_body(&[(1, 0, 1), (1, 1, 2), (2, 1, 3), (6, 5, 4)]);
        let mut offline = Offline::empty();
        let names = (body[label_range(1)].to_vec(), body[label_range(4)].to_vec());
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        assert_eq!(places.move_all(1, 2, NOW), Err(Refusal::Occupied));
        // Refused also where every Pokémon would fit.
        assert_eq!(places.move_all(1, 6, NOW), Err(Refusal::Occupied));
        assert_eq!((places.count(1), places.count(6)), (2, 1));
        places.move_all(1, 4, NOW).unwrap();
        assert_eq!((places.count(1), places.count(4)), (0, 2));
        assert_eq!(places.body[label_range(4)], names.1[..]);
        assert_eq!(places.body[label_range(1)], names.0[..]);
        // Each Pokémon got flag 0 and the time of the move (`put` gave
        // them flag 3 and their number as their time); the target's other
        // slots are as they were.
        for slot in 0..BOX_SLOTS {
            let (flag, stamp) = body_slot(4, slot).data.unwrap();
            let moved = slot < 2;
            assert_eq!(places.body[flag], 0, "slot {slot}");
            assert_eq!(
                places.body[stamp],
                if moved { NOW } else { [0; 8] },
                "slot {slot}"
            );
        }
        // The slots they left are empty and keep their flag and time; the
        // others of the box left are not touched.
        for slot in 0..BOX_SLOTS {
            let (flag, stamp) = body_slot(1, slot).data.unwrap();
            let left = slot < 2;
            assert_eq!(places.body[flag], if left { 3 } else { 0 }, "slot {slot}");
            assert_eq!(
                places.body[stamp],
                if left { [slot as u8 + 1; 8] } else { [0; 8] },
                "slot {slot}"
            );
            assert!(!places.occupied_slot(1, slot));
        }
        assert_eq!(
            places.move_all(4, TRANSPORT, NOW),
            Err(Refusal::IntoTransport)
        );
        assert_eq!(places.move_all(1, 5, NOW), Err(Refusal::Empty));
        assert_eq!(places.move_all(1, 1, NOW), Err(Refusal::Empty));
        // Put down on their own box: they stay in their slots with flag 0
        // and the time of this put-down; its other slots are not touched.
        let later = [0xea, 0x07, 11, 9, 12, 31, 0, 0];
        let before = places.body.to_vec();
        places.move_all(4, 4, later).unwrap();
        assert_eq!(places.count(4), 2);
        for slot in 0..BOX_SLOTS {
            let at = body_slot(4, slot);
            let (flag, stamp) = at.data.clone().unwrap();
            assert_eq!(
                places.body[at.record.clone()],
                before[at.record],
                "slot {slot}"
            );
            assert_eq!(places.body[flag], 0, "slot {slot}");
            assert_eq!(
                places.body[stamp],
                if slot < 2 { later } else { [0; 8] },
                "slot {slot}"
            );
        }
    }

    #[test]
    fn only_the_green_cursor_takes_the_transport_box_s_pokemon_and_only_into_an_empty_box() {
        let mut body = bank_body(&[(9, 0, 5)]);
        // Three Pokémon in the transport box.
        for slot in 0..3 {
            put(&mut body, TRANSPORT, slot, 10 + slot as u32);
        }
        let mut offline = Offline::empty();
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        assert_eq!(places.swap(TRANSPORT, 10, UNKNOWN), Err(Refusal::Place));
        assert_eq!(places.move_all(TRANSPORT, 9, NOW), Err(Refusal::Occupied));
        let untouched = places.body[body_slot(TRANSPORT, 5).record].to_vec();
        // On itself it keeps what it holds, as it keeps no times.
        let before = places.body.to_vec();
        places.move_all(TRANSPORT, TRANSPORT, NOW).unwrap();
        assert!(places.body[..] == before[..]);
        places.move_all(TRANSPORT, 10, NOW).unwrap();
        assert_eq!((places.count(TRANSPORT), places.count(10)), (0, 3));
        // Only the slots that held a Pokémon are emptied.
        assert_eq!(places.body[body_slot(TRANSPORT, 5).record], untouched[..]);
        assert_eq!(places.body[body_slot(10, 0).data.unwrap().1], NOW[..]);
        assert_eq!(places.body[body_slot(10, 0).data.unwrap().0], 0);
    }

    fn setup() -> (Shared, BankFiles<Shared>, Vec<u8>) {
        let body = bank_body(&[(0, 0, 1), (0, 1, 2), (5, 3, 3)]);
        let disk = fresh(&body);
        let mut files = opened(&disk);
        let (body, _) = load(&mut files);
        (disk, files, body)
    }

    /// The date a session's write is made at.
    const SESSION: Date = [2026, 11, 9, 14, 3, 7];

    /// The session's folder of a card that had none of `SESSION`.
    fn folder() -> Path {
        sdfiles::dated_folder(SESSION)
    }

    /// The date an opening is made at, which names the folder of the SD
    /// card that keeps what a settled write leaves.
    const DATE: Date = [2026, 11, 16, 14, 3, 7];
    const KEPT: &str = "/20261116-140307-backup";
    /// The folder of a second folder kept at the same opening: the next
    /// second's.
    const KEPT_NEXT: &str = "/20261116-140308-backup";

    /// Whether Bank's extdata holds the folder of an interrupted write.
    fn interrupted(card: &Shared) -> bool {
        !card.0.borrow().ext_dirs.is_empty()
    }

    /// Puts the file `name` of a folder into Bank's extdata, with `boxes` at
    /// their positions and zeros before them.
    fn in_extdata(disk: &Shared, folder: &str, name: &str, boxes: &[(usize, [u8; FILE_SIZE])]) {
        let length = boxes.iter().map(|(at, _)| at + 1).max().unwrap() * FILE_SIZE;
        let mut bytes = vec![0; length];
        for (at, one) in boxes {
            bytes[at * FILE_SIZE..(at + 1) * FILE_SIZE].copy_from_slice(one);
        }
        let mut card = disk.0.borrow_mut();
        let path = std::format!("/{folder}/{name}").into_bytes();
        card.ext_dirs.insert(std::format!("/{folder}").into_bytes());
        card.ext_written.insert(path.clone(), length);
        card.ext.insert(path, bytes);
    }

    /// The path of `name` in the root folder.
    fn at(name: &str) -> Vec<u8> {
        [ROOT, name.as_bytes()].concat()
    }

    /// The state after a session that moved box 1 (0) offline, box 6 (5) to
    /// box 21 (20), and checked box 21 and offline 2.
    fn session(body: &mut Vec<u8>, offline: &mut Offline) -> [bool; PLACES] {
        let mut places = Places { body, offline };
        places.swap(0, OFFLINE + 1, UNKNOWN).unwrap();
        places.move_all(5, 20, NOW).unwrap();
        let mut checked = [false; PLACES];
        checked[20] = true;
        checked[OFFLINE + 1] = true;
        checked
    }

    fn run_commit(
        files: &mut BankFiles<Shared>,
        sd: &mut Shared,
        body: &mut [u8],
        offline: &mut Offline,
        checked: &[bool; PLACES],
    ) -> Result<(), Failed<Shared, Shared>> {
        let mut ext = ExtCard(sd.clone());
        commit(
            files,
            sd,
            &mut ext,
            body,
            offline,
            &Commit {
                checked,
                date: SESSION,
            },
            &mut [0; FILE_SIZE],
        )
    }

    #[test]
    fn a_commit_writes_the_bank_the_session_and_the_cells() {
        let (disk, mut files, mut body) = setup();
        let mut offline = Offline::empty();
        for (index, default) in offline.defaults.iter_mut().enumerate() {
            default[..4].copy_from_slice(&[b'B', 0, b'1' + index as u8, 0]);
        }
        offline.name_defaults();
        let checked = session(&mut body, &mut offline);
        // The Bank is written clean: the slots the green move left lose
        // their flag and time.
        let mut expected = body.clone();
        sections::clean(&mut expected).unwrap();
        assert!(expected != body);
        let mut sd = disk.clone();
        run_commit(&mut files, &mut sd, &mut body, &mut offline, &checked).unwrap();
        let loaded = load(&mut opened(&disk.reboot())).0;
        assert!(loaded == expected);
        let sd = disk.0.borrow();
        let names: Vec<_> = sd.sd.keys().cloned().collect();
        // The first save writes every cell, with Pokémon or without; no
        // file of the write is left, nor the folder it lay in.
        assert_eq!(
            names,
            [
                at("/20261109-140307/bank21.box"),
                at("/20261109-140307/mover2.box"),
                at("/mover1.box"),
                at("/mover2.box"),
                at("/mover3.box"),
            ]
        );
        let folders: Vec<_> = sd.sd_dirs.iter().cloned().collect();
        assert_eq!(
            folders,
            [
                sdfiles::PARENT.to_vec(),
                ROOT.to_vec(),
                at("/20261109-140307")
            ]
        );
        let read = |name: &str| sd.sd[&at(name)].clone();
        // The checked box of Bank: its pieces and of its label the name's
        // text, the same file the backup makes from the Bank file.
        let copy = read("/20261109-140307/bank21.box");
        let payload: Vec<u8> = BANK_RANGES
            .iter()
            .flat_map(|range| expected[range.clone()].iter().copied())
            .collect();
        let mut from_bank = [0u8; FILE_SIZE];
        boxfile::encode_bank_box(&payload, 20, &mut from_bank).unwrap();
        assert!(copy[..] == from_bank[..]);
        assert_eq!(
            BoxFile::parse(&copy).unwrap().name(),
            &expected[label_range(20)][..NAME_TEXT]
        );
        // The checked offline box was copied to the session with its
        // Pokémon, and its cell holds an empty box with the cell's default
        // name, as the screen's memory does.
        let moved = read("/20261109-140307/mover2.box");
        assert_eq!(BoxFile::parse(&moved).unwrap().occupied(), 2);
        let left = read("/mover2.box");
        assert_eq!(left[..], offline.files[1][..]);
        let left = BoxFile::parse(&left).unwrap();
        assert_eq!(left.occupied(), 0);
        assert_eq!(left.name(), offline.defaults[1]);
        for path in ["/mover1.box", "/mover3.box"] {
            assert_eq!(BoxFile::parse(&read(path)).unwrap().occupied(), 0);
        }
    }

    #[test]
    fn a_save_writes_only_the_cells_that_changed() {
        let (disk, mut files, mut body) = setup();
        let mut offline = Offline::empty();
        // A cell without a file is an empty box without a name: unnamed it
        // is no change; with the screen's default names all three are.
        let checked = [false; PLACES];
        run_commit(
            &mut files,
            &mut disk.clone(),
            &mut body,
            &mut offline,
            &checked,
        )
        .unwrap();
        assert!(disk.0.borrow().sd.is_empty());
        files = opened(&disk);
        body = load(&mut files).0;
        for (index, default) in offline.defaults.iter_mut().enumerate() {
            default[..4].copy_from_slice(&[b'B', 0, b'1' + index as u8, 0]);
        }
        offline.name_defaults();
        let checked = [false; PLACES];
        // A save, then the Bank read again as the next session reads it.
        let run = |files: &mut BankFiles<Shared>, body: &mut Vec<u8>, offline: &mut Offline| {
            run_commit(files, &mut disk.clone(), body, offline, &checked).unwrap();
            *files = opened(&disk);
            *body = load(files).0;
        };
        let journal = |disk: &Shared| opened(&disk.reboot()).phase().unwrap();
        run(&mut files, &mut body, &mut offline);
        assert_eq!(disk.0.borrow().sd.len(), 3);
        let first = journal(&disk);
        // As on the card: no cell is written, the Bank is.
        let cards = disk.0.borrow().sd.clone();
        run(&mut files, &mut body, &mut offline);
        assert!(disk.0.borrow().sd == cards);
        assert!(journal(&disk) != first);
        let first = journal(&disk);
        // A new name: only that cell, and the Bank.
        offline.files[2][boxfile::NAME][0] = b'Z';
        boxfile::seal(&mut offline.files[2]);
        let cards = disk.0.borrow().sd.clone();
        let folders = disk.0.borrow().sd_dirs.clone();
        run(&mut files, &mut body, &mut offline);
        let card = disk.0.borrow();
        let file = &card.sd[&at("/mover3.box")];
        assert_eq!(file[boxfile::NAME][0], b'Z');
        let changed: Vec<_> = card
            .sd
            .iter()
            .filter(|(path, bytes)| cards.get(*path) != Some(bytes))
            .map(|(path, _)| path.clone())
            .collect();
        assert_eq!(changed, [at("/mover3.box")]);
        assert_eq!(card.sd.len(), 3);
        assert!(card.sd_dirs == folders);
        drop(card);
        assert!(journal(&disk) != first);
    }

    #[test]
    fn every_save_writes_the_bank_as_save_and_quit_does() {
        // Also a save that changes nothing, or only what a write cleans
        // away: the Bank is written, cleaned, in a record of its own.
        let (disk, mut files, mut body) = setup();
        let mut offline = Offline::empty();
        let journal = |disk: &Shared| opened(&disk.reboot()).phase().unwrap();
        let save = |files: &mut BankFiles<Shared>, body: &mut Vec<u8>, offline: &mut Offline| {
            run_commit(files, &mut disk.clone(), body, offline, &[false; PLACES]).unwrap();
            *files = opened(&disk);
            *body = load(files).0;
        };
        let mut before = journal(&disk);
        let as_it_was = body.clone();
        save(&mut files, &mut body, &mut offline);
        assert!(journal(&disk) != before);
        assert!(body == as_it_was);
        // The label's unused byte and the time of an empty slot: cleaned away.
        before = journal(&disk);
        body[label_range(3).start + NAME_TEXT] = 0x77;
        body[body_slot(4, 9).data.unwrap().1].fill(0x66);
        save(&mut files, &mut body, &mut offline);
        assert!(journal(&disk) != before);
        assert!(body == as_it_was);
        // Nothing on the SD card: no cell changed and none was checked.
        assert!(disk.0.borrow().sd.is_empty());
        assert!(disk.0.borrow().sd_dirs.is_empty());
    }

    #[test]
    fn the_transport_box_s_session_copy_carries_its_name() {
        let (disk, mut files, mut body) = setup();
        put(&mut body, TRANSPORT, 0, 7);
        let mut offline = Offline::empty();
        offline.transport[..4].copy_from_slice(&[b'T', 0, b'B', 0]);
        let mut checked = [false; PLACES];
        checked[TRANSPORT] = true;
        let mut sd = disk.clone();
        run_commit(&mut files, &mut sd, &mut body, &mut offline, &checked).unwrap();
        let sd = disk.0.borrow();
        let file = &sd.sd[&at("/20261109-140307/mover0.box")];
        let file = BoxFile::parse(file).unwrap();
        assert_eq!(file.occupied(), 1);
        assert_eq!(file.name(), offline.transport);
    }

    /// What an opening finds, after it settled what a save left: every
    /// Pokémon in the Bank and the cells, with its place, and every Pokémon
    /// anywhere on the card, the copies in the session's folders included.
    /// A Pokémon in two places of the Bank and the cells is kept at the
    /// last, and makes the places of no save.
    fn on_card(disk: &Shared) -> (BTreeMap<Identity, usize>, Vec<Identity>) {
        let mut files = BankFiles::new(disk.reboot());
        let mut sd = disk.clone();
        files.open().unwrap();
        interrupted::settle(
            &mut files,
            &mut sd,
            &mut ExtCard(disk.clone()),
            &mut vec![0; offline_core::native_blob::BLOB_SIZE],
            DATE,
        )
        .unwrap();
        let (body, _) = load(&mut files);
        let mut offline = Offline::empty();
        load_offline::<(), _>(&mut sd, &mut offline).unwrap();
        let mut held = BTreeMap::new();
        let mut copy = body.clone();
        let places = Places {
            body: &mut copy,
            offline: &mut offline,
        };
        for place in 0..PLACES {
            for slot in 0..BOX_SLOTS {
                let (record, ..) = places.get_slot(place, slot);
                if let Some(identity) = sections::stored_identity(&record) {
                    held.insert(identity, place);
                }
            }
        }
        let mut all: Vec<Identity> = held.keys().copied().collect();
        for bytes in disk.0.borrow().sd.values() {
            let Ok(file) = BoxFile::parse(bytes) else {
                continue;
            };
            for record in file.records().chunks_exact(RECORD_SIZE) {
                all.extend(sections::stored_identity(record.try_into().unwrap()));
            }
        }
        all.sort();
        all.dedup();
        (held, all)
    }

    /// A cell's box with Pokémon `id` in slot 4, as a user's box file.
    fn cell_with(id: u32) -> [u8; FILE_SIZE] {
        let mut other = bank_body(&[(0, 4, id)]);
        let mut start = Offline::empty();
        Places {
            body: &mut other,
            offline: &mut start,
        }
        .swap(0, OFFLINE, UNKNOWN)
        .unwrap();
        start.files[0]
    }

    /// Puts `file` on the card as cell `index`.
    fn place_cell(disk: &Shared, index: usize, file: &[u8; FILE_SIZE]) {
        let mut card = disk.0.borrow_mut();
        card.sd_dirs.insert(sdfiles::PARENT.to_vec());
        card.sd_dirs.insert(ROOT.to_vec());
        card.sd
            .insert(cell_path(index).text().to_vec(), file.to_vec());
    }

    /// A save the screen asks for: its body, its cells, its checks, and
    /// whether the Bank is written.
    struct Save {
        name: &'static str,
        body: Vec<u8>,
        offline: Offline,
        checked: [bool; PLACES],
    }

    /// The saves the cut tests run, each on a card of its own.
    fn saves() -> Vec<(Shared, Save)> {
        let mut out = Vec::new();
        // Box 1 out to a cell, box 6 to box 21, box 21 and the cell checked.
        let (disk, files, mut body) = setup();
        drop(files);
        let mut offline = Offline::empty();
        let checked = session(&mut body, &mut offline);
        out.push((
            disk,
            Save {
                name: "bank and checked cell",
                body,
                offline,
                checked,
            },
        ));
        // Box 1 and cell 1 change places, both with Pokémon.
        let (disk, files, mut body) = setup();
        drop(files);
        place_cell(&disk, 0, &cell_with(9));
        let mut offline = Offline::empty();
        offline.files[0] = cell_with(9);
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .swap(0, OFFLINE, UNKNOWN)
        .unwrap();
        out.push((
            disk,
            Save {
                name: "bank and cell swap",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        // The cells alone: cell 1 to cell 3, cell 2 checked.
        let (disk, files, body) = setup();
        drop(files);
        place_cell(&disk, 0, &cell_with(9));
        place_cell(&disk, 1, &cell_with(11));
        let mut offline = Offline::empty();
        offline.files[0] = cell_with(9);
        offline.files[1] = cell_with(11);
        Places {
            body: &mut body.clone(),
            offline: &mut offline,
        }
        .swap(OFFLINE, OFFLINE + 2, UNKNOWN)
        .unwrap();
        let mut checked = [false; PLACES];
        checked[OFFLINE + 1] = true;
        out.push((
            disk,
            Save {
                name: "cells alone",
                body,
                offline,
                checked,
            },
        ));
        // The transport box's Pokémon into cell 2: the transport box
        // changes, the Bank file does not.
        let mut start = bank_body(&[(0, 0, 1), (5, 3, 3)]);
        put(&mut start, TRANSPORT, 0, 21);
        put(&mut start, TRANSPORT, 7, 22);
        let disk = fresh(&start);
        let mut body = load(&mut opened(&disk)).0;
        let mut offline = Offline::empty();
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .move_all(TRANSPORT, OFFLINE + 1, NOW)
        .unwrap();
        out.push((
            disk,
            Save {
                name: "transport box into a cell",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        // Three cells round: 1 to 2, 2 to 3, 3 to 1.
        let (disk, files, body) = setup();
        drop(files);
        let start = [cell_with(9), cell_with(11), cell_with(13)];
        for (index, file) in start.iter().enumerate() {
            place_cell(&disk, index, file);
        }
        let mut offline = Offline::empty();
        offline.files = [start[2], start[0], start[1]];
        out.push((
            disk,
            Save {
                name: "three cells round",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        // Box 1 and box 6 swapped and swapped back, and cell 1 and cell 3
        // swapped: the Bank is written with the checksums it has.
        let (disk, files, mut body) = setup();
        drop(files);
        place_cell(&disk, 0, &cell_with(9));
        place_cell(&disk, 2, &cell_with(13));
        let mut offline = Offline::empty();
        offline.files[0] = cell_with(9);
        offline.files[2] = cell_with(13);
        let mut places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        places.swap(0, 5, UNKNOWN).unwrap();
        places.swap(0, 5, UNKNOWN).unwrap();
        places.swap(OFFLINE, OFFLINE + 2, UNKNOWN).unwrap();
        assert_eq!(body_crcs(&body), state(&disk));
        out.push((
            disk,
            Save {
                name: "the bank as it was, and cells",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        // A delivery's Pokémon into box 11: the record that makes the save
        // current takes the delivery.
        let (disk, files, _) = setup();
        drop(files);
        deliver_one(&disk, 72);
        let mut body = load(&mut opened(&disk)).0;
        let mut offline = Offline::empty();
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .move_all(TRANSPORT, 10, NOW)
        .unwrap();
        out.push((
            disk,
            Save {
                name: "a delivery into a box of bank",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        // A delivery waits; cell 1 and cell 3 swap, and the save's own record
        // takes the delivery.
        let (disk, files, body) = setup();
        drop(files);
        deliver_one(&disk, 73);
        place_cell(&disk, 0, &cell_with(9));
        let mut offline = Offline::empty();
        offline.files[0] = cell_with(9);
        let mut shown = load(&mut opened(&disk)).0;
        Places {
            body: &mut shown,
            offline: &mut offline,
        }
        .swap(OFFLINE, OFFLINE + 2, UNKNOWN)
        .unwrap();
        drop(body);
        out.push((
            disk,
            Save {
                name: "cells while a delivery waits",
                body: shown,
                offline,
                checked: [false; PLACES],
            },
        ));
        // A delivery's Pokémon into cell 1, saved; then a second delivery's
        // into cell 2: the transport box ends as the save before left it,
        // byte for byte, and the delivery is taken.
        let (disk, files, _) = setup();
        drop(files);
        deliver_one(&disk, 70);
        let mut files = opened(&disk);
        let mut body = load(&mut files).0;
        let mut offline = Offline::empty();
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .move_all(TRANSPORT, OFFLINE, NOW)
        .unwrap();
        run_commit(
            &mut files,
            &mut disk.clone(),
            &mut body,
            &mut offline,
            &[false; PLACES],
        )
        .unwrap();
        drop(files);
        deliver_one(&disk, 71);
        let mut files = opened(&disk);
        let mut body = load(&mut files).0;
        drop(files);
        let mut offline = Offline::empty();
        load_offline::<(), _>(&mut disk.clone(), &mut offline).unwrap();
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .move_all(TRANSPORT, OFFLINE + 1, NOW)
        .unwrap();
        let mut clean = body.clone();
        sections::clean(&mut clean).unwrap();
        assert_eq!(body_crcs(&clean), state(&disk));
        out.push((
            disk,
            Save {
                name: "a delivery into a cell, the transport box as before",
                body,
                offline,
                checked: [false; PLACES],
            },
        ));
        out
    }

    /// How many Pokémon the Bank and the cells hold, a Pokémon in two places
    /// counted twice. Settles nothing: after `on_card`.
    fn held_count(disk: &Shared) -> usize {
        let mut files = opened(&disk.reboot());
        let mut body = load(&mut files).0;
        let mut offline = Offline::empty();
        load_offline::<(), _>(&mut disk.clone(), &mut offline).unwrap();
        let places = Places {
            body: &mut body,
            offline: &mut offline,
        };
        (0..PLACES)
            .map(|place| {
                (0..BOX_SLOTS)
                    .filter(|slot| places.occupied_slot(place, *slot))
                    .count()
            })
            .sum()
    }

    /// The checksums the journal names for the Bank file and the transport
    /// box.
    fn state(disk: &Shared) -> (u32, u32) {
        let Phase::Clean(current) = opened(&disk.reboot()).phase().unwrap() else {
            unreachable!()
        };
        (current.bank.crc, current.mover.crc)
    }

    /// Transporter delivers one Pokémon, `id`.
    fn deliver_one(disk: &Shared, id: u32) {
        let mut source = body(0, 0, 0);
        source[TRANSPORT_RECORDS.start..][..RECORD_SIZE].copy_from_slice(&pokemon(id));
        source[TRANSPORT_TAGS.start] = 2;
        offline_core::transport::deliver(
            &mut Visit(disk.clone()),
            &source[TRANSPORT_RECORDS],
            &source[TRANSPORT_TAGS],
        )
        .unwrap();
    }

    fn run_save(card: &Shared, save: &Save) -> Result<(), Failed<Shared, Shared>> {
        let mut files = opened(card);
        let mut staging = load(&mut files).0;
        staging.copy_from_slice(&save.body);
        let mut copy = Offline {
            files: save.offline.files,
            ..Offline::empty()
        };
        run_commit(
            &mut files,
            &mut card.clone(),
            &mut staging,
            &mut copy,
            &save.checked,
        )
    }

    #[test]
    fn a_cut_at_every_write_of_a_save_loses_no_pokemon() {
        for (disk, save) in saves() {
            for console in [true, false] {
                let base = disk.reboot().like(console);
                let before = on_card(&base.copy());
                let counting = base.cut_after(usize::MAX);
                run_save(&counting, &save).unwrap();
                let total = counting.0.borrow().operations;
                let after = on_card(&counting);
                // A Pokémon in two places counts twice: as before the save,
                // or as after it, no more.
                let counts = [held_count(&base.copy()), held_count(&counting)];
                assert_eq!(after.1, before.1, "{}", save.name);
                assert!(after.0 != before.0, "{}", save.name);
                for cut in 0..=total {
                    let card = base.cut_after(cut);
                    let answer = run_save(&card, &save);
                    let card = card.reboot();
                    let what = std::format!("{}, console {console}, cut {cut}", save.name);
                    // A save that answers that it is made went through.
                    if answer.is_ok() {
                        assert!(on_card(&card.copy()).0 == after.0, "{what}");
                    }
                    // Whatever the cut left, the next opening settles it: no
                    // Pokémon is lost or doubled, and the Bank and the cells
                    // are as before the save or as after it.
                    let (held, all) = on_card(&card);
                    assert_eq!(all, before.1, "{what}");
                    assert!(held == before.0 || held == after.0, "{what}");
                    let count = counts[usize::from(held != before.0)];
                    assert_eq!(held_count(&card), count, "{what}");
                    // No folder of an interrupted write is left: the journal
                    // names the state before or after. A second opening
                    // changes nothing.
                    let sd = card.0.borrow().sd.clone();
                    assert!(!interrupted(&card), "{what}");
                    assert!(only_spare_copies(&card, &held), "{what}");
                    assert_eq!(on_card(&card).0, held, "{what}");
                    assert!(card.0.borrow().sd == sd, "{what}");
                }
            }
        }
    }

    /// Whether a folder of the date holds only spare copies: every Pokémon
    /// in it is also in the Bank or a cell (`held`) or in a session's
    /// folder.
    fn only_spare_copies(card: &Shared, held: &BTreeMap<Identity, usize>) -> bool {
        let sd = card.0.borrow();
        let resolved = |path: &[u8]| {
            let folder = &path[..path.iter().rposition(|byte| *byte == b'/').unwrap_or(0)];
            folder.ends_with(interrupted::RETIRED)
        };
        let identities = |bytes: &[u8]| -> Vec<Identity> {
            BoxFile::parse(bytes)
                .map(|file| {
                    file.records()
                        .chunks_exact(RECORD_SIZE)
                        .filter_map(|record| sections::stored_identity(record.try_into().unwrap()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut elsewhere: Vec<Identity> = held.keys().copied().collect();
        for (_, bytes) in sd.sd.iter().filter(|(path, _)| !resolved(path)) {
            elsewhere.extend(identities(bytes));
        }
        sd.sd
            .iter()
            .filter(|(path, _)| resolved(path))
            .all(|(_, bytes)| identities(bytes).iter().all(|id| elsewhere.contains(id)))
    }

    /// The opening's step for the SD card on `card` (`interrupted::settle`),
    /// as far as the card lets it go.
    fn settle_on(card: &Shared) -> Result<(), ()> {
        let mut files = BankFiles::new(card.clone());
        files.open().map_err(|_| ())?;
        interrupted::settle(
            &mut files,
            &mut card.clone(),
            &mut ExtCard(card.clone()),
            &mut vec![0; 2 * FILE_SIZE],
            DATE,
        )
        .map_err(|_| ())
    }

    #[test]
    fn a_cut_while_the_next_opening_settles_a_cut_save_loses_no_pokemon() {
        // Each save cut wherever it leaves its folder, then the opening that
        // settles it cut at every write: the opening after that ends as one
        // that was not cut. On the console's card, which loses what a cut
        // left unsynced.
        for (disk, save) in saves() {
            let base = disk.reboot().like(true);
            let before = on_card(&base.copy());
            let counting = base.cut_after(usize::MAX);
            run_save(&counting, &save).unwrap();
            let total = counting.0.borrow().operations;
            let after = on_card(&counting);
            let counts = [held_count(&base.copy()), held_count(&counting)];
            // Most cuts fall inside writes of Bank's files and leave the
            // same SD card and journal: each such state once.
            let mut seen = Vec::new();
            for cut in 0..total {
                let card = base.cut_after(cut);
                let _ = run_save(&card, &save);
                let card = card.reboot();
                let state = {
                    let disk = card.0.borrow();
                    (
                        disk.sd.clone(),
                        disk.sd_dirs.clone(),
                        disk.ext.clone(),
                        disk.ext_damaged.clone(),
                    )
                };
                let state = (state, opened(&card).phase().unwrap());
                if !interrupted(&card) || seen.contains(&state) {
                    continue;
                }
                seen.push(state);
                let counting = card.cut_after(usize::MAX);
                settle_on(&counting).unwrap();
                let steps = counting.0.borrow().operations;
                for second in 0..steps {
                    let twice = card.cut_after(second);
                    let _ = settle_on(&twice);
                    let twice = twice.reboot();
                    let what = std::format!("{}, cut {cut}, then {second}", save.name);
                    let (held, all) = on_card(&twice);
                    assert_eq!(all, before.1, "{what}");
                    assert!(held == before.0 || held == after.0, "{what}");
                    let count = counts[usize::from(held != before.0)];
                    assert_eq!(held_count(&twice), count, "{what}");
                    assert!(!interrupted(&twice), "{what}");
                }
            }
        }
    }

    #[test]
    fn a_card_without_room_changes_nothing() {
        for (disk, save) in saves() {
            let before = on_card(&disk.reboot());
            let card = disk.reboot();
            card.0.borrow_mut().sd_full = true;
            let files = card.0.borrow().sd.clone();
            let error = match run_save(&card, &save) {
                // A save of the Bank alone needs no room on the card.
                Ok(()) => {
                    assert!(card.0.borrow().sd == files, "{}", save.name);
                    continue;
                }
                Err(Error::Sd(error)) => error,
                Err(_) => panic!("{}: a full card must refuse", save.name),
            };
            assert!(Shared::no_room(&error));
            card.0.borrow_mut().sd_full = false;
            assert!(on_card(&card.reboot()).0 == before.0, "{}", save.name);
        }
    }

    #[test]
    fn no_room_to_finish_a_cell_leaves_it_for_the_next_opening() {
        // Box 1 and cell 1 change places; the power goes after the journal,
        // before the cell is written. The next opening finds the card full:
        // it says so, and the folder stays for the one after.
        let (disk, save) = saves().remove(1);
        let old_cell = disk.0.borrow().sd.get(cell_path(0).text()).cloned();
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let after = on_card(&counting).0;
        let start = opened(&disk.reboot()).phase().unwrap();
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let cell = card.0.borrow().sd.get(cell_path(0).text()).cloned();
            if cell != old_cell || !interrupted(&card) || opened(&card).phase().unwrap() == start {
                continue;
            }
            card.0.borrow_mut().sd_full = true;
            let mut files = opened(&card);
            let failed = interrupted::settle(
                &mut files,
                &mut card.clone(),
                &mut ExtCard(card.clone()),
                &mut vec![0; 2 * FILE_SIZE],
                DATE,
            );
            let Err(Error::Sd(error)) = failed else {
                panic!("a full card must refuse, cut {cut}");
            };
            assert!(Shared::no_room(&error));
            assert!(interrupted(&card));
            card.0.borrow_mut().sd_full = false;
            assert!(on_card(&card).0 == after, "cut {cut}");
            assert!(!interrupted(&card));
            return;
        }
        panic!("no cut left the journal written and the cell not");
    }

    #[test]
    fn a_save_cut_before_the_journal_needs_no_room_to_be_undone() {
        // The first save, box 1 and cell 1 swapped, every cell without a
        // file. Wherever a cut leaves the journal as it was, the next START
        // settles it on a card without room for a file: no cell holds
        // anything but the empty box it held, also where the cut fell in
        // that box's own write.
        let (disk, files, mut body) = setup();
        drop(files);
        assert!(disk.0.borrow().sd.is_empty());
        let mut offline = Offline::empty();
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .swap(0, OFFLINE, UNKNOWN)
        .unwrap();
        let save = Save {
            name: "the first save",
            body,
            offline,
            checked: [false; PLACES],
        };
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let start = opened(&disk.reboot()).phase().unwrap();
        let mut empty = [0u8; FILE_SIZE];
        empty_box(&mut empty, &[0; NAME_TEXT]);
        let mut tried = 0;
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            if !interrupted(&card) || opened(&card).phase().unwrap() != start {
                continue;
            }
            tried += 1;
            card.0.borrow_mut().sd_full = true;
            assert!(settle_on(&card).is_ok(), "cut {cut}");
            assert!(!interrupted(&card), "cut {cut}");
            let sd = card.0.borrow();
            let cell = sd.sd.get(cell_path(0).text());
            assert!(cell.is_none_or(|cell| cell[..] == empty), "cut {cut}");
        }
        assert!(
            tried > 0,
            "no cut left the folder and the journal as it was"
        );
    }

    #[test]
    fn transporter_waits_wherever_a_save_that_takes_a_delivery_is_cut() {
        // The save of a second delivery into a cell, cut at every write; its
        // own record takes the delivery.
        // Settled as before it, the delivered Pokémon are in the transport
        // box, waiting or taken, and Transporter delivers nothing over
        // them; settled as after it, they are in the cell and the box is
        // empty for the next delivery.
        let (disk, save) = saves().pop().unwrap();
        let before = on_card(&disk.reboot()).0;
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let after = on_card(&counting).0;
        let total = counting.0.borrow().operations;
        for cut in 0..=total {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let held = on_card(&card).0;
            if held == before {
                assert!(try_deliver(&card, 1).is_none(), "cut {cut}");
            } else {
                assert!(held == after, "cut {cut}");
                assert!(try_deliver(&card, 1).is_some(), "cut {cut}");
            }
        }
    }

    #[test]
    fn a_delivery_after_a_cut_leaves_the_cells_as_before() {
        // Box 1 and cell 1 change places; the power goes before the journal
        // is written. Transporter delivers in between.
        let (disk, save) = saves().remove(1);
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let before = on_card(&disk.reboot()).0;
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let unwritten =
                opened(&card).phase().unwrap() == opened(&disk.reboot()).phase().unwrap();
            if !interrupted(&card) || !unwritten {
                continue;
            }
            deliver(&card, 1);
            let (held, _) = on_card(&card);
            for (identity, place) in &before {
                assert_eq!(held.get(identity), Some(place), "cut {cut}");
            }
            return;
        }
        panic!("no cut left the folder and the journal not written");
    }

    /// The folder of `save`'s cells, on `card` before it.
    fn save_folder(card: &Shared, save: &Save) -> Path {
        let mut files = opened(&card.reboot());
        let mut body = load(&mut files).0;
        body.copy_from_slice(&save.body);
        let [before, after] = files
            .commit_states(&Cleaned::new(&mut body).unwrap())
            .unwrap()
            .map(|(bank, mover)| interrupted::State { bank, mover });
        interrupted::folder(before, after)
    }

    #[test]
    fn a_cut_after_the_journal_is_finished_in_the_cells() {
        // Box 1 and cell 1 change places; the power goes after the journal
        // record, before the folder is deleted. The journal names the
        // folder's state after: the cell gets its new box, and the cell's
        // old box is in the Bank now.
        let (disk, save) = saves().remove(1);
        let Phase::Clean(before) = opened(&disk.reboot()).phase().unwrap() else {
            unreachable!()
        };
        let hanging = interrupted::cells_file(&save_folder(&disk, &save), true);
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let after = on_card(&counting).0;
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let written = opened(&card).phase().unwrap() != Phase::Clean(before);
            if !written || !card.0.borrow().ext.contains_key(hanging.text()) {
                continue;
            }
            // The cells as after the save, no file of the write, and both
            // sides of the cell kept.
            assert!(on_card(&card).0 == after, "cut {cut}");
            let card = card.0.borrow();
            assert!(card.ext.is_empty(), "cut {cut}");
            assert!(card.ext_dirs.is_empty(), "cut {cut}");
            let kept = &card.sd[&at(&std::format!("{KEPT}/mover1.old.box"))];
            assert_eq!(kept[..], cell_with(9)[..], "cut {cut}");
            assert_eq!(
                card.sd[&at(&std::format!("{KEPT}/mover1.new.box"))][..],
                save.offline.files[0][..],
                "cut {cut}"
            );
            return;
        }
        panic!("no cut left the journal written and the folder there");
    }

    /// Puts back what a Checkpoint backup of Bank's extdata holds: its
    /// files and folders, with nothing of the plain files of the SD card.
    fn restore_extdata(card: &Shared, backup: &Shared) {
        let saved = backup.0.borrow();
        let mut disk = card.0.borrow_mut();
        disk.files = saved.files.clone();
        disk.absent = saved.absent.clone();
        disk.unreadable = saved.unreadable.clone();
        disk.ext = saved.ext.clone();
        disk.ext_dirs = saved.ext_dirs.clone();
        disk.ext_written = saved.ext_written.clone();
        disk.ext_damaged = saved.ext_damaged.clone();
    }

    /// Whether a box file anywhere on the card holds the Pokémon `id`.
    fn on_the_card(card: &Shared, id: u32) -> bool {
        let wanted = sections::stored_identity(&pokemon(id));
        card.0.borrow().sd.values().any(|bytes| {
            BoxFile::parse(bytes).is_ok_and(|file| {
                file.records()
                    .chunks_exact(RECORD_SIZE)
                    .any(|record| sections::stored_identity(record.try_into().unwrap()) == wanted)
            })
        })
    }

    #[test]
    fn a_checked_box_survives_a_cut_after_the_journal_and_a_restored_backup() {
        // Offline box 1 holds Pikachu; it is checked and saved, and the power
        // goes after the record, before the cell. A Checkpoint backup, START,
        // Pikachu copied back into the cell and every other file on the SD
        // card deleted, the backup restored, START: the journal says "after"
        // and the cell gets the empty box, with Pikachu kept.
        const PIKACHU: u32 = 9;
        let (disk, files, body) = setup();
        drop(files);
        place_cell(&disk, 0, &cell_with(PIKACHU));
        let mut offline = Offline::empty();
        offline.files[0] = cell_with(PIKACHU);
        let mut checked = [false; PLACES];
        checked[OFFLINE] = true;
        let save = Save {
            name: "a checked cell",
            body,
            offline,
            checked,
        };
        let folder = save_folder(&disk, &save);
        let new = interrupted::cells_file(&folder, true);
        let start = opened(&disk.reboot()).phase().unwrap();
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let cell = card.0.borrow().sd.get(cell_path(0).text()).cloned();
            if opened(&card).phase().unwrap() == start
                || !card.0.borrow().ext.contains_key(new.text())
                || cell.as_deref() != Some(&cell_with(PIKACHU)[..])
            {
                continue;
            }
            let backup = card.copy();
            on_card(&card);
            {
                let mut state = card.0.borrow_mut();
                state.sd.clear();
                state.sd_dirs.clear();
            }
            place_cell(&card, 0, &cell_with(PIKACHU));
            restore_extdata(&card, &backup);
            on_card(&card);
            assert!(on_the_card(&card, PIKACHU), "cut {cut}");
            assert!(!interrupted(&card), "cut {cut}");
            return;
        }
        panic!("no cut left the journal written and the cell not");
    }

    #[test]
    fn a_box_survives_a_cut_before_the_journal_and_a_restored_backup() {
        // Offline box 1 holds Pikachu, which moves into Bank; the power goes
        // before the record. A Checkpoint backup, START, Eevee put into
        // offline box 1, the backup restored, START: the journal says
        // "before", the cell holds Eevee, and Pikachu is only in `.old`.
        const PIKACHU: u32 = 9;
        const EEVEE: u32 = 13;
        let (disk, save) = saves().remove(1);
        let folder = save_folder(&disk, &save);
        let (old, new) = (
            interrupted::cells_file(&folder, false),
            interrupted::cells_file(&folder, true),
        );
        let start = opened(&disk.reboot()).phase().unwrap();
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let whole = |path: &Path| {
                let state = card.0.borrow();
                state.ext.get(path.text()).is_some_and(|file| {
                    !state.ext_damaged.contains(path.text())
                        && state.ext_written.get(path.text()) == Some(&file.len())
                })
            };
            if !whole(&old) || !whole(&new) || opened(&card).phase().unwrap() != start {
                continue;
            }
            let backup = card.copy();
            on_card(&card);
            place_cell(&card, 0, &cell_with(EEVEE));
            restore_extdata(&card, &backup);
            on_card(&card);
            assert!(on_the_card(&card, PIKACHU), "cut {cut}");
            assert_eq!(
                card.0.borrow().sd[cell_path(0).text()][..],
                cell_with(EEVEE)[..],
                "cut {cut}"
            );
            return;
        }
        panic!("no cut left both sides written and the journal not");
    }

    #[test]
    fn a_read_that_fails_once_loses_nothing_it_does_not_report() {
        // Each save cut wherever it leaves its folder, then an opening whose
        // one read fails, at each read in turn. It ends in one of three ways:
        // as the opening without the failure, stopped with the folder there
        // for the next START, or with the folder gone and the damage reported.
        for (disk, save) in saves() {
            let base = disk.reboot().like(true);
            let before = on_card(&base.copy());
            let counting = base.cut_after(usize::MAX);
            run_save(&counting, &save).unwrap();
            let total = counting.0.borrow().operations;
            let after = on_card(&counting);
            let mut seen = Vec::new();
            for cut in 0..total {
                let card = base.cut_after(cut);
                let _ = run_save(&card, &save);
                let card = card.reboot();
                let state = {
                    let disk = card.0.borrow();
                    (
                        disk.sd.clone(),
                        disk.sd_dirs.clone(),
                        disk.ext.clone(),
                        disk.ext_damaged.clone(),
                    )
                };
                let state = (state, opened(&card).phase().unwrap());
                if !interrupted(&card) || seen.contains(&state) {
                    continue;
                }
                seen.push(state);
                let plain = card.copy();
                settle_on(&plain).unwrap();
                let reads = plain.0.borrow().reads;
                for read in 0..reads {
                    let what = std::format!("{}, cut {cut}, read {read}", save.name);
                    let once = card.copy();
                    once.0.borrow_mut().fail_reads.insert(read);
                    let answer = settle_on(&once);
                    let once = once.reboot();
                    if answer.is_err() && !interrupted(&once) {
                        // Reported, once: the next opening has nothing left.
                        assert!(settle_on(&once).is_ok(), "{what}");
                        continue;
                    }
                    let (held, all) = on_card(&once);
                    assert_eq!(all, before.1, "{what}");
                    assert!(held == before.0 || held == after.0, "{what}");
                    assert!(!interrupted(&once), "{what}");
                }
            }
        }
    }

    #[test]
    fn a_finished_save_deletes_new_before_old() {
        // Wherever a cut falls in the deletion, `.old` is there as long as
        // anything of the folder is.
        let (disk, save) = saves().remove(1);
        let folder = save_folder(&disk, &save);
        let (old, new) = (
            interrupted::cells_file(&folder, false),
            interrupted::cells_file(&folder, true),
        );
        let start = opened(&disk.reboot()).phase().unwrap();
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let mut only_old = false;
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            if opened(&card).phase().unwrap() == start {
                continue;
            }
            let state = card.0.borrow();
            let (has_old, has_new) = (
                state.ext.contains_key(old.text()),
                state.ext.contains_key(new.text()),
            );
            assert!(has_old || !has_new, "cut {cut}");
            only_old |= has_old && !has_new;
        }
        assert!(only_old, "no cut fell between the two deletions");
    }

    #[test]
    fn a_checked_cell_that_is_no_whole_box_is_refused_before_anything_is_written() {
        let (disk, mut files, mut body) = setup();
        let mut half = cell_with(9);
        half[100] ^= 1;
        place_cell(&disk, 1, &half);
        let before = disk.0.borrow().sd.clone();
        let folders = disk.0.borrow().sd_dirs.clone();
        let mut checked = [false; PLACES];
        checked[OFFLINE + 1] = true;
        checked[3] = true;
        let failed = run_commit(
            &mut files,
            &mut disk.clone(),
            &mut body,
            &mut Offline::empty(),
            &checked,
        );
        assert!(matches!(failed, Err(Error::BadFile(place)) if place == OFFLINE + 1));
        assert!(disk.0.borrow().sd == before);
        assert!(disk.0.borrow().sd_dirs == folders);
    }

    #[test]
    fn a_save_of_a_bank_replaced_since_is_kept_in_a_folder_of_the_date() {
        // Box 1 and cell 1 change places; the power goes before the journal,
        // with both sides written. Before the next START the Bank is
        // replaced, here by a save that knows nothing of them, as a restored
        // backup of the extdata would: the journal names neither of the
        // folder's states. The cell stays, and both sides are kept.
        let (disk, save) = saves().remove(1);
        let folder = save_folder(&disk, &save);
        let (old, new) = (
            interrupted::cells_file(&folder, false),
            interrupted::cells_file(&folder, true),
        );
        let counting = disk.reboot().cut_after(usize::MAX);
        run_save(&counting, &save).unwrap();
        let start = opened(&disk.reboot()).phase().unwrap();
        for cut in 0..counting.0.borrow().operations {
            let card = disk.reboot().cut_after(cut);
            let _ = run_save(&card, &save);
            let card = card.reboot();
            let whole = |path: &Path| {
                let state = card.0.borrow();
                state.ext.get(path.text()).is_some_and(|file| {
                    !state.ext_damaged.contains(path.text())
                        && state.ext_written.get(path.text()) == Some(&file.len())
                })
            };
            if !whole(&old) || !whole(&new) || opened(&card).phase().unwrap() != start {
                continue;
            }
            let mut files = opened(&card);
            let mut body = load(&mut files).0;
            put(&mut body, 50, 0, 77);
            files.commit(Cleaned::new(&mut body).unwrap()).unwrap();
            on_card(&card);
            let state = card.0.borrow();
            let kept = &state.sd[&at(&std::format!("{KEPT}/mover1.old.box"))];
            assert_eq!(kept[..], cell_with(9)[..], "cut {cut}");
            assert_eq!(
                state.sd[&at(&std::format!("{KEPT}/mover1.new.box"))][..],
                save.offline.files[0][..],
                "cut {cut}"
            );
            assert_eq!(state.sd[cell_path(0).text()][..], cell_with(9)[..]);
            drop(state);
            assert!(!interrupted(&card));
            return;
        }
        panic!("no cut left both sides written and the journal not");
    }

    #[test]
    fn every_interrupted_folder_that_names_no_state_of_the_journal_is_kept_in_a_folder_of_the_date()
    {
        // Folders of two states the journal does not name: one with both
        // halves of a backup, one with a transfer save's cell. What each holds goes, as
        // single box files, into a folder of the date, in the order of the
        // list, each to the next free second.
        let (disk, ..) = setup();
        const ONE: &str = "1111111122222222";
        const TWO: &str = "3333333344444444";
        in_extdata(&disk, ONE, "bank28.old", &[(2, cell_with(1))]);
        in_extdata(&disk, ONE, "bank28.new", &[(2, cell_with(2))]);
        in_extdata(&disk, ONE, "mover0.new", &[(0, cell_with(3))]);
        in_extdata(&disk, TWO, "mover0.new", &[(2, cell_with(4))]);
        on_card(&disk);
        let card = disk.0.borrow();
        let names: Vec<_> = card.sd.keys().cloned().collect();
        assert_eq!(
            names,
            [
                at(&std::format!("{KEPT}/bank30.new.box")),
                at(&std::format!("{KEPT}/bank30.old.box")),
                at(&std::format!("{KEPT}/mover0.new.box")),
                at(&std::format!("{KEPT_NEXT}/mover2.new.box")),
            ]
        );
        assert_eq!(
            card.sd[&at(&std::format!("{KEPT_NEXT}/mover2.new.box"))],
            cell_with(4)
        );
        assert!(card.ext.is_empty() && card.ext_dirs.is_empty());
        let folders: Vec<_> = card.sd_dirs.iter().cloned().collect();
        assert_eq!(
            folders,
            [
                sdfiles::PARENT.to_vec(),
                ROOT.to_vec(),
                at(KEPT),
                at(KEPT_NEXT),
            ]
        );
    }

    #[test]
    fn a_folder_of_the_date_never_takes_the_name_of_one_that_is_there() {
        // The clock came back to a second whose folders are there: a
        // session's, with its only copy of a box, and one of a kept backup,
        // the next second taken too, and a file of that name at the one
        // after.
        let (disk, mut files, mut body) = setup();
        let file = |name: &str, bytes: &[u8; FILE_SIZE]| {
            let mut card = disk.0.borrow_mut();
            let path = at(name);
            let folder = path[..path.iter().rposition(|byte| *byte == b'/').unwrap()].to_vec();
            card.sd_dirs.insert(sdfiles::PARENT.to_vec());
            card.sd_dirs.insert(ROOT.to_vec());
            card.sd_dirs.insert(folder);
            card.sd.insert(path, bytes.to_vec());
        };
        file("/20261109-140307/mover1.box", &cell_with(1));
        file("/20261109-140308/mover1.box", &cell_with(2));
        file("/20261109-140309", &cell_with(3));
        file(&std::format!("{KEPT}/mover1.box"), &cell_with(4));
        in_extdata(
            &disk,
            "1111111122222222",
            "mover0.new",
            &[(1, cell_with(5))],
        );
        let before = disk.0.borrow().sd.clone();
        // A session that checks the first cell, then an opening that keeps
        // the folder in Bank's extdata.
        place_cell(&disk, 0, &cell_with(6));
        let mut offline = Offline::new();
        load_offline::<(), _>(&mut disk.clone(), &mut offline).unwrap();
        let mut checked = [false; PLACES];
        checked[OFFLINE] = true;
        run_commit(
            &mut files,
            &mut disk.clone(),
            &mut body,
            &mut offline,
            &checked,
        )
        .unwrap();
        on_card(&disk);
        let card = disk.0.borrow();
        // Everything that was there is there as it was.
        for (path, bytes) in &before {
            assert_eq!(card.sd.get(path), Some(bytes), "{path:?}");
        }
        assert_eq!(card.sd[&at("/20261109-140310/mover1.box")], cell_with(6));
        assert_eq!(
            card.sd[&at(&std::format!("{KEPT_NEXT}/mover1.new.box"))],
            cell_with(5)
        );
    }

    #[test]
    fn a_box_file_of_any_box_brought_into_a_cell_is_an_offline_box() {
        // A session's copy of the transport box and one of a box of Bank,
        // copied to the SD card as offline boxes 1 and 2.
        let (disk, mut files, mut body) = setup();
        put(&mut body, TRANSPORT, 0, 7);
        let mut transport = [0u8; FILE_SIZE];
        boxfile::encode_transport(
            &body[TRANSPORT_RECORDS],
            &body[TRANSPORT_TAGS],
            &named("Transport Box"),
            &mut transport,
        )
        .unwrap();
        let mut online = [0u8; FILE_SIZE];
        boxfile::encode_body_box(&body, 5, &mut online).unwrap();
        put(&mut body, TRANSPORT, 0, 0);
        place_cell(&disk, 0, &transport);
        place_cell(&disk, 1, &online);
        let mut sd = disk.clone();
        let mut offline = Offline::new();
        load_offline::<(), _>(&mut sd, &mut offline).unwrap();
        // Each as it was written.
        assert!(offline.files[0] == transport);
        assert!(offline.files[1] == online);
        assert_eq!(
            BoxFile::parse(&offline.files[0]).unwrap().name(),
            named("Transport Box")
        );
        // A box of Bank, with its flag bytes and times, swapped into the
        // cell the transport box's file lies in, and saved: it loads again.
        Places {
            body: &mut body,
            offline: &mut offline,
        }
        .swap(0, OFFLINE, UNKNOWN)
        .unwrap();
        run_commit(
            &mut files,
            &mut sd,
            &mut body,
            &mut offline,
            &[false; PLACES],
        )
        .unwrap();
        let mut again = Offline::new();
        load_offline::<(), _>(&mut sd, &mut again).unwrap();
        assert!(again.files == offline.files);
    }

    #[test]
    fn a_damaged_offline_file_is_refused_and_left_alone() {
        let disk = Shared::default();
        disk.0
            .borrow_mut()
            .sd
            .insert(at("/mover2.box"), vec![1; FILE_SIZE]);
        let mut sd = disk.clone();
        let mut offline = Offline::empty();
        assert!(matches!(
            load_offline::<(), _>(&mut sd, &mut offline),
            Err(Error::BadFile(place)) if place == OFFLINE + 1
        ));
        assert_eq!(disk.0.borrow().sd.len(), 1);
    }

    #[test]
    fn reading_the_cells_forgets_the_names_of_an_earlier_screen() {
        // The room the cells lie in holds what other work left there: a
        // screen whose texts do not load names nothing with it.
        let mut offline = Offline::empty();
        offline.defaults = [[0x5a; NAME_TEXT]; OFFLINE_COUNT];
        offline.transport = [0xa5; NAME_TEXT];
        load_offline::<(), _>(&mut Shared::default(), &mut offline).unwrap();
        assert_eq!(offline.defaults, [[0; NAME_TEXT]; OFFLINE_COUNT]);
        assert_eq!(offline.transport, [0; NAME_TEXT]);
        offline.name_defaults();
        for file in &offline.files {
            assert_eq!(file[boxfile::NAME], [0; NAME_TEXT]);
        }
    }

    #[test]
    fn the_files_are_named_by_their_places() {
        assert_eq!(folder().text(), at("/20261109-140307"));
        assert!(folder().valid());
        for (place, name) in [
            (0, "/20261109-140307/bank01.box"),
            (99, "/20261109-140307/bank00.box"),
            (TRANSPORT, "/20261109-140307/mover0.box"),
            (OFFLINE + 2, "/20261109-140307/mover3.box"),
        ] {
            assert_eq!(sdfiles::box_file(&folder(), place).text(), at(name));
        }
        assert_eq!(cell_path(2).text(), at("/mover3.box"));
        let part = |crc| offline_core::Part {
            slot: offline_core::Slot::A,
            crc,
        };
        let state = |crc| interrupted::State {
            bank: part(crc),
            mover: part(crc),
        };
        let folder = interrupted::folder(state(1), state(2));
        let mut inside = folder;
        inside.push(b"/mover0.new");
        assert_eq!(interrupted::cells_file(&folder, true), inside);
        assert!(inside.valid());
    }
}
