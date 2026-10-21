//! The original language screen, used to choose which language's games the
//! game list offers. The language of the screens themselves is never changed:
//! it stays the one the original takes from the console at start.
//!
//! Language ids are the original's (`002CA380`, `002AFE0C`): 1 Japanese,
//! 2 English, 3 French, 4 Italian, 5 German, 7 Spanish, 8 Korean, 9 and 10
//! the two Chinese ones. No Gen 1, 2 or 5 game exists in Chinese, so those
//! two are never a filter.
//!
//! Nothing is stored: the original has no save data, and a choice lasts until
//! the application is closed. Until one is made, the filter is the language
//! of the screens.
//!
//! The title screen shows a choice: its logo and its start prompt are
//! pictures the original keeps once per language, and after a choice they
//! are loaded in the chosen language.
//!
//! The screen lists the six languages that are not the listed one and, below
//! them, a Back button made of the entry of one Chinese language
//! (`lytpatch`), with the text of the game-selection screen's Back button.

use core::sync::atomic::{AtomicU8, Ordering};

pub const ENGLISH: u8 = 2;
/// The language id of each row of the original's Virtual Console table
/// (`002AFE0C`, 39 rows of `{ version, language, title }`; the builder
/// compares them with the executable it patches). Rows are in the order
/// Red, Green or Blue, Japanese Blue, Yellow, Gold, Silver, Crystal.
pub const VC_LANGUAGES: [u8; 39] = [
    1, 2, 3, 4, 5, 7, // Red
    1, 2, 3, 4, 5, 7, // Green in Japanese, Blue elsewhere
    1, // Blue in Japanese
    1, 2, 3, 4, 5, 7, // Yellow
    1, 2, 3, 4, 5, 7, 8, // Gold
    1, 2, 3, 4, 5, 7, 8, // Silver
    1, 2, 3, 4, 5, 7, // Crystal
];
/// The language id of each Virtual Console title by the index the scan of
/// their saves (`001E419C`) and the installed check (`002512A8`) number it
/// with (`002C2B1C`, one word each; the builder compares them with the
/// executable it patches). The same titles as `VC_LANGUAGES`, in another
/// order.
pub const VC_TITLE_LANGUAGES: [u8; 39] = [
    1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 7, 7, 7, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 7, 7, 8,
    8, 1, 2, 3, 4, 5, 7,
];
/// Entries the original's game list and its table of trainer names hold.
pub const LIST_CAPACITY: usize = 40;

/// Entries of the language screen's list. The original numbers its buttons,
/// and what it calls the picked language, by these list indices.
pub const ENTRIES: usize = 9;
/// Language id of each list index (`002CA380`).
pub const LIST_IDS: [u8; ENTRIES] = [2, 7, 3, 5, 4, 1, 8, 9, 10];
/// Pane of each list index (`002CA3A4`).
pub const LIST_PANES: [u32; ENTRIES] = [4, 7, 10, 13, 16, 1, 19, 22, 25];
/// The list index whose button is the Back button: Simplified Chinese.
pub const BACK: u8 = 7;
/// Entries shown: six languages and Back.
pub const SHOWN: usize = 7;
/// The game-selection screen's message file and its "Back" (`002427BC`).
pub const GAME_LIST_MESSAGES: u32 = 0x18;
pub const BACK_MESSAGE: u32 = 0x1f;
/// Vertical distance of two entries: that of the long buttons in Bank's main
/// menu (`Turtle_lower5.bclyt`: 58, 26, -6, -38, -70, -102), which leaves two
/// units between the 30-unit buttons. The original language list has to fit
/// nine entries and puts them 26 apart (`0022BCC0`), overlapping; seven fit
/// with the menu's distance.
pub const STEP: i32 = 32;
/// Where entries that are not shown are placed: far off the 240-unit screen.
pub const HIDDEN_Y: i32 = 1000;

/// The chosen language id; 0 until a choice is made. Written on the main
/// thread by the language screen, read there and by the cartridge task.
static FILTER: AtomicU8 = AtomicU8::new(0);

/// The languages Gen 1, 2 and 5 games exist in: the original's language id,
/// which is also the language byte of a Gen 5 save, and the last letter of a
/// Gen 5 game code (`00243568`).
pub const GAME_LANGUAGES: [(u8, u8); 7] = [
    (1, b'J'),
    (2, b'O'),
    (3, b'F'),
    (4, b'I'),
    (5, b'D'),
    (7, b'S'),
    (8, b'K'),
];

fn is_game_language(id: u8) -> bool {
    GAME_LANGUAGES.iter().any(|&(known, _)| known == id)
}

/// The language id of a game-code letter, if games exist in it.
pub fn id_of_letter(letter: u8) -> Option<u8> {
    GAME_LANGUAGES
        .iter()
        .find(|&&(_, known)| known == letter)
        .map(|&(id, _)| id)
}

/// A language id games exist in; anything else becomes English, which is
/// also what the original shows consoles set to a language it lacks.
pub fn normalise(id: u8) -> u8 {
    if is_game_language(id) {
        id
    } else {
        ENGLISH
    }
}

/// The language letter of a Gen 5 game code; English for a language games
/// do not exist in.
pub fn letter(id: u8) -> u8 {
    let id = normalise(id);
    GAME_LANGUAGES
        .iter()
        .find(|&&(known, _)| known == id)
        .map_or(b'O', |&(_, letter)| letter)
}

/// What a choice of `chosen` (0: none yet) means while the screens are in
/// `ui_language`.
pub fn effective(chosen: u8, ui_language: u8) -> u8 {
    normalise(if chosen == 0 { ui_language } else { chosen })
}

/// The language screen ended with `id`. The Back button reports the id of
/// the entry it is made of, which is no game language and changes nothing.
pub fn choose(id: u8) {
    if is_game_language(id) {
        FILTER.store(id, Ordering::Relaxed);
    }
}

/// The language whose games are listed.
pub fn current(ui_language: u8) -> u8 {
    effective(FILTER.load(Ordering::Relaxed), ui_language)
}

/// The language of the title screen's pictures: the chosen one, and the
/// screens' own as long as none was chosen (so a console in a language
/// without games keeps its own title).
pub fn title(chosen: u8, ui_language: u8) -> u8 {
    if chosen == 0 {
        ui_language
    } else {
        normalise(chosen)
    }
}

/// Whether a Virtual Console title in `row_language` is listed.
pub fn vc_listed(filter: u8, row_language: u32) -> bool {
    row_language == u32::from(filter)
}

/// The list indices in the order they are shown: the languages other than
/// `filter` in the original's order, then Back. The last two, `filter` and
/// the other Chinese language, are hidden.
pub fn order(filter: u8) -> [u8; ENTRIES] {
    let filter = normalise(filter);
    let mut out = [0; ENTRIES];
    let mut at = 0;
    let mut hidden = 0;
    for index in 0..BACK {
        if LIST_IDS[index as usize] == filter {
            hidden = index;
        } else {
            out[at] = index;
            at += 1;
        }
    }
    out[at] = BACK;
    out[at + 1] = hidden;
    out[at + 2] = BACK + 1;
    out
}

/// Height of the entry at `position` of `shown`, the shown ones centred on
/// the screen's middle as the original's nine are.
pub fn y(position: usize, shown: usize) -> i32 {
    if position < shown {
        STEP / 2 * (shown as i32 - 1) - STEP * position as i32
    } else {
        HIDDEN_Y
    }
}

/// `y` as the float the layout takes. A table, because the patch is built
/// without floating-point code.
pub fn y_float(y: i32) -> f32 {
    match y {
        96 => 96.0,
        64 => 64.0,
        32 => 32.0,
        0 => 0.0,
        -32 => -32.0,
        -64 => -64.0,
        -96 => -96.0,
        _ => 1000.0,
    }
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::lytpatch;
    use core::{mem::transmute, slice};
    use patch_common::native::read_word;

    /// Points at the byte holding the language of the screens (`001E356C`).
    const UI_LANGUAGE: *const *const u8 = 0x0032_938c as *const *const u8;
    /// `(language id)`: the language in which archives with one variant per
    /// language are read. The original calls it once per change of the
    /// screens' language, with that language (`00104EBC`).
    const SET_ARCHIVE_LANGUAGE: usize = 0x0010_71b0;
    /// `(layout work, 1, pane, &[x, y, z])`: places a pane.
    const PLACE_PANE: usize = 0x001a_1b3c;
    /// `(buttons, id, sound)`: the sound of a button.
    const BUTTON_SOUND: usize = 0x001a_18cc;
    const CANCEL_SOUND: u32 = 0x0005_000a;
    /// `(buttons, id)`: disables a button.
    const DISABLE_BUTTON: usize = 0x0022_cb98;
    /// `(screen)`: the cursor's range and the enabled buttons of the screen
    /// part that is about to be shown.
    const ORIGINAL_BUTTONS: usize = 0x001a_1fe0;

    /// Fields of the language screen (`0022BD28`) and of its layout work.
    const SCREEN_BUTTONS: usize = 0x10;
    const SCREEN_LAYOUT: usize = 0x5c;
    const SCREEN_PART: usize = 0x7c;
    const SCREEN_PICKED: usize = 0x80;
    const SCREEN_ORDER: usize = 0x88;
    const SCREEN_RANGE_END: usize = 0xb0;
    const SCREEN_CURSOR: usize = 0xb4;
    const PART_LIST: u8 = 0;
    const WORK_STRINGS: usize = 0x04;
    const WORK_LAYOUTS: usize = 0x1c;
    /// The lower screen's layout is the second of the work's 8-byte rows.
    const LOWER_LAYOUT: usize = 8;
    /// `[parameter + 4]` of the screen's constructor is its heap.
    const PARAMETER_HEAP: usize = 0x04;

    fn ui_language() -> u8 {
        unsafe { UI_LANGUAGE.read().read() }
    }
    fn filter() -> u8 {
        current(ui_language())
    }

    /// The game-code letter of the language whose games are listed.
    pub fn filter_letter() -> u8 {
        letter(filter())
    }

    /// Replaces "switch to this language" (`0025C5FC`, was `bl 0022AF28`)
    /// when the language screen ends.
    #[no_mangle]
    pub extern "aapcs" fn transporter_language_chosen(id: u32) {
        choose(id as u8);
    }

    // 002412D4 (was: bl 002512A8, "is the title of this row installed?",
    // r0 = version, r1 = language id). A row in another language than the
    // listed one answers no; the original answers for the others. It runs
    // on the scanner's thread and only reads the filter.
    patch_common::stub!(
        transporter_vc_scan_stub,
        [
            "push {{r0, r1, r4, lr}}",
            "mov r0, r1",
            "bl {vc_scan_listed}",
            "movs r12, r0",
            "pop {{r0, r1, r4, lr}}",
            "moveq r0, #0",
            "bxeq lr",
            "ldr r12, =0x002512a8",
            "bx r12",
        ],
        vc_scan_listed = sym vc_scan_listed,
    );
    /// Whether the scanner of the Virtual Console titles (`0024120C`) looks
    /// at a row of its table in `language` at all (`002412D4`). It numbers
    /// the titles it finds and files their trainer names under that number,
    /// so a title that is not listed must not be found.
    extern "aapcs" fn vc_scan_listed(language: u32) -> u32 {
        u32::from(vc_listed(filter(), language))
    }

    // The scan of the Virtual Console saves (001E419C), for title r5: the
    // call that mounts its save (001E41C0, was: bl 0010E4A4, r0 = the mount
    // name, r1 = the title). Application init's scan, the one before the
    // payload can run, is skipped (0010621C). A title the entry does not
    // want: on with the next title (001E42D0), with nothing mounted or
    // unmounted; its entry in the scan's table was cleared at 001E41B4.
    // Otherwise the call, which returns to the scan.
    patch_common::stub!(
        transporter_vc_mount_stub,
        [
            "push {{r0, r1, r12, lr}}",
            "mov r0, r5",
            "bl {vc_wanted}",
            "cmp r0, #0",
            "pop {{r0, r1, r12, lr}}",
            "ldreq r12, =0x001e42d0",
            "bxeq r12",
            "ldr r12, =0x0010e4a4",
            "bx r12",
        ],
        vc_wanted = sym vc_wanted,
    );
    /// Whether the scan of the Virtual Console saves (`001E419C`) mounts
    /// the save of title `index` at all (`001E41C0`): a title in another
    /// language than the listed one is never listed, so its save need not
    /// be looked at. An index outside the table is looked at, as by the
    /// original.
    extern "aapcs" fn vc_wanted(index: u32) -> u32 {
        VC_TITLE_LANGUAGES
            .get(index as usize)
            .map_or(1, |&language| {
                u32::from(vc_listed(filter(), u32::from(language)))
            })
    }

    // The title screen's constructor (0024B9E4), around its call that loads
    // its archives and builds its layouts (0024BA70): 0024BA44 (was:
    // mov r7, #3), while the call's arguments are set up, and 0024BA74
    // (was: mov r3, #0x27), right after the call. Every register is
    // returned as it came.
    patch_common::stub!(
        transporter_title_begin_stub,
        [
            "push {{r0-r3, r12, lr}}",
            "mov r0, #1",
            "bl {title_language}",
            "pop {{r0-r3, r12, lr}}",
            "mov r7, #3",
            "bx lr",
        ],
        title_language = sym title_language,
    );
    patch_common::stub!(
        transporter_title_end_stub,
        [
            "push {{r0-r3, r12, lr}}",
            "mov r0, #0",
            "bl {title_language}",
            "pop {{r0-r3, r12, lr}}",
            "mov r3, #0x27",
            "bx lr",
        ],
        title_language = sym title_language,
    );
    /// Brackets the title screen's loading of its archives (`0024BA44`,
    /// `0024BA74`): one of them holds the logo and the start prompt once per
    /// language. With `chosen` it is read in the chosen language; afterwards
    /// archives are read in the screens' language again, as always.
    extern "aapcs" fn title_language(chosen: u32) {
        let ui = ui_language();
        let language = if chosen != 0 {
            title(FILTER.load(Ordering::Relaxed), ui)
        } else {
            ui
        };
        let set: unsafe extern "aapcs" fn(u32) = unsafe { transmute(SET_ARCHIVE_LANGUAGE) };
        unsafe { set(u32::from(language)) };
    }

    // Reached from the layout check beside the start-up hook (bootstrap.rs)
    // for a layout binary of the language screen's size, with the lr of the
    // patched site 0013AEE4. r0 and r1 are the layout binary and are
    // returned as they came, with every other register.
    patch_common::stub!(
        transporter_layout_adjust_stub,
        [
            "push {{r0-r3, r12, lr}}",
            "bl {layout_built}",
            "pop {{r0-r3, r12, lr}}",
            "bx lr",
        ],
        layout_built = sym layout_built,
    );
    /// Runs on a layout binary of the language screen's size before the
    /// original builds a layout from it (`0013AEE4`, through
    /// `transporter_layout_check_stub`); only that screen's is changed.
    /// # Safety
    /// `layout` is the layout resource the original just looked up.
    unsafe extern "aapcs" fn layout_built(layout: *mut u8) {
        unsafe {
            let size = layout
                .add(lytpatch::SIZE_FIELD)
                .cast::<u32>()
                .read_unaligned();
            if layout.cast::<[u8; 4]>().read() == lytpatch::MAGIC && size as usize == lytpatch::SIZE
            {
                lytpatch::apply(slice::from_raw_parts_mut(layout, lytpatch::SIZE));
            }
        }
    }

    /// Gives the Back button the text of the game-selection screen's Back
    /// button, in the language of the screens, from that screen's message
    /// file (`ui::message_text`).
    unsafe fn set_back_text(screen: *mut u8, parameter: *const u8) {
        unsafe {
            let heap = parameter.add(PARAMETER_HEAP).cast::<*mut u8>().read();
            let work = screen.add(SCREEN_LAYOUT).cast::<*mut u8>().read();
            let strings = work.add(WORK_STRINGS).cast::<*mut u8>().read();
            let layouts = work.add(WORK_LAYOUTS).cast::<*mut u8>().read();
            let layout = layouts.add(LOWER_LAYOUT).cast::<*mut u8>().read();
            // Without the text the button keeps the original's.
            let _ = crate::ui::message_text(
                heap,
                strings,
                layout,
                lytpatch::TEXT_PANE,
                GAME_LIST_MESSAGES,
                BACK_MESSAGE,
            );
        }
    }

    // 0022BEE0 (was: bl 0022BC0C). r0 = screen; r6 = the constructor's
    // parameter, whose heap the screen's objects come from.
    patch_common::stub!(
        transporter_language_order_stub,
        ["mov r1, r6", "b {language_order}"],
        language_order = sym language_order,
    );
    // 0022B3B8 (was: b 0022B510, B pressed on a part of the screen that has
    // no Back button; r0 = the part). On the list, part 0, B now presses the
    // Back button, button 7, as the original does for the two other parts at
    // 0022B3BC..0022B3DC. r4 = screen.
    patch_common::stub!(
        transporter_language_back_stub,
        [
            "cmp r0, #0",
            "ldrne r12, =0x0022b510",
            "bxne r12",
            "ldr r0, [r4, #0x10]",
            "mov r1, #7",
            "ldr r12, =0x0022b3dc",
            "bx r12",
        ],
    );
    /// Replaces the original's ordering of the list (`0022BEE0`, was
    /// `bl 0022BC0C`), which put the current language first and showed all
    /// nine entries. `parameter` is the constructor's own (its `r6`).
    /// # Safety
    /// `screen` is the language screen under construction.
    unsafe extern "aapcs" fn language_order(screen: *mut u8, parameter: *const u8) {
        let order = order(filter());
        unsafe {
            let place: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const [f32; 3]) =
                transmute(PLACE_PANE);
            let work = screen.add(SCREEN_LAYOUT).cast::<*mut u8>().read();
            for (position, &index) in order.iter().enumerate() {
                screen
                    .add(SCREEN_ORDER + position * 4)
                    .cast::<u32>()
                    .write(u32::from(index));
                let at = [0.0, y_float(y(position, SHOWN)), 0.0];
                place(work, 1, LIST_PANES[index as usize], &at);
            }
            screen
                .add(SCREEN_PICKED)
                .cast::<u32>()
                .write(u32::from(order[0]));
            let sound: unsafe extern "aapcs" fn(*mut u8, u32, u32) = transmute(BUTTON_SOUND);
            let buttons = screen.add(SCREEN_BUTTONS).cast::<*mut u8>().read();
            sound(buttons, u32::from(BACK), CANCEL_SOUND);
            set_back_text(screen, parameter);
        }
    }

    /// Replaces the call that sets the cursor's range and the enabled
    /// buttons when the list is shown (`0022B5B0`, was `bl 001A1FE0`): the
    /// range ends after the shown entries, and the others cannot be pressed.
    /// # Safety
    /// `screen` is the live language screen.
    #[no_mangle]
    pub unsafe extern "aapcs" fn transporter_language_buttons(screen: *mut u8) {
        unsafe {
            let original: unsafe extern "aapcs" fn(*mut u8) = transmute(ORIGINAL_BUTTONS);
            original(screen);
            if screen.add(SCREEN_PART).read() != PART_LIST {
                return;
            }
            let disable: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(DISABLE_BUTTON);
            let buttons = screen.add(SCREEN_BUTTONS).cast::<*mut u8>().read();
            for position in SHOWN..ENTRIES {
                let index = read_word(screen, SCREEN_ORDER + position * 4);
                disable(buttons, index);
            }
            screen
                .add(SCREEN_RANGE_END)
                .cast::<u32>()
                .write(SHOWN as u32);
            let cursor = screen.add(SCREEN_CURSOR).cast::<u32>();
            if cursor.read() >= SHOWN as u32 {
                cursor.write(0);
            }
        }
    }
}
#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_language_has_its_letter_and_the_others_become_english() {
        // Ids in the order of the letters the original accepts.
        let ids = [1u8, 2, 3, 4, 5, 7, 8];
        let letters: [u8; 7] = ids.map(letter);
        assert_eq!(&letters, b"JOFIDSK");
        for (id, letter) in GAME_LANGUAGES {
            assert_eq!(id_of_letter(letter), Some(id));
        }
        assert_eq!(id_of_letter(b'E'), None);
        for id in ids {
            assert_eq!(normalise(id), id);
        }
        for other in [0u8, 6, 9, 10, 11, 255] {
            assert_eq!(normalise(other), ENGLISH);
            assert_eq!(letter(other), b'O');
        }
    }

    #[test]
    fn the_filter_is_the_screens_language_until_one_is_chosen() {
        assert_eq!(effective(0, 5), 5);
        assert_eq!(effective(0, 1), 1);
        // A Chinese console lists English games.
        assert_eq!(effective(0, 9), ENGLISH);
        assert_eq!(effective(0, 10), ENGLISH);
        // A choice wins, whatever the screens are in.
        assert_eq!(effective(3, 5), 3);
        assert_eq!(effective(1, 9), 1);
    }

    #[test]
    fn a_choice_is_kept_and_back_changes_nothing() {
        choose(7);
        assert_eq!(current(5), 7);
        // Back reports the id of the Chinese entry it is made of.
        choose(LIST_IDS[BACK as usize]);
        assert_eq!(current(5), 7);
        choose(0);
        assert_eq!(current(5), 7);
    }

    #[test]
    fn the_title_follows_a_choice_and_is_the_consoles_own_without_one() {
        // No choice: the screens' language, also where no games exist in it.
        assert_eq!(title(0, 5), 5);
        assert_eq!(title(0, 9), 9);
        // A choice: that language, whatever the screens are in.
        assert_eq!(title(3, 5), 3);
        assert_eq!(title(1, 9), 1);
        assert_eq!(title(5, 5), 5);
    }

    #[test]
    fn one_language_never_fills_the_game_list() {
        // One entry per DS game (the cartridge stands in for the save of
        // its own game, `Saves::scan`) and the titles of one language.
        let most = (0..=u8::MAX)
            .map(|language| {
                VC_LANGUAGES
                    .iter()
                    .filter(|&&row| vc_listed(language, u32::from(row)))
                    .count()
            })
            .max()
            .unwrap();
        assert_eq!(most, 7);
        // Nothing in the patch stops the list at its capacity, as nothing
        // in the original does; this is what keeps it below. The builder
        // holds the same bound against the original's own table.
        assert_eq!(crate::sdsave::GAMES.len() + most, 11);
        assert!(crate::sdsave::GAMES.len() + most <= LIST_CAPACITY);
        // Every row is in a language whose games can be listed.
        for row in VC_LANGUAGES {
            assert_eq!(normalise(row), row);
        }
    }

    #[test]
    fn the_scan_numbers_the_same_titles_in_another_order() {
        let mut rows = VC_LANGUAGES;
        let mut titles = VC_TITLE_LANGUAGES;
        rows.sort_unstable();
        titles.sort_unstable();
        assert_eq!(rows, titles);
        // In German six of the 39 saves are looked at.
        let german = VC_TITLE_LANGUAGES
            .iter()
            .filter(|&&language| vc_listed(5, u32::from(language)))
            .count();
        assert_eq!(german, 6);
    }

    #[test]
    fn virtual_console_titles_are_listed_in_the_filter_language_only() {
        assert!(vc_listed(5, 5));
        assert!(!vc_listed(5, 2));
        assert!(!vc_listed(2, 8));
    }

    #[test]
    fn the_list_shows_the_other_six_languages_and_back_below_them() {
        // German listed (list index 3): the others in the original's order.
        assert_eq!(order(5), [0, 1, 2, 4, 5, 6, 7, 3, 8]);
        // English is the first entry, Korean the last language.
        assert_eq!(order(2), [1, 2, 3, 4, 5, 6, 7, 0, 8]);
        assert_eq!(order(8), [0, 1, 2, 3, 4, 5, 7, 6, 8]);
        // A Chinese console lists English games (`effective`), so English is
        // the hidden entry there too. The Chinese entries are never listed
        // as languages: one is the Back button, the other is hidden.
        assert_eq!(order(effective(0, 9)), order(2));
        assert_eq!(order(effective(0, 10)), order(2));
        assert_eq!(order(9), order(2));
        for filter in [1u8, 2, 3, 4, 5, 7, 8] {
            let order = order(filter);
            // Every entry once; the listed language and traditional Chinese
            // never shown; Back last of the shown.
            let mut seen = [false; ENTRIES];
            for index in order {
                assert!(!core::mem::replace(&mut seen[index as usize], true));
            }
            for &index in &order[..SHOWN - 1] {
                assert_ne!(LIST_IDS[index as usize], filter);
                assert!(index < BACK);
            }
            assert_eq!(order[SHOWN - 1], BACK);
        }
    }

    #[test]
    fn the_shown_entries_are_centred_with_the_spacing_of_banks_menu() {
        let shown: [i32; 9] = core::array::from_fn(|position| y(position, SHOWN));
        assert_eq!(shown, [96, 64, 32, 0, -32, -64, -96, HIDDEN_Y, HIDDEN_Y]);
        // The 30-unit buttons stay on the 240-unit screen.
        assert!(shown[0] + 15 <= 120 && shown[SHOWN - 1] - 15 >= -120);
        // Every height that is used has its float.
        for value in shown {
            assert_eq!(y_float(value), value as f32);
        }
    }
}
