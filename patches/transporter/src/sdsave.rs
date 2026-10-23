//! Black/White saves that TWiLight Menu++ and nds-bootstrap keep on the SD
//! card: which files are accepted, what each one is, and the bookkeeping that
//! ties an entry of the original game list to a file. No file access here.
//!
//! Only the names GodMode9 gives a cartridge dump are accepted, in TWiLight
//! Menu's default folder:
//!
//! ```text
//! /roms/nds/saves/POKEMON_<B|W|B2|W2>_IR<g><l>01_<rr>.sav
//! ```
//!
//! `<g>` is the game letter, `<l>` the language letter the original accepts,
//! `<rr>` the revision of that game's cartridge in that language (`revision`):
//! one name per game and language, no other. The four-letter game code the
//! original reads from a cartridge is taken from the name, so a save is
//! offered only if its own game and language bytes say the same
//! (`Save::identifies`): the original checks a save's layout, not which game
//! of a pair or which language it belongs to.

use crate::language;

/// Exact size of a Gen 5 save.
pub const SAVE_SIZE: u64 = 0x8_0000;
/// Longest path built here, with terminator.
pub const PATH_CAPACITY: usize = 48;
/// At most this many saves are offered: one per game, in the one language
/// that is listed. The original list holds 40 entries.
pub const MAX_SAVES: usize = GAMES.len();
/// Trainer block of a save copy, where the original reads the trainer's name
/// and ID (`0019A994`, `0019A970`); the language byte and the game byte
/// follow each other at `+0x1E`.
const TRAINER_IDENTITY: u64 = 0x19400 + 0x1E;

const FOLDER: &[u8] = b"/roms/nds/saves/POKEMON_";

/// One of the four games, as the original tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Game {
    /// Title part of the cartridge header and of the file name.
    pub title: &'static [u8],
    /// Third letter of the game code.
    pub letter: u8,
    /// Entry kind in the original game list (`00244EE8`): 1 Black, 2 White,
    /// 3 Black 2, 4 White 2.
    pub kind: u8,
    /// Game byte of the save, the version the original stores for the kind
    /// (`002448A0`).
    pub version: u8,
}
pub const GAMES: [Game; 4] = [
    Game {
        title: b"B",
        letter: b'B',
        kind: 1,
        version: 21,
    },
    Game {
        title: b"W",
        letter: b'A',
        kind: 2,
        version: 20,
    },
    Game {
        title: b"B2",
        letter: b'E',
        kind: 3,
        version: 23,
    },
    Game {
        title: b"W2",
        letter: b'D',
        kind: 4,
        version: 22,
    },
];

/// The revision of the cartridge of `game` in `language`, as the cartridge
/// dumps list them: 01 for the Japanese Black 2 and White 2, 00 for every
/// other game and language.
pub fn revision(game: u8, language: u8) -> u8 {
    let sequel = GAMES[game as usize % GAMES.len()].title.len() == 2;
    u8::from(sequel && language == b'J')
}

/// True for the game codes the original treats as a Gen 5 cartridge.
pub fn is_gen5_code(code: [u8; 4]) -> bool {
    code[0] == b'I'
        && code[1] == b'R'
        && GAMES.iter().any(|game| game.letter == code[2])
        && language::id_of_letter(code[3]).is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Save {
    /// Index into `GAMES`.
    pub game: u8,
    pub language: u8,
}
impl Save {
    pub const NONE: Self = Self {
        game: 0,
        language: 0,
    };
    fn info(self) -> Game {
        GAMES[self.game as usize % GAMES.len()]
    }
    pub fn kind(self) -> u8 {
        self.info().kind
    }
    /// The code a cartridge of this game reports, as the little-endian word
    /// the original stores.
    pub fn game_code(self) -> u32 {
        u32::from_le_bytes([b'I', b'R', self.info().letter, self.language])
    }
    /// Where the language and game bytes lie in the save's two copies.
    /// Black 2 and White 2 keep their second copy further on (`0019AA0C`).
    pub fn identity_offsets(self) -> [u64; 2] {
        let copy = if self.info().title.len() == 2 {
            0x26000
        } else {
            0x24000
        };
        [TRAINER_IDENTITY, copy + TRAINER_IDENTITY]
    }
    /// Whether the language and game bytes read at `identity_offsets` say
    /// this game in this language. A copy whose game byte is none of the four
    /// says nothing (never written); at least one copy has to say it, and no
    /// copy may say another game or language.
    pub fn identifies(self, copies: [[u8; 2]; 2]) -> bool {
        // The save's language byte is the language id.
        let language = language::id_of_letter(self.language);
        let wanted = [language.unwrap_or(0), self.info().version];
        let mut named = false;
        for copy in copies {
            if !GAMES.iter().any(|game| game.version == copy[1]) {
                continue;
            }
            if language.is_none() || copy != wanted {
                return false;
            }
            named = true;
        }
        named
    }
    /// Writes the terminated path and returns its length with terminator.
    /// `backup` names the copy made before the first change.
    pub fn path(self, backup: bool, out: &mut [u8; PATH_CAPACITY]) -> usize {
        let mut at = 0;
        let mut put = |bytes: &[u8]| {
            out[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(FOLDER);
        put(self.info().title);
        put(b"_IR");
        put(&[self.info().letter, self.language]);
        put(b"01_");
        put(&[b'0', b'0' + revision(self.game, self.language)]);
        put(b".sav");
        if backup {
            put(b".bak");
        }
        put(&[0]);
        at
    }
}

/// Item value for the cartridge slot; any other value indexes `Saves::list`.
const CARTRIDGE: u8 = u8::MAX;

/// One DS game the list can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// The cartridge slot, with the list kind of the game in it.
    Cartridge {
        kind: u8,
    },
    Save(Save),
}
impl Item {
    pub fn kind(self) -> u8 {
        match self {
            Self::Cartridge { kind } => kind,
            Self::Save(save) => save.kind(),
        }
    }
}

/// What the game list offers as DS games in this session: the cartridge, if
/// a Gen 5 one is inserted, and the saves found on the SD card. Also which
/// of them is presented to the original's cartridge code right now.
///
/// Only games in the language chosen on the language screen are offered. The
/// order is always Black, White, Black 2, White 2, whatever the source. A
/// cartridge in that language takes the place of its own game and hides the
/// SD save of that game, so the game in the slot is always the one used. A
/// Gen 5 cartridge in another language is not offered at all.
///
/// A removed Game Card concerns the cartridge alone (`card_missing`): a save
/// on the SD card is read and written whatever is in the slot.
#[derive(Clone, Copy, Debug)]
pub struct Saves {
    list: [Save; MAX_SAVES],
    count: u8,
    /// List kind of the cartridge's game, 0 without a listed cartridge.
    cartridge_kind: u8,
    /// A Gen 5 cartridge is inserted but is in another language.
    cartridge_hidden: bool,
    /// Everything offered, in list order, as item values.
    items: [u8; MAX_SAVES + 1],
    item_count: u8,
    /// Position in `items` of the item being read for the list.
    at: u8,
    /// The item presented right now.
    current: u8,
    /// List position at which the read in progress will add its entry.
    started_at: u8,
    /// What each DS entry of the game list stands for, as item values.
    entries: [u8; MAX_SAVES + 1],
    entry_count: u8,
    /// Saves already copied to their backup in this session, as a bit set.
    backed_up: u8,
    /// The Game Card was taken out in this session. The original's signal
    /// for it is cleared by every read that starts, a save's included, so
    /// it is kept here as well.
    cartridge_removed: bool,
    /// A write of this session has reached a save's file (`touch`).
    touched: bool,
}
impl Saves {
    pub const EMPTY: Self = Self {
        list: [Save::NONE; MAX_SAVES],
        count: 0,
        cartridge_kind: 0,
        cartridge_hidden: false,
        items: [CARTRIDGE; MAX_SAVES + 1],
        item_count: 0,
        at: 0,
        current: CARTRIDGE,
        started_at: 0,
        entries: [CARTRIDGE; MAX_SAVES + 1],
        entry_count: 0,
        backed_up: 0,
        cartridge_removed: false,
        touched: false,
    };

    /// A new session. `cartridge` is the game code the cartridge slot
    /// reported, if any. `language` is the letter of the language whose games
    /// are listed. `identity` gives, for a path that names a file of exactly
    /// `SAVE_SIZE` bytes that opens for reading and writing, the two bytes at
    /// each of the given offsets, and `None` for any other path. Each game has
    /// one name in a language (`Save::path`), and its save is offered if the
    /// bytes say that game and language (`Save::identifies`).
    ///
    /// The first item is presented in the cartridge's place: the cartridge
    /// itself if its game comes first, otherwise a save.
    pub fn scan(
        &mut self,
        cartridge: Option<[u8; 4]>,
        language: u8,
        mut identity: impl FnMut(&[u8], [u64; 2]) -> Option<[[u8; 2]; 2]>,
    ) {
        *self = Self::EMPTY;
        let cartridge = cartridge.filter(|&code| is_gen5_code(code));
        self.cartridge_hidden = cartridge.is_some_and(|code| code[3] != language);
        let cartridge = cartridge.filter(|code| code[3] == language);
        let mut path = [0; PATH_CAPACITY];
        for game in 0..GAMES.len() as u8 {
            let info = GAMES[game as usize];
            if cartridge.is_some_and(|code| code[2] == info.letter) {
                self.cartridge_kind = info.kind;
                self.items[self.item_count as usize] = CARTRIDGE;
                self.item_count += 1;
                continue;
            }
            let save = Save { game, language };
            let length = save.path(false, &mut path);
            let found = identity(&path[..length], save.identity_offsets())
                .is_some_and(|copies| save.identifies(copies));
            if found && (self.count as usize) < MAX_SAVES {
                self.list[self.count as usize] = save;
                self.items[self.item_count as usize] = self.count;
                self.item_count += 1;
                self.count += 1;
            }
        }
        if self.item_count != 0 {
            self.current = self.items[0];
        }
    }

    pub fn count(&self) -> usize {
        self.count as usize
    }
    /// True when a Gen 5 cartridge is inserted that is not offered, and no
    /// save stands in its place: the slot is then presented as holding no
    /// Pokémon game.
    pub fn hides_cartridge(&self) -> bool {
        self.cartridge_hidden && self.current().is_none()
    }
    /// The save currently presented; `None` while the cartridge slot itself
    /// is, which the original code then handles alone.
    pub fn current(&self) -> Option<Save> {
        (self.current < self.count).then(|| self.list[self.current as usize])
    }
    fn item(&self, value: u8) -> Item {
        if value < self.count {
            Item::Save(self.list[value as usize])
        } else {
            Item::Cartridge {
                kind: self.cartridge_kind,
            }
        }
    }

    /// Called when the read for a list entry has ended, with the number of
    /// list entries now present. Records the entry if one was added, and
    /// moves on to the next item. Returns that item if there is one to read
    /// next; `list_count` is where its entry will go.
    pub fn next_for_list(&mut self, list_count: usize) -> Option<Item> {
        if list_count == self.started_at as usize + 1
            && (self.entry_count as usize) < self.entries.len()
            && self.entry_count == self.started_at
        {
            // The original appended the entry for what was just read.
            self.entries[self.entry_count as usize] = self.current;
            self.entry_count += 1;
        }
        if self.at + 1 >= self.item_count || list_count > u8::MAX as usize {
            return None;
        }
        self.at += 1;
        self.current = self.items[self.at as usize];
        self.started_at = list_count as u8;
        Some(self.item(self.current))
    }

    /// What the DS entry at `position` of the game list stands for, as an
    /// item value.
    fn entry(&self, position: usize) -> Option<u8> {
        (position < self.entry_count as usize).then(|| self.entries[position])
    }

    /// The user picked the DS entry at `position` of the game list.
    pub fn select(&mut self, position: usize) {
        if let Some(item) = self.entry(position) {
            self.current = item;
        }
    }

    /// Whether the Game Card the presented item needs is gone: only ever
    /// for the cartridge slot, once the card was taken out in this session.
    /// `removed` asks the original's signal, which is only read; a removal
    /// it reports is kept until the next scan.
    pub fn card_missing(&mut self, removed: impl FnOnce() -> bool) -> bool {
        self.card_missing_for(self.current, removed)
    }
    /// The same for the DS entry at `position` of the game list, before it
    /// is picked. A position that is no DS entry the hooks know of is taken
    /// for the cartridge, whose answer is the original's.
    pub fn card_missing_at(&mut self, position: usize, removed: impl FnOnce() -> bool) -> bool {
        let item = self.entry(position).unwrap_or(CARTRIDGE);
        self.card_missing_for(item, removed)
    }
    fn card_missing_for(&mut self, item: u8, removed: impl FnOnce() -> bool) -> bool {
        if removed() {
            self.cartridge_removed = true;
        }
        item >= self.count && self.cartridge_removed
    }

    /// Whether the current save still needs its backup, and marks it done.
    pub fn take_backup_duty(&mut self) -> bool {
        let bit = 1u8 << (self.current as u32 % 8);
        let needed = self.backed_up & bit == 0;
        self.backed_up |= bit;
        needed
    }
    /// Undoes `take_backup_duty` after a failed backup.
    pub fn backup_failed(&mut self) {
        self.backed_up &= !(1u8 << (self.current as u32 % 8));
    }
    /// Notes that a write is about to reach a save's file.
    pub fn touch(&mut self) {
        self.touched = true;
    }
    /// Whether a write of this session has reached a save's file: from then
    /// on a failed write may leave the save half-written.
    pub fn touched(&self) -> bool {
        self.touched
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::language::GAME_LANGUAGES;
    use std::{string::String, vec, vec::Vec};

    fn text(save: Save, backup: bool) -> String {
        let mut out = [0; PATH_CAPACITY];
        let length = save.path(backup, &mut out);
        assert_eq!(out[length - 1], 0);
        String::from_utf8(out[..length - 1].to_vec()).unwrap()
    }
    fn save(game: u8, language: u8) -> Save {
        Save { game, language }
    }
    /// German is the listed language unless a test says otherwise.
    fn scan(present: &[&str]) -> Saves {
        scan_with(None, present)
    }
    fn scan_with(cartridge: Option<[u8; 4]>, present: &[&str]) -> Saves {
        scan_in(cartridge, b'D', present)
    }
    /// The language and game bytes in both copies of a save of this game.
    fn bytes_of(save: Save) -> [[u8; 2]; 2] {
        let language = language::id_of_letter(save.language).unwrap();
        [[language, GAMES[save.game as usize].version]; 2]
    }
    /// The save a present file is by its name, so that its bytes match it.
    fn named(path: &[u8]) -> Save {
        let at = path.windows(3).position(|w| w == b"_IR").unwrap() + 3;
        let game = GAMES.iter().position(|g| g.letter == path[at]).unwrap();
        save(game as u8, path[at + 1])
    }
    fn scan_in(cartridge: Option<[u8; 4]>, language: u8, present: &[&str]) -> Saves {
        let mut saves = Saves::EMPTY;
        saves.scan(cartridge, language, |path, _| {
            let name = core::str::from_utf8(&path[..path.len() - 1]).unwrap();
            present.contains(&name).then(|| bytes_of(named(path)))
        });
        saves
    }

    #[test]
    fn every_game_and_language_has_its_one_name() {
        // German White, White 2 and Black 2 are the names confirmed on a
        // console. Revision 01 only for the Japanese Black 2 and White 2.
        let mut names = Vec::new();
        for (_, language) in GAME_LANGUAGES {
            for game in 0..GAMES.len() as u8 {
                names.push(text(save(game, language), false));
            }
        }
        assert_eq!(
            names[..8],
            [
                "/roms/nds/saves/POKEMON_B_IRBJ01_00.sav",
                "/roms/nds/saves/POKEMON_W_IRAJ01_00.sav",
                "/roms/nds/saves/POKEMON_B2_IREJ01_01.sav",
                "/roms/nds/saves/POKEMON_W2_IRDJ01_01.sav",
                "/roms/nds/saves/POKEMON_B_IRBO01_00.sav",
                "/roms/nds/saves/POKEMON_W_IRAO01_00.sav",
                "/roms/nds/saves/POKEMON_B2_IREO01_00.sav",
                "/roms/nds/saves/POKEMON_W2_IRDO01_00.sav",
            ]
        );
        assert_eq!(names.len(), 28);
        assert_eq!(
            names
                .iter()
                .filter(|name| name.ends_with("_01.sav"))
                .count(),
            2
        );
        assert!(names.contains(&String::from("/roms/nds/saves/POKEMON_W_IRAD01_00.sav")));
        assert!(names.contains(&String::from("/roms/nds/saves/POKEMON_W2_IRDD01_00.sav")));
        assert!(names.contains(&String::from("/roms/nds/saves/POKEMON_B2_IRED01_00.sav")));
        assert_eq!(
            text(save(2, b'J'), true),
            "/roms/nds/saves/POKEMON_B2_IREJ01_01.sav.bak"
        );
        // The longest path fits, terminator included.
        let mut out = [0; PATH_CAPACITY];
        assert!(save(2, b'J').path(true, &mut out) <= PATH_CAPACITY);
    }

    #[test]
    fn game_code_and_list_kind_are_the_ones_the_original_uses() {
        // 00243598: 'A' White, 'B' Black, 'D' White 2, 'E' Black 2.
        // 00244EE8: kinds 1 Black, 2 White, 3 Black 2, 4 White 2.
        for (game, code, kind) in [
            (0, *b"IRBD", 1),
            (1, *b"IRAD", 2),
            (2, *b"IRED", 3),
            (3, *b"IRDD", 4),
        ] {
            let save = save(game, b'D');
            assert_eq!(save.game_code().to_le_bytes(), code);
            assert_eq!(save.kind(), kind);
            assert!(is_gen5_code(code));
        }
        for other in [*b"IPKD", *b"IRCD", *b"IRBX", *b"ARBD", *b"\0\0\0\0"] {
            assert!(!is_gen5_code(other));
        }
    }

    #[test]
    fn the_language_and_game_bytes_lie_in_the_trainer_block_of_each_copy() {
        // A German White and a German White 2 save hold 5, 20 and 5, 22 there.
        assert_eq!(save(1, b'D').identity_offsets(), [0x1941E, 0x3D41E]);
        assert_eq!(save(0, b'J').identity_offsets(), [0x1941E, 0x3D41E]);
        assert_eq!(save(3, b'D').identity_offsets(), [0x1941E, 0x3F41E]);
        assert_eq!(save(2, b'K').identity_offsets(), [0x1941E, 0x3F41E]);
        assert!(save(1, b'D').identifies([[5, 20], [5, 20]]));
        assert!(save(3, b'D').identifies([[5, 22], [5, 22]]));
        for (game, version) in [(0, 21), (1, 20), (2, 23), (3, 22)] {
            for (language, letter) in GAME_LANGUAGES {
                assert!(save(game, letter).identifies([[language, version]; 2]));
            }
        }
    }

    #[test]
    fn a_save_is_identified_only_as_the_game_and_language_it_holds() {
        let white = save(1, b'D');
        // One copy is enough while the other was never written.
        assert!(white.identifies([[5, 20], [0, 0]]));
        assert!(white.identifies([[0xFF, 0xFF], [5, 20]]));
        // Neither copy says anything.
        assert!(!white.identifies([[0, 0], [0xFF, 0xFF]]));
        // The other game of the pair, or the same game in another language.
        assert!(!white.identifies([[5, 21]; 2]));
        assert!(!white.identifies([[2, 20]; 2]));
        assert!(!save(2, b'D').identifies([[5, 22]; 2]));
        // One copy right and one another game or language: refused.
        assert!(!white.identifies([[5, 20], [5, 21]]));
        assert!(!white.identifies([[2, 20], [5, 20]]));
    }

    #[test]
    fn a_save_under_another_games_name_is_not_offered() {
        let mut saves = Saves::EMPTY;
        // German Black bytes under the German White name, German White 2
        // bytes under the German Black 2 name, English White 2 bytes under
        // the German White 2 name; only the German Black is what it says.
        saves.scan(None, b'D', |path, _| match named(path) {
            Save { game: 0, .. } => Some([[5, 21]; 2]),
            Save { game: 1, .. } => Some([[5, 21]; 2]),
            Save { game: 2, .. } => Some([[5, 22]; 2]),
            _ => Some([[2, 22]; 2]),
        });
        assert_eq!(saves.list[..saves.count()], [save(0, b'D')]);
    }

    #[test]
    fn scan_finds_only_the_exact_name_of_each_game() {
        let present = [
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IRED01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IREO01_00.sav",
            // Not accepted: the other revision, other game, lower case,
            // renamed, other folder.
            "/roms/nds/saves/POKEMON_B_IRBD01_01.sav",
            "/roms/nds/saves/POKEMON_B2_IREJ01_00.sav",
            "/roms/nds/saves/POKEMON_HG_IPKD01_00.sav",
            "/roms/nds/saves/pokemon_w2_irdd01_00.sav",
            "/roms/nds/saves/White 2.sav",
            "/roms/nds/POKEMON_W2_IRDD01_00.sav",
        ];
        let found = scan(&present);
        let list: Vec<Save> = found.list[..found.count()].to_vec();
        // Game order; the English Black 2 is not in the listed language.
        assert_eq!(list, vec![save(1, b'D'), save(2, b'D')]);
        assert_eq!(found.current(), Some(save(1, b'D')));
        // With English listed, only that one.
        let found = scan_in(None, b'O', &present);
        assert_eq!(found.list[..found.count()], [save(2, b'O')]);
        // The Japanese Black 2 is found only under its revision 01.
        assert_eq!(scan_in(None, b'J', &present).count(), 0);
        let japanese = ["/roms/nds/saves/POKEMON_B2_IREJ01_01.sav"];
        let found = scan_in(None, b'J', &japanese);
        assert_eq!(found.list[..found.count()], [save(2, b'J')]);
        // A language without saves offers nothing.
        assert_eq!(scan_in(None, b'K', &present).count(), 0);
    }

    #[test]
    fn nothing_found_leaves_the_original_alone() {
        for cartridge in [None, Some(*b"IPKD"), Some(*b"IRAD")] {
            let mut none = scan_with(cartridge, &[]);
            assert_eq!((none.count(), none.current()), (0, None));
            assert!(!none.hides_cartridge());
            assert_eq!(none.next_for_list(1), None);
            none.select(0);
            assert_eq!(none.current(), None);
        }
    }

    const B: &str = "/roms/nds/saves/POKEMON_B_IRBD01_00.sav";
    const W: &str = "/roms/nds/saves/POKEMON_W_IRAD01_00.sav";
    const W_ENGLISH: &str = "/roms/nds/saves/POKEMON_W_IRAO01_00.sav";
    const B2: &str = "/roms/nds/saves/POKEMON_B2_IRED01_00.sav";
    const W2: &str = "/roms/nds/saves/POKEMON_W2_IRDD01_00.sav";

    /// Walks the whole list as the hooks do when every read adds its entry,
    /// and returns what each list position stands for (`None`: cartridge).
    fn listed(saves: &mut Saves) -> Vec<(u8, Option<Save>)> {
        let mut out = Vec::new();
        if saves.item_count == 0 {
            return out;
        }
        let mut item = saves.item(saves.current);
        loop {
            assert_eq!(
                saves.current(),
                match item {
                    Item::Save(save) => Some(save),
                    Item::Cartridge { .. } => None,
                }
            );
            out.push((item.kind(), saves.current()));
            match saves.next_for_list(out.len()) {
                Some(next) => item = next,
                None => break,
            }
        }
        // Picking each position presents exactly what was listed there.
        for (position, &(_, source)) in out.iter().enumerate().rev() {
            saves.select(position);
            assert_eq!(saves.current(), source, "position {position}");
        }
        out
    }

    #[test]
    fn the_order_is_always_black_white_black2_white2_whatever_the_source() {
        let black = (1, Some(save(0, b'D')));
        let white = (2, Some(save(1, b'D')));
        let black2 = (3, Some(save(2, b'D')));
        let white2 = (4, Some(save(3, b'D')));
        // No cartridge.
        assert_eq!(listed(&mut scan(&[W2, B])), vec![black, white2]);
        assert_eq!(
            listed(&mut scan(&[B, W, B2, W2])),
            vec![black, white, black2, white2]
        );
        // The cartridge takes the place of its game: last, middle, first.
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRDD"), &[B, W])),
            vec![black, white, (4, None)]
        );
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRAD"), &[B, B2])),
            vec![black, (2, None), black2]
        );
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRBD"), &[W, B2, W2])),
            vec![(1, None), white, black2, white2]
        );
        // Alone, with or without saves of its own game.
        assert_eq!(listed(&mut scan_with(Some(*b"IRED"), &[])), vec![(3, None)]);
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRED"), &[B2])),
            vec![(3, None)]
        );
    }

    #[test]
    fn only_games_in_the_listed_language_are_offered() {
        let present = [B, W, W_ENGLISH, W2];
        let german = vec![
            (1, Some(save(0, b'D'))),
            (2, Some(save(1, b'D'))),
            (4, Some(save(3, b'D'))),
        ];
        assert_eq!(listed(&mut scan(&present)), german);
        assert_eq!(
            listed(&mut scan_in(None, b'O', &present)),
            vec![(2, Some(save(1, b'O')))]
        );
        // A German White cartridge replaces the German White save.
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRAD"), &present)),
            vec![
                (1, Some(save(0, b'D'))),
                (2, None),
                (4, Some(save(3, b'D')))
            ]
        );
        // A French White cartridge is not offered while German is listed and
        // hides nothing; the saves stand in the slot's place.
        let mut french = scan_with(Some(*b"IRAF"), &present);
        assert!(!french.hides_cartridge());
        assert_eq!(listed(&mut french), german);
        // With French listed it is the only entry.
        assert_eq!(
            listed(&mut scan_in(Some(*b"IRAF"), b'F', &present)),
            vec![(2, None)]
        );
        // A cartridge that is not Gen 5 hides nothing and is not listed.
        let mut other = scan_with(Some(*b"IPKD"), &present);
        assert_eq!(other.count(), 3);
        assert_eq!(listed(&mut other).len(), 3);
    }

    #[test]
    fn a_cartridge_in_another_language_alone_is_presented_as_no_game() {
        let mut saves = scan_with(Some(*b"IRAF"), &[]);
        assert_eq!((saves.count(), saves.current()), (0, None));
        assert!(saves.hides_cartridge());
        assert_eq!(saves.next_for_list(0), None);
        // In its own language, or with no Gen 5 cartridge, nothing is hidden.
        assert!(!scan_in(Some(*b"IRAF"), b'F', &[]).hides_cartridge());
        assert!(!scan_with(Some(*b"IPKD"), &[]).hides_cartridge());
        assert!(!scan_with(None, &[]).hides_cartridge());
    }

    #[test]
    fn an_item_the_original_rejects_does_not_shift_the_others() {
        // Black on SD, a Black 2 cartridge whose save is damaged, White 2 on SD.
        let mut saves = scan_with(Some(*b"IRED"), &[B, B2, W2]);
        assert_eq!(saves.count(), 2);
        assert_eq!(saves.current(), Some(save(0, b'D')));
        // Black added at position 0; next is the cartridge.
        assert_eq!(saves.next_for_list(1), Some(Item::Cartridge { kind: 3 }));
        assert_eq!(saves.current(), None);
        // The cartridge added no entry (count stays 1); next is White 2.
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(3, b'D'))));
        assert_eq!(saves.next_for_list(2), None);
        saves.select(1);
        assert_eq!(saves.current(), Some(save(3, b'D')));
        saves.select(0);
        assert_eq!(saves.current(), Some(save(0, b'D')));
        // The first item rejected: a cartridge in front, then one save.
        let mut saves = scan_with(Some(*b"IRBD"), &[W]);
        assert_eq!(saves.next_for_list(0), Some(Item::Save(save(1, b'D'))));
        assert_eq!(saves.next_for_list(1), None);
        saves.select(0);
        assert_eq!(saves.current(), Some(save(1, b'D')));
    }

    #[test]
    fn one_save_per_game_is_looked_for_in_the_listed_language_only() {
        let mut probes = 0;
        let mut saves = Saves::EMPTY;
        saves.scan(None, b'D', |path, _| {
            probes += 1;
            Some(bytes_of(named(path)))
        });
        assert_eq!(saves.count(), MAX_SAVES);
        // One probe per game, found or not.
        assert_eq!(probes, GAMES.len());
        let mut probes = 0;
        let mut empty = Saves::EMPTY;
        empty.scan(None, b'D', |_, _| {
            probes += 1;
            None
        });
        assert_eq!(probes, GAMES.len());
        // A listed cartridge's own game is not probed.
        let mut probes = 0;
        empty.scan(Some(*b"IRBD"), b'D', |_, _| {
            probes += 1;
            None
        });
        assert_eq!(probes, GAMES.len() - 1);
        // A cartridge in another language takes no game's place.
        let mut probes = 0;
        empty.scan(Some(*b"IRBO"), b'D', |_, _| {
            probes += 1;
            None
        });
        assert_eq!(probes, GAMES.len());
    }

    #[test]
    fn list_entries_map_back_to_their_files_even_when_one_is_skipped() {
        let mut saves = scan(&[
            "/roms/nds/saves/POKEMON_B_IRBD01_00.sav",
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IRED01_00.sav",
        ]);
        // Save 0 is read by the original first; its entry lands at position 0.
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(1, b'D'))));
        // Save 1 is damaged: the original adds no entry (count stays 1).
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(2, b'D'))));
        // Save 2 is added at position 1; nothing is left.
        assert_eq!(saves.next_for_list(2), None);
        assert_eq!(saves.entry_count, 2);
        saves.select(1);
        assert_eq!(saves.current(), Some(save(2, b'D')));
        saves.select(0);
        assert_eq!(saves.current(), Some(save(0, b'D')));
        // A position that is not a DS entry (a Virtual Console title) changes
        // nothing.
        saves.select(5);
        assert_eq!(saves.current(), Some(save(0, b'D')));
    }

    #[test]
    fn the_first_save_failing_shifts_the_following_entries_correctly() {
        let mut saves = scan(&[
            "/roms/nds/saves/POKEMON_B_IRBD01_00.sav",
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
        ]);
        // Save 0 damaged: no entry. Save 1 then lands at position 0.
        assert_eq!(saves.next_for_list(0), Some(Item::Save(save(1, b'D'))));
        assert_eq!(saves.next_for_list(1), None);
        saves.select(0);
        assert_eq!(saves.current(), Some(save(1, b'D')));
    }

    #[test]
    fn a_removed_card_concerns_only_the_entry_of_the_cartridge() {
        // A German White cartridge hides the German White save behind it;
        // a German Black 2 save is listed beside it.
        let mut saves = scan_with(Some(*b"IRAD"), &[W, B2]);
        assert_eq!(
            listed(&mut saves),
            vec![(2, None), (3, Some(save(2, b'D')))]
        );
        // The card is taken out while the list is on screen: the White entry
        // is the cartridge, and the save behind it is not used in its place.
        assert!(saves.card_missing_at(0, || true));
        saves.select(0);
        assert_eq!(saves.current(), None);
        assert!(saves.card_missing(|| true));
        // The Black 2 save is not concerned, whatever the signal says.
        assert!(!saves.card_missing_at(1, || true));
        saves.select(1);
        assert!(!saves.card_missing(|| true));
        // A new session forgets it.
        let mut again = scan_with(Some(*b"IRAD"), &[W, B2]);
        assert!(!again.card_missing(|| false));
        // With the card in, nothing is missing.
        assert!(!again.card_missing_at(0, || false));
    }

    #[test]
    fn a_removal_is_kept_after_a_save_was_read_and_the_signal_cleared() {
        // A Black cartridge, then a White save.
        let mut saves = scan_with(Some(*b"IRBD"), &[W]);
        assert!(!saves.card_missing(|| false));
        // The cartridge's entry is added; the White save is read next, and
        // the card is taken out before that read clears the signal.
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(1, b'D'))));
        assert!(!saves.card_missing(|| true));
        assert_eq!(saves.next_for_list(2), None);
        // The cartridge's entry is still the one whose card is gone.
        assert!(saves.card_missing_at(0, || false));
        assert!(!saves.card_missing_at(1, || false));
    }

    #[test]
    fn a_cartridge_whose_card_is_gone_is_skipped_and_shifts_no_entry() {
        // Black on SD, a White cartridge, White 2 on SD. The card is taken
        // out after the Black save was read: the list-next entry asks before
        // each read and skips the cartridge without reading it.
        let mut saves = scan_with(Some(*b"IRAD"), &[B, W2]);
        assert_eq!(saves.current(), Some(save(0, b'D')));
        assert_eq!(saves.next_for_list(1), Some(Item::Cartridge { kind: 2 }));
        assert!(saves.card_missing(|| true));
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(3, b'D'))));
        assert!(!saves.card_missing(|| false));
        assert_eq!(saves.next_for_list(2), None);
        saves.select(1);
        assert_eq!(saves.current(), Some(save(3, b'D')));
        saves.select(0);
        assert_eq!(saves.current(), Some(save(0, b'D')));
        // The cartridge was the first item, gone before it was read: the
        // first save lands at position 0.
        let mut saves = scan_with(Some(*b"IRBD"), &[W]);
        assert!(saves.card_missing(|| true));
        assert_eq!(saves.next_for_list(0), Some(Item::Save(save(1, b'D'))));
        assert!(!saves.card_missing(|| true));
        assert_eq!(saves.next_for_list(1), None);
        saves.select(0);
        assert_eq!(saves.current(), Some(save(1, b'D')));
    }

    #[test]
    fn each_save_is_backed_up_once_per_session_and_again_after_a_failure() {
        let mut saves = scan(&[
            "/roms/nds/saves/POKEMON_B_IRBD01_00.sav",
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
        ]);
        assert!(saves.take_backup_duty());
        assert!(!saves.take_backup_duty());
        saves.backup_failed();
        assert!(saves.take_backup_duty());
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(1, b'D'))));
        assert!(saves.take_backup_duty());
        assert!(!saves.take_backup_duty());
        // A new scan starts a new session.
        let mut again = scan(&["/roms/nds/saves/POKEMON_B_IRBD01_00.sav"]);
        assert!(again.take_backup_duty());
    }
}
