//! The transfer screen as a box manager (docs/internals.md, "The transfer screen"):
//! the original's box selection for Pokémon HOME (task `0x1B`, its scene 1,
//! the selection object of vtable `0036071c`), reached from Back on game
//! selection, with four cells more (`layout`), the cursor's three colours
//! and what they do (`bank_common::transfer_screen`). What each key does is decided
//! here in plain code; the entries apply it with the original's calls.

use bank_common::transfer_screen::{Changes, Refusal, OFFLINE, PLACES, TRANSPORT};

/// The original's messages of the screen, text file 38.
pub const MOVE_TO_HOME: u32 = 0x38;
pub const QUIT_MOVING: u32 = 0x39;
pub const SAVE_AND_QUIT: u32 = 0x20;
pub const QUIT_WITHOUT_SAVING: u32 = 0x21;
pub const CANNOT_TRANSPORT: u32 = 0x23;
/// "Transport Box", the name the upper screen shows for the transport box
/// and for an offline place without a box.
pub const TRANSPORT_BOX: u32 = 0x2a;

/// The box screen's sounds (`0028decc`, `00285ba8`, `002885e8`, played by
/// `001e9bc0`): lifting `0x50006`; putting down, and back with B, `0x50007`
/// in the frame after the key (`00289f3c`), with none on the key itself (B
/// goes straight to the put-back, `002886cc`; its `0x5000A` at `00288688`
/// ends the green hand's selection, flag `+0x6e5`, not a hold, `+0x6e4`); a
/// place what is held cannot go `0x5000E`, on the key where it stays in the
/// hand (`0028e000`), with the message where it goes back (`0028d91c`); a
/// box checked, and a message closed, `0x50009`.
pub const SOUND_LIFT: u32 = 0x0005_0006;
pub const SOUND_PUT: u32 = 0x0005_0007;
pub const SOUND_REFUSED: u32 = 0x0005_000e;
pub const SOUND_CHECK: u32 = 0x0005_0009;
pub const SOUND_CLOSE: u32 = 0x0005_0009;

/// The keys, in the original's pad bits (`002c7f70` makes them from the
/// console's: left, right, up, down `1`..`8`, A, B, X, Y `0x10`..`0x80`, L
/// and R `0x100`, `0x200`, START `0x1000`; SELECT has none). START cycles the
/// hand as on the box screen (`002918e4`: START is its action 9, which
/// steps its hand, `002889cc`); the transfer screen's own key update leaves
/// it.
pub const KEY_A: u32 = 0x10;
pub const KEY_B: u32 = 0x20;
pub const KEY_X: u32 = 0x40;
pub const KEY_Y: u32 = 0x80;
pub const KEY_L: u32 = 0x100;
pub const KEY_R: u32 = 0x200;
pub const KEY_START: u32 = 0x1000;

/// What a key does while a box is held, as on the box screen while it holds
/// a Pokémon (its state 4, `00288398`, flag `+0x6e4`): A puts down
/// (`0028dfac`), and does nothing off the places (`0028dfb0`); B puts back
/// (`002886cc`); X, Y and START do nothing (`00288720`, `002886f0`,
/// `002889d4`), and so do L and R here, which turn the box screen's pages
/// and press a button on this screen; the cursor moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldKey {
    /// A on a place: put down there, if it can go there.
    Put(usize),
    /// B: back where it came from.
    Back,
    Nothing,
    /// No key of these: the original's key update moves the cursor.
    Cursor,
}
/// The keys that do nothing while a box is held: this screen's buttons (Y
/// DONE, X RESET, R ALL, `00285d84`..`00285e04`), L, and the hand's START.
pub const KEYS_OFF_WHILE_HELD: u32 = KEY_X | KEY_Y | KEY_L | KEY_R | KEY_START;
/// `pressed` tells whether a key of a mask was pressed now; `place` is the
/// place under the cursor (`place_of`). A comes before B, as the box screen
/// reads them (`00291a5c`).
pub fn held_key(pressed: impl Fn(u32) -> bool, place: Option<usize>) -> HeldKey {
    if pressed(KEY_A) {
        place.map_or(HeldKey::Nothing, HeldKey::Put)
    } else if pressed(KEY_B) {
        HeldKey::Back
    } else if pressed(KEYS_OFF_WHILE_HELD) {
        HeldKey::Nothing
    } else {
        HeldKey::Cursor
    }
}

/// Whether A with the hand of `mode` lifts `place`: one with Pokémon. The
/// red hand lifts nothing. The blue hand lifts a checked box too, and its check goes with it, but leaves
/// the transport box, whose Pokémon only the green hand takes; the green hand
/// leaves a checked box. What is left stays where it is, without a sound, as
/// a place without Pokémon.
pub fn liftable(mode: Mode, place: usize, count: u32, checked: bool) -> bool {
    count != 0
        && match mode {
            Mode::Check => false,
            Mode::Swap => place != TRANSPORT,
            Mode::Move => !checked,
        }
}

/// The question before the boxes (page 3 of 39:0x00), shortened in each
/// language the original holds: its words about Pokémon HOME, and "still",
/// go. Each row: the page as the original has it, what is cut from it, in
/// order, and what is left. Japanese has one row for both of its archives.
pub const QUESTION: [(&str, &[&str], &str); 9] = [
    // Japanese.
    (
        "このまま\u{3000}ポケモンバンクの\u{3000}ポケモンを\nPokémon HOMEに\u{3000}ひっこし\u{3000}しますか？",
        &["このまま\u{3000}", "\nPokémon HOMEに"],
        "ポケモンバンクの\u{3000}ポケモンを\u{3000}ひっこし\u{3000}しますか？",
    ),
    // English.
    (
        "Do you still want to move the Pokémon in Pokémon Bank\nto Pokémon HOME?",
        &["still ", "\nto Pokémon HOME"],
        "Do you want to move the Pokémon in Pokémon Bank?",
    ),
    // French.
    (
        "Voulez-vous transférer vos Pokémon de Banque Pokémon\nvers Pokémon HOME malgré tout\u{e07f}?",
        &["\nvers Pokémon HOME malgré tout"],
        "Voulez-vous transférer vos Pokémon de Banque Pokémon\u{e07f}?",
    ),
    // Italian.
    (
        "Vuoi trasferire comunque i Pokémon della Banca Pokémon\nin Pokémon HOME?",
        &["comunque ", "\nin Pokémon HOME"],
        "Vuoi trasferire i Pokémon della Banca Pokémon?",
    ),
    // German.
    (
        "Möchtest du die Pokémon, die sich derzeit in Pokémon Bank\nbefinden, trotzdem auf Pokémon HOME transferieren?",
        &["trotzdem auf Pokémon HOME "],
        "Möchtest du die Pokémon, die sich derzeit in Pokémon Bank\nbefinden, transferieren?",
    ),
    // Spanish.
    (
        "¿Quieres trasladar tus Pokémon del Banco de Pokémon a\nPokémon HOME de todos modos?",
        &[" a\nPokémon HOME de todos modos"],
        "¿Quieres trasladar tus Pokémon del Banco de Pokémon?",
    ),
    // Korean.
    (
        "이대로 포켓몬 뱅크에 있는 포켓몬을\nPokémon HOME으로 이사하겠습니까?",
        &["이대로 ", "\nPokémon HOME으로"],
        "포켓몬 뱅크에 있는 포켓몬을 이사하겠습니까?",
    ),
    // Simplified Chinese.
    (
        "要直接将宝可梦虚拟银行内的\n宝可梦转移至Pokémon HOME吗？",
        &["直接", "至Pokémon HOME"],
        "要将宝可梦虚拟银行内的\n宝可梦转移吗？",
    ),
    // Traditional Chinese.
    (
        "要直接將寶可夢虛擬銀行的\n寶可夢轉移至Pokémon HOME嗎？",
        &["直接", "至Pokémon HOME"],
        "要將寶可夢虛擬銀行的\n寶可夢轉移嗎？",
    ),
];

/// The question DONE asks with a box checked (38:0x38), shortened in each
/// language the original holds: its words about Pokémon HOME go. Each row
/// as `QUESTION`'s; Japanese has one row for both of its archives.
pub const MOVE_QUESTION: [(&str, &[&str], &str); 9] = [
    // Japanese.
    (
        "Pokémon HOME\u{3000}に\u{3000}ひっこしますか？",
        &["Pokémon HOME\u{3000}に\u{3000}"],
        "ひっこしますか？",
    ),
    // English.
    (
        "Move these Pokémon to Pokémon HOME?",
        &[" to Pokémon HOME"],
        "Move these Pokémon?",
    ),
    // French.
    (
        "Transférer ces Pokémon vers Pokémon HOME\u{e07f}?",
        &[" vers Pokémon HOME"],
        "Transférer ces Pokémon\u{e07f}?",
    ),
    // Italian.
    (
        "Vuoi trasferire i Pokémon in Pokémon HOME?",
        &[" in Pokémon HOME"],
        "Vuoi trasferire i Pokémon?",
    ),
    // German.
    (
        "Soll der Transfer auf Pokémon HOME beginnen?",
        &[" auf Pokémon HOME"],
        "Soll der Transfer beginnen?",
    ),
    // Spanish.
    (
        "¿Quieres iniciar el traslado a Pokémon HOME?",
        &[" a Pokémon HOME"],
        "¿Quieres iniciar el traslado?",
    ),
    // Korean.
    (
        "Pokémon HOME으로 이사하겠습니까?",
        &["Pokémon HOME으로 "],
        "이사하겠습니까?",
    ),
    // Simplified Chinese.
    (
        "要转移至Pokémon HOME吗？",
        &["至Pokémon HOME"],
        "要转移吗？",
    ),
    // Traditional Chinese.
    (
        "要轉移至Pokémon HOME嗎？",
        &["至Pokémon HOME"],
        "要轉移嗎？",
    ),
];

/// Where `needle` begins in `text` at `from` or after.
fn find(text: &[u16], from: usize, needle: &str) -> Option<usize> {
    let size = needle.encode_utf16().count();
    (from..=text.len().checked_sub(size)?).find(|&at| {
        text[at..at + size]
            .iter()
            .copied()
            .eq(needle.encode_utf16())
    })
}

/// Shortens the question in `room[..len]` (`QUESTION`) if it is one of the
/// original's pages exactly, moving what is left together and ending it;
/// any other text stays whole. Answers the length.
pub fn shorten_question(room: &mut [u16], len: usize) -> usize {
    shorten(&QUESTION, room, len)
}
/// The same for DONE's question with a box checked (`MOVE_QUESTION`).
pub fn shorten_move_question(room: &mut [u16], len: usize) -> usize {
    shorten(&MOVE_QUESTION, room, len)
}
fn shorten(table: &[(&str, &[&str], &str)], room: &mut [u16], len: usize) -> usize {
    let Some((_, cuts, _)) = table
        .iter()
        .find(|(page, _, _)| room[..len].iter().copied().eq(page.encode_utf16()))
    else {
        return len;
    };
    let mut places = [(0, 0); 2];
    let mut from = 0;
    for (place, cut) in places.iter_mut().zip(cuts.iter()) {
        let Some(at) = find(&room[..len], from, cut) else {
            return len;
        };
        from = at + cut.encode_utf16().count();
        *place = (at, from);
    }
    let (mut write, mut read) = (0, 0);
    for &(start, end) in &places[..cuts.len()] {
        room.copy_within(read..start, write);
        write += start - read;
        read = end;
    }
    room.copy_within(read..len, write);
    write += len - read;
    if write < room.len() {
        room[write] = 0;
    }
    write
}

/// The word for "box" in the transport box's name (38:0x2A) in each of the
/// ten text archives: the name as the original has it, and the word an
/// offline box's default name takes from it, with a capital first letter
/// where it stands inside a word. The whole name and a number do not fit a
/// box name everywhere (16 characters: "Caja de Traslado 1" has 18).
pub const BOX_WORD: [(&str, &str); 10] = [
    // Japanese, kana and kanji.
    ("てんそうボックス", "ボックス"),
    ("転送ボックス", "ボックス"),
    ("Transport Box", "Box"),
    ("Boîte Transfert", "Boîte"),
    ("Trasferibox", "Box"),
    ("Transferbox", "Box"),
    ("Caja de Traslado", "Caja"),
    ("전송박스", "박스"),
    // Simplified and traditional Chinese.
    ("传送盒", "盒"),
    ("傳送盒", "盒"),
];

/// The word for "box" of the transport box's name `transport` (`BOX_WORD`).
fn box_word(transport: &[u16]) -> Option<&'static str> {
    BOX_WORD
        .iter()
        .find(|(name, _)| transport.iter().copied().eq(name.encode_utf16()))
        .map(|(_, word)| *word)
}

/// What the top line names an offline box's category, whatever the box is
/// called: the word for "box" of `transport`, the transport box's name as a
/// box file holds it. `None` for a name that is not one of `BOX_WORD`'s.
pub fn offline_category(
    transport: &[u8; offline_core::boxfile::NAME_TEXT],
) -> Option<[u8; offline_core::boxfile::NAME_TEXT]> {
    let mut units = [0u16; offline_core::boxfile::NAME_TEXT / 2];
    for (unit, pair) in units.iter_mut().zip(transport.chunks_exact(2)) {
        *unit = u16::from_le_bytes([pair[0], pair[1]]);
    }
    let len = units.iter().position(|unit| *unit == 0)?;
    let word = box_word(&units[..len])?;
    let mut name = [0u16; offline_core::boxfile::NAME_TEXT / 2];
    let mut len = 0;
    for unit in word.encode_utf16() {
        *name.get_mut(len)? = unit;
        len += 1;
    }
    offline_core::boxfile::name_text(&name[..len])
}

/// The default name of an offline box, in the pattern of the 100 boxes'
/// ("Bank 37"): the word for "box" (`BOX_WORD`) from `transport`, the
/// transport box's name, then what `default`, a box's default name (39:0x45
/// with its number), has between its word and its number: spaces and a
/// colon, ": " in German, nothing in Japanese, Korean and Chinese, then its
/// number's digits, as the console wrote them. "Box 1" for box 1 in English.
/// `None` for a name that is not one of `BOX_WORD`'s, a default without a
/// number, or a result too long for a name; the texts end at their first
/// zero.
pub fn offline_default(
    default: &[u16],
    transport: &[u16],
) -> Option<[u8; offline_core::boxfile::NAME_TEXT]> {
    let ended = |text: &[u16]| {
        text.iter()
            .position(|unit| *unit == 0)
            .unwrap_or(text.len())
    };
    let (default, transport) = (&default[..ended(default)], &transport[..ended(transport)]);
    let word = box_word(transport)?;
    let digit = |unit: &u16| matches!(unit, 0x30..=0x39 | 0xFF10..=0xFF19);
    let number = default.len() - default.iter().rev().take_while(|unit| digit(unit)).count();
    if number == default.len() {
        return None;
    }
    let separator = |unit: &u16| matches!(unit, 0x20 | 0x3A | 0x3000 | 0xFF1A);
    let between = number
        - default[..number]
            .iter()
            .rev()
            .take_while(|unit| separator(unit))
            .count();
    let mut name = [0u16; offline_core::boxfile::NAME_TEXT / 2];
    let mut len = 0;
    for unit in word
        .encode_utf16()
        .chain(default[between..].iter().copied())
    {
        *name.get_mut(len)? = unit;
        len += 1;
    }
    offline_core::boxfile::name_text(&name[..len])
}

/// The cursor's colour, and what A does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Red: A checks or unchecks a box, for the backup.
    Check,
    /// Blue: A lifts a box and puts it down on another; the two change
    /// places whole.
    Swap,
    /// Green: A lifts a box's Pokémon and puts them into another.
    Move,
}
impl Mode {
    /// START: red, blue, green, red.
    pub fn next(self) -> Self {
        match self {
            Self::Check => Self::Swap,
            Self::Swap => Self::Move,
            Self::Move => Self::Check,
        }
    }
}

/// The place under the cursor's place on the screen: the boxes 0..99, and
/// the four cells behind them (`layout::extra_place`) the transport box
/// and the offline boxes. `None` for the button row.
pub fn place_of(cursor: u32) -> Option<usize> {
    let cursor = cursor as usize;
    match cursor {
        0..=99 => Some(cursor),
        113..=116 => Some(TRANSPORT + (cursor - 113)),
        _ => None,
    }
}

/// The cursor's place on the screen for `place`: what `place_of` reads.
pub fn cursor_of(place: usize) -> u32 {
    if place < TRANSPORT {
        place as u32
    } else {
        (113 + place - TRANSPORT) as u32
    }
}

/// What goes back to the place it came from, with B or refused back
/// (`Refused::Back`), as the box screen takes a Pokémon back (`002876a4`:
/// B, flag `+0x6e9`, and a refusal alike name the slot it came from,
/// `+0x6e7`). While the box screen shows another box than the one it came
/// from, it first turns back to that box (`0028d50c`): the turn is its
/// box-change slide (animation 6 or 7 of the panel's group, `0028fa74`:
/// `box_move01_left01` or `right01`, 11 frames), and state 0x10 sets the
/// glide up, with the put-down sound, only in the frame that finds the
/// slide at its last frame (`00289b14`, `001e9778`), 10 frames later than
/// without a turn. The cursor stays where it is through the turn and the
/// glide; when the put-down state ends, after a refusal's message, it is
/// set onto the slot the Pokémon went back to (`0028d800`: `001da7a8` finds
/// the slot's place and `001e6620` sets the cursor).
///
/// Here the upper screen is what shows a box. With the blue hand it shows
/// the held box all along: no turn. With the green hand it shows the box
/// under the cursor: unless that is the box the Pokémon came from, the
/// upper screen turns back to it, showing it with them over it, and the
/// glide is set up `TURN_FRAMES` later. This screen's upper layout loads no
/// slide (its animations are `box_name1`, `box_moving` and `box_change`,
/// `0033d89c`), so the box only changes; the frames are the box screen's.
/// With either hand the cursor is set onto the place at the glide's end.
/// The box shown is `shown` (`shown_place`): as the box screen compares the
/// box it shows with the one they came from (`00287730`), there is no turn
/// where it shows that box already, as on the button row, where it shows
/// their box (`shown_place`).
pub fn turns_back(mode: Mode, put: Put, shown: usize) -> bool {
    mode == Mode::Move && put.to == put.from && shown != put.from
}
/// The place the upper screen shows for the cursor `cursor`: its place;
/// on the button row, where the original's preview shows box 100 (it clamps
/// the cursor to box index 99, `00285e78`), the box the green hand's
/// Pokémon came from while it holds them (`green`), else the last place
/// shown (`last`): a deliberate difference, so that the upper screen does
/// not change when the cursor only goes to the buttons.
pub fn shown_place(cursor: u32, last: usize, green: Option<usize>) -> usize {
    place_of(cursor).unwrap_or(green.unwrap_or(last))
}
/// Whether the original's own drawing of the upper screen after RESET and
/// ALL is right for the cursor `cursor`: on a box of the Bank only. It
/// draws box 100's check for any other place, while the upper screen shows
/// a cell, or on the button row the box `shown_place` names, so the patch
/// draws it again there.
pub fn original_draws(cursor: u32) -> bool {
    place_of(cursor).is_some_and(|place| place < TRANSPORT)
}
/// The frames from the turn back to the glide's set-up.
pub const TURN_FRAMES: u8 = 10;
/// Whether the turn back is over, `frames` after it began.
pub fn turned(frames: u8) -> bool {
    frames >= TURN_FRAMES
}
/// The place the upper screen shows a box for (`upper`): the cursor's, or
/// while what is held goes back after a turn (`back`), the place it came
/// from.
pub fn upper_cursor(held: Option<usize>, back: bool, cursor: usize) -> usize {
    match held {
        Some(held) if back => held,
        _ => cursor,
    }
}

/// What the upper screen shows while `held` is lifted with `mode` and the
/// cursor is on `cursor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Upper {
    /// The box shown, with its name and its check.
    pub place: usize,
    /// Its Pokémon are lifted out of it: its slots show empty.
    pub emptied: bool,
    /// The box whose Pokémon are shown lifted over it.
    pub lifted: Option<usize>,
}
/// The blue cursor holds a box as it is: the upper screen shows the held
/// box until it is put down. The green cursor holds a box's Pokémon: the
/// upper screen shows the box under the cursor, as with nothing held, and
/// the held box's Pokémon lifted over it; over their own box, that box
/// shows empty, as they are out of it.
pub fn upper(mode: Mode, held: Option<usize>, cursor: usize) -> Upper {
    match (mode, held) {
        (Mode::Swap, Some(held)) => Upper {
            place: held,
            emptied: false,
            lifted: None,
        },
        (Mode::Move, Some(held)) => Upper {
            place: cursor,
            emptied: cursor == held,
            lifted: Some(held),
        },
        _ => Upper {
            place: cursor,
            emptied: false,
            lifted: None,
        },
    }
}

/// The state the upper screen's box icon shows for `upper` (animation 2 of
/// group 0): empty while its Pokémon are lifted out of it, else `state`.
pub fn upper_state(upper: Upper, state: u8) -> u8 {
    if upper.emptied {
        0
    } else {
        state
    }
}

/// The upper screen's name label is blue while it shows the transport box,
/// as the box screen's is (its `box_name2`), and beige otherwise.
pub fn blue_label(upper: Upper) -> bool {
    upper.place == TRANSPORT
}

/// What the top line names: a group of the Bank, or the transport box or an
/// offline box, each under its own name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Of {
    Group(u8),
    Place(usize),
}

/// The lower screen's top line, laid out as the box screen's group label
/// (`layout::TITLE`): the title "Boxenauswahl" (38:0x37) with the count of
/// checked boxes, or "Gruppe: " (38:0x00), a name, and a box's place in its
/// group as "index/amount" (38:0x3A with its 100 turned into the amount and
/// its brackets taken off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Title,
    Group { of: Of, index: u16, amount: u16 },
}

/// The line for `mode`, what is `held` and the place under the `cursor`
/// (`None` on the button row); `group` gives a box of Bank's group and
/// position in it, `amount` how many boxes a group has. The red hand, and
/// any hand on the button row, shows the title. The blue and the green
/// hand show the box under the cursor: a box of Bank by its group, the
/// transport box as 1/1, offline box k as k/3. A box the blue hand
/// holds shows the group and position it would have where the cursor is,
/// by the swap's rules (`Places::swap`): of the same group the two change
/// positions, of another group each keeps its own, and a box of Bank's
/// place keeps its group for an offline box put there. A box with a
/// category the Bank has no group for shows the title.
pub fn line(
    mode: Mode,
    held: Option<usize>,
    cursor: Option<usize>,
    group: impl Fn(usize) -> Option<(u8, u16)>,
    amount: impl Fn(u8) -> u16,
) -> Line {
    let Some(place) = cursor.filter(|_| mode != Mode::Check) else {
        return Line::Title;
    };
    if place == TRANSPORT {
        return Line::Group {
            of: Of::Place(place),
            index: 1,
            amount: 1,
        };
    }
    if place >= OFFLINE {
        return Line::Group {
            of: Of::Place(place),
            index: (place - OFFLINE + 1) as u16,
            amount: (PLACES - OFFLINE) as u16,
        };
    }
    let shown = match (mode, held.and_then(&group), group(place)) {
        (Mode::Swap, Some(own), Some(there)) if own.0 != there.0 => Some(own),
        (_, _, there) => there,
    };
    match shown {
        Some((group, position)) => Line::Group {
            of: Of::Group(group),
            index: position + 1,
            amount: amount(group),
        },
        None => Line::Title,
    }
}

/// L held, as the box screen counts it: its key reading (`002918e4`) keeps
/// a word at `+0x18` that counts the frames L is held. In the first three
/// it reads no other key and no touch; in the fourth, L still held, L fires
/// (its action 6, `00288750`), so a press of three frames or less does
/// nothing; after that L fires whenever the pad's "pressed or repeat" test
/// (`001e9c78`) is true, and in every other frame the keys and touch are
/// read as usual (`00291a5c`). Letting go starts it again. The box screen
/// counts R too, for L and R together (action 8); here R is ALL, so only L
/// is counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shoulder {
    frames: u8,
    fired: bool,
    /// The last step was `Hold` or `Fire`.
    blocks: bool,
}

/// What a frame does with L (`Shoulder::step`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShoulderKey {
    /// L is not held: every key as usual.
    Idle,
    /// L is held, in its first three frames: no other key and no button
    /// does anything this frame.
    Hold,
    /// L fires: no other key and no button does anything this frame.
    Fire,
    /// L is held between its fires: every key as usual.
    Free,
}

impl Shoulder {
    pub const fn new() -> Self {
        Self {
            frames: 0,
            fired: false,
            blocks: false,
        }
    }

    /// The frame's step, from whether L is `held` and whether the pad's
    /// "pressed or repeat" test is true for it (`again`).
    pub fn step(&mut self, held: bool, again: bool) -> ShoulderKey {
        let key = if !held {
            *self = Self::new();
            ShoulderKey::Idle
        } else if !self.fired {
            if self.frames < 3 {
                self.frames += 1;
                ShoulderKey::Hold
            } else {
                self.fired = true;
                ShoulderKey::Fire
            }
        } else if again {
            ShoulderKey::Fire
        } else {
            ShoulderKey::Free
        };
        self.blocks = matches!(key, ShoulderKey::Hold | ShoulderKey::Fire);
        key
    }

    /// The last step read no other key and no button (`Hold`, `Fire`).
    pub fn blocks(&self) -> bool {
        self.blocks
    }
}

/// The position L inserts a box at in a group of `amount` boxes: one place
/// before, the first at the end (`Changes::insert` moves the boxes between
/// to make room).
pub fn lowered(position: u16, amount: u16) -> u16 {
    match position {
        0 => amount.saturating_sub(1),
        _ => position - 1,
    }
}

/// Turns the last "100" in the `len` units of `text` (38:0x3A, "({n}/100)"
/// in every language) into `total`, up to three digits: 104 for the count
/// of checked boxes, a group's amount for its line. The new length; the
/// text keeps its end.
pub fn out_of(text: &mut [u16], len: usize, total: u16) -> Option<usize> {
    let hundred = [u16::from(b'1'), u16::from(b'0'), u16::from(b'0')];
    let at = (0..=len.checked_sub(3)?)
        .rev()
        .find(|&at| text.get(at..at + 3) == Some(&hundred[..]))?;
    if total == 0 || total > 999 {
        return None;
    }
    let mut digits = [0u16; 3];
    let mut count = 0;
    let mut rest = total;
    while rest > 0 {
        digits[count] = u16::from(b'0') + rest % 10;
        rest /= 10;
        count += 1;
    }
    digits[..count].reverse();
    text.get_mut(at..at + count)?
        .copy_from_slice(&digits[..count]);
    // The rest of the text, its end included, follows the digits.
    text.get(len)?;
    text.copy_within(at + 3..=len, at + count);
    Some(len + count - 3)
}

/// Takes the brackets off the `len` units of a count (38:0x3A after
/// `out_of`, "(n/x)", full-width "（n/x）" in Japanese and Chinese), so
/// that a group's line shows "n/x", as the box screen shows its number
/// plain. The new length; the text keeps its end. `None` for a text that
/// is not in brackets.
pub fn unbracketed(text: &mut [u16], len: usize) -> Option<usize> {
    let pairs = [(0x28, 0x29), (0xff08, 0xff09)];
    let (first, last) = (*text.first()?, *text.get(len.checked_sub(1)?)?);
    if len < 2 || !pairs.contains(&(first, last)) {
        return None;
    }
    text.get(len)?;
    text.copy_within(1..len - 1, 0);
    text.copy_within(len..=len, len - 2);
    Some(len - 2)
}

/// The frame a place on the lower screen shows (its state, 0..2), or
/// `None` while it is hidden. The blue cursor lifts a box whole, so its
/// place shows nothing until it is put down; the green cursor lifts its
/// Pokémon, so it shows empty.
pub fn place_frame(mode: Mode, held: Option<usize>, place: usize, state: u8) -> Option<u8> {
    match (mode, held == Some(place)) {
        (Mode::Swap, true) => None,
        (_, true) => Some(0),
        _ => Some(state),
    }
}

/// How far under the hand the box screen holds what it lifts (`0028d3c4`,
/// the float at `0028d508`): it is placed at the hand's height in the
/// cursor's pane less this (`001d8c60`), on every frame of the lift's rise
/// (`0028d4fc`) and when the cursor moves (`001da954`, `0028c644`). The
/// hand stands at its rest (20) once the lift is over, so what is held lies
/// 10 over its place until it is put down.
pub const HELD_UNDER_HAND: f32 = 10.0;
pub fn held_height(hand: f32) -> f32 {
    hand - HELD_UNDER_HAND
}

/// The box screen's lift (`0028d300`, its state 0x0F), run from the frame
/// after A, each step before the layout's update of its frame
/// (`before_update`). That update first moves every animation on by a
/// frame (`00222490`) and then sets the panes from it (`001e4af4`), so an
/// animation started in a step shows its second frame in that frame. The
/// lift stops the bob (`0028d358`) and plays `cursol_move_duwn`
/// (`0028d370`, its file `0x21`): the hand shows (17, 10) at once. In the
/// next frame that animation is at its last frame (`001e9778`,
/// `00229ca4`), so the lift takes what it lifts out of its place into the
/// held set with its shadow (`0029099c`, `00290dbc`), places it under the
/// hand as the hand stands before the update, on its place, and plays
/// `cursol_move_up` (file `0x22`), which shows the hand at (17, 20). In the
/// next it places what is held under the hand at rest, 10 over its place,
/// and the lift is over (`0028d4e0`, state 4); keys are taken from the
/// frame after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiftFrame {
    /// The bob stops and the hand dips (`layout::HAND_LOWERED`).
    Dip,
    /// What is lifted is taken under the dipped hand, and the hand is back
    /// at its rest (`layout::HAND_AT`).
    Take,
    /// What is held is placed under the hand at rest.
    Rise,
    /// The lift is over.
    Done,
}
/// What the `frames`th frame after A does.
pub fn lifting(frames: u8) -> LiftFrame {
    match frames {
        0 | 1 => LiftFrame::Dip,
        2 => LiftFrame::Take,
        3 => LiftFrame::Rise,
        _ => LiftFrame::Done,
    }
}

/// The box screen's put-down and put-back. A on a place what is held can
/// go (`0028dfac`) and B (`002886cc`), both with no sound,
/// switch to its state 0x10 (`00289f2c`), which runs in the next frame: it
/// sets up the glide (`002876a4`; `00290388`, `002912ec`, one entry each by
/// `001d88cc`), plays `0x50007` and hides the shadow (`0028a068`), and
/// switches to state 0x11 (`0028d5fc`). That moves each icon from where it
/// stands to its slot in a straight line, an eighth of the way per frame
/// (`0.125`), for 8 frames from the next (`0028ebe8`, its counter at
/// `+0x24f6`); on the next it sets each onto its slot exactly, and on the
/// one after that the screen lays its icons out again from the data
/// (`0028f424`) and the hand bobs again (`0028d790`). The held icon goes to
/// the slot it is put into; one that was in that slot (`00290778`), moved
/// to the end of the layout's top pane (`001d8a14`), to the held one's own
/// slot. The patch draws that one under the held box (`glide`). No key is
/// taken from A or B to the end. Each step runs before the layout's update
/// of its frame (`before_update`).
pub const GLIDE_STEPS: u8 = 8;
/// A glide of one icon: where it is, where it goes, and its step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    pub at: (f32, f32),
    pub end: (f32, f32),
    step: (f32, f32),
}
impl Glide {
    pub fn new(start: (f32, f32), end: (f32, f32)) -> Self {
        Self {
            at: start,
            end,
            step: ((end.0 - start.0) * 0.125, (end.1 - start.1) * 0.125),
        }
    }
    /// One frame of the glide (`landing`): a step, or onto the end.
    pub fn frame(&mut self, step: LandingFrame) {
        match step {
            LandingFrame::Step => {
                self.at.0 += self.step.0;
                self.at.1 += self.step.1;
            }
            LandingFrame::Land => self.at = self.end,
            LandingFrame::Done => {}
        }
    }
}
/// What the `frames`th frame after the key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingFrame {
    Step,
    Land,
    Done,
}
pub fn landing(frames: u8) -> LandingFrame {
    match frames {
        frames if frames <= GLIDE_STEPS => LandingFrame::Step,
        frames if frames == GLIDE_STEPS + 1 => LandingFrame::Land,
        _ => LandingFrame::Done,
    }
}

/// DONE (Y)'s question: the original's question, which decides what Yes
/// does, and the text its bar shows. The screen has two questions: Yes to
/// 38:0x38 goes on and writes (`writes`), Yes to 38:0x39 quits, and No to
/// either closes it (`00286994`). With a box checked, 38:0x38 showing "Move
/// these Pokémon?" (38:0x38 without its words about Pokémon HOME,
/// `MOVE_QUESTION`); with none, 38:0x38 showing "Would you like to save and
/// quit?" (38:0x20) when something `changed` (a put-down of the green hand,
/// or a place's bytes differ from when the screen opened, `Changes::any`),
/// and 38:0x39 with its own text,
/// "Quit moving Pokémon?", otherwise.
pub fn done(checked: bool, changed: bool) -> (u32, u32) {
    match (checked, changed) {
        (true, _) => (MOVE_TO_HOME, MOVE_TO_HOME),
        (false, true) => (MOVE_TO_HOME, SAVE_AND_QUIT),
        (false, false) => (QUIT_MOVING, QUIT_MOVING),
    }
}

/// B with nothing held: the original asks its 38:0x39 (`00285c14`), whose
/// Yes quits without writing, and its bar shows "Would you like to quit
/// without saving?" (38:0x21) instead of "Quit moving Pokémon?", whatever
/// changed.
pub const BACK_TEXT: u32 = QUIT_WITHOUT_SAVING;

/// Whether Yes to the original's `question` writes: Yes to 38:0x38.
pub fn writes(question: u32) -> bool {
    question == MOVE_TO_HOME
}

/// What A does on a place the held box cannot go, as the box screen does
/// with a Pokémon it holds (`0028dfc8`: the result of `0028af74`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The refusal sound on the key, and it stays in the hand (the box
    /// screen's results 8 and 9, `0028e000`).
    Stays,
    /// It goes back to its place as B puts it back, with no sound on the
    /// key and the put-down sound in the frame after; once it is back, the
    /// refusal sound and the message, which stands until A or B (the box
    /// screen's result 5 for the transport box: state 0x10 glides it back,
    /// and the glide's end plays `0x5000E` and shows message 10 of
    /// `001d9f70`, 38:0x23, `0028d91c`).
    Back,
}
/// Nothing goes into the transport box: what is put onto it goes back with
/// "Cannot be transported there." (38:0x23). Anything else stays in the
/// hand.
pub fn refused(refusal: Refusal) -> Refused {
    match refusal {
        Refusal::IntoTransport => Refused::Back,
        _ => Refused::Stays,
    }
}

/// What task `0x1B`'s frames do before the boxes. Its notice is skipped;
/// the question is the one the original asks when Bank opens without a
/// game (task 3, states `0x18`, `0x21`..`0x25`), with the same calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// The question's text goes up: page 3 of message 0, "Do you still
    /// want to move the Pokémon in Pokémon Bank to Pokémon HOME?".
    Ask,
    /// The text is written out; then the buttons "Begin the move" and
    /// "Cancel" (messages `0x59`, `0x5A`) are labelled.
    Question,
    /// The buttons slide in; then they take a press.
    ButtonsIn,
    /// The task's listener answers a press with state 5 or 6
    /// (`button_pressed`).
    Waiting,
    /// The buttons slide out; then the loading panel (Begin the move) or
    /// the start screen (Cancel).
    ButtonsOut { begin: bool },
    /// The loading panel stands while the offline boxes are read; `first`
    /// on its first frame.
    Loading { first: bool },
    /// The original's update runs the boxes (its state 9).
    Boxes,
}

/// The button the task's listener (`002d0a38`) answered with the task's
/// state: 5 for the first, "Begin the move", 6 for "Cancel".
pub fn button_pressed(task_state: u32) -> Option<bool> {
    match task_state {
        5 => Some(true),
        6 => Some(false),
        _ => None,
    }
}

/// Whether the question's text is written out: the dialog's frame answers
/// 4 or 0 (`001d6600`), as task 3's state `0x21` waits for.
pub fn written_out(poll: u32) -> bool {
    matches!(poll, 0 | 4)
}

/// The session's state while the screen is up.
pub struct Session {
    /// Task `0x1B` the session belongs to: a new task starts a new one.
    pub task: usize,
    /// The frames before the boxes.
    pub opening: Opening,
    pub mode: Mode,
    /// The place lifted with A, blue or green.
    pub held: Option<usize>,
    /// The checks of the transport box and the offline boxes; the boxes'
    /// own are the original's.
    pub extra: [bool; PLACES - TRANSPORT],
    /// What the moves changed (`Changes`).
    pub changes: Changes,
    /// The four cells show what they hold.
    pub cells_shown: bool,
    /// What Yes asked to write, taken when it was answered: the checked
    /// places.
    pub commit: Option<[bool; PLACES]>,
    /// A refusal's message is up, until A or B.
    pub notice: bool,
    /// The hand does not bob: something is held.
    pub lifted: bool,
    /// The height of what is held in the cursor's pane (`held_height`),
    /// and the Pokémon the green cursor holds over their slots.
    pub held_y: f32,
    /// While a lift runs: the task's frames run since A (`lifting`) and
    /// the place lifted.
    pub lifting: Option<(u8, usize)>,
    /// A put-down or put-back from A or B, set up in the next frame, or
    /// after the turn back (`turns_back`).
    pub put: Option<Put>,
    /// The frames since the turn back began, while it runs.
    pub turn: Option<u8>,
    /// The upper screen shows the box what is held goes back to
    /// (`upper_cursor`), from the turn back to the glide's end.
    pub back: bool,
    /// While what was put down or back glides into place (`GLIDE_STEPS`).
    pub landing: Option<Landing>,
    /// L held (`Shoulder`), counted with every hand and while a box is held.
    pub shoulder: Shoulder,
    /// The frames left of the box screen's page turn after an L that moved
    /// a box (`TURN_FRAMES`), in which no key does anything; counted down
    /// at the start of every frame (`new_frame`).
    pub paging: u8,
    /// This frame is one of the page turn's.
    pub turning: bool,
    /// The key update ran in this frame. Bank skips it in a frame without
    /// any key held (`0025d76c`..`0025d780`), so a frame without it is one
    /// with L let go.
    pub keyed: bool,
    /// The last place the cursor was on, which the upper screen keeps
    /// showing while the cursor is on the button row (`shown_place`).
    pub last_shown: usize,
}

/// A put-down or put-back waiting for its frame (`GLIDE_STEPS`): from where
/// and to where, whether the box there changes places with it, and whether
/// it goes back refused (`Refused::Back`), with its message once it is back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Put {
    pub from: usize,
    pub to: usize,
    pub swapped: bool,
    pub refused: bool,
}

/// A put-down or a put-back gliding into place: the data is already
/// moved; the screen shows the move and lays the places out at the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    /// The task's frames run since it was set up (`landing`).
    pub frames: u8,
    /// Where the held box came from and where it goes.
    pub from: usize,
    pub to: usize,
    /// The held box in the cursor's pane.
    pub held: Glide,
    /// The box that was where it goes, on its way to `from`.
    pub moved: Option<Moved>,
    /// How high the green cursor's Pokémon lie over their slots on the
    /// upper screen, down to 0.
    pub lifted: Option<Glide>,
    /// It goes back refused: its message is shown once it is back.
    pub refused: bool,
}
/// A place's panes on their way to another place, in the layout's top pane:
/// the panes, how many, and where they lie in their own parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    pub panes: [u32; 3],
    pub count: usize,
    pub home: [f32; 3],
    pub z: f32,
    pub glide: Glide,
}
impl Session {
    pub const fn new() -> Self {
        Self {
            task: 0,
            opening: Opening::Ask,
            mode: Mode::Check,
            held: None,
            extra: [false; PLACES - TRANSPORT],
            changes: Changes::new(),
            cells_shown: false,
            commit: None,
            notice: false,
            lifted: false,
            held_y: crate::layout::HELD_AT.1,
            lifting: None,
            put: None,
            turn: None,
            back: false,
            landing: None,
            shoulder: Shoulder::new(),
            paging: 0,
            turning: false,
            keyed: false,
            last_shown: 0,
        }
    }
    /// L is in its first three frames or fires, or its page turn runs: no
    /// other key and no button does anything.
    pub fn paging_keys(&self) -> bool {
        self.turning || self.shoulder.blocks()
    }
    /// A lift runs, or what was put down or back waits for its frame or
    /// glides into place: no key does anything (`bank_transfer_input`).
    pub fn moving(&self) -> bool {
        self.lifting.is_some() || self.put.is_some() || self.landing.is_some()
    }
    /// Nothing is lifted, held or moving and no refusal's message is up:
    /// DONE, RESET and ALL can do something.
    pub fn free(&self) -> bool {
        !self.moving() && self.held.is_none() && !self.notice
    }
    /// Which of the screen's five buttons take a press (`001e22c4`,
    /// `001e23e8`), with a question up or not: DONE, RESET and ALL (0..2)
    /// while the screen is free (`free`) and no question is up; Yes and No
    /// (3, 4) only while a question is up. So a touch where a button lies
    /// under the question, or hidden, does nothing.
    pub fn button_takes_presses(&self, button: u32, question: bool) -> bool {
        match button {
            0..=2 => self.free() && !question,
            _ => question,
        }
    }
    /// Whether the buttons' update (`bank_transfer_event`) goes on: while
    /// the screen is free (`free`) and L blocks no key (`paging_keys`).
    pub fn takes_events(&self) -> bool {
        self.free() && !self.paging_keys()
    }
    /// The start of every frame, also of one without a key, as the box
    /// screen runs its state 4 and its page turn every frame (`002918e4`,
    /// state 0xE): the page turn goes on by a frame whatever is pressed,
    /// and after a frame without a key L is let go and counts anew.
    pub fn new_frame(&mut self) {
        self.turning = self.paging != 0;
        self.paging = self.paging.saturating_sub(1);
        if !self.keyed {
            self.shoulder = Shoulder::new();
        }
        self.keyed = false;
    }
    /// Task `0x1B` has ended and the router answered `destination`: the next
    /// task `0x1B` starts a new session, also at the same address. What Yes
    /// asked to write is kept only for Save and Quit's task right after it,
    /// which takes it (`take_commit`); with any other task it is dropped,
    /// so that no later Save and Quit, with a game, finds it.
    pub fn ended(&mut self, destination: u32) {
        self.task = 0;
        if destination != crate::navigation::SAVE {
            self.commit = None;
        }
    }
}
impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// The outcome task `0x1B` ends with when Yes asked to write: the router
/// answers Save and Quit's task (`navigation`). Quit leaves its own, 3.
pub const OUTCOME_WRITE: u8 = 4;

#[cfg(target_arch = "arm")]
pub use console::*;

#[cfg(target_arch = "arm")]
mod console {
    use super::*;
    use crate::{
        layout::{self, CELLS, HAND, HANDS, HELD},
        storage_worker::ROOM,
    };
    use bank_common::{
        native_bank::NativeBank,
        transfer_screen::{self, Places},
    };
    use core::{arch::global_asm, mem::transmute};
    use patch_common::native::{code, follow, pointer, read_word, write_word};

    /// `001e7084(context, group, pane, shown)`.
    const PANE_SHOWN: usize = 0x001e_7084;
    /// `001db7e0(selection, message)`: the question overlay with Yes and No.
    const QUESTION: usize = 0x001d_b7e0;
    /// The pad's tests, as the box screen's key reading uses them: each
    /// reads one word of the pad's entry for `mask`. `001e9cc8(pad, mask,
    /// 0)` whether a key was pressed now (word 4), `001e754c` whether it is
    /// held (word 0), `001e9c78` whether it was pressed now or repeats
    /// (words 4 and 0xC), at the pad's own repeat.
    const PRESSED: usize = 0x001e_9cc8;
    const HOLDING_KEY: usize = 0x001e_754c;
    const AGAIN: usize = 0x001e_9c78;
    /// The selection object's update for a key (`00285ba8`) and for its
    /// buttons and answers (`0028672c`), and task `0x1B`'s update
    /// (`002a7bf0`), whose vtable slots the entries take.
    const SELECTION_INPUT: usize = 0x0028_5ba8;
    const SELECTION_EVENT: usize = 0x0028_672c;
    const TRANSFER_UPDATE: usize = 0x002a_7bf0;
    /// The parser of a layout (`0024f1f8`), and the maker of the cursor's
    /// places (`001e663c`).
    const LAYOUT_BUILD: usize = 0x0024_f1f8;
    const MAKE_CURSOR: usize = 0x001e_663c;
    /// The cursor's places as the original has them.
    const PLACES_TABLE: usize = 0x0033_77fc;
    /// Where the original keeps the manager: `[[manager] + 0x1c] + 0xcc` is
    /// the Bank object.
    const MANAGER: usize = 0x003a_b90c;
    /// The layout group of the screen's lower layout.
    const GROUP: u32 = 1;

    // The original sets a cell's state as the frame of its animation:
    // `00230b2c(context, group, animation, 0)` with the frame in `s0`. The
    // patch is built without floating-point registers, so this shim moves
    // the frame's bits there.
    global_asm!(
        ".syntax unified",
        ".arm",
        ".fpu vfp",
        ".section .text.bank_transfer_frame, \"ax\", %progbits",
        ".align 2",
        ".global bank_transfer_frame",
        ".type bank_transfer_frame, %function",
        "bank_transfer_frame:",
        "vmov s0, r3",
        "mov r3, #0",
        "ldr r12, =0x00230b2c",
        "bx r12",
        ".ltorg",
        ".size bank_transfer_frame, .-bank_transfer_frame",
    );
    // The screen calls its key and button updates through the selection
    // object's second table (`00360754`, the object at `+0x74`), whose
    // entries `00285ba0` and `00286724` subtract `0x74` and fall through
    // into `00285ba8` and `0028672c`. These do the same for the entries.
    global_asm!(
        ".syntax unified",
        ".arm",
        ".section .text.bank_transfer_input_thunk, \"ax\", %progbits",
        ".align 2",
        ".global bank_transfer_input_thunk",
        ".type bank_transfer_input_thunk, %function",
        "bank_transfer_input_thunk:",
        "sub r0, r0, #0x74",
        "b bank_transfer_input",
        ".size bank_transfer_input_thunk, .-bank_transfer_input_thunk",
        ".section .text.bank_transfer_event_thunk, \"ax\", %progbits",
        ".align 2",
        ".global bank_transfer_event_thunk",
        ".type bank_transfer_event_thunk, %function",
        "bank_transfer_event_thunk:",
        "sub r0, r0, #0x74",
        "b bank_transfer_event",
        ".size bank_transfer_event_thunk, .-bank_transfer_event_thunk",
    );
    unsafe extern "aapcs" {
        /// # Safety
        /// Only the second table's slots may enter them, with the object at
        /// `+0x74` and the original's arguments.
        pub fn bank_transfer_input_thunk();
        pub fn bank_transfer_event_thunk();
        fn bank_transfer_frame(context: *mut u8, group: u32, animation: u32, frame: u32);
    }

    // SAFETY: the main thread alone, as every hook's state.
    static SESSION: patch_common::exclusive::Exclusive<Session> =
        unsafe { patch_common::exclusive::Exclusive::new(Session::new()) };

    /// The cursor's extended places, built once, and the name the preview
    /// shows for an offline box. The extended layout is built at every build
    /// of the screen's layout, into `ROOM`. What the original reads is
    /// aligned as its own data is: the cursor keeps a pointer to the places
    /// and reads their words.
    #[repr(C, align(8))]
    struct Built {
        places: [u8; layout::EXTENDED_PLACES * 8],
        name: [u16; NAME_UNITS],
        places_made: bool,
        /// The place whose Pokémon lie on the spare icon set (`lift`), and
        /// their icons.
        lifted: Option<usize>,
        icons: [Icon; 30],
    }

    /// An icon of the upper screen as the preview keeps one for each slot
    /// (`[selection + 0xf8]`, `0x24` bytes): the key of the icon on the pane
    /// (`+0x00`), the key of the one asked for next (`+0x10`), whose first
    /// half-word is the species, 0 for none, and whether it waits for the
    /// icon manager (`+0x20`). A key's word at `+0xc` goes to the manager's
    /// calls with it.
    #[repr(C, align(4))]
    #[derive(Clone, Copy)]
    struct Icon([u8; 0x24]);
    /// The icon manager (`[selection + 0xf0]`): `001db6dc(manager, key,
    /// word, 0)` whether an icon is loaded, `001db4f0` asks for it,
    /// `001db624(manager, key, word, pane, 0, 0, 0)` puts it on a pane,
    /// `001e67b4(manager, pane, 0, 0)` takes it off. The Pokémon object's
    /// calls the preview makes a key with: `0022bc70` before and `0022a8b0`
    /// after, `00220e2c` the species, `00220df4` the form, `002c978c`,
    /// `00220430(pokemon, 2)` and `002c9520`.
    const ICON_LOADED: usize = 0x001d_b6dc;
    const ICON_ASK: usize = 0x001d_b4f0;
    const ICON_PUT: usize = 0x001d_b624;
    const ICON_OFF: usize = 0x001e_67b4;
    const POKEMON_OPEN: usize = 0x0022_bc70;
    const POKEMON_CLOSE: usize = 0x0022_a8b0;
    const POKEMON_SPECIES: usize = 0x0022_0e2c;
    const POKEMON_FORM: usize = 0x0022_0df4;
    const POKEMON_ICON_3: usize = 0x002c_978c;
    const POKEMON_ICON_5: usize = 0x0022_0430;
    const POKEMON_ICON_WORD: usize = 0x002c_9520;
    impl Icon {
        const NEXT: usize = 0x10;
        const WAITING: usize = 0x20;
        fn species(&self, at: usize) -> u16 {
            u16::from_le_bytes([self.0[at], self.0[at + 1]])
        }
        fn word(&self, at: usize) -> u32 {
            u32::from_le_bytes(self.0[at + 0xc..at + 0x10].try_into().unwrap_or([0; 4]))
        }
        fn loaded(&self, manager: *mut u8, at: usize) -> bool {
            let call: unsafe extern "aapcs" fn(*mut u8, *const u8, u32, u32) -> u32 =
                unsafe { transmute(ICON_LOADED) };
            unsafe { call(manager, self.0.as_ptr().add(at), self.word(at), 0) != 0 }
        }
        fn ask(&self, manager: *mut u8, at: usize) {
            let call: unsafe extern "aapcs" fn(*mut u8, *const u8, u32, u32) -> u32 =
                unsafe { transmute(ICON_ASK) };
            unsafe { call(manager, self.0.as_ptr().add(at), self.word(at), 0) };
        }
        fn put(&self, manager: *mut u8, pane: *mut u8) {
            let call: unsafe extern "aapcs" fn(*mut u8, *const u8, u32, *mut u8, u32, u32, u32) =
                unsafe { transmute(ICON_PUT) };
            unsafe { call(manager, self.0.as_ptr(), self.word(0), pane, 0, 0, 0) };
        }
        fn off(manager: *mut u8, pane: *mut u8) {
            let call: unsafe extern "aapcs" fn(*mut u8, *mut u8, u32, u32) =
                unsafe { transmute(ICON_OFF) };
            unsafe { call(manager, pane, 0, 0) };
        }
        /// The next key goes to the current one.
        fn advance(&mut self) {
            self.0.copy_within(Self::NEXT..Self::NEXT + 0x10, 0);
            self.0[Self::NEXT..Self::NEXT + 2].fill(0);
        }
        /// The next key: of the Pokémon in `pokemon` (`00286458`..`002864b0`).
        fn key(&mut self, pokemon: *mut u8) {
            unsafe {
                let open: unsafe extern "aapcs" fn(*mut u8) -> u32 = transmute(POKEMON_OPEN);
                let close: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(POKEMON_CLOSE);
                type Get = unsafe extern "aapcs" fn(*mut u8) -> u32;
                let get: [Get; 4] = [
                    transmute::<usize, Get>(POKEMON_SPECIES),
                    transmute::<usize, Get>(POKEMON_FORM),
                    transmute::<usize, Get>(POKEMON_ICON_3),
                    transmute::<usize, Get>(POKEMON_ICON_WORD),
                ];
                let fifth: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
                    transmute(POKEMON_ICON_5);
                let state = open(pokemon);
                let next = &mut self.0[Self::NEXT..Self::NEXT + 0x10];
                next[..2].copy_from_slice(&(get[0](pokemon) as u16).to_le_bytes());
                next[2] = get[1](pokemon) as u8;
                next[3] = get[2](pokemon) as u8;
                next[5] = fifth(pokemon, 2) as u8;
                next[4] = 0;
                next[8..12].fill(0);
                next[12..16].copy_from_slice(&get[3](pokemon).to_le_bytes());
                close(pokemon, state);
            }
        }
        /// Shows the next key on `pane`, as the preview does for a slot
        /// (`002864d0`..`00286600`): hidden without a Pokémon; else its
        /// icon if loaded, or none until it is (`frame`).
        fn show(&mut self, manager: *mut u8, pane: *mut u8) {
            if self.species(Self::NEXT) == 0 {
                visible(pane, false);
                return;
            }
            visible(pane, true);
            if self.species(0) != 0 && !self.loaded(manager, 0) {
                Self::off(manager, pane);
                self.0[Self::WAITING] = 1;
                return;
            }
            self.advance();
            if self.loaded(manager, 0) {
                self.put(manager, pane);
            } else {
                Self::off(manager, pane);
                self.ask(manager, 0);
                self.0[Self::WAITING] = 1;
            }
        }
        /// A frame of the preview's update for a waiting icon
        /// (`00286b04`..`00286ba8`).
        fn frame(&mut self, manager: *mut u8, pane: *mut u8) {
            if self.0[Self::WAITING] == 0 || !self.loaded(manager, 0) {
                return;
            }
            if self.species(Self::NEXT) != 0 {
                self.ask(manager, Self::NEXT);
                self.advance();
            } else {
                self.put(manager, pane);
                self.0[Self::WAITING] = 0;
            }
        }
    }
    /// Room for a box name: 18 UTF-16 units and its end.
    const NAME_UNITS: usize = 0x13;
    struct BuiltCell(core::cell::UnsafeCell<Built>);
    // SAFETY: the main thread alone.
    unsafe impl Sync for BuiltCell {}
    static BUILT: BuiltCell = BuiltCell(core::cell::UnsafeCell::new(Built {
        places: [0; layout::EXTENDED_PLACES * 8],
        name: [0; NAME_UNITS],
        places_made: false,
        lifted: None,
        icons: [Icon([0; 0x24]); 30],
    }));

    fn shown(context: *mut u8, pane: u32, on: bool) {
        let call: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
            unsafe { transmute(PANE_SHOWN) };
        unsafe { call(context, GROUP, pane, u32::from(on)) };
    }
    fn ask(selection: *mut u8, message: u32) {
        let call: unsafe extern "aapcs" fn(*mut u8, u32) = unsafe { transmute(QUESTION) };
        unsafe { call(selection, message) };
    }
    fn sound(id: u32) {
        unsafe { bank_common::ui::sound(id) };
    }

    /// `001e21c8(context, group, pane, message)`: a message of the screen
    /// into a pane.
    const INTO_PANE: usize = 0x001e_21c8;
    /// The question's overlay `mi`, its Yes and No window `mk`, and the text
    /// in its bar `my`.
    const OVERLAY: u32 = layout::id(b"mi");
    const ANSWERS: u32 = layout::id(b"mk");
    const BAR_TEXT: u32 = layout::id(b"my");

    /// A refusal: its message in the question's bar, without Yes and No,
    /// as `001db7e0` shows a question (the cursor hidden, the overlay
    /// shown), until A or B closes it (`close_notice`).
    fn notice(session: &mut Session, selection: *mut u8, message: u32) {
        let Some(context) = context(selection) else {
            return;
        };
        let into: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
            unsafe { transmute(INTO_PANE) };
        unsafe { into(context, GROUP, BAR_TEXT, message) };
        shown(context, layout::CURSOR, false);
        shown(context, ANSWERS, false);
        shown(context, OVERLAY, true);
        session.notice = true;
    }
    fn close_notice(session: &mut Session, selection: *mut u8) {
        let Some(context) = context(selection) else {
            return;
        };
        shown(context, OVERLAY, false);
        shown(context, ANSWERS, true);
        shown(context, layout::CURSOR, true);
        session.notice = false;
    }

    /// DONE's question (`done`): the original's question, then its text in
    /// the bar, written as the counter's text is; "Move these Pokémon to
    /// Pokémon HOME?" without its words about Pokémon HOME
    /// (`shorten_move_question`).
    fn ask_done(session: &Session, selection: *mut u8) {
        let (question, shown) = done(any_checked(session, selection), session.changes.any());
        ask(selection, question);
        bar_text(selection, shown);
    }

    /// The question's bar shows `shown`, written as the counter's text is;
    /// 38:0x38 without its words about Pokémon HOME.
    fn bar_text(selection: *mut u8, shown: u32) {
        let Some(context) = context(selection) else {
            return;
        };
        let Some(text) = message(context, shown) else {
            return;
        };
        let Some(len) = text.iter().position(|unit| *unit == 0) else {
            return;
        };
        if shown == MOVE_TO_HOME {
            shorten_move_question(text, len);
        }
        let into: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const u8) =
            unsafe { transmute(PANE_TEXT) };
        unsafe { into(context, GROUP, BAR_TEXT, text.as_ptr().cast()) };
    }

    /// `001e22c4(buttons, button, animate)` lets a button of the screen's
    /// five (`[selection + 0x10]`, made by `001e9144` from the table at
    /// `0033cda4`: DONE, RESET, ALL, Yes, No) take presses, `001e23e8`
    /// stops it (its flag at `+0x35`, which its touch test reads,
    /// `00266218`); neither changes anything for a button that is so
    /// already, and without `animate` neither changes its look. Every frame
    /// (`Session::button_takes_presses`).
    const BUTTON_ON: usize = 0x001e_22c4;
    const BUTTON_OFF: usize = 0x001e_23e8;
    fn buttons(session: &Session, selection: *mut u8) {
        let Some(buttons) = (unsafe { follow(selection, 0x10) }) else {
            return;
        };
        let question = unsafe { read_word(selection, 0x530) } != u32::MAX;
        for button in 0..5 {
            let set: unsafe extern "aapcs" fn(*mut u8, u32, u32) = unsafe {
                transmute(if session.button_takes_presses(button, question) {
                    BUTTON_ON
                } else {
                    BUTTON_OFF
                })
            };
            unsafe { set(buttons, button, 0) };
        }
    }

    /// `001e9428(context, group, pane, place)` sets a pane's place in its
    /// parent (`[pane + 0x28]`, x, y, z), as the box screen places what it
    /// holds (`001d8bbc`); the pane is found by `001d7ed8(layout, pane)` in
    /// the group's layout, and one not found is left alone. (`001e9b38`
    /// would read the place on the screen, adding the parents'.)
    const FIND_PANE: usize = 0x001d_7ed8;
    const SET_PANE_PLACE: usize = 0x001e_9428;
    #[repr(C, align(8))]
    struct Place([f32; 4]);

    /// The blue and the green hand where the hand `n6` is: they bob and
    /// lower with it (`cursol_move` moves `n6` alone). Every frame.
    fn follow_hand(selection: *mut u8) {
        let Some(context) = context(selection) else {
            return;
        };
        let set: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const Place) =
            unsafe { transmute(SET_PANE_PLACE) };
        let Some((x, y, z)) = hand_place(context) else {
            return;
        };
        let hand = Place([x, y, z, 0.0]);
        for name in HANDS {
            unsafe { set(context, GROUP, layout::id(name), &hand) };
        }
    }

    /// The hand's place in `n8`: x, y, z (`[pane + 0x28]`).
    fn hand_place(context: *mut u8) -> Option<(f32, f32, f32)> {
        let pane = hand_pane(context)?;
        let read = |at: usize| f32::from_bits(unsafe { read_word(pane, at) });
        Some((read(0x28), read(0x2c), read(0x30)))
    }
    fn set_hand(context: *mut u8, (x, y): (f32, f32)) {
        let Some((_, _, z)) = hand_place(context) else {
            return;
        };
        let set: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const Place) =
            unsafe { transmute(SET_PANE_PLACE) };
        unsafe { set(context, GROUP, HAND, &Place([x, y, z, 0.0])) };
    }

    /// What is held where the box screen places it (`001d8c60`): at the
    /// cursor's x and the hand's height less 10 (`held_height`), the
    /// shadow 5 right and 5 down of it; the Pokémon the green cursor holds
    /// as high over their slots on the upper screen (the spare set stays
    /// where it is for the blue one: moved over the slots, it would cover
    /// them). On each frame of the lift's rise and when the cursor moves.
    fn place_held(session: &mut Session, selection: *mut u8) {
        let Some(context) = context(selection) else {
            return;
        };
        let Some((_, hand, z)) = hand_place(context) else {
            return;
        };
        session.held_y = held_height(hand);
        let set: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const Place) =
            unsafe { transmute(SET_PANE_PLACE) };
        let held = Place([layout::HELD_AT.0, session.held_y, z, 0.0]);
        for name in HELD {
            unsafe { set(context, GROUP, layout::id(name), &held) };
        }
        let shadow = Place([
            held.0[0] + layout::SHADOW_OFFSET.0,
            held.0[1] + layout::SHADOW_OFFSET.1,
            z,
            0.0,
        ]);
        for name in layout::SHADOWS {
            unsafe { set(context, GROUP, layout::id(name), &shadow) };
        }
        if session.mode == Mode::Move {
            place_lifted(context, session.held_y);
        }
    }

    /// The hand `n6` of the screen's layout (`001d7ed8`).
    fn hand_pane(context: *mut u8) -> Option<*mut u8> {
        let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 =
            unsafe { transmute(FIND_PANE) };
        let layout = unsafe { patch_common::ui::layout(context, GROUP) }?;
        pointer(unsafe { find(layout, HAND) } as u32)
    }

    /// `002863f8(selection, box)` shows a box on the upper screen; its
    /// check mark is the frame of animation 2 of group 0 (`00230b2c`); its
    /// name goes into pane `0x357` of group 0 (`001e7e84`). The original's
    /// key update does this when the cursor moves (`00285e6c`..`00285ef0`);
    /// the patch does it after what it changed under the cursor.
    /// What it shows follows `upper`: the preview's hooks
    /// (`bank_transfer_slot`, `bank_transfer_name`) serve the shown place (`upper_shown`),
    /// and the Pokémon the green cursor holds lie over it (`lift`).
    const PREVIEW: usize = 0x0028_63f8;
    const NAME_PANE: u32 = 0x357;
    const PANE_TEXT: usize = 0x001e_7e84;
    fn preview(session: &Session, selection: *mut u8) {
        set_holding(session);
        let (Some(context), Some(cursor)) = (context(selection), cursor(selection)) else {
            return;
        };
        let boxed = cursor.min(99);
        let show: unsafe extern "aapcs" fn(*mut u8, u32) = unsafe { transmute(PREVIEW) };
        unsafe { show(selection, boxed) };
        check_frame(session, selection);
        if let Some(helper) = helper() {
            let name = unsafe { bank_transfer_name(helper, boxed) };
            let text: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const u8) =
                unsafe { transmute(PANE_TEXT) };
            unsafe { text(context, UPPER, NAME_PANE, name) };
        }
        lift(session, selection);
    }

    /// The upper screen's box icon: the state of the place it shows, empty
    /// while the green cursor holds that place's Pokémon (`upper_state`).
    /// The original sets it from the cursor's box, and box 100's for any
    /// place past it. And its name label, blue for the transport box
    /// (`blue_label`, `NAME_LABEL`).
    fn check_frame(session: &Session, selection: *mut u8) {
        let (Some(context), Some(cursor)) = (context(selection), cursor(selection)) else {
            return;
        };
        let green = session.held.filter(|_| session.mode == Mode::Move);
        let shown = shown_place(cursor, session.last_shown, green);
        let cursor = upper_cursor(session.held, session.back, shown);
        let shown = upper(session.mode, session.held, cursor);
        let place = shown.place;
        let state = if place < TRANSPORT {
            unsafe { states(selection).add(place).read() }
        } else {
            with_places(|places| {
                transfer_screen::cell_state(places.count(place), session.extra[place - TRANSPORT])
            })
            .unwrap_or(0)
        };
        let state = upper_state(shown, state);
        unsafe { bank_transfer_frame(context, UPPER, 2, (state as f32).to_bits()) };
        let frame = if blue_label(shown) { 1.0f32 } else { 0.0 };
        unsafe { bank_transfer_frame(context, UPPER, NAME_LABEL, frame.to_bits()) };
    }

    /// The upper screen's name label `nl`. The box screen shows "Transport
    /// Box" (38:0x2A) on blue: it puts the message into the name pane
    /// `0x357` and plays its upper layout's animation 10, `box_name2`
    /// (`002882bc`, `00289bdc`), and for a box of the Bank its name and
    /// animation 9, `box_name1` (`001da8ac`). A layout's animation acts
    /// only on the group its file names: both name `xv`, which holds only
    /// `nl`; the check's `box_change` names `xx`, the check icon. On the
    /// label `box_name1` has one key, beige; `box_name2` is beige at frame
    /// 0 and blue at frame 1. This screen's upper layout loads `box_name1`
    /// as its animation 0 and only plays it once in its set-up
    /// (`00286094`), which leaves the label beige, as the layout has it.
    /// The patch makes animation 0 `box_name2` (file `0x3b` for `0x3a` in
    /// the layout's list, `0033d89c`), takes the set-up's play away, which
    /// would end on blue, and sets its frame with the check's: 1 on the
    /// transport box, else 0.
    const NAME_LABEL: u32 = 0;

    /// The upper layout's group, and its two sets of 30 icon panes in slot
    /// order: the preview's (`ot`.., `0033ce58`) and the first spare one
    /// (`sd`.., `003344a4`), which lies off the screen. The box screen draws
    /// what its green hand holds with that spare set (`001d8c60`, the list
    /// at `[+0x5a8]`), the same 30 apart on the grid of the slots, without a
    /// shadow, at its upper hand's height less 10, as the lower one; this
    /// screen has no hand on the upper screen, so the held Pokémon lie over
    /// the slots they came from as high as the lower screen's held box lies
    /// over its cell (`Session::held_y`).
    const UPPER: u32 = 0;
    const PREVIEW_PANES: usize = 0x0033_ce58;
    const LIFTED_PANES: usize = 0x0033_44a4;
    /// `001e9018(context, group)` the group's layout, `001e6910(layout,
    /// pane)` a pane of it, as the preview finds its icon panes.
    const GROUP_LAYOUT: usize = 0x001e_9018;
    const LAYOUT_PANE: usize = 0x001e_6910;
    fn upper_pane(context: *mut u8, table: usize, slot: usize) -> Option<(u32, *mut u8)> {
        let id = unsafe { read_word(table as *mut u8, slot * 4) };
        let layout: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 =
            unsafe { transmute(GROUP_LAYOUT) };
        let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 =
            unsafe { transmute(LAYOUT_PANE) };
        let layout = pointer(unsafe { layout(context, UPPER) } as u32)?;
        Some((id, pointer(unsafe { find(layout, id) } as u32)?))
    }
    /// A picture pane is drawn while bit 0 of `+0xb7` is set, as the preview
    /// shows and hides its icons (`002864dc`, `0028654c`).
    fn visible(pane: *mut u8, on: bool) {
        unsafe {
            let flags = pane.add(0xb7);
            flags.write((flags.read() & !1) | u8::from(on));
        }
    }

    /// The Pokémon the green cursor holds, over the upper screen's box: on
    /// the spare set, each over its slot, its icon as the preview makes one
    /// (`icon`); taken away when nothing green is held. Only when that
    /// changes.
    fn lift(session: &Session, selection: *mut u8) {
        let built = unsafe { &mut *BUILT.0.get() };
        // What is lifted does not depend on the cursor.
        let held = upper(session.mode, session.held, 0).lifted;
        if built.lifted == held {
            return;
        }
        built.lifted = held;
        let Some(context) = context(selection) else {
            return;
        };
        let Some(place) = held else {
            for slot in 0..30 {
                if let Some((_, pane)) = upper_pane(context, LIFTED_PANES, slot) {
                    visible(pane, false);
                }
            }
            return;
        };
        let (Some(helper), Some(manager), Some(pokemon)) =
            (helper(), unsafe { follow(selection, 0xf0) }, unsafe {
                follow(selection, 0xf4)
            })
        else {
            return;
        };
        place_lifted(context, session.held_y);
        for (slot, icon) in built.icons.iter_mut().enumerate() {
            let Some((_, pane)) = upper_pane(context, LIFTED_PANES, slot) else {
                continue;
            };
            unsafe { load_place(helper, pokemon, place, slot, false) };
            icon.key(pokemon);
            icon.show(manager, pane);
        }
    }

    /// The spare set over the preview's slots, `height` higher.
    fn place_lifted(context: *mut u8, height: f32) {
        let set: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const Place) =
            unsafe { transmute(SET_PANE_PLACE) };
        for slot in 0..30 {
            let (Some((id, _)), Some((_, under))) = (
                upper_pane(context, LIFTED_PANES, slot),
                upper_pane(context, PREVIEW_PANES, slot),
            ) else {
                continue;
            };
            let read = |at: usize| f32::from_bits(unsafe { read_word(under, at) });
            let at = Place([read(0x28), read(0x2c) + height, read(0x30), 0.0]);
            unsafe { set(context, UPPER, id, &at) };
        }
    }

    /// Every frame while green holds: an icon that was still loading goes
    /// on its pane once it is there, as the preview's own (`00286ae8`).
    fn lift_frame(selection: *mut u8) {
        let built = unsafe { &mut *BUILT.0.get() };
        if built.lifted.is_none() {
            return;
        }
        let (Some(context), Some(manager)) =
            (context(selection), unsafe { follow(selection, 0xf0) })
        else {
            return;
        };
        for (slot, icon) in built.icons.iter_mut().enumerate() {
            if let Some((_, pane)) = upper_pane(context, LIFTED_PANES, slot) {
                icon.frame(manager, pane);
            }
        }
    }
    fn pressed(pad: *mut u8, mask: u32) -> bool {
        pad_test(PRESSED, pad, mask)
    }
    fn held(pad: *mut u8, mask: u32) -> bool {
        pad_test(HOLDING_KEY, pad, mask)
    }
    fn again(pad: *mut u8, mask: u32) -> bool {
        pad_test(AGAIN, pad, mask)
    }
    fn pad_test(test: usize, pad: *mut u8, mask: u32) -> bool {
        let call: unsafe extern "aapcs" fn(*mut u8, u32, u32) -> u32 = unsafe { transmute(test) };
        unsafe { call(pad, mask, 0) != 0 }
    }
    fn context(selection: *mut u8) -> Option<*mut u8> {
        unsafe { follow(selection, 0x5c) }
    }
    fn cursor(selection: *mut u8) -> Option<u32> {
        let cursor = unsafe { follow(selection, 0x88) }?;
        Some(u32::from(unsafe { cursor.add(0x14).read() }))
    }
    /// The original's states of the 100 boxes: 0 empty, 1 with Pokémon,
    /// 2 checked.
    fn states(selection: *mut u8) -> *mut u8 {
        unsafe { selection.add(0x8c) }
    }
    fn bank() -> Option<NativeBank<'static>> {
        unsafe {
            let manager = follow(MANAGER as *mut u8, 0)?;
            let coordinator = follow(manager, 0x1c)?;
            let bank = follow(coordinator, 0xcc)?;
            NativeBank::from_raw(bank).ok()
        }
    }
    /// The time of a move, as the original stamps a slot.
    fn now() -> [u8; 8] {
        let mut stamp = [0u8; 8];
        let written = unsafe {
            crate::hooks::bank_offline_timestamp(core::ptr::null_mut(), stamp.as_mut_ptr())
        };
        if written == 0 {
            [0; 8]
        } else {
            stamp
        }
    }

    fn checked(session: &Session, selection: *mut u8, place: usize) -> bool {
        if place < TRANSPORT {
            unsafe { states(selection).add(place).read() == 2 }
        } else {
            session.extra[place - TRANSPORT]
        }
    }

    /// Shows `place` as what it holds now, or as `place_frame` has it while
    /// it is lifted: hidden for the blue cursor, empty for the green one.
    /// A box of Bank is its pane in the cursor's places (`PLACES_TABLE`),
    /// its state the frame of its animation; a cell one picture per state.
    fn refresh(session: &mut Session, selection: *mut u8, places: &Places, place: usize) {
        let Some(context) = context(selection) else {
            return;
        };
        let count = places.count(place);
        if place < TRANSPORT {
            let state = unsafe { states(selection).add(place) };
            let now = transfer_screen::cell_state(count, unsafe { state.read() } == 2);
            unsafe { state.write(now) };
            let frame = place_frame(session.mode, session.held, place, now);
            if let Some(frame) = frame {
                unsafe {
                    bank_transfer_frame(context, GROUP, place as u32 + 2, (frame as f32).to_bits())
                };
            }
            let pane = unsafe { read_word(PLACES_TABLE as *mut u8, place * 8) };
            shown(context, pane, frame.is_some());
        } else {
            let cell = place - TRANSPORT;
            if count == 0 {
                session.extra[cell] = false;
            }
            let now = transfer_screen::cell_state(count, session.extra[cell]);
            let frame = place_frame(session.mode, session.held, place, now);
            for (state, names) in CELLS.iter().enumerate() {
                shown(context, layout::id(names[cell]), frame == Some(state as u8));
            }
        }
    }

    /// The hand of the cursor's colour, and the lifted box under it.
    fn refresh_hand(session: &mut Session, selection: *mut u8, places: &Places) {
        let Some(context) = context(selection) else {
            return;
        };
        // The red hand stays shown, drawn with alpha 0 while another is up:
        // a hidden pane is not moved by its animations, and the others
        // follow its bobbing. Its alpha is at `+0xb4`, the alpha it is drawn
        // with at `+0xb5` (`001ec734`).
        shown(context, HAND, true);
        if let Some(hand) = hand_pane(context) {
            let alpha = if session.mode == Mode::Check { 0xff } else { 0 };
            unsafe {
                hand.add(0xb4).write(alpha);
                hand.add(0xb5).write(alpha);
            }
        }
        shown(context, layout::id(HANDS[0]), session.mode == Mode::Swap);
        shown(context, layout::id(HANDS[1]), session.mode == Mode::Move);
        let held = session.held.map(|place| {
            usize::from(transfer_screen::cell_state(
                places.count(place),
                checked(session, selection, place),
            ))
        });
        for (state, name) in HELD.iter().enumerate() {
            shown(context, layout::id(name), held == Some(state));
        }
        for (state, name) in layout::SHADOWS.iter().enumerate() {
            shown(context, layout::id(name), held == Some(state));
        }
        // What is taken lies under the hand; a put-down bobs it again, as
        // the box screen's (`bob`).
        if held.is_some() != session.lifted {
            session.lifted = held.is_some();
            if session.lifted {
                place_held(session, selection);
            }
            bob(context, !session.lifted);
        }
    }

    /// The bobbing `cursol_move`: animation 1 of group 1 here (`0033d8a8`).
    /// The box screen's lift stops it (`0028d358`, `001e69ac(context,
    /// group, 3, 0)`, its own index) and its put-down plays it again
    /// (`0028d790`, `001e98ec(context, group, 3, 1, 1)`), from the hand's
    /// rest. This screen does not load `cursol_move_duwn` and
    /// `cursol_move_up`: the lift sets the hand where their keys have it
    /// (`lifting`, `lift_step`).
    const BOB: u32 = 1;
    const STOP_ANIMATION: usize = 0x001e_69ac;
    const PLAY_ANIMATION: usize = 0x001e_98ec;
    fn bob(context: *mut u8, on: bool) {
        if on {
            let play: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32) =
                unsafe { transmute(PLAY_ANIMATION) };
            unsafe { play(context, GROUP, BOB, 1, 1) };
            set_hand(context, layout::HAND_AT);
        } else {
            let stop: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
                unsafe { transmute(STOP_ANIMATION) };
            unsafe { stop(context, GROUP, BOB, 0) };
        }
    }

    /// A frame of a lift (`lifting`), from the frame after A on, before the
    /// layout's update (`before_update`): the hand dips; what is lifted is
    /// taken out of its place under the dipped hand, with its shadow, and
    /// the hand is back at its rest; then what is held is placed under it.
    fn lift_step(session: &mut Session, selection: *mut u8) {
        let Some((frames, place)) = session.lifting else {
            return;
        };
        let Some(context) = context(selection) else {
            return;
        };
        let frames = frames.saturating_add(1);
        session.lifting = Some((frames, place));
        match lifting(frames) {
            LiftFrame::Dip => {
                bob(context, false);
                set_hand(context, layout::HAND_LOWERED);
            }
            LiftFrame::Take => {
                // Placed under the hand as it stands before this frame's
                // update, dipped; then the hand shows at its rest.
                session.held = Some(place);
                with_places(|places| {
                    refresh(session, selection, places, place);
                    refresh_hand(session, selection, places);
                });
                preview(session, selection);
                set_hand(context, layout::HAND_AT);
            }
            LiftFrame::Rise => place_held(session, selection),
            LiftFrame::Done => session.lifting = None,
        }
    }

    /// Checks `place` or not: a box of Bank by its state byte, a cell by
    /// the session's.
    fn set_checked(session: &mut Session, selection: *mut u8, place: usize, on: bool) {
        if place < TRANSPORT {
            unsafe { states(selection).add(place).write(if on { 2 } else { 1 }) };
        } else {
            session.extra[place - TRANSPORT] = on;
        }
    }

    /// The top line (`transfer_screen::line`, `layout::TITLE`). The title:
    /// "Boxenauswahl" (38:0x37, which the set-up writes into `n2`,
    /// `00286178`) with "({n}/100)" (38:0x3A) in the counter `0x33c`, as the
    /// original's key update writes it after a check (`00285d24`..`00285d6c`:
    /// the value `001e6c0c(context, 0, n, 3, 0, 1)`), but counting the four
    /// cells too, and out of 104, the title from `layout::TITLE_START`. A
    /// group: "Gruppe: " (38:0x00) in `nf`, the name in `n2`, and the counter
    /// with the box's index as its value and out of the group's amount
    /// (`out_of`), without its brackets (`unbracketed`).
    const COUNTER_PANE: u32 = 0x33c;
    const COUNTER: u32 = 0x3a;
    const TITLE_TEXT: u32 = 0x37;
    const GROUP_TEXT: u32 = 0x00;
    const NUMBER: usize = 0x001e_6c0c;
    fn show_line(session: &Session, selection: *mut u8) -> Option<()> {
        let context = context(selection)?;
        let cursor = cursor(selection).and_then(place_of);
        let mut name = [0u16; NAME_UNITS];
        let line = with_places(|places| {
            let line = super::line(
                session.mode,
                session.held,
                cursor,
                |place| places.group(place),
                |group| places.group_size(group),
            );
            if let Line::Group { of, .. } = line {
                let bytes = match of {
                    Of::Group(group) => places.group_name(group)?,
                    Of::Place(TRANSPORT) => places.offline.transport,
                    Of::Place(_) => super::offline_category(&places.offline.transport)
                        .unwrap_or([0; offline_core::boxfile::NAME_TEXT]),
                };
                for (unit, pair) in name.iter_mut().zip(bytes.chunks_exact(2)) {
                    *unit = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            Some(line)
        })
        .flatten()?;
        let into: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
            unsafe { transmute(INTO_PANE) };
        let text_into: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const u8) =
            unsafe { transmute(PANE_TEXT) };
        let (value, total) = match line {
            Line::Title => {
                shown(context, layout::GROUP_LABEL, false);
                title_at(context, layout::TITLE_START + layout::TITLE_AT.1 / 2.0);
                unsafe { into(context, GROUP, layout::TITLE, TITLE_TEXT) };
                let checked = (0..TRANSPORT)
                    .filter(|&place| checked(session, selection, place))
                    .count()
                    + session.extra.iter().filter(|on| **on).count();
                (checked as u32, PLACES as u16)
            }
            Line::Group { index, amount, .. } => {
                title_at(context, layout::TITLE_AT.0);
                unsafe { into(context, GROUP, layout::GROUP_LABEL, GROUP_TEXT) };
                shown(context, layout::GROUP_LABEL, true);
                unsafe { text_into(context, GROUP, layout::TITLE, name.as_ptr().cast()) };
                (u32::from(index), amount)
            }
        };
        let number: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32, u32) =
            unsafe { transmute(NUMBER) };
        unsafe { number(context, 0, value, 3, 0, 1) };
        let text = message(context, COUNTER)?;
        let len = text.iter().position(|unit| *unit == 0)?;
        let len = out_of(text, len, total)?;
        if let Line::Group { .. } = line {
            unbracketed(text, len)?;
        }
        unsafe { text_into(context, GROUP, COUNTER_PANE, text.as_ptr().cast()) };
        Some(())
    }

    /// Places `n2`, which holds the title or a group's name, at the centre x
    /// `x`, its height and depth as they are.
    fn title_at(context: *mut u8, x: f32) {
        let Some(pane) = find_pane(context, layout::TITLE) else {
            return;
        };
        let read = |at: usize| f32::from_bits(unsafe { read_word(pane, at) });
        set_pane(context, layout::TITLE, [x, read(0x2c), read(0x30)]);
    }

    /// L with the blue hand on a box of Bank: one place before in its group,
    /// the first to the end (`lowered`), through the change record; whether
    /// it moved. In a group of one box, nothing.
    fn lower(session: &mut Session, place: usize) -> bool {
        with_places(|places| {
            let (group, position) = places.group(place)?;
            let to = lowered(position, places.group_size(group));
            session.changes.insert(places, place, to).ok()
        })
        .flatten()
        .is_some()
    }

    fn with_places<T>(work: impl FnOnce(&mut Places) -> T) -> Option<T> {
        let mut bank = bank()?;
        let (offline, _) = unsafe { ROOM.boxes() };
        let mut places = Places {
            body: bank.body_mut(),
            offline,
        };
        Some(work(&mut places))
    }

    /// A for the blue or the green cursor on `place`: a lift, or a
    /// put-down whose data moves now and whose screen glides (`land`).
    fn pick_or_drop(session: &mut Session, selection: *mut u8, place: usize) {
        let mode = session.mode;
        let mut put = None;
        with_places(|places| {
            match session.held {
                None => {
                    // The lift runs over the next frames (`lift_step`).
                    if liftable(
                        mode,
                        place,
                        places.count(place),
                        checked(session, selection, place),
                    ) {
                        sound(SOUND_LIFT);
                        session.lifting = Some((0, place));
                    }
                }
                Some(from) => {
                    // What is held is put down, or goes back where it was.
                    let checks = (
                        checked(session, selection, from),
                        checked(session, selection, place),
                    );
                    // Green put down on its own box is a move too: its
                    // Pokémon get the time (`Changes::move_all`).
                    let moved = match (from == place, mode) {
                        (true, Mode::Swap) => Ok(()),
                        (false, Mode::Swap) => {
                            let defaults = [
                                default_of(session.task, places, from),
                                default_of(session.task, places, place),
                            ];
                            session.changes.swap(places, from, place, defaults)
                        }
                        (_, _) => session.changes.move_all(places, from, place, now()),
                    };
                    match moved {
                        Ok(()) => {
                            if from != place {
                                // A check goes with its box. The green
                                // cursor moves no check: it lifts no
                                // checked place and puts only onto one
                                // without Pokémon, which none checks.
                                if mode == Mode::Swap {
                                    set_checked(session, selection, place, checks.0);
                                    set_checked(session, selection, from, checks.1);
                                }
                            }
                            // The blue cursor's box changes places with
                            // the one it is put on.
                            put = Some(Put {
                                from,
                                to: place,
                                swapped: mode == Mode::Swap && from != place,
                                refused: false,
                            });
                        }
                        Err(refusal) => match refused(refusal) {
                            Refused::Stays => sound(SOUND_REFUSED),
                            // Its sound comes with its message (`land`).
                            Refused::Back => {
                                put = Some(Put {
                                    from,
                                    to: from,
                                    swapped: false,
                                    refused: true,
                                });
                            }
                        },
                    }
                }
            }
        });
        // A put-down glides from the next frame (`GLIDE_STEPS`).
        if put.is_some() {
            session.put = put;
        }
    }

    /// `001e9b38(place, context, group, pane)`: a pane's place on the
    /// screen, its parents' added; `001d8a14(context, group, parent, pane,
    /// mode, sibling)` moves a pane into another, to the end of its
    /// children (mode 0, `001ec618`) or before `sibling` (mode 2,
    /// `0024d9bc`); children are drawn in their order. The screen's top
    /// pane `gi` holds the boxes' pane `gx` and, after it, the cursor `n8`,
    /// both at its origin on the screen's. A box gliding to the held one's
    /// place goes before `n8`: over the other boxes, under the held box and
    /// the hand, where the box screen's lift puts what it holds (`00290a10`,
    /// mode 2 before its cursor pane). (Its put-down puts the icon it moves
    /// away at the end, over its hand, `00290778`.)
    const ON_SCREEN: usize = 0x001e_9b38;
    const REPARENT: usize = 0x001d_8a14;
    const TOP: u32 = layout::id(b"gi");
    const BOXES: u32 = layout::id(b"gx");
    fn on_screen(context: *mut u8, pane: u32) -> Option<[f32; 3]> {
        let layout = unsafe { patch_common::ui::layout(context, GROUP) }?;
        let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 =
            unsafe { transmute(FIND_PANE) };
        pointer(unsafe { find(layout, pane) } as u32)?;
        let read: unsafe extern "aapcs" fn(*mut [f32; 3], *mut u8, u32, u32) =
            unsafe { transmute(ON_SCREEN) };
        let mut at = [0.0; 3];
        unsafe { read(&mut at, context, GROUP, pane) };
        Some(at)
    }
    /// `001e6620(cursor, place)` sets the cursor's place (`+0x14`, the one
    /// before at `+0x15`), as the box screen sets its cursor after a
    /// put-down (`001da808`); `001e0c90(cursor, place)` is the place's
    /// pane, at whose place on the screen the original's key update puts
    /// the cursor's pane `n8` when it moved the cursor
    /// (`00285e2c`..`00285e68`).
    const SET_CURSOR: usize = 0x001e_6620;
    const CURSOR_PANE: usize = 0x001e_0c90;
    /// Puts the cursor on `place`, as the original's key update moves it,
    /// and tells whether it was elsewhere. The caller shows the upper
    /// screen for it (`preview`).
    fn cursor_to(selection: *mut u8, place: usize) -> bool {
        let (Some(context), Some(cursor)) =
            (context(selection), unsafe { follow(selection, 0x88) })
        else {
            return false;
        };
        let set: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = unsafe { transmute(SET_CURSOR) };
        let pane: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = unsafe { transmute(CURSOR_PANE) };
        let index = cursor_of(place);
        if unsafe { set(cursor, index) } == 0 {
            return false;
        }
        if let Some(at) = on_screen(context, unsafe { pane(cursor, index) }) {
            set_pane(context, layout::CURSOR, at);
        }
        true
    }
    fn reparent(context: *mut u8, parent: u32, pane: u32, before: Option<u32>) {
        let call: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32, u32) =
            unsafe { transmute(REPARENT) };
        let (mode, sibling) = before.map_or((0, 0), |sibling| (2, sibling));
        unsafe { call(context, GROUP, parent, pane, mode, sibling) };
    }
    fn set_pane(context: *mut u8, pane: u32, at: [f32; 3]) {
        let set: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const Place) =
            unsafe { transmute(SET_PANE_PLACE) };
        unsafe { set(context, GROUP, pane, &Place([at[0], at[1], at[2], 0.0])) };
    }
    /// A place's panes: a box of Bank its one, a cell its three.
    fn place_panes(place: usize) -> ([u32; 3], usize) {
        if place < TRANSPORT {
            let pane = unsafe { read_word(PLACES_TABLE as *mut u8, place * 8) };
            ([pane, 0, 0], 1)
        } else {
            let cell = place - TRANSPORT;
            (CELLS.map(|names| layout::id(names[cell])), 3)
        }
    }

    /// Sets up the glide of a put-down (`from` to `to`) or a put-back
    /// (`to` is `from`) in the frame after the key, as the box screen's
    /// state 0x10 (`GLIDE_STEPS`): its sound, the shadow hidden; the held
    /// box from under the hand onto `to`; with `swapped`, the box that is
    /// at `to` on the screen onto `from`, under the held box; the green
    /// cursor's Pokémon on the upper screen down into their slots. The
    /// places are laid out from the data once it is over (`land`), and a
    /// refusal's message shown then; what went back has the cursor set
    /// onto its place then (`turns_back`).
    fn glide(session: &mut Session, selection: *mut u8, put: Put) {
        let Put {
            from,
            to,
            swapped,
            refused,
        } = put;
        sound(SOUND_PUT);
        if let Some(context) = context(selection) {
            for name in layout::SHADOWS {
                shown(context, layout::id(name), false);
            }
        }
        let built = unsafe { &*BUILT.0.get() };
        let lifted = built
            .lifted
            .is_some()
            .then(|| Glide::new((0.0, session.held_y), (0.0, 0.0)));
        let mut landing = Landing {
            frames: 0,
            from,
            to,
            held: Glide::new(
                (layout::HELD_AT.0, session.held_y),
                (layout::HELD_AT.0, session.held_y),
            ),
            moved: None,
            lifted,
            refused,
        };
        if let Some(context) = context(selection) {
            let (target, _) = place_panes(to);
            let (origin, _) = place_panes(from);
            if let (Some(cursor), Some(target), Some(origin)) = (
                on_screen(context, layout::CURSOR),
                on_screen(context, target[0]),
                on_screen(context, origin[0]),
            ) {
                landing.held = Glide::new(
                    landing.held.at,
                    (target[0] - cursor[0], target[1] - cursor[1]),
                );
                if swapped {
                    let (panes, count) = place_panes(to);
                    let home = find_pane(context, panes[0]).map(|pane| {
                        let read = |at: usize| f32::from_bits(unsafe { read_word(pane, at) });
                        [read(0x28), read(0x2c), read(0x30)]
                    });
                    if let Some(home) = home {
                        for &pane in &panes[..count] {
                            reparent(context, TOP, pane, Some(layout::CURSOR));
                            set_pane(context, pane, target);
                        }
                        landing.moved = Some(Moved {
                            panes,
                            count,
                            home,
                            z: target[2],
                            glide: Glide::new((target[0], target[1]), (origin[0], origin[1])),
                        });
                    }
                }
            }
        }
        session.landing = Some(landing);
    }

    /// A frame of the glide (`landing`); at its end the places, the hand and
    /// the upper screen as the data has them.
    fn land(session: &mut Session, selection: *mut u8) {
        let Some(mut now) = session.landing else {
            return;
        };
        let Some(context) = context(selection) else {
            return;
        };
        now.frames = now.frames.saturating_add(1);
        let frame = landing(now.frames);
        if frame != LandingFrame::Done {
            now.held.frame(frame);
            let z = hand_place(context).map_or(0.0, |(_, _, z)| z);
            let held = [now.held.at.0, now.held.at.1, z];
            for name in HELD {
                set_pane(context, layout::id(name), held);
            }
            let shadow = [
                held[0] + layout::SHADOW_OFFSET.0,
                held[1] + layout::SHADOW_OFFSET.1,
                z,
            ];
            for name in layout::SHADOWS {
                set_pane(context, layout::id(name), shadow);
            }
            if let Some(moved) = now.moved.as_mut() {
                moved.glide.frame(frame);
                let at = [moved.glide.at.0, moved.glide.at.1, moved.z];
                for &pane in &moved.panes[..moved.count] {
                    set_pane(context, pane, at);
                }
            }
            if let Some(lifted) = now.lifted.as_mut() {
                lifted.frame(frame);
                place_lifted(context, lifted.at.1);
            }
            session.landing = Some(now);
            return;
        }
        session.landing = None;
        if let Some(moved) = now.moved {
            for &pane in &moved.panes[..moved.count] {
                reparent(context, BOXES, pane, None);
                set_pane(context, pane, moved.home);
            }
        }
        session.held = None;
        with_places(|places| {
            refresh(session, selection, places, now.from);
            refresh(session, selection, places, now.to);
            refresh_hand(session, selection, places);
        });
        // What went back, with B or refused: the cursor is set onto its
        // place, and the upper screen shows the cursor's box again.
        session.back = false;
        if now.to == now.from {
            cursor_to(selection, now.from);
            // The place the upper screen keeps should the keys of this same
            // frame take the cursor on to the buttons.
            session.last_shown = now.from;
            LAST_SHOWN.store(now.from, core::sync::atomic::Ordering::Relaxed);
        }
        if now.refused {
            sound(SOUND_REFUSED);
            notice(session, selection, CANNOT_TRANSPORT);
        }
        preview(session, selection);
        show_line(session, selection);
    }

    fn find_pane(context: *mut u8, id: u32) -> Option<*mut u8> {
        let find: unsafe extern "aapcs" fn(*mut u8, u32) -> *mut u8 =
            unsafe { transmute(FIND_PANE) };
        let layout = unsafe { patch_common::ui::layout(context, GROUP) }?;
        pointer(unsafe { find(layout, id) } as u32)
    }

    fn any_checked(session: &Session, selection: *mut u8) -> bool {
        (0..TRANSPORT).any(|place| checked(session, selection, place))
            || session.extra.iter().any(|on| *on)
    }

    /// Replaces the selection object's key update (its vtable slot 13,
    /// `00285ba8`): START, and A and B for what the patch adds; every other
    /// key, B with nothing held (38:0x39 with sound `0x5000A`, `00285c14`,
    /// its bar then showing `BACK_TEXT`), and every key while a question is
    /// up, the original's.
    /// # Safety
    /// The original's call, with its selection object and pad.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_input(selection: *mut u8, pad: *mut u8) -> u32 {
        let original: unsafe extern "aapcs" fn(*mut u8, *mut u8) -> u32 =
            unsafe { transmute(SELECTION_INPUT) };
        SELECTION.store(selection as usize, core::sync::atomic::Ordering::Relaxed);
        let idle = unsafe { read_word(selection, 0x7c) } != 0
            && unsafe { read_word(selection, 0x530) } == u32::MAX;
        let Some(mut session) = SESSION.take() else {
            return unsafe { original(selection, pad) };
        };
        session.keyed = true;
        if !idle || pointer(pad as u32).is_none() {
            // The original may call the button update (`bank_transfer_event`)
            // from here, which takes the state itself.
            drop(session);
            return unsafe { original(selection, pad) };
        }
        if session.moving() {
            // No key while a lift runs or what was put down glides.
            return 0;
        }
        if session.notice {
            // A refusal's message takes every key; A or B closes it.
            if pressed(pad, KEY_A | KEY_B) {
                sound(SOUND_CLOSE);
                close_notice(&mut session, selection);
            }
            return 0;
        }
        let place = cursor(selection).and_then(place_of);
        if session.turning {
            // The box screen's page turn after L reads no key.
            return 0;
        }
        // L as the box screen's state 4 counts it (`Shoulder`), with every
        // hand and while a box is held; it moves a box only with the blue
        // hand and nothing held. Between its fires every key works.
        match session.shoulder.step(held(pad, KEY_L), again(pad, KEY_L)) {
            ShoulderKey::Idle | ShoulderKey::Free => {}
            ShoulderKey::Hold => return 0,
            ShoulderKey::Fire => {
                // One place before in its group, with the box screen's sound
                // for L (`002887fc`) and its page turn; elsewhere nothing.
                if session.held.is_none()
                    && session.mode == Mode::Swap
                    && place.is_some_and(|place| lower(&mut session, place))
                {
                    sound(bank_common::ui::HAND_SOUND);
                    show_line(&session, selection);
                    session.paging = TURN_FRAMES;
                }
                return 0;
            }
        }
        if let Some(held) = session.held {
            // Only A, B and the cursor while a box is held (`held_key`).
            match held_key(|mask| pressed(pad, mask), place) {
                HeldKey::Put(place) => {
                    pick_or_drop(&mut session, selection, place);
                    return 0;
                }
                HeldKey::Back => {
                    // Put back, gliding from the next frame.
                    session.put = Some(Put {
                        from: held,
                        to: held,
                        swapped: false,
                        refused: false,
                    });
                    return 0;
                }
                HeldKey::Nothing => return 0,
                HeldKey::Cursor => {}
            }
        } else {
            if pressed(pad, KEY_START) {
                session.mode = session.mode.next();
                // The box screen's sound for its hand (`00288ab0`).
                unsafe { bank_common::ui::sound(bank_common::ui::HAND_SOUND) };
                with_places(|places| refresh_hand(&mut session, selection, places));
                show_line(&session, selection);
                return 0;
            }
            if pressed(pad, KEY_A) {
                match (session.mode, place) {
                    (Mode::Check, Some(place)) if place >= TRANSPORT => {
                        let checked = with_places(|places| {
                            let full = places.count(place) != 0;
                            if full {
                                sound(SOUND_CHECK);
                                let cell = place - TRANSPORT;
                                session.extra[cell] = !session.extra[cell];
                                refresh(&mut session, selection, places, place);
                            }
                            full
                        });
                        if checked == Some(true) {
                            preview(&session, selection);
                        }
                        show_line(&session, selection);
                        return 0;
                    }
                    (Mode::Swap | Mode::Move, Some(place)) => {
                        pick_or_drop(&mut session, selection, place);
                        return 0;
                    }
                    _ => {}
                }
            }
        }
        let before = cursor(selection);
        let checks = pressed(pad, KEY_A);
        let back = pressed(pad, KEY_B);
        drop(session);
        let result = unsafe { original(selection, pad) };
        if back && unsafe { read_word(selection, 0x530) } == QUIT_MOVING {
            // The original asked its 38:0x39 for B: its bar shows B's text.
            bar_text(selection, BACK_TEXT);
        }
        if let Some(mut session) = SESSION.take() {
            // The original's preview after a move of the cursor showed the
            // place `upper` names through the hooks; its check is the
            // cursor's box's, box 100's past it. What is held is placed
            // again from the hand's height, as the box screen's move does.
            if cursor(selection) != before {
                check_frame(&session, selection);
                if session.held.is_some() {
                    place_held(&mut session, selection);
                }
            }
            // The line for the box now under the cursor, and the original's
            // own count after A on a box: the four cells too.
            if checks || cursor(selection) != before {
                show_line(&session, selection);
            }
        }
        result
    }

    /// Replaces the selection object's update for its buttons and answers
    /// (vtable slot 12, `0028672c`): DONE asks the original's question by
    /// what was checked and changed, with its own text (`done`); RESET and
    /// ALL also take the four cells; Yes takes the checks for the write when
    /// it goes on (`writes`); No is the original's.
    /// # Safety
    /// The original's call, with its selection object and event.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_event(selection: *mut u8, event: u32) -> u32 {
        let original: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
            unsafe { transmute(SELECTION_EVENT) };
        let Some(mut session) = SESSION.take() else {
            return unsafe { original(selection, event) };
        };
        if !session.takes_events() {
            // The buttons do nothing while a box is lifted, held, or
            // glides into place, while a refusal's message is up, and in L's
            // first three frames, a frame it fires and its page turn.
            return 0;
        }
        match event {
            0 => {
                ask_done(&session, selection);
                0
            }
            1 | 2 => {
                drop(session);
                let result = unsafe { original(selection, event) };
                if let Some(mut session) = SESSION.take() {
                    with_places(|places| {
                        for cell in 0..session.extra.len() {
                            session.extra[cell] = event == 2 && places.count(TRANSPORT + cell) != 0;
                            refresh(&mut session, selection, places, TRANSPORT + cell);
                        }
                    });
                    // The original's RESET and ALL show box 100's check on
                    // the upper screen for any place past it: on a cell or
                    // the button row the upper screen shows its box again.
                    if !cursor(selection).is_some_and(original_draws) {
                        preview(&session, selection);
                    }
                    show_line(&session, selection);
                }
                result
            }
            3 => {
                // The original's Yes goes on or quits by its question; the
                // checks go along when it goes on.
                session.commit = writes(unsafe { read_word(selection, 0x530) }).then(|| {
                    let mut checks = [false; PLACES];
                    for (place, check) in checks.iter_mut().enumerate() {
                        *check = checked(&session, selection, place);
                    }
                    checks
                });
                drop(session);
                unsafe { original(selection, event) }
            }
            _ => {
                drop(session);
                unsafe { original(selection, event) }
            }
        }
    }

    /// Replaces scene 1's end in task `0x1B`'s table (`0033d38c`, the
    /// original's `002a7f08`): the task ends, with the outcome that asks
    /// for the write if Yes asked for it; there is no upload.
    /// # Safety
    /// The original's call, with task `0x1B`.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_scene_done(task: *mut u8) {
        SELECTION.store(0, core::sync::atomic::Ordering::Relaxed);
        let write = SESSION
            .take()
            .is_some_and(|session| session.commit.is_some())
            && unsafe { task.add(0x58).read() } == 0;
        unsafe {
            task.add(0x48).write(3);
            if write {
                task.add(0x30).write(OUTCOME_WRITE);
            }
        }
    }

    /// The shared UI's calls of the question (task 3, states `0x21`..`0x25`):
    /// `001d6600(context)` a frame of the dialog; `001d644c(ui)` resets the
    /// two buttons; `001e0ef8(ui, 1, pane, message)` labels one;
    /// `001d650c(ui, 0)` sets them up; `001d67c8(ui, animation, 1, out)`
    /// plays their slide, true once it is over; `001e9748(buttons, on)` and
    /// `001d6758(ui, 0)` take presses and put the cursor on the first.
    const DIALOG_POLL: usize = 0x001d_6600;
    const RESET_BUTTONS: usize = 0x001d_644c;
    const LABEL: usize = 0x001e_0ef8;
    const SET_BUTTONS: usize = 0x001d_650c;
    const ANIMATE: usize = 0x001d_67c8;
    const BUTTONS_ON: usize = 0x001e_9748;
    const FIRST_BUTTON: usize = 0x001d_6758;
    /// The slides of the two buttons, in and out.
    const BUTTONS_IN: usize = 0x0033_5fe0;
    const BUTTONS_OUT: usize = 0x0033_5fd8;
    /// "Begin the move" and "Cancel" (text file 39), on panes 3 and 8 of
    /// group 1, inside pane `0xa`.
    const BEGIN_THE_MOVE: u32 = 0x59;
    const CANCEL: u32 = 0x5a;

    /// One frame of the question (`Opening`), on the shared UI `ui`.
    fn question(session: &mut Session, task: *mut u8, ui: *mut u8) -> Option<u32> {
        let context = unsafe { follow(ui, 0x5c) }?;
        match session.opening {
            Opening::Ask => {
                // The set-up leaves the UI as game selection left it, so
                // that its text window is not gone for a frame: it is hidden
                // here, in the frame the question comes up.
                unsafe { bank_common::ui::hide_all(ui) }.ok()?;
                unsafe { bank_common::ui::notice_from(ui, 0, 2, shorten_question) }.ok()?;
                session.opening = Opening::Question;
            }
            Opening::Question => {
                let poll: unsafe extern "aapcs" fn(*mut u8) -> u32 =
                    unsafe { transmute(DIALOG_POLL) };
                if written_out(unsafe { poll(context) }) {
                    unsafe {
                        let reset: unsafe extern "aapcs" fn(*mut u8) = transmute(RESET_BUTTONS);
                        let label: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
                            transmute(LABEL);
                        let set: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(SET_BUTTONS);
                        let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
                            transmute(PANE_SHOWN);
                        reset(ui);
                        label(ui, 1, 3, BEGIN_THE_MOVE);
                        label(ui, 1, 8, CANCEL);
                        pane(context, 1, 0xa, 1);
                        set(ui, 0);
                    }
                    session.opening = Opening::ButtonsIn;
                }
            }
            Opening::ButtonsIn => {
                if slid(ui, BUTTONS_IN, false) {
                    unsafe {
                        if let Some(scene) = follow(ui, 0x10) {
                            scene.add(0x25).write(1);
                        }
                        let on: unsafe extern "aapcs" fn(u32, u32) = transmute(BUTTONS_ON);
                        let first: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(FIRST_BUTTON);
                        on(read_word(ui, 0x80), 1);
                        ui.add(0x88).write(0);
                        write_word(ui, 0x84, 0);
                        first(ui, 0);
                        write_word(ui, 0x7c, task.add(0x34) as u32);
                        write_word(task, 0x44, 3);
                    }
                    session.opening = Opening::Waiting;
                }
            }
            Opening::Waiting => {
                if let Some(begin) = button_pressed(unsafe { read_word(task, 0x44) }) {
                    session.opening = Opening::ButtonsOut { begin };
                }
            }
            Opening::ButtonsOut { begin } => {
                if slid(ui, BUTTONS_OUT, true) {
                    unsafe { write_word(ui, 0x7c, 0) };
                    if !begin {
                        // To the start screen.
                        unsafe { task.add(0x30).write(3) };
                        return Some(1);
                    }
                    session.opening = Opening::Loading { first: true };
                }
            }
            Opening::Loading { .. } | Opening::Boxes => {}
        }
        Some(0)
    }
    /// The scene of the boxes: scene 1 of task `0x1B` (`[task + 0x3c]`,
    /// `002a8004`, vtable `00360780`), and its selection object
    /// (`[scene + 0x34]`, `002870e4`, vtable `0036071c`).
    const SCENE_TABLE: u32 = 0x0036_0780;
    const SELECTION_TABLE: u32 = 0x0036_071c;
    fn selection_of(task: *mut u8) -> Option<*mut u8> {
        unsafe {
            let scene = follow(task, 0x3c)?;
            if read_word(scene, 0) != SCENE_TABLE {
                return None;
            }
            let selection = follow(scene, 0x34)?;
            (read_word(selection, 0) == SELECTION_TABLE).then_some(selection)
        }
    }

    /// Every frame of the boxes, from the task's update: the selection
    /// object for the upper screen's hooks; on the first, the four cells,
    /// the hand and the counter; and the hands and what is held where the
    /// hand `n6` bobs. The key update runs only when there is a key to
    /// take, so it cannot do this.
    fn frame(selection: *mut u8) {
        SELECTION.store(selection as usize, core::sync::atomic::Ordering::Relaxed);
        let Some(mut session) = SESSION.take() else {
            return;
        };
        if !session.cells_shown {
            session.cells_shown = true;
            name_offline(session.task, selection);
            with_places(|places| {
                for place in TRANSPORT..PLACES {
                    refresh(&mut session, selection, places, place);
                }
                refresh_hand(&mut session, selection, places);
            });
            show_line(&session, selection);
        }
        buttons(&session, selection);
        lift_frame(selection);
    }

    /// On the boxes' first frame, from the console's texts: the transport
    /// box's name (38:0x2A), for its session copy, and each cell's default
    /// name (`offline_default`: "Box 1" in English, from 38:0x2A and the
    /// box default 39:0x45 with the cell's number), which every offline box
    /// without a name takes (`Offline::name_defaults`). A text that does not
    /// load leaves its name empty.
    fn name_offline(task: usize, selection: *mut u8) {
        let (offline, _) = unsafe { ROOM.boxes() };
        // A copy: the next message is loaded into the same work strings.
        let mut transport = [0u16; NAME_UNITS];
        if let Some(text) = context(selection).and_then(|context| message(context, TRANSPORT_BOX)) {
            let len = text
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(text.len())
                .min(NAME_UNITS - 1);
            transport[..len].copy_from_slice(&text[..len]);
            if let Some(name) = offline_core::boxfile::name_text(&transport) {
                offline.transport = name;
            }
        }
        for (index, default) in offline.defaults.iter_mut().enumerate() {
            if let Some(name) =
                default_text(task, index).and_then(|text| offline_default(text, &transport))
            {
                *default = name;
            }
        }
        offline.name_defaults();
    }

    /// The steps of a lift and of a glide, every frame before the
    /// original's update: the selection object's update (`001e9500`) runs
    /// its key update and then updates the screen's layout (`001e7e48` on
    /// `[selection + 0x5c]`: its animations and the places the panes are
    /// drawn at), as the box screen runs its state before its layout
    /// (`00291bb4`, `00291bf4`). A pane shown, hidden or moved here is
    /// drawn so in this frame; after the update its place would only follow
    /// in the next, and it would stand for a frame where it was.
    /// The blue and the green hand follow the hand here too, after the
    /// lift has set it, so they stand where it stands in the same frame.
    fn before_update(session: &mut Session, selection: *mut u8) {
        session.new_frame();
        // The place the cursor was on until this frame's keys, which the
        // upper screen keeps showing should they take it to the buttons.
        if let Some(place) = cursor(selection).and_then(place_of) {
            session.last_shown = place;
            LAST_SHOWN.store(place, core::sync::atomic::Ordering::Relaxed);
        }
        lift_step(session, selection);
        land(session, selection);
        if let Some(put) = session.put {
            // The green hand's Pokémon that go back to a box the upper
            // screen does not show: it turns back to that box first, and
            // the glide is set up `TURN_FRAMES` later (`turns_back`).
            let ready = match session.turn {
                Some(frames) => {
                    session.turn = Some(frames.saturating_add(1));
                    turned(frames.saturating_add(1))
                }
                None if cursor(selection).is_some_and(|cursor| {
                    let shown = shown_place(cursor, session.last_shown, Some(put.from));
                    turns_back(session.mode, put, shown)
                }) =>
                {
                    session.turn = Some(0);
                    session.back = true;
                    preview(session, selection);
                    false
                }
                None => true,
            };
            if ready {
                session.put = None;
                session.turn = None;
                glide(session, selection, put);
            }
        }
        follow_hand(selection);
    }

    fn slid(ui: *mut u8, animation: usize, out: bool) -> bool {
        let animate: unsafe extern "aapcs" fn(*mut u8, usize, u32, u32) -> u32 =
            unsafe { transmute(ANIMATE) };
        unsafe { animate(ui, animation, 1, u32::from(out)) != 0 }
    }

    /// Replaces task `0x1B`'s update (vtable slot 3, `002a7bf0`): its set-up
    /// hides nothing and shows no notice (`002a8318`), so the question comes
    /// first, then the
    /// loading panel while the offline boxes are read (`Job::LoadOffline`),
    /// then the original's update with the boxes (state 9, scene 1).
    /// # Safety
    /// The original's call, with task `0x1B`.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_update(task: *mut u8) -> u32 {
        let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
            unsafe { transmute(TRANSFER_UPDATE) };
        let Some(mut session) = SESSION.take() else {
            return 0;
        };
        if session.task != task as usize {
            *session = Session::new();
            session.task = task as usize;
        }
        match session.opening {
            Opening::Boxes => {
                if let Some(selection) = selection_of(task) {
                    before_update(&mut session, selection);
                }
                drop(session);
                let result = unsafe { original(task) };
                if result == 0 {
                    if let Some(selection) = selection_of(task) {
                        frame(selection);
                    }
                }
                result
            }
            Opening::Loading { first } => {
                session.opening = Opening::Loading { first: false };
                drop(session);
                match unsafe { crate::hooks::transfer_load(task, first) } {
                    crate::hooks::TransferFrame::Loading => 0,
                    crate::hooks::TransferFrame::Ready => {
                        if let Some(mut session) = SESSION.take() {
                            session.opening = Opening::Boxes;
                        }
                        // As the original's state 5 goes on: scene 1, the
                        // selection, in state 9.
                        unsafe {
                            task.add(0x48).write(1);
                            write_word(task, 0x44, 9);
                            original(task)
                        }
                    }
                    crate::hooks::TransferFrame::Stopped(result) => result,
                }
            }
            _ => {
                // Once the error screen is up, nothing of the question runs.
                if let Some(result) = unsafe { crate::hooks::transfer_fault(task, false) } {
                    return result;
                }
                let ui = unsafe { follow(task, 0x2c).and_then(|manager| follow(manager, 0x80)) };
                let result = ui.and_then(|ui| question(&mut session, task, ui));
                drop(session);
                match result {
                    Some(result) => result,
                    None => unsafe { crate::hooks::transfer_fault(task, true) }.unwrap_or(0),
                }
            }
        }
    }

    /// `0022bca4(helper, pokemon, box, slot)` loads a box's slot into a
    /// Pokémon object: the object's first virtual call with the record,
    /// then `002204a0(pokemon, tag)`. `001db7c0(helper, box)` is the box's
    /// name.
    const LOAD_SLOT: usize = 0x0022_bca4;
    const SET_TAG: usize = 0x0022_04a0;
    const BOX_NAME: usize = 0x001d_b7c0;

    /// The selection object the key update was last called with: the
    /// upper screen's preview asks for the box under its cursor.
    static SELECTION: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

    /// What the blue or the green cursor holds, for the preview's hooks,
    /// which run while the session is taken: 0 for nothing, else the place
    /// + 1, with bit 31 for green. Set with every preview.
    static HOLDING: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    const GREEN: u32 = 1 << 31;
    /// The session's `last_shown`, for the preview's hooks.
    static LAST_SHOWN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    /// What is held goes back after a turn (`Session::back`).
    const BACK: u32 = 1 << 30;
    fn set_holding(session: &Session) {
        let value = match (session.mode, session.held) {
            (Mode::Swap, Some(place)) => place as u32 + 1,
            (Mode::Move, Some(place)) => (place as u32 + 1) | GREEN,
            _ => 0,
        };
        let back = if session.back { BACK } else { 0 };
        HOLDING.store(value | back, core::sync::atomic::Ordering::Relaxed);
    }

    /// What the upper screen shows when the original's preview asks for
    /// `boxed` (`upper`): the original asks for box 100 for any place past
    /// it, so there the cursor's place is taken, one of the four cells.
    fn upper_shown(boxed: u32) -> Option<Upper> {
        let selection = SELECTION.load(core::sync::atomic::Ordering::Relaxed) as *mut u8;
        pointer(selection as u32)?;
        let holding = HOLDING.load(core::sync::atomic::Ordering::Relaxed);
        let back = holding & BACK != 0;
        let place = (holding & !(GREEN | BACK)) as usize;
        let (mode, held) = match place {
            0 => (Mode::Check, None),
            _ if holding & GREEN != 0 => (Mode::Move, Some(place - 1)),
            _ => (Mode::Swap, Some(place - 1)),
        };
        let cursor = if boxed == 99 {
            let last = LAST_SHOWN.load(core::sync::atomic::Ordering::Relaxed);
            let green = held.filter(|_| mode == Mode::Move);
            cursor(selection)
                .map(|cursor| shown_place(cursor, last, green))
                .unwrap_or(99)
        } else {
            boxed as usize
        };
        Some(upper(mode, held, upper_cursor(held, back, cursor)))
    }

    /// Replaces the preview's load of a slot (`bl 0022bca4` at `0028644c`):
    /// the slot of the place the upper screen shows (`upper_shown`), an empty one
    /// while its Pokémon are lifted out.
    /// # Safety
    /// The original's call.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_slot(
        helper: *mut u8,
        pokemon: *mut u8,
        boxed: u32,
        slot: u32,
    ) {
        match upper_shown(boxed).filter(|_| slot < 30) {
            Some(upper) if upper.emptied || upper.place != boxed as usize => unsafe {
                load_place(helper, pokemon, upper.place, slot as usize, upper.emptied)
            },
            _ => unsafe { load_place(helper, pokemon, boxed as usize, slot as usize, false) },
        }
    }

    /// Loads slot `slot` of `place` into a Pokémon object, as `0022bca4`
    /// loads a box's: a box of the Bank by the original's call; the
    /// transport box, an offline box, or an empty slot by the patch.
    /// # Safety
    /// The original's helper and Pokémon object.
    unsafe fn load_place(
        helper: *mut u8,
        pokemon: *mut u8,
        place: usize,
        slot: usize,
        empty: bool,
    ) {
        let load: unsafe extern "aapcs" fn(*mut u8, *mut u8, u32, u32) =
            unsafe { transmute(LOAD_SLOT) };
        if place < TRANSPORT && !empty {
            return unsafe { load(helper, pokemon, place as u32, slot as u32) };
        }
        let (record, tag) = (!empty)
            .then(|| with_places(|places| places.slot(place, slot)))
            .flatten()
            .unwrap_or((transfer_screen::empty_record(), 0));
        // The record goes to the original from the room for one box file,
        // which is aligned as the original's own records are and which the
        // write uses only after the screen; a box file's records are not.
        let (_, scratch) = unsafe { ROOM.boxes() };
        scratch[..record.len()].copy_from_slice(&record);
        let record = scratch.as_ptr();
        unsafe {
            let Some(vtable) = follow(pokemon, 0) else {
                return;
            };
            // A function pointer read from the original's object is called
            // only if it lies in Bank's code (`native::code`).
            let Some(address) = code::<bank_common::program::Bank15>(read_word(vtable, 0)) else {
                return;
            };
            let from_record: unsafe extern "aapcs" fn(*mut u8, *const u8) = transmute(address);
            let set_tag: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(SET_TAG);
            from_record(pokemon, record);
            set_tag(pokemon, u32::from(tag));
        }
    }

    /// Replaces the preview's box name (`bl 001db7c0` at `00285ed8` and
    /// `00286db0`): the name of the place the upper screen shows (`upper_shown`);
    /// for the transport box "Transport Box" (38:0x2A), for an offline box
    /// its name, which is its cell's default unless it was given another;
    /// both as the screen's first frame keeps them (`name_offline`).
    /// # Safety
    /// The original's call.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_name(helper: *mut u8, boxed: u32) -> *const u8 {
        let name: unsafe extern "aapcs" fn(*mut u8, u32) -> *const u8 =
            unsafe { transmute(BOX_NAME) };
        let place = upper_shown(boxed).map_or(boxed as usize, |upper| upper.place);
        if place < TRANSPORT {
            return unsafe { name(helper, place as u32) };
        }
        // The name's bytes (`name_offline`), into an aligned copy: a file's
        // bytes are not aligned.
        let (offline, _) = unsafe { ROOM.boxes() };
        let built = unsafe { &mut *BUILT.0.get() };
        let label = if place == TRANSPORT {
            &offline.transport[..]
        } else {
            &offline.files[place - transfer_screen::OFFLINE][offline_core::boxfile::NAME]
        };
        for (unit, bytes) in built.name.iter_mut().zip(label.chunks_exact(2)) {
            *unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        }
        built.name[NAME_UNITS - 1] = 0;
        built.name.as_ptr().cast()
    }

    /// A message of the file of `context`, its values filled in
    /// (`bank_common::ui::filled`): its text with its end.
    fn message(context: *mut u8, message: u32) -> Option<&'static mut [u16]> {
        unsafe { bank_common::ui::filled(context, message, |_| {}) }
    }

    /// A box of Bank's default name, as the original names the boxes of a
    /// new Bank (`002ae8b0`): message `0x45` of the shared UI's text file
    /// (39) with the box's number, three digits
    /// (`002298e4(words, 0, number, 3, 0, 0010a9f0() != 0)`), filled in. The
    /// original gives it to the box by `002ba124(helper, text, box)`, which
    /// copies the name's text; `Places::swap` does that itself.
    const SET_NUMBER: usize = 0x0022_98e4;
    const DIGIT_KIND: usize = 0x0010_a9f0;
    const DEFAULT_NAME: u32 = 0x45;
    /// The default name of box `boxed` in the console's language, with its
    /// end: in the shared UI's second work string. The shared UI is task
    /// `0x1B`'s, `[[task + 0x2c] + 0x80]`, as its set-up finds it
    /// (`002a8284`..`002a828c`); its context's file is text file 39.
    fn default_text(task: usize, boxed: usize) -> Option<&'static mut [u16]> {
        unsafe {
            let ui = follow(task as *mut u8, 0x2c).and_then(|owner| follow(owner, 0x80))?;
            let context = follow(ui, 0x5c)?;
            let number: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32, u32, u32) =
                transmute(SET_NUMBER);
            let kind: unsafe extern "aapcs" fn() -> u32 = transmute(DIGIT_KIND);
            bank_common::ui::filled(context, DEFAULT_NAME, |words| {
                number(words, 0, boxed as u32 + 1, 3, 0, u32::from(kind() != 0))
            })
        }
    }
    /// The Bank's helper, `[bank + 0xbb520]`, which names a box.
    fn helper() -> Option<*mut u8> {
        unsafe {
            follow(MANAGER as *mut u8, 0)
                .and_then(|manager| follow(manager, 0x1c))
                .and_then(|coordinator| follow(coordinator, 0xcc))
                .and_then(|bank| follow(bank, 0xbb520))
        }
    }
    /// The default name of `place` in the console's language, for a blue
    /// move (`Places::swap`): a box of Bank's from the console's texts
    /// (`default_text`), an offline box's its cell's (`name_offline`). All
    /// zero if it is not known.
    fn default_of(
        task: usize,
        places: &Places,
        place: usize,
    ) -> [u8; offline_core::boxfile::NAME_TEXT] {
        if place >= transfer_screen::OFFLINE {
            return places.offline.defaults[place - transfer_screen::OFFLINE];
        }
        default_text(task, place)
            .and_then(|text| offline_core::boxfile::name_text(text))
            .unwrap_or([0; offline_core::boxfile::NAME_TEXT])
    }

    /// `001d5ec0(helper)`: the Pokémon in the Bank's hundred boxes.
    const BANK_COUNT: usize = 0x001d_5ec0;

    /// Replaces task `0x1B`'s count of the Bank's Pokémon in its set-up
    /// (`bl 001d5ec0` at `002a8308`): the screen opens also for a Bank
    /// without Pokémon, whose offline boxes may hold some. The original
    /// showed "What are we going to move?" (37:0x1A) instead.
    /// # Safety
    /// The original's call, with the Bank's helper.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_count(helper: *mut u8) -> u32 {
        let count: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(BANK_COUNT) };
        unsafe { count(helper) }.max(1)
    }

    /// Task `0x1B` has ended and the router answered `destination`
    /// (`Session::ended`).
    pub fn end(destination: u32) {
        if let Some(mut session) = SESSION.take() {
            session.ended(destination);
        }
    }
    /// What Yes asked to write, for Save and Quit's task, taken once.
    pub fn take_commit() -> Option<[bool; PLACES]> {
        SESSION.take()?.commit.take()
    }

    /// Replaces the parser slot of the layout's vtable (`0035f118`, the
    /// original's `0024f1f8`): the transfer screen's lower layout is handed over
    /// with the panes the patch adds (`layout::extend`); every other the
    /// original's.
    /// # Safety
    /// The original's call.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_layout_build(
        this: *mut u8,
        bytes: *const u8,
        accessor: *mut u8,
        extra: u32,
    ) -> u32 {
        let build: unsafe extern "aapcs" fn(*mut u8, *const u8, *mut u8, u32) -> u32 =
            unsafe { transmute(LAYOUT_BUILD) };
        let original = || unsafe { build(this, bytes, accessor, extra) };
        if pointer(bytes as u32).is_none() {
            return original();
        }
        let size = unsafe { (bytes.add(0xc) as *const u32).read_unaligned() } as usize;
        if size != layout::ORIGINAL_SIZE {
            return original();
        }
        let input = unsafe { core::slice::from_raw_parts(bytes, size) };
        if &input[..4] != b"CLYT" || offline_core::crc32(input) != layout::ORIGINAL_CRC {
            return original();
        }
        // Only the transfer screen builds this layout, and the room stays
        // its own while the screen is up.
        let room = unsafe { ROOM.layout() };
        if layout::extend(input, room).is_none() {
            return original();
        }
        unsafe { build(this, room.as_ptr(), accessor, extra) }
    }

    /// Replaces the selection object's making of its cursor (`bl 001e663c`
    /// at `00286cf4`): the cursor gets the extended places
    /// (`layout::extend_places`) instead of the original's 113.
    /// # Safety
    /// The original's call.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_transfer_cursor(
        cursor: *mut u8,
        places: *const u8,
        count: u32,
        r3: u32,
        stack: u32,
    ) -> *mut u8 {
        let make: unsafe extern "aapcs" fn(*mut u8, *const u8, u32, u32, u32) -> *mut u8 =
            unsafe { transmute(MAKE_CURSOR) };
        let built = unsafe { &mut *BUILT.0.get() };
        if places as usize != PLACES_TABLE || count != layout::PLACES as u32 {
            return unsafe { make(cursor, places, count, r3, stack) };
        }
        if !built.places_made {
            let original = unsafe { &*(PLACES_TABLE as *const [u8; layout::PLACES * 8]) };
            layout::extend_places(original, &mut built.places);
            built.places_made = true;
        }
        let made = unsafe {
            make(
                cursor,
                built.places.as_ptr(),
                layout::EXTENDED_PLACES as u32,
                r3,
                stack,
            )
        };
        // The four cells and the hands as the session starts; the upper
        // screen's layout and icon manager are new.
        if let Some(mut session) = SESSION.take() {
            session.mode = Mode::Check;
            session.held = None;
            session.cells_shown = false;
            set_holding(&session);
        }
        built.lifted = None;
        built.icons = [Icon([0; 0x24]); 30];
        made
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn units(text: &str) -> std::vec::Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn an_offline_box_s_category_is_the_word_for_box_in_every_language() {
        let name = |text: &str| offline_core::boxfile::name_text(&units(text)).unwrap();
        for (transport, word) in [
            ("てんそうボックス", "ボックス"),
            ("転送ボックス", "ボックス"),
            ("Transport Box", "Box"),
            ("Boîte Transfert", "Boîte"),
            ("Trasferibox", "Box"),
            ("Transferbox", "Box"),
            ("Caja de Traslado", "Caja"),
            ("전송박스", "박스"),
            ("传送盒", "盒"),
            ("傳送盒", "盒"),
        ] {
            assert_eq!(
                offline_category(&name(transport)),
                Some(name(word)),
                "{transport}"
            );
        }
        // A name that is none of the original's, or none at all.
        assert_eq!(offline_category(&name("Transport")), None);
        assert_eq!(
            offline_category(&[0; offline_core::boxfile::NAME_TEXT]),
            None
        );
    }

    #[test]
    fn an_offline_box_s_default_name_follows_the_boxes_pattern_in_every_language() {
        // Each archive's transport box name (38:0x2A), its box 3's default
        // name (39:0x45 filled in), and what offline box 3 is named.
        let archives = [
            ("てんそうボックス", "バンク3", "ボックス3"),
            ("転送ボックス", "バンク3", "ボックス3"),
            ("Transport Box", "Bank 3", "Box 3"),
            ("Boîte Transfert", "Coffre 3", "Boîte 3"),
            ("Trasferibox", "Box Banca 3", "Box 3"),
            ("Transferbox", "Online-Box: 3", "Box: 3"),
            ("Caja de Traslado", "Caja Fuerte 3", "Caja 3"),
            ("전송박스", "뱅크3", "박스3"),
            ("传送盒", "银行3", "盒3"),
            ("傳送盒", "銀行3", "盒3"),
        ];
        assert_eq!(archives.len(), BOX_WORD.len());
        let name = |text: &str| offline_core::boxfile::name_text(&units(text)).unwrap();
        for (transport, default, expected) in archives {
            assert_eq!(
                offline_default(&units(default), &units(transport)),
                Some(name(expected)),
                "{transport}"
            );
        }
        // The texts end at their first zero, as the Japanese archives pad
        // theirs; full-width digits stay as the console wrote them.
        let mut padded = units("てんそうボックス");
        padded.extend([0; 8]);
        assert_eq!(
            offline_default(&units("バンク１２"), &padded),
            Some(name("ボックス１２"))
        );
        // Another name, or a default without a number: none.
        assert_eq!(offline_default(&units("Bank 3"), &units("Transport")), None);
        assert_eq!(
            offline_default(&units("Bank"), &units("Transport Box")),
            None
        );
    }

    #[test]
    fn the_question_is_shortened_in_every_language_only_when_it_is_the_original() {
        assert!(QUESTION.iter().all(|(_, cuts, _)| cuts.len() <= 2));
        for (page, _, left) in QUESTION {
            let mut room = units(page);
            room.extend([7; 8]);
            let len = shorten_question(&mut room, page.encode_utf16().count());
            assert_eq!(room[..len], units(left)[..], "{left}");
            assert_eq!(room[len], 0);
        }
        // The user's example, in German.
        let (page, _, _) = QUESTION[4];
        let mut room = units(page);
        room.push(0);
        let whole = room.len() - 1;
        let len = shorten_question(&mut room, whole);
        assert_eq!(
            std::string::String::from_utf16(&room[..len]).unwrap(),
            "Möchtest du die Pokémon, die sich derzeit in Pokémon Bank\nbefinden, transferieren?"
        );
        // Any other text stays whole.
        for other in ["Do you still want to move the Pokémon?", ""] {
            let mut room = units(other);
            room.push(0);
            let len = other.encode_utf16().count();
            assert_eq!(shorten_question(&mut room, len), len);
            assert_eq!(room[..len], units(other)[..]);
        }
    }

    #[test]
    fn the_question_goes_on_as_the_original_listener_answers() {
        // Its first button, "Begin the move", is state 5; "Cancel" 6.
        assert_eq!(button_pressed(5), Some(true));
        assert_eq!(button_pressed(6), Some(false));
        for state in [0, 2, 3, 4, 7, 9] {
            assert_eq!(button_pressed(state), None);
        }
        // The buttons come once the text is written out, as task 3 waits.
        assert!(written_out(0) && written_out(4));
        assert!(!written_out(1) && !written_out(2) && !written_out(3));
        assert_eq!(Session::new().opening, Opening::Ask);
    }

    #[test]
    fn the_lift_dips_the_hand_takes_the_box_and_rises_with_it() {
        use crate::layout::{HAND_AT, HAND_LOWERED};
        let frames: std::vec::Vec<_> = (1..=5).map(lifting).collect();
        use LiftFrame::*;
        assert_eq!(frames, [Dip, Take, Rise, Done, Done]);
        // Taken on its place under the dipped hand, then 10 over it under
        // the hand at rest.
        assert_eq!(held_height(HAND_LOWERED.1), 0.0);
        assert_eq!(held_height(HAND_AT.1), 10.0);
    }

    #[test]
    fn a_glide_takes_8_steps_of_an_eighth_from_the_frame_after_its_setup() {
        let frames: std::vec::Vec<_> = (1..=11).map(landing).collect();
        use LandingFrame::*;
        assert_eq!(
            frames,
            [Step, Step, Step, Step, Step, Step, Step, Step, Land, Done, Done]
        );
        let mut glide = Glide::new((0.0, 10.0), (24.0, -26.0));
        let mut seen = std::vec::Vec::new();
        for frame in 1..=9 {
            glide.frame(landing(frame));
            seen.push(glide.at);
        }
        assert_eq!(seen[0], (3.0, 5.5));
        assert_eq!(seen[3], (12.0, -8.0));
        assert_eq!(seen[7], (24.0, -26.0));
        assert_eq!(seen[8], (24.0, -26.0));
    }

    #[test]
    fn a_lifted_place_is_hidden_for_blue_and_empty_for_green() {
        let held = Some(3);
        assert_eq!(place_frame(Mode::Swap, held, 3, 2), None);
        assert_eq!(place_frame(Mode::Move, held, 3, 2), Some(0));
        for mode in [Mode::Check, Mode::Swap, Mode::Move] {
            assert_eq!(place_frame(mode, held, 4, 1), Some(1));
            assert_eq!(place_frame(mode, None, 3, 2), Some(2));
        }
        // The upper screen's box icon: empty over the green cursor's own box.
        assert_eq!(upper_state(upper(Mode::Move, held, 3), 1), 0);
        assert_eq!(upper_state(upper(Mode::Move, held, 4), 1), 1);
        assert_eq!(upper_state(upper(Mode::Swap, held, 4), 2), 2);
    }

    #[test]
    fn start_cycles_red_blue_green() {
        assert_eq!(Mode::Check.next(), Mode::Swap);
        assert_eq!(Mode::Swap.next(), Mode::Move);
        assert_eq!(Mode::Move.next(), Mode::Check);
    }

    #[test]
    fn the_green_hand_s_pokemon_go_back_after_a_turn_of_ten_frames() {
        let back = |from, to| Put {
            from,
            to,
            swapped: false,
            refused: false,
        };
        // Back to their box while the upper screen shows another: a turn.
        assert!(turns_back(Mode::Move, back(3, 3), TRANSPORT));
        assert!(turns_back(Mode::Move, back(3, 3), TRANSPORT + 2));
        // It shows their box, a put-down elsewhere, the blue hand: none.
        assert!(!turns_back(Mode::Move, back(3, 3), 3));
        assert!(!turns_back(Mode::Move, back(3, 7), 7));
        assert!(!turns_back(Mode::Swap, back(3, 3), TRANSPORT));
        // Back from the button row, where the upper screen shows their box:
        // none, from any box.
        for from in [0, 37, 99, TRANSPORT + 1] {
            assert!(!turns_back(
                Mode::Move,
                back(from, from),
                shown_place(105, 12, Some(from))
            ));
        }
        // On the button row the upper screen keeps the last place shown, with
        // the red hand, and with the blue or green hand holding nothing; the
        // green hand's Pokémon show their box.
        assert_eq!(shown_place(105, 12, None), 12);
        assert_eq!(shown_place(112, TRANSPORT + 2, None), TRANSPORT + 2);
        assert_eq!(shown_place(105, 12, Some(37)), 37);
        // On a place, that place.
        assert_eq!(shown_place(40, 12, Some(37)), 40);
        assert_eq!(shown_place(113, 12, None), TRANSPORT);
        // The original draws the upper screen right after RESET and ALL
        // only on a box of the Bank; on a cell and the button row the patch
        // draws it again.
        assert!(original_draws(0) && original_draws(99));
        assert!(!original_draws(105) && !original_draws(113) && !original_draws(116));
        // The glide is set up ten frames after the turn began.
        assert!(!turned(0) && !turned(9) && turned(10));
        // Meanwhile the upper screen shows their box, not the cursor's.
        assert_eq!(upper_cursor(Some(3), true, TRANSPORT), 3);
        assert_eq!(upper_cursor(Some(3), false, TRANSPORT), TRANSPORT);
        assert_eq!(upper_cursor(None, true, 5), 5);
        let shown = upper(Mode::Move, Some(3), upper_cursor(Some(3), true, TRANSPORT));
        assert_eq!(
            (shown.place, shown.emptied, shown.lifted),
            (3, true, Some(3))
        );
        // The cursor's place of every place, and back.
        for place in 0..PLACES {
            assert_eq!(place_of(cursor_of(place)), Some(place));
        }
        assert_eq!((cursor_of(99), cursor_of(TRANSPORT)), (99, 113));
    }

    #[test]
    fn the_cursor_places_are_the_boxes_then_the_four_cells() {
        assert_eq!(place_of(0), Some(0));
        assert_eq!(place_of(99), Some(99));
        assert_eq!(place_of(100), None);
        assert_eq!(place_of(112), None);
        assert_eq!(place_of(113), Some(TRANSPORT));
        assert_eq!(place_of(116), Some(OFFLINE + 2));
        assert_eq!(place_of(117), None);
    }

    #[test]
    fn the_line_names_the_group_and_place_by_the_hand_and_what_it_holds() {
        // Boxes 0..99: group place % 3 at position place / 3; group 0 has 34.
        let group =
            |place: usize| (place < TRANSPORT).then_some(((place % 3) as u8, (place / 3) as u16));
        let amount = |group: u8| if group == 0 { 34 } else { 33 };
        let at = |mode, held, cursor| line(mode, held, cursor, group, amount);
        let of = |group: u8, index: u16, amount: u16| Line::Group {
            of: Of::Group(group),
            index,
            amount,
        };
        // Red, and the button row: the title.
        assert_eq!(at(Mode::Check, None, Some(7)), Line::Title);
        assert_eq!(at(Mode::Swap, None, None), Line::Title);
        assert_eq!(at(Mode::Move, Some(3), None), Line::Title);
        // Blue and green: the box under the cursor.
        for mode in [Mode::Swap, Mode::Move] {
            assert_eq!(at(mode, None, Some(7)), of(1, 3, 33));
            assert_eq!(at(mode, None, Some(99)), of(0, 34, 34));
            assert_eq!(
                at(mode, None, Some(TRANSPORT)),
                Line::Group {
                    of: Of::Place(TRANSPORT),
                    index: 1,
                    amount: 1
                }
            );
            assert_eq!(
                at(mode, None, Some(OFFLINE + 1)),
                Line::Group {
                    of: Of::Place(OFFLINE + 1),
                    index: 2,
                    amount: 3
                }
            );
        }
        // Blue, holding box 3 (group 0, 2nd): over box 6 of its group, box 6's
        // position; over box 7 of another group, its own; over itself, its own.
        assert_eq!(at(Mode::Swap, Some(3), Some(6)), of(0, 3, 34));
        assert_eq!(at(Mode::Swap, Some(3), Some(7)), of(0, 2, 34));
        assert_eq!(at(Mode::Swap, Some(3), Some(3)), of(0, 2, 34));
        // Holding an offline box over a box of Bank: that place's group.
        assert_eq!(at(Mode::Swap, Some(OFFLINE), Some(7)), of(1, 3, 33));
        // Over an offline box or the transport box: that box.
        assert_eq!(
            at(Mode::Swap, Some(3), Some(OFFLINE + 2)),
            Line::Group {
                of: Of::Place(OFFLINE + 2),
                index: 3,
                amount: 3
            }
        );
        // Green, holding: the box under the cursor as it is.
        assert_eq!(at(Mode::Move, Some(3), Some(7)), of(1, 3, 33));
        // A category the Bank has no group for: the title.
        let none = |_: usize| None;
        assert_eq!(line(Mode::Swap, None, Some(5), none, amount), Line::Title);
    }

    #[test]
    fn a_group_s_count_loses_its_brackets_in_every_form() {
        let run = |text: &str| {
            let mut units: std::vec::Vec<u16> = text.encode_utf16().collect();
            let len = units.len();
            units.push(0);
            let new = unbracketed(&mut units, len)?;
            assert_eq!(units[new], 0);
            std::string::String::from_utf16(&units[..new]).ok()
        };
        assert_eq!(run("(3/12)").as_deref(), Some("3/12"));
        assert_eq!(run("（2/3）").as_deref(), Some("2/3"));
        assert_eq!(run("(1/1)").as_deref(), Some("1/1"));
        assert_eq!(run("3/12"), None);
        assert_eq!(run("(3/12）"), None);
        assert_eq!(run("("), None);
        assert_eq!(run(""), None);
    }

    #[test]
    fn the_page_turn_and_l_go_on_by_frames_also_without_a_key() {
        // An L that moved a box: `TURN_FRAMES` frames of page turn follow,
        // counted at the start of every frame, also of one without a key,
        // so nothing is left of it once they are over.
        let mut session = Session::new();
        session.paging = TURN_FRAMES;
        let mut turning = 0;
        for _ in 0..TURN_FRAMES + 3 {
            session.new_frame();
            turning += usize::from(session.turning);
        }
        assert_eq!(turning, usize::from(TURN_FRAMES));
        assert!(!session.paging_keys());
        // Three frames of L, a frame without any key (Bank skips the key
        // update then), three frames of L: never four in a row, so L never
        // fires, as letting go starts it again.
        for frame in 0..7 {
            session.new_frame();
            if frame != 3 {
                session.keyed = true;
                assert_ne!(
                    session.shoulder.step(true, false),
                    ShoulderKey::Fire,
                    "{frame}"
                );
            }
        }
        // Four in a row fire.
        let mut session = Session::new();
        let keys: std::vec::Vec<ShoulderKey> = (0..4)
            .map(|_| {
                session.new_frame();
                session.keyed = true;
                session.shoulder.step(true, false)
            })
            .collect();
        assert_eq!(keys.last(), Some(&ShoulderKey::Fire));
    }

    #[test]
    fn l_held_fires_on_its_fourth_frame_and_then_at_the_pads_repeat() {
        use ShoulderKey::{Fire, Free, Hold, Idle};
        let run = |frames: &[(bool, bool)]| {
            let mut shoulder = Shoulder::default();
            frames
                .iter()
                .map(|&(held, again)| {
                    let key = shoulder.step(held, again);
                    // Only its first three frames and a fire read nothing
                    // else.
                    assert_eq!(shoulder.blocks(), matches!(key, Hold | Fire));
                    key
                })
                .collect::<std::vec::Vec<_>>()
        };
        // A press of three frames or less: nothing fires.
        let tap = [(true, true), (true, false), (true, false), (false, false)];
        assert_eq!(run(&tap), [Hold, Hold, Hold, Idle]);
        // Held on: the fourth frame fires, then each repeat tick, and between
        // them every key works.
        let hold = [
            (true, true),
            (true, false),
            (true, false),
            (true, false),
            (true, false),
            (true, true),
            (true, false),
            (true, true),
        ];
        assert_eq!(run(&hold), [Hold, Hold, Hold, Fire, Free, Fire, Free, Fire]);
        // Let go and pressed again: it waits three frames again.
        let again = [
            (true, false),
            (true, false),
            (true, false),
            (true, false),
            (false, false),
            (true, true),
            (true, true),
            (true, true),
            (true, true),
        ];
        assert_eq!(
            run(&again),
            [Hold, Hold, Hold, Fire, Idle, Hold, Hold, Hold, Fire]
        );
    }

    #[test]
    fn l_moves_a_box_one_down_and_the_first_to_the_last() {
        assert_eq!(lowered(12, 13), 11);
        assert_eq!(lowered(1, 13), 0);
        assert_eq!(lowered(0, 13), 12);
        assert_eq!(lowered(0, 1), 0);
    }

    #[test]
    fn the_count_s_hundred_becomes_the_total_in_every_form() {
        let run = |text: &str, total: u16| {
            let mut units: std::vec::Vec<u16> = text.encode_utf16().collect();
            let len = units.len();
            units.push(0);
            let new = out_of(&mut units, len, total).unwrap();
            assert_eq!(units[new], 0);
            std::string::String::from_utf16(&units[..new]).unwrap()
        };
        assert_eq!(run("(5/100)", 104), "(5/104)");
        assert_eq!(run("(12/100)", 13), "(12/13)");
        assert_eq!(run("(1/100)", 1), "(1/1)");
        assert_eq!(run("（2/100）", 3), "（2/3）");
        assert_eq!(run("(100/100)", 100), "(100/100)");
        let mut none: std::vec::Vec<u16> = "(1/99)".encode_utf16().collect();
        none.push(0);
        assert_eq!(out_of(&mut none, 6, 3), None);
    }

    #[test]
    fn the_upper_screen_keeps_a_blue_box_and_lifts_green_pokemon() {
        let plain = |place| Upper {
            place,
            emptied: false,
            lifted: None,
        };
        // Nothing held, or red: what is under the cursor.
        for mode in [Mode::Check, Mode::Swap, Mode::Move] {
            assert_eq!(upper(mode, None, 7), plain(7));
        }
        // Blue: the held box wherever the cursor goes.
        assert_eq!(upper(Mode::Swap, Some(3), 7), plain(3));
        assert_eq!(upper(Mode::Swap, Some(TRANSPORT), 3), plain(TRANSPORT));
        // Green: the box under the cursor, the held Pokémon over it; their
        // own box without them.
        assert_eq!(
            upper(Mode::Move, Some(3), 7),
            Upper {
                place: 7,
                emptied: false,
                lifted: Some(3),
            }
        );
        assert_eq!(
            upper(Mode::Move, Some(OFFLINE), OFFLINE),
            Upper {
                place: OFFLINE,
                emptied: true,
                lifted: Some(OFFLINE),
            }
        );
        // The label is blue for the transport box shown, also emptied or
        // with other Pokémon over it, and for nothing else.
        assert!(blue_label(upper(Mode::Check, None, TRANSPORT)));
        assert!(blue_label(upper(Mode::Move, Some(TRANSPORT), TRANSPORT)));
        assert!(blue_label(upper(Mode::Move, Some(3), TRANSPORT)));
        assert!(!blue_label(upper(Mode::Move, Some(TRANSPORT), 3)));
        assert!(!blue_label(upper(Mode::Swap, Some(OFFLINE), TRANSPORT)));
        for place in [0, 99, OFFLINE, PLACES - 1] {
            assert!(!blue_label(upper(Mode::Check, None, place)));
        }
    }

    #[test]
    fn what_yes_asked_to_write_is_kept_only_for_the_save_task_that_follows() {
        use crate::navigation::{destination, CLEANUP, SAVE, TRANSFER_SCREEN};
        let asked = || Session {
            task: 0x1000,
            commit: Some([false; PLACES]),
            ..Session::new()
        };
        // Every way the transfer screen's task can end: only the outcome
        // that asks for the write, not cancelled, leads to Save and Quit's
        // task with the write still asked for.
        for outcome in 0..=u8::MAX {
            for native in 0..=0x1d {
                for cancelled in [false, true] {
                    let next = destination(TRANSFER_SCREEN, outcome, native, cancelled);
                    let mut session = asked();
                    session.ended(next);
                    assert_eq!(session.task, 0);
                    assert_eq!(
                        session.commit.is_some(),
                        outcome == OUTCOME_WRITE && !cancelled,
                        "{outcome:#x} {native:#x} {cancelled}"
                    );
                    assert_eq!(session.commit.is_some(), next == SAVE);
                }
            }
        }
        // As when the scene ended without the outcome set, or cancelled.
        let mut session = asked();
        session.ended(CLEANUP);
        assert_eq!(session.commit, None);
    }

    #[test]
    fn done_and_back_ask_by_what_was_checked_and_moved() {
        // The original's question, and the text its bar shows.
        assert_eq!(done(true, false), (MOVE_TO_HOME, MOVE_TO_HOME));
        assert_eq!(done(true, true), (MOVE_TO_HOME, MOVE_TO_HOME));
        assert_eq!(done(false, true), (MOVE_TO_HOME, SAVE_AND_QUIT));
        assert_eq!(done(false, false), (QUIT_MOVING, QUIT_MOVING));
        // B's question quits, with the text of quitting without saving.
        assert_eq!(BACK_TEXT, QUIT_WITHOUT_SAVING);
        // Yes writes to the original's 38:0x38, also with the save text;
        // to its 38:0x39, DONE's and B's, it quits.
        assert!(writes(MOVE_TO_HOME));
        assert!(!writes(QUIT_MOVING));
        assert!(!writes(u32::MAX));
    }

    #[test]
    fn dones_question_loses_pokemon_home_in_every_language_only_when_it_is_the_original() {
        assert!(MOVE_QUESTION.iter().all(|(_, cuts, _)| cuts.len() <= 2));
        for (page, cuts, left) in MOVE_QUESTION {
            assert!(cuts.iter().all(|cut| cut.contains("Pokémon HOME")));
            assert!(!left.contains("HOME"));
            // As the message stands in its room: ended, with more behind.
            let mut room = units(page);
            room.extend([0; 8]);
            let len = shorten_move_question(&mut room, page.encode_utf16().count());
            assert_eq!(room[..len], units(left)[..], "{left}");
            assert_eq!(room[len], 0);
        }
        for other in [
            "Move these Pokémon to Pokémon HOME",
            "Quit moving Pokémon?",
            "",
        ] {
            let mut room = units(other);
            room.push(0);
            let len = other.encode_utf16().count();
            assert_eq!(shorten_move_question(&mut room, len), len);
            assert_eq!(room[..len], units(other)[..]);
        }
    }

    #[test]
    fn only_what_would_go_into_the_transport_box_goes_back_with_its_message() {
        assert_eq!(refused(Refusal::IntoTransport), Refused::Back);
        for refusal in [Refusal::Occupied, Refusal::Empty, Refusal::Place] {
            assert_eq!(refused(refusal), Refused::Stays);
        }
    }

    /// A session in each state that leaves the screen not free, with
    /// whether it is one where no key does anything (`moving`).
    fn busy_sessions() -> [(Session, bool); 5] {
        let put = Put {
            from: 3,
            to: 4,
            swapped: false,
            refused: false,
        };
        let landing = Landing {
            frames: 0,
            from: 3,
            to: 4,
            held: Glide::new((0.0, 0.0), (1.0, 1.0)),
            moved: None,
            lifted: None,
            refused: false,
        };
        [
            (
                Session {
                    held: Some(3),
                    ..Session::new()
                },
                false,
            ),
            (
                Session {
                    lifting: Some((0, 3)),
                    ..Session::new()
                },
                true,
            ),
            (
                Session {
                    put: Some(put),
                    ..Session::new()
                },
                true,
            ),
            (
                Session {
                    landing: Some(landing),
                    ..Session::new()
                },
                true,
            ),
            (
                Session {
                    notice: true,
                    ..Session::new()
                },
                false,
            ),
        ]
    }

    #[test]
    fn the_buttons_take_presses_only_when_they_can_do_something() {
        let free = Session::new();
        assert!(free.free() && !free.moving() && free.takes_events());
        for button in 0..3 {
            assert!(free.button_takes_presses(button, false));
            // Not under a question: ALL there would change what Yes takes.
            assert!(!free.button_takes_presses(button, true));
        }
        for button in 3..5 {
            assert!(!free.button_takes_presses(button, false));
            assert!(free.button_takes_presses(button, true));
        }
        for (index, (session, moving)) in busy_sessions().into_iter().enumerate() {
            assert!(!session.free(), "{index}");
            assert_eq!(session.moving(), moving, "{index}");
            assert!(!session.takes_events(), "{index}");
            for button in 0..3 {
                assert!(!session.button_takes_presses(button, false), "{index}");
            }
            // Yes and No answer a question whatever else is so.
            for button in 3..5 {
                assert!(session.button_takes_presses(button, true), "{index}");
            }
        }
    }

    #[test]
    fn the_buttons_update_waits_for_l_and_its_page_turn() {
        // In L's page turn the buttons look as they are, but their update
        // does nothing; after it, it goes on.
        let mut session = Session {
            paging: 1,
            ..Session::new()
        };
        session.new_frame();
        assert!(session.free() && session.paging_keys() && !session.takes_events());
        session.new_frame();
        assert!(session.takes_events());
        // L held: in its first three frames and when it fires, nothing;
        // between its fires the buttons' update goes on.
        let mut session = Session::new();
        for _ in 0..3 {
            assert_eq!(session.shoulder.step(true, true), ShoulderKey::Hold);
            assert!(session.free() && !session.takes_events());
        }
        assert_eq!(session.shoulder.step(true, true), ShoulderKey::Fire);
        assert!(!session.takes_events());
        assert_eq!(session.shoulder.step(true, false), ShoulderKey::Free);
        assert!(session.takes_events());
    }

    #[test]
    fn while_a_box_is_held_only_a_b_and_the_cursor_do_something() {
        let key = |keys: u32, place| held_key(|mask| keys & mask != 0, place);
        assert_eq!(key(KEY_A, Some(7)), HeldKey::Put(7));
        // Off the places, on the button row, A does nothing.
        assert_eq!(key(KEY_A, None), HeldKey::Nothing);
        assert_eq!(key(KEY_B, Some(7)), HeldKey::Back);
        assert_eq!(key(KEY_B, None), HeldKey::Back);
        for off in [KEY_X, KEY_Y, KEY_L, KEY_R, KEY_START] {
            assert_eq!(key(off, Some(7)), HeldKey::Nothing);
            // Not with a direction either: the cursor stays.
            assert_eq!(key(off | 1, Some(7)), HeldKey::Nothing);
        }
        // The directions, and no key at all, are the cursor's.
        for direction in [0, 1, 2, 4, 8] {
            assert_eq!(key(direction, Some(7)), HeldKey::Cursor);
        }
        // A before B, as the box screen reads them.
        assert_eq!(key(KEY_A | KEY_B, Some(7)), HeldKey::Put(7));
    }

    #[test]
    fn the_green_hand_does_not_lift_a_checked_box_the_blue_hand_not_the_transport_box() {
        for mode in [Mode::Swap, Mode::Move] {
            for place in [3, OFFLINE] {
                assert!(liftable(mode, place, 1, false));
                assert!(liftable(mode, place, 30, false));
                assert!(!liftable(mode, place, 0, false));
                assert!(!liftable(mode, place, 0, true));
            }
        }
        // The blue hand takes a checked box, with its check.
        assert!(liftable(Mode::Swap, 3, 1, true));
        assert!(!liftable(Mode::Move, 3, 1, true));
        // The red hand lifts nothing.
        assert!(!liftable(Mode::Check, 3, 1, false));
        // Only the green hand takes the transport box's Pokémon.
        assert!(!liftable(Mode::Swap, TRANSPORT, 3, false));
        assert!(!liftable(Mode::Swap, TRANSPORT, 3, true));
        assert!(liftable(Mode::Move, TRANSPORT, 3, false));
        assert!(!liftable(Mode::Move, TRANSPORT, 3, true));
    }
}
