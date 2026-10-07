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
//! `<rr>` the cartridge revision. The four-letter game code the original
//! reads from a cartridge is taken from the name.

/// Exact size of a Gen 5 save.
pub const SAVE_SIZE: u64 = 0x8_0000;
/// Longest path built here, with terminator.
pub const PATH_CAPACITY: usize = 48;
/// At most this many saves are offered. The original list holds 40 entries
/// and may add up to 39 Virtual Console titles after these.
pub const MAX_SAVES: usize = 8;
/// Language letters of the game code the original accepts (`00243568`).
pub const LANGUAGES: [u8; 7] = *b"JOFIDSK";
/// Cartridge revisions looked for. Only 00 and 01 are known to exist.
pub const REVISIONS: u8 = 2;

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
}
pub const GAMES: [Game; 4] = [
    Game {
        title: b"B",
        letter: b'B',
        kind: 1,
    },
    Game {
        title: b"W",
        letter: b'A',
        kind: 2,
    },
    Game {
        title: b"B2",
        letter: b'E',
        kind: 3,
    },
    Game {
        title: b"W2",
        letter: b'D',
        kind: 4,
    },
];

/// True for the game codes the original treats as a Gen 5 cartridge.
pub fn is_gen5_code(code: [u8; 4]) -> bool {
    code[0] == b'I'
        && code[1] == b'R'
        && GAMES.iter().any(|game| game.letter == code[2])
        && LANGUAGES.contains(&code[3])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Save {
    /// Index into `GAMES`.
    pub game: u8,
    pub language: u8,
    pub revision: u8,
}
impl Save {
    pub const NONE: Self = Self {
        game: 0,
        language: 0,
        revision: 0,
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
        put(&[b'0' + self.revision / 10 % 10, b'0' + self.revision % 10]);
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
/// The order is always Black, White, Black 2, White 2, and within a game the
/// order of `LANGUAGES`, whatever the source. A cartridge takes the place of
/// its own game and language and hides the SD save of exactly that one, so
/// the game in the slot is always the one used; other languages of the same
/// game on the SD card stay listed, as separate entries.
#[derive(Clone, Copy, Debug)]
pub struct Saves {
    list: [Save; MAX_SAVES],
    count: u8,
    /// List kind of the cartridge's game, 0 without a Gen 5 cartridge.
    cartridge_kind: u8,
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
}
impl Saves {
    pub const EMPTY: Self = Self {
        list: [Save::NONE; MAX_SAVES],
        count: 0,
        cartridge_kind: 0,
        items: [CARTRIDGE; MAX_SAVES + 1],
        item_count: 0,
        at: 0,
        current: CARTRIDGE,
        started_at: 0,
        entries: [CARTRIDGE; MAX_SAVES + 1],
        entry_count: 0,
        backed_up: 0,
    };

    /// A new session. `cartridge` is the game code the cartridge slot
    /// reported, if any. `exists` answers whether a path names a file of
    /// exactly `SAVE_SIZE` bytes that opens for reading and writing. For each
    /// game and language the highest revision wins.
    ///
    /// The first item is presented in the cartridge's place: the cartridge
    /// itself if its game comes first, otherwise a save.
    pub fn scan(&mut self, cartridge: Option<[u8; 4]>, mut exists: impl FnMut(&[u8]) -> bool) {
        *self = Self::EMPTY;
        let cartridge = cartridge.filter(|&code| is_gen5_code(code));
        let mut path = [0; PATH_CAPACITY];
        for game in 0..GAMES.len() as u8 {
            for language in LANGUAGES {
                let info = GAMES[game as usize];
                if cartridge.is_some_and(|code| code[2] == info.letter && code[3] == language) {
                    self.cartridge_kind = info.kind;
                    self.items[self.item_count as usize] = CARTRIDGE;
                    self.item_count += 1;
                    continue;
                }
                for revision in (0..REVISIONS).rev() {
                    let save = Save {
                        game,
                        language,
                        revision,
                    };
                    let length = save.path(false, &mut path);
                    if exists(&path[..length]) {
                        if (self.count as usize) < MAX_SAVES {
                            self.list[self.count as usize] = save;
                            self.items[self.item_count as usize] = self.count;
                            self.item_count += 1;
                            self.count += 1;
                        }
                        break;
                    }
                }
            }
        }
        if self.item_count != 0 {
            self.current = self.items[0];
        }
    }

    pub fn count(&self) -> usize {
        self.count as usize
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

    /// The user picked the DS entry at `position` of the game list.
    pub fn select(&mut self, position: usize) {
        if position < self.entry_count as usize {
            self.current = self.entries[position];
        }
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
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{string::String, vec, vec::Vec};

    fn text(save: Save, backup: bool) -> String {
        let mut out = [0; PATH_CAPACITY];
        let length = save.path(backup, &mut out);
        assert_eq!(out[length - 1], 0);
        String::from_utf8(out[..length - 1].to_vec()).unwrap()
    }
    fn save(game: u8, language: u8, revision: u8) -> Save {
        Save {
            game,
            language,
            revision,
        }
    }
    fn scan(present: &[&str]) -> Saves {
        scan_with(None, present)
    }
    fn scan_with(cartridge: Option<[u8; 4]>, present: &[&str]) -> Saves {
        let mut saves = Saves::EMPTY;
        saves.scan(cartridge, |path| {
            let name = core::str::from_utf8(&path[..path.len() - 1]).unwrap();
            present.contains(&name)
        });
        saves
    }

    #[test]
    fn names_match_the_files_godmode9_and_twilight_menu_produce() {
        // The three names confirmed on a console, and Black by analogy.
        assert_eq!(
            text(save(1, b'D', 0), false),
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav"
        );
        assert_eq!(
            text(save(3, b'D', 0), false),
            "/roms/nds/saves/POKEMON_W2_IRDD01_00.sav"
        );
        assert_eq!(
            text(save(2, b'D', 0), false),
            "/roms/nds/saves/POKEMON_B2_IRED01_00.sav"
        );
        assert_eq!(
            text(save(0, b'O', 1), false),
            "/roms/nds/saves/POKEMON_B_IRBO01_01.sav"
        );
        assert_eq!(
            text(save(2, b'J', 1), true),
            "/roms/nds/saves/POKEMON_B2_IREJ01_01.sav.bak"
        );
        // The longest path fits, terminator included.
        let mut out = [0; PATH_CAPACITY];
        assert!(save(2, b'J', 1).path(true, &mut out) <= PATH_CAPACITY);
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
            let save = save(game, b'D', 0);
            assert_eq!(save.game_code().to_le_bytes(), code);
            assert_eq!(save.kind(), kind);
            assert!(is_gen5_code(code));
        }
        for other in [*b"IPKD", *b"IRCD", *b"IRBX", *b"ARBD", *b"\0\0\0\0"] {
            assert!(!is_gen5_code(other));
        }
    }

    #[test]
    fn scan_finds_only_exact_names_and_prefers_the_higher_revision() {
        let found = scan(&[
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IRED01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IRED01_01.sav",
            "/roms/nds/saves/POKEMON_B2_IREO01_00.sav",
            // Not accepted: other game, lower case, renamed, other folder.
            "/roms/nds/saves/POKEMON_HG_IPKD01_00.sav",
            "/roms/nds/saves/pokemon_w2_irdd01_00.sav",
            "/roms/nds/saves/White 2.sav",
            "/roms/nds/POKEMON_W2_IRDD01_00.sav",
        ]);
        let list: Vec<Save> = found.list[..found.count()].to_vec();
        // Game order, then language order; Black 2 German at revision 01.
        assert_eq!(
            list,
            vec![save(1, b'D', 0), save(2, b'O', 0), save(2, b'D', 1)]
        );
        assert_eq!(found.current(), Some(save(1, b'D', 0)));
    }

    #[test]
    fn nothing_found_leaves_the_original_alone() {
        for cartridge in [None, Some(*b"IPKD"), Some(*b"IRAD")] {
            let mut none = scan_with(cartridge, &[]);
            assert_eq!((none.count(), none.current()), (0, None));
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
        let black = (1, Some(save(0, b'D', 0)));
        let white = (2, Some(save(1, b'D', 0)));
        let black2 = (3, Some(save(2, b'D', 0)));
        let white2 = (4, Some(save(3, b'D', 0)));
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
    fn a_cartridge_hides_only_the_save_of_its_own_game_and_language() {
        let present = [B, W, W_ENGLISH, W2];
        // A German White cartridge replaces the German White save only; the
        // English one stays, before it, in language order.
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRAD"), &present)),
            vec![
                (1, Some(save(0, b'D', 0))),
                (2, Some(save(1, b'O', 0))),
                (2, None),
                (4, Some(save(3, b'D', 0)))
            ]
        );
        // A French White cartridge hides nothing: it stands between English
        // and German.
        assert_eq!(
            listed(&mut scan_with(Some(*b"IRAF"), &present)),
            vec![
                (1, Some(save(0, b'D', 0))),
                (2, Some(save(1, b'O', 0))),
                (2, None),
                (2, Some(save(1, b'D', 0))),
                (4, Some(save(3, b'D', 0)))
            ]
        );
        // Without it, both languages of White are listed, in language order.
        assert_eq!(
            listed(&mut scan(&present)),
            vec![
                (1, Some(save(0, b'D', 0))),
                (2, Some(save(1, b'O', 0))),
                (2, Some(save(1, b'D', 0))),
                (4, Some(save(3, b'D', 0)))
            ]
        );
        // A cartridge that is not Gen 5 hides nothing and is not listed.
        let mut other = scan_with(Some(*b"IPKD"), &present);
        assert_eq!(other.count(), 4);
        assert_eq!(listed(&mut other).len(), 4);
    }

    #[test]
    fn an_item_the_original_rejects_does_not_shift_the_others() {
        // Black on SD, a Black 2 cartridge whose save is damaged, White 2 on SD.
        let mut saves = scan_with(Some(*b"IRED"), &[B, B2, W2]);
        assert_eq!(saves.count(), 2);
        assert_eq!(saves.current(), Some(save(0, b'D', 0)));
        // Black added at position 0; next is the cartridge.
        assert_eq!(saves.next_for_list(1), Some(Item::Cartridge { kind: 3 }));
        assert_eq!(saves.current(), None);
        // The cartridge added no entry (count stays 1); next is White 2.
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(3, b'D', 0))));
        assert_eq!(saves.next_for_list(2), None);
        saves.select(1);
        assert_eq!(saves.current(), Some(save(3, b'D', 0)));
        saves.select(0);
        assert_eq!(saves.current(), Some(save(0, b'D', 0)));
        // The first item rejected: a cartridge in front, then one save.
        let mut saves = scan_with(Some(*b"IRBD"), &[W]);
        assert_eq!(saves.next_for_list(0), Some(Item::Save(save(1, b'D', 0))));
        assert_eq!(saves.next_for_list(1), None);
        saves.select(0);
        assert_eq!(saves.current(), Some(save(1, b'D', 0)));
    }

    #[test]
    fn no_more_than_the_list_can_hold_are_offered() {
        let mut probes = 0;
        let mut saves = Saves::EMPTY;
        saves.scan(None, |_| {
            probes += 1;
            true
        });
        assert_eq!(saves.count(), MAX_SAVES);
        // One probe per game and language when the highest revision exists.
        assert_eq!(probes, GAMES.len() * LANGUAGES.len());
        let mut probes = 0;
        let mut empty = Saves::EMPTY;
        empty.scan(None, |_| {
            probes += 1;
            false
        });
        assert_eq!(probes, GAMES.len() * LANGUAGES.len() * REVISIONS as usize);
        // With a cartridge its own game and language is not probed.
        let mut probes = 0;
        empty.scan(Some(*b"IRBD"), |_| {
            probes += 1;
            false
        });
        assert_eq!(
            probes,
            (GAMES.len() * LANGUAGES.len() - 1) * REVISIONS as usize
        );
    }

    #[test]
    fn list_entries_map_back_to_their_files_even_when_one_is_skipped() {
        let mut saves = scan(&[
            "/roms/nds/saves/POKEMON_B_IRBD01_00.sav",
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
            "/roms/nds/saves/POKEMON_B2_IRED01_00.sav",
        ]);
        // Save 0 is read by the original first; its entry lands at position 0.
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(1, b'D', 0))));
        // Save 1 is damaged: the original adds no entry (count stays 1).
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(2, b'D', 0))));
        // Save 2 is added at position 1; nothing is left.
        assert_eq!(saves.next_for_list(2), None);
        assert_eq!(saves.entry_count, 2);
        saves.select(1);
        assert_eq!(saves.current(), Some(save(2, b'D', 0)));
        saves.select(0);
        assert_eq!(saves.current(), Some(save(0, b'D', 0)));
        // A position that is not a DS entry (a Virtual Console title) changes
        // nothing.
        saves.select(5);
        assert_eq!(saves.current(), Some(save(0, b'D', 0)));
    }

    #[test]
    fn the_first_save_failing_shifts_the_following_entries_correctly() {
        let mut saves = scan(&[
            "/roms/nds/saves/POKEMON_B_IRBD01_00.sav",
            "/roms/nds/saves/POKEMON_W_IRAD01_00.sav",
        ]);
        // Save 0 damaged: no entry. Save 1 then lands at position 0.
        assert_eq!(saves.next_for_list(0), Some(Item::Save(save(1, b'D', 0))));
        assert_eq!(saves.next_for_list(1), None);
        saves.select(0);
        assert_eq!(saves.current(), Some(save(1, b'D', 0)));
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
        assert_eq!(saves.next_for_list(1), Some(Item::Save(save(1, b'D', 0))));
        assert!(saves.take_backup_duty());
        assert!(!saves.take_backup_duty());
        // A new scan starts a new session.
        let mut again = scan(&["/roms/nds/saves/POKEMON_B_IRBD01_00.sav"]);
        assert!(again.take_backup_duty());
    }
}
