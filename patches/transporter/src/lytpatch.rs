//! Turns the unused Simplified Chinese entry of the language screen's layout
//! into a Back button, in the layout binary the original has loaded, before
//! it builds its panes from it.
//!
//! The layout is `Turtle_lang_select_lower.bclyt`. Its list entries are long
//! buttons (`long_button.bclim`, here with a blue bar) that hold the
//! language's name as a picture. The language screen is one of Bank's
//! ("Turtle") screens, and Bank's menus end in a Back button of the same
//! family: the main menu's (`Turtle_lower5.bclyt` in Bank, pane `l`) is a
//! long button with a centred text pane at (0, 0) and a 30x30
//! `return_icon.bclim` at x 137, drawn as the texture is: teal, `008899`.
//! Transporter's game-selection screen (`Salmon_lower4.bclyt`, pane `3`) has
//! the same button in its own brown, where the material turns the icon white
//! and the pane's corner colours tint it `533324`. Everything for Bank's form
//! is already loaded with the language screen:
//!
//! - the button `m` with its bounding pane and animations;
//! - its picture `1a`, which becomes the icon: the texture name it refers to
//!   is replaced, and the icon's texture is in the common archive that this
//!   screen loads as well. Its material's black colour, dark grey for the
//!   name pictures, becomes the zero that Bank's icon material has, so the
//!   icon shows the texture's own colour;
//! - the text pane `2e` of the kana/kanji part that is no longer reached. A
//!   layout is a flat list of sections in which `pas1`/`pae1` bracket the
//!   children of the pane before them, and nothing refers to a section by
//!   its offset, so the pane is moved into `m` by rotating bytes. Its text
//!   there is "Select language"; the screen gives it the game-selection
//!   screen's "Back" when it is set up (`language`).
//!
//! Only the exact original layout is changed; every offset below belongs to
//! it.

use offline_core::crc32;

/// Size of the layout, also the size field at `SIZE_FIELD` of its header.
pub const SIZE: usize = 0x3188;
pub const MAGIC: [u8; 4] = *b"CLYT";
pub const SIZE_FIELD: usize = 0x0c;
pub const ORIGINAL_CRC: u32 = 0xf0ae_2423;
pub const ADJUSTED_CRC: u32 = 0x2bde_aaff;

/// Name of texture 6 in the texture list, with its terminator.
const TEXTURE_NAME: core::ops::Range<usize> = 0xf3..0x10a;
const OLD_TEXTURE: &[u8] = b"lang_select_simp.bclim";
const NEW_TEXTURE: &[u8] = b"return_icon.bclim";
/// Black colour of material `1a`. The name pictures have no colour of their
/// own and are drawn in this one, `323232`. Bank's return-icon material
/// (`a` in `Turtle_lower5.bclyt`) has zero here.
const MATERIAL_BLACK: usize = 0x820 + 0x14;
/// Picture pane `1a`, child of the button `m`.
const ICON: usize = 0x1aa0;
/// Text pane `2e`, its length, and its id (the name read in base 36).
const TEXT: usize = 0x27d0;
const TEXT_LEN: usize = 0x9c;
pub const TEXT_PANE: u32 = 86;
/// The bounding pane `o`, which follows `1a` inside `m`.
const AFTER_ICON: usize = 0x1b20;

/// Fields of a pane section.
const PANE_ORIGIN: usize = 0x09;
const PANE_NAME: usize = 0x0c;
const PANE_X: usize = 0x24;
const PANE_Y: usize = 0x28;
const PANE_WIDTH: usize = 0x44;
const PANE_HEIGHT: usize = 0x48;
/// The four corner colours of a picture pane; left white, as in Bank.
#[cfg(test)]
const PICTURE_COLOURS: usize = 0x4c;
const TEXT_POSITION: usize = 0x54;
const CENTRE: u8 = 4;

/// The icon as in Bank's main menu (pane `o` there) and on the
/// game-selection screen (pane `6` there).
const ICON_X: f32 = 137.0;
const ICON_SIZE: f32 = 30.0;

fn put(layout: &mut [u8], at: usize, value: f32) {
    layout[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// The edits themselves, without any check of what `layout` holds.
pub fn adjust(layout: &mut [u8; SIZE]) {
    let name = &mut layout[TEXTURE_NAME];
    name.fill(0);
    name[..NEW_TEXTURE.len()].copy_from_slice(NEW_TEXTURE);
    layout[MATERIAL_BLACK..MATERIAL_BLACK + 4].fill(0);

    put(layout, ICON + PANE_X, ICON_X);
    put(layout, ICON + PANE_Y, 0.0);
    put(layout, ICON + PANE_WIDTH, ICON_SIZE);
    put(layout, ICON + PANE_HEIGHT, ICON_SIZE);

    layout[TEXT + PANE_ORIGIN] = CENTRE;
    put(layout, TEXT + PANE_X, 0.0);
    // Centred on the button, as the text of Bank's Back button (pane `q`).
    put(layout, TEXT + PANE_Y, 0.0);
    layout[TEXT + TEXT_POSITION] = CENTRE;
    // Move the text pane to directly after the icon.
    layout[AFTER_ICON..TEXT + TEXT_LEN].rotate_right(TEXT_LEN);
}

/// True when `layout` is the language screen's layout with the Back button
/// in it afterwards: the original, which is adjusted here, or one adjusted
/// before. Anything else is left alone.
pub fn apply(layout: &mut [u8]) -> bool {
    let Ok(layout) = <&mut [u8; SIZE]>::try_from(layout) else {
        return false;
    };
    if layout[..4] != MAGIC {
        return false;
    }
    match crc32(layout) {
        ORIGINAL_CRC => {
            // The hash settles it; these say what the offsets stand for.
            debug_assert_eq!(&layout[TEXTURE_NAME][..OLD_TEXTURE.len()], OLD_TEXTURE);
            debug_assert_eq!(&layout[ICON + PANE_NAME..ICON + PANE_NAME + 3], b"1a\0");
            debug_assert_eq!(&layout[TEXT + PANE_NAME..TEXT + PANE_NAME + 3], b"2e\0");
            adjust(layout);
            true
        }
        ADJUSTED_CRC => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{boxed::Box, vec::Vec};

    /// A buffer of the layout's size in which every byte tells where it was.
    fn marked() -> Box<[u8; SIZE]> {
        let bytes: Vec<u8> = (0..SIZE).map(|at| (at % 251) as u8).collect();
        bytes.into_boxed_slice().try_into().unwrap()
    }

    #[test]
    fn the_text_pane_moves_behind_the_icon_and_nothing_else_moves() {
        let before = marked();
        let mut after = marked();
        adjust(&mut after);
        // Up to the icon and behind the old place of the text: in place.
        // (The texture name, the material colour and the icon's fields are
        // edited in place and checked below.)
        assert_eq!(
            after[AFTER_ICON - 4..AFTER_ICON],
            before[AFTER_ICON - 4..AFTER_ICON]
        );
        assert_eq!(after[TEXT + TEXT_LEN..], before[TEXT + TEXT_LEN..]);
        // What was between them follows the text pane, unchanged.
        assert_eq!(
            after[AFTER_ICON + TEXT_LEN..TEXT + TEXT_LEN],
            before[AFTER_ICON..TEXT]
        );
        // The text pane itself, apart from its edited fields.
        let moved = &after[AFTER_ICON..AFTER_ICON + TEXT_LEN];
        let text = &before[TEXT..TEXT + TEXT_LEN];
        for at in 0..TEXT_LEN {
            let edited =
                at == PANE_ORIGIN || (PANE_X..PANE_Y + 4).contains(&at) || at == TEXT_POSITION;
            assert!(edited || moved[at] == text[at], "text byte {at:#x}");
        }
        assert_eq!((moved[PANE_ORIGIN], moved[TEXT_POSITION]), (CENTRE, CENTRE));
        assert_eq!(moved[PANE_X..PANE_Y + 4], [0; 8]);
    }

    #[test]
    fn the_picture_becomes_the_icon_of_the_game_selection_screen() {
        let before = marked();
        let mut after = marked();
        adjust(&mut after);
        assert_eq!(after[TEXTURE_NAME][..18], *b"return_icon.bclim\0");
        assert!(after[TEXTURE_NAME][18..].iter().all(|&byte| byte == 0));
        let icon = &after[ICON..AFTER_ICON];
        // 137.0, 0.0; 30.0 x 30.0.
        assert_eq!(icon[PANE_X..PANE_Y + 4], [0, 0, 0x09, 0x43, 0, 0, 0, 0]);
        assert_eq!(
            icon[PANE_WIDTH..PANE_HEIGHT + 4],
            [0, 0, 0xf0, 0x41, 0, 0, 0xf0, 0x41]
        );
        assert_eq!(after[MATERIAL_BLACK..MATERIAL_BLACK + 4], [0; 4]);
        // No tint: the corner colours stay as they are.
        assert_eq!(
            icon[PICTURE_COLOURS..PICTURE_COLOURS + 16],
            before[ICON + PICTURE_COLOURS..ICON + PICTURE_COLOURS + 16]
        );
        // Everything else of the icon's pane and before it stays.
        for at in (0..AFTER_ICON).filter(|at| {
            !TEXTURE_NAME.contains(at)
                && !(MATERIAL_BLACK..MATERIAL_BLACK + 4).contains(at)
                && !(ICON + PANE_X..ICON + PANE_Y + 4).contains(at)
                && !(ICON + PANE_WIDTH..ICON + PICTURE_COLOURS).contains(at)
        }) {
            assert_eq!(after[at], before[at], "byte {at:#x}");
        }
    }

    #[test]
    fn only_the_exact_layout_is_touched() {
        // Wrong size, wrong magic, right shape with other content.
        let mut short = [0u8; 64];
        assert!(!apply(&mut short));
        let mut other = marked();
        let copy = other.clone();
        assert!(!apply(&mut other[..]));
        other[..4].copy_from_slice(&MAGIC);
        assert!(!apply(&mut other[..]));
        assert_eq!(other[4..], copy[4..]);
    }

    /// With `TRANSPORTER_LANGUAGE_LAYOUT` naming the layout taken from your
    /// own copy of the game (not part of the repository), checks the two
    /// hashes and that a second call changes nothing.
    #[test]
    fn the_real_layout_is_recognised_before_and_after() {
        let Ok(path) = std::env::var("TRANSPORTER_LANGUAGE_LAYOUT") else {
            return;
        };
        let mut layout = std::fs::read(path).unwrap();
        assert_eq!((layout.len(), crc32(&layout)), (SIZE, ORIGINAL_CRC));
        assert!(apply(&mut layout));
        assert_eq!(crc32(&layout), ADJUSTED_CRC);
        let adjusted = layout.clone();
        assert!(apply(&mut layout));
        assert_eq!(layout, adjusted);
    }
}
