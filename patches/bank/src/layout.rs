//! The transfer screen's lower layout (`Turtle_lower1_3`, romfs `a/0/4/7`) with
//! the panes the box manager adds (docs/internals.md, "The transfer screen").
//! The original's bytes are taken as the original's layout parser is handed
//! them (`bank_layout_build`), and a copy is built with:
//!
//! - four textures more: `box_check_01`, `box_check_02` (the cells' other
//!   two states) and `cursol02`, `cursol03` (the blue and the green cursor);
//! - five materials more, copies of the last cell's (`me`) and the cursor's
//!   (`n6`), each with one of the textures;
//! - four cells behind the last one, in the free places of the last row,
//!   each as three pictures, one per state, copies of `me`;
//! - inside the cursor's pane `n8`: a held cell, three pictures, drawn
//!   under the hand, and the blue and the green hand over it.
//!
//! The new panes are named as panes of the same pack that this layout does
//! not have, so the original finds them by their ID as it finds its own
//! (the ID is the name read as a number in base 36). Nothing else changes.

/// The layout as the original has it: its size and CRC-32.
pub const ORIGINAL_SIZE: usize = 28_392;
pub const ORIGINAL_CRC: u32 = 0x064b_98bc;
/// Room for the extended layout: the original grows by 3212 bytes (31604 in
/// all).
pub const EXTENDED_ROOM: usize = 32 * 1024;
/// Room for the extended layout, aligned as the original's files are: the
/// parser reads words from it, and a word it reads at an odd address is an
/// alignment fault. Every section inside the layout is 4-byte aligned.
#[repr(C, align(128))]
pub struct Room(pub [u8; EXTENDED_ROOM]);

/// The extra cells, three panes each (state 0, 1, 2): `CELLS[state][cell]`.
pub const CELLS: [[&[u8]; 4]; 3] = [
    [b"2", b"4", b"6", b"8"],
    [b"a", b"c", b"e", b"g"],
    [b"i", b"k", b"m", b"o"],
];
/// The held cell's three panes.
pub const HELD: [&[u8]; 3] = [b"q", b"s", b"u"];
/// The blue and the green hand.
pub const HANDS: [&[u8]; 2] = [b"w", b"y"];
/// The held cell's shadow, one picture per state as the held cell's, from
/// the same textures (the checked box's is smaller). The box screen's
/// shadow of a held Pokémon is its pane `42` (`Turtle_lower1`), 5 right and
/// 5 down of what is held (`001d8d90`), half transparent (pane alpha
/// `0x80`) and black; these are named after panes of that layout too.
pub const SHADOWS: [&[u8]; 3] = [b"42", b"40", b"3y"];
pub const SHADOW_OFFSET: (f32, f32) = (5.0, -5.0);

/// A pane's ID: its name read as a number in base 36.
pub const fn id(name: &[u8]) -> u32 {
    let mut value = 0u32;
    let mut index = 0;
    while index < name.len() {
        let byte = name[index];
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'z' => byte - b'a' + 10,
            _ => 0,
        };
        value = value * 36 + digit as u32;
        index += 1;
    }
    value
}

/// The top line, laid out as the box screen's group label (`Turtle_upper1`:
/// "Gruppe: " `nf` right-aligned, ending where the group's name `nh` starts
/// left-aligned, 140 before its number `nj`), measured from this screen's
/// counter `n0`, which stays where its text starts (x 70): the title `n2`
/// moves to the name's place and shows the title or the group's name; `nf`
/// is a copy of it that ends where it starts.
pub const TITLE: u32 = id(b"n2");
pub const GROUP_LABEL: u32 = id(b"nf");
/// Where a group's name starts, 140 before the counter's text (x 70), and
/// "Gruppe: " ends.
pub const NAME_START: f32 = -70.0;
/// Where the title starts with the red hand: the width of the box screen's
/// number `nj` (30) before the name, in every language. In the name's 140
/// px every title fits (the widest, English "Select Boxes to Move", 127 px,
/// ends at x 27).
pub const TITLE_START: f32 = NAME_START - 30.0;
/// `n2`: centre x 0 and 140 wide, so that its left-aligned text starts at
/// `NAME_START`; with the red hand its centre moves so that it starts at
/// `TITLE_START`.
pub const TITLE_AT: (f32, f32) = (0.0, 140.0);
/// `nf`: centre x -115 and 90 wide, right-aligned, so that its text ends at
/// x -70.
pub const GROUP_LABEL_AT: (f32, f32) = (-115.0, 90.0);

/// The original's cursor pane `n8` (`0x344`) and hand `n6` (`0x342`).
pub const CURSOR: u32 = id(b"n8");
pub const HAND: u32 = id(b"n6");

const TEXTURES: [&[u8]; 4] = [
    b"box_check_01.bclim",
    b"box_check_02.bclim",
    b"cursol02.bclim",
    b"cursol03.bclim",
];
const MATERIALS: [&[u8]; 5] = [
    b"hx_cell0",
    b"hx_cell1",
    b"hx_cell2",
    b"hx_blue",
    b"hx_green",
];

/// Where the extra cells lie in `gx`, beside the last one (`me` at
/// 150, -112, 24 apart).
pub const fn cell_x(cell: usize) -> f32 {
    174.0 + 24.0 * cell as f32
}
pub const CELL_Y: f32 = -112.0;
/// Where the held cell lies in `n8` as the layout is built, hidden: 10 below
/// the hand at rest. Its height is set when something is lifted and when
/// the cursor moves (`transfer_screen::held_height`).
pub const HELD_AT: (f32, f32) = (0.0, 10.0);
/// Where the hand `n6` rests in `n8`; its bobbing (`cursol_move`) moves it
/// from there, and the blue and the green hand follow it.
pub const HAND_AT: (f32, f32) = (17.0, 20.0);
/// Where the hand stands while something is held: the end of
/// `cursol_move_duwn`, which the box screen plays when it lifts
/// (`0028d370`) and this screen does not load.
pub const HAND_LOWERED: (f32, f32) = (17.0, 10.0);

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// One section of the layout: where it starts and its size.
#[derive(Clone, Copy)]
struct Section {
    at: usize,
    size: usize,
}
impl Section {
    fn magic<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.at..self.at + 4]
    }
    /// The name of a pane section.
    fn name<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        let name = &bytes[self.at + 12..self.at + 28];
        let end = name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name.len());
        &name[..end]
    }
    fn bytes<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.at..self.at + self.size]
    }
}

const MAX_SECTIONS: usize = 400;

fn sections(bytes: &[u8], out: &mut [Section; MAX_SECTIONS]) -> Option<usize> {
    if bytes.get(..4)? != b"CLYT" {
        return None;
    }
    let header = usize::from(u16_at(bytes, 6)?);
    let count = usize::from(u16_at(bytes, 0x10)?);
    if count > MAX_SECTIONS {
        return None;
    }
    let mut at = header;
    for section in out.iter_mut().take(count) {
        let size = u32_at(bytes, at + 4)? as usize;
        if size < 8 || at + size > bytes.len() {
            return None;
        }
        *section = Section { at, size };
        at += size;
    }
    Some(count)
}

/// A growing output.
struct Out<'a> {
    bytes: &'a mut [u8],
    len: usize,
}
impl Out<'_> {
    fn put(&mut self, bytes: &[u8]) -> Option<usize> {
        let at = self.len;
        self.bytes
            .get_mut(at..at + bytes.len())?
            .copy_from_slice(bytes);
        self.len += bytes.len();
        Some(at)
    }
    fn set_u32(&mut self, at: usize, value: u32) -> Option<()> {
        self.bytes
            .get_mut(at..at + 4)?
            .copy_from_slice(&value.to_le_bytes());
        Some(())
    }
    fn pad(&mut self) -> Option<()> {
        while self.len % 4 != 0 {
            self.put(&[0])?;
        }
        Some(())
    }
}

/// The material index of a picture pane section.
const PIC_MATERIAL: usize = 8 + 0x44 + 0x10;
const PANE_FLAG: usize = 8;
const PANE_ALPHA: usize = 10;
const PANE_NAME: usize = 12;
/// A picture's four vertex colours (RGBA).
const PIC_COLORS: usize = 8 + 0x44;
const PANE_X: usize = 8 + 0x1c;
const PANE_Y: usize = 8 + 0x20;
const PANE_WIDTH: usize = 8 + 0x3c;
/// A text pane's text position: horizontally its value mod 3 (0 left, 1
/// centre, 2 right), vertically its value / 3.
const TEXT_POSITION: usize = 8 + 0x44 + 8;
const TEXT_RIGHT: u8 = 2;
/// A material's name and its first texture's index.
const MATERIAL_TEXTURE: usize = 0x34;

/// A copy of pane section `source` named `name`, shown or hidden.
fn copy_pane(out: &mut Out, source: &[u8], name: &[u8], shown: bool) -> Option<usize> {
    let at = out.put(source)?;
    let section = out.bytes.get_mut(at..at + source.len())?;
    section.get_mut(PANE_NAME..PANE_NAME + 16)?.fill(0);
    section
        .get_mut(PANE_NAME..PANE_NAME + name.len())?
        .copy_from_slice(name);
    let flag = section.get_mut(PANE_FLAG)?;
    *flag = if shown { *flag | 1 } else { *flag & !1 };
    Some(at)
}

/// A copy of picture section `pic` named `name`, at `x`, `y`, with material
/// `material`, shown or hidden.
fn picture(
    out: &mut Out,
    source: &[u8],
    name: &[u8],
    (x, y): (f32, f32),
    material: u16,
    shown: bool,
) -> Option<usize> {
    let at = copy_pane(out, source, name, shown)?;
    let section = out.bytes.get_mut(at..at + source.len())?;
    section
        .get_mut(PANE_X..PANE_X + 4)?
        .copy_from_slice(&x.to_le_bytes());
    section
        .get_mut(PANE_Y..PANE_Y + 4)?
        .copy_from_slice(&y.to_le_bytes());
    section
        .get_mut(PIC_MATERIAL..PIC_MATERIAL + 2)?
        .copy_from_slice(&material.to_le_bytes());
    Some(at)
}

/// Places the pane at `at`: its centre's x and its width.
fn place(out: &mut Out, at: usize, (x, width): (f32, f32)) -> Option<()> {
    out.bytes
        .get_mut(at + PANE_X..at + PANE_X + 4)?
        .copy_from_slice(&x.to_le_bytes());
    out.bytes
        .get_mut(at + PANE_WIDTH..at + PANE_WIDTH + 4)?
        .copy_from_slice(&width.to_le_bytes());
    Some(())
}

/// Makes the picture at `at` a shadow: black, half transparent.
fn shade(out: &mut Out, at: usize) -> Option<()> {
    *out.bytes.get_mut(at + PANE_ALPHA)? = 0x80;
    for color in out
        .bytes
        .get_mut(at + PIC_COLORS..at + PIC_COLORS + 16)?
        .chunks_exact_mut(4)
    {
        color.copy_from_slice(&[0, 0, 0, 0xff]);
    }
    Some(())
}

/// The material entries of a `mat1` section: offsets from its start.
fn materials(section: &[u8], out: &mut [(usize, usize); 512]) -> Option<usize> {
    let count = u32_at(section, 8)? as usize;
    if count > out.len() {
        return None;
    }
    for (index, entry) in out.iter_mut().enumerate().take(count) {
        let start = u32_at(section, 12 + index * 4)? as usize;
        let end = if index + 1 < count {
            u32_at(section, 12 + (index + 1) * 4)? as usize
        } else {
            section.len()
        };
        if start >= end || end > section.len() {
            return None;
        }
        *entry = (start, end);
    }
    Some(count)
}

/// Builds the extended layout from the original's `bytes` into `room`; the
/// length written. `None` for a layout that is not the one expected, which
/// the original is then given unchanged.
pub fn extend(bytes: &[u8], room: &mut [u8]) -> Option<usize> {
    let mut list = [Section { at: 0, size: 0 }; MAX_SECTIONS];
    let count = sections(bytes, &mut list)?;
    let list = &list[..count];
    let find = |magic: &[u8], name: &[u8]| {
        list.iter()
            .position(|section| section.magic(bytes) == magic && section.name(bytes) == name)
    };
    let last_cell = find(b"pic1", b"me")?;
    let title = find(b"txt1", b"n2")?;
    let hand = find(b"pic1", b"n6")?;
    let cursor = find(b"pan1", b"n8")?;
    // `n8` holds `n6` alone: its children begin right behind it.
    if list.get(cursor + 1)?.magic(bytes) != b"pas1" || hand != cursor + 2 {
        return None;
    }
    // None of the new names is a pane of this layout yet.
    for name in CELLS
        .iter()
        .flatten()
        .chain(HELD.iter())
        .chain(HANDS.iter())
        .chain(SHADOWS.iter())
        .chain([b"nf".as_slice()].iter())
    {
        if list
            .iter()
            .any(|section| section.size > 28 && section.name(bytes) == *name)
        {
            return None;
        }
    }
    let textures_at = list
        .iter()
        .position(|section| section.magic(bytes) == b"txl1")?;
    let materials_at = list
        .iter()
        .position(|section| section.magic(bytes) == b"mat1")?;
    let material_section = list[materials_at].bytes(bytes);
    let mut entries = [(0usize, 0usize); 512];
    let material_count = materials(material_section, &mut entries)?;
    let entries = &entries[..material_count];
    let cell_material = usize::from(u16_at(list[last_cell].bytes(bytes), PIC_MATERIAL)?);
    let hand_material = usize::from(u16_at(list[hand].bytes(bytes), PIC_MATERIAL)?);
    let (cell_start, cell_end) = *entries.get(cell_material)?;
    let (hand_start, hand_end) = *entries.get(hand_material)?;
    let cell_texture = u16_at(material_section, cell_start + MATERIAL_TEXTURE)?;
    let texture_section = list[textures_at].bytes(bytes);
    let texture_count = u32_at(texture_section, 8)? as usize;

    let header = usize::from(u16_at(bytes, 6)?);
    let mut out = Out {
        bytes: room,
        len: 0,
    };
    out.put(&bytes[..header])?;
    for (index, section) in list.iter().enumerate() {
        if index == textures_at {
            // The names' offsets count from the first offset.
            let at = out.put(&[0; 12])?;
            out.bytes[at..at + 4].copy_from_slice(b"txl1");
            let total = texture_count + TEXTURES.len();
            out.set_u32(at + 8, total as u32)?;
            let table = out.put(&[0; 4 * 64][..total * 4])?;
            for name in 0..total {
                let offset = out.len - table;
                out.set_u32(table + name * 4, offset as u32)?;
                if name < texture_count {
                    let from = 12 + u32_at(texture_section, 12 + name * 4)? as usize;
                    let end = from
                        + texture_section
                            .get(from..)?
                            .iter()
                            .position(|byte| *byte == 0)?;
                    out.put(&texture_section[from..end])?;
                } else {
                    out.put(TEXTURES[name - texture_count])?;
                }
                out.put(&[0])?;
            }
            out.pad()?;
            let size = out.len - at;
            out.set_u32(at + 4, size as u32)?;
            continue;
        }
        if index == materials_at {
            let at = out.put(&[0; 12])?;
            out.bytes[at..at + 4].copy_from_slice(b"mat1");
            let total = material_count + MATERIALS.len();
            out.set_u32(at + 8, total as u32)?;
            let table = out.put(&[0; 4 * 512][..total * 4])?;
            let put_material = |out: &mut Out,
                                slot: usize,
                                source: (usize, usize),
                                name: Option<&[u8]>,
                                texture: Option<u16>|
             -> Option<()> {
                let offset = out.len - at;
                out.set_u32(table + slot * 4, offset as u32)?;
                let start = out.put(&material_section[source.0..source.1])?;
                if let Some(name) = name {
                    out.bytes.get_mut(start..start + 20)?.fill(0);
                    out.bytes
                        .get_mut(start..start + name.len())?
                        .copy_from_slice(name);
                }
                if let Some(texture) = texture {
                    out.bytes
                        .get_mut(start + MATERIAL_TEXTURE..start + MATERIAL_TEXTURE + 2)?
                        .copy_from_slice(&texture.to_le_bytes());
                }
                Some(())
            };
            for (slot, entry) in entries.iter().enumerate() {
                put_material(&mut out, slot, *entry, None, None)?;
            }
            let new = texture_count as u16;
            let added: [((usize, usize), u16); 5] = [
                ((cell_start, cell_end), cell_texture),
                ((cell_start, cell_end), new),
                ((cell_start, cell_end), new + 1),
                ((hand_start, hand_end), new + 2),
                ((hand_start, hand_end), new + 3),
            ];
            for (offset, (source, texture)) in added.iter().enumerate() {
                put_material(
                    &mut out,
                    material_count + offset,
                    *source,
                    Some(MATERIALS[offset]),
                    Some(*texture),
                )?;
            }
            let size = out.len - at;
            out.set_u32(at + 4, size as u32)?;
            continue;
        }
        if index == hand {
            // Under the hand: the held cell's shadow, then the held cell,
            // one picture per state.
            let source = list[last_cell].bytes(bytes);
            for (state, name) in SHADOWS.iter().enumerate() {
                let shadow = picture(
                    &mut out,
                    source,
                    name,
                    (HELD_AT.0 + SHADOW_OFFSET.0, HELD_AT.1 + SHADOW_OFFSET.1),
                    (material_count + state) as u16,
                    false,
                )?;
                shade(&mut out, shadow)?;
            }
            for (state, name) in HELD.iter().enumerate() {
                picture(
                    &mut out,
                    source,
                    name,
                    HELD_AT,
                    (material_count + state) as u16,
                    false,
                )?;
            }
        }
        if index == title {
            // "Gruppe: " in front of the title: a copy, hidden, its text
            // right-aligned; then the title in the name's place.
            let source = section.bytes(bytes);
            let label = copy_pane(&mut out, source, b"nf", false)?;
            place(&mut out, label, GROUP_LABEL_AT)?;
            *out.bytes.get_mut(label + TEXT_POSITION)? = TEXT_RIGHT;
            let at = out.put(source)?;
            place(&mut out, at, TITLE_AT)?;
            continue;
        }
        out.put(section.bytes(bytes))?;
        if index == hand {
            // Over the hand: the blue and the green one.
            let source = section.bytes(bytes);
            let at = (
                f32::from_le_bytes(source.get(PANE_X..PANE_X + 4)?.try_into().ok()?),
                f32::from_le_bytes(source.get(PANE_Y..PANE_Y + 4)?.try_into().ok()?),
            );
            for (color, name) in HANDS.iter().enumerate() {
                picture(
                    &mut out,
                    source,
                    name,
                    at,
                    (material_count + 3 + color) as u16,
                    false,
                )?;
            }
        }
        if index == last_cell {
            let source = section.bytes(bytes);
            for (state, names) in CELLS.iter().enumerate() {
                for (cell, name) in names.iter().enumerate() {
                    picture(
                        &mut out,
                        source,
                        name,
                        (cell_x(cell), CELL_Y),
                        (material_count + state) as u16,
                        state == 0,
                    )?;
                }
            }
        }
    }
    let added = CELLS.len() * 4 + SHADOWS.len() + HELD.len() + HANDS.len() + 1;
    let sections = u16::try_from(count + added).ok()?;
    out.bytes
        .get_mut(0x10..0x12)?
        .copy_from_slice(&sections.to_le_bytes());
    let len = out.len;
    out.set_u32(0xc, len as u32)?;
    Some(len)
}

/// The cursor's places on the transfer screen (`003377fc`): 113 entries of 8
/// bytes, a pane ID and the places up, down, left and right. 0..99 the
/// boxes, 13 rows of 13 but the last with 9, then the button row's 13
/// places. The extended table adds the four cells as places 113..116, the
/// last row's places 9..12.
pub const PLACES: usize = 113;
pub const EXTENDED_PLACES: usize = PLACES + 4;
pub const fn extra_place(cell: usize) -> usize {
    PLACES + cell
}

pub fn extend_places(original: &[u8; PLACES * 8], out: &mut [u8; EXTENDED_PLACES * 8]) {
    out[..PLACES * 8].copy_from_slice(original);
    let set = |out: &mut [u8; EXTENDED_PLACES * 8], place: usize, direction: usize, to: usize| {
        out[place * 8 + 4 + direction] = to as u8;
    };
    const UP: usize = 0;
    const DOWN: usize = 1;
    const LEFT: usize = 2;
    const RIGHT: usize = 3;
    for (cell, name) in CELLS[0].iter().enumerate() {
        let place = extra_place(cell);
        out[place * 8..place * 8 + 4].copy_from_slice(&id(name).to_le_bytes());
        let above = 87 + cell;
        let below = 109 + cell;
        set(out, place, UP, above);
        set(out, place, DOWN, below);
        set(out, place, LEFT, if cell == 0 { 99 } else { place - 1 });
        set(out, place, RIGHT, if cell == 3 { 91 } else { place + 1 });
        set(out, above, DOWN, place);
        set(out, below, UP, place);
    }
    set(out, 99, RIGHT, extra_place(0));
    set(out, 91, LEFT, extra_place(3));
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    fn section(magic: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = magic.to_vec();
        out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }
    fn pane(magic: &[u8], name: &[u8], material: u16) -> Vec<u8> {
        let mut body = vec![0u8; 0x44 + 20];
        body[0] = 1;
        body[4..4 + name.len()].copy_from_slice(name);
        body[0x1c..0x20].copy_from_slice(&1.5f32.to_le_bytes());
        body[0x44 + 0x10..0x44 + 0x12].copy_from_slice(&material.to_le_bytes());
        section(magic, &body)
    }
    fn material(name: &[u8], texture: u16) -> Vec<u8> {
        let mut out = vec![0u8; 0x38];
        out[..name.len()].copy_from_slice(name);
        out[0x30] = 1;
        out[0x34..0x36].copy_from_slice(&texture.to_le_bytes());
        out
    }
    /// A layout with the parts the extension looks for.
    fn layout() -> Vec<u8> {
        let names: [&[u8]; 2] = [b"box_check_00.bclim", b"cursol01.bclim"];
        let mut textures = 2u32.to_le_bytes().to_vec();
        let mut strings = Vec::new();
        for name in names {
            textures.extend_from_slice(&((8 + strings.len()) as u32).to_le_bytes());
            strings.extend_from_slice(name);
            strings.push(0);
        }
        textures.extend_from_slice(&strings);
        while textures.len() % 4 != 0 {
            textures.push(0);
        }
        let mats = [material(b"me", 0), material(b"n6", 1)];
        let mut mat1 = 2u32.to_le_bytes().to_vec();
        let mut at = 8 + 4 + 8;
        for entry in &mats {
            mat1.extend_from_slice(&(at as u32).to_le_bytes());
            at += entry.len();
        }
        for entry in &mats {
            mat1.extend_from_slice(entry);
        }
        let parts = [
            section(b"lyt1", &[0; 12]),
            section(b"txl1", &textures),
            section(b"mat1", &mat1),
            pane(b"pan1", b"RootPane", 0),
            section(b"pas1", &[]),
            pane(b"pan1", b"gx", 0),
            section(b"pas1", &[]),
            pane(b"pic1", b"me", 0),
            section(b"pae1", &[]),
            pane(b"txt1", b"n2", 0),
            pane(b"pan1", b"n8", 0),
            section(b"pas1", &[]),
            pane(b"pic1", b"n6", 1),
            section(b"pae1", &[]),
            section(b"pae1", &[]),
        ];
        let mut out = vec![0u8; 0x14];
        out[..4].copy_from_slice(b"CLYT");
        out[6..8].copy_from_slice(&0x14u16.to_le_bytes());
        out[0x10..0x12].copy_from_slice(&(parts.len() as u16).to_le_bytes());
        for part in parts {
            out.extend_from_slice(&part);
        }
        let len = out.len() as u32;
        out[0xc..0x10].copy_from_slice(&len.to_le_bytes());
        out
    }

    #[test]
    fn the_layout_gets_the_cells_the_held_cell_and_the_hands() {
        let original = layout();
        let mut room = vec![0u8; EXTENDED_ROOM];
        let len = extend(&original, &mut room).unwrap();
        let extended = &room[..len];
        let mut list = [Section { at: 0, size: 0 }; MAX_SECTIONS];
        let count = sections(extended, &mut list).unwrap();
        let list = &list[..count];
        assert_eq!(count, 15 + 21);
        assert_eq!(u32_at(extended, 0xc).unwrap() as usize, len);
        let order: Vec<Vec<u8>> = list
            .iter()
            .filter(|section| section.magic(extended) == b"pic1")
            .map(|section| section.name(extended).to_vec())
            .collect();
        let mut expected: Vec<Vec<u8>> = vec![b"me".to_vec()];
        for state in CELLS {
            for name in state {
                expected.push(name.to_vec());
            }
        }
        expected.extend(SHADOWS.iter().map(|name| name.to_vec()));
        expected.extend(HELD.iter().map(|name| name.to_vec()));
        expected.push(b"n6".to_vec());
        expected.extend(HANDS.iter().map(|name| name.to_vec()));
        assert_eq!(order, expected);
        // The textures and materials the new panes use.
        let textures = list
            .iter()
            .find(|section| section.magic(extended) == b"txl1")
            .unwrap()
            .bytes(extended);
        assert_eq!(u32_at(textures, 8), Some(6));
        let name = |index: usize| {
            let from = 12 + u32_at(textures, 12 + index * 4).unwrap() as usize;
            let end = from + textures[from..].iter().position(|byte| *byte == 0).unwrap();
            textures[from..end].to_vec()
        };
        assert_eq!(name(1), b"cursol01.bclim");
        assert_eq!(name(5), b"cursol03.bclim");
        let mats = list
            .iter()
            .find(|section| section.magic(extended) == b"mat1")
            .unwrap()
            .bytes(extended);
        let mut entries = [(0usize, 0usize); 512];
        assert_eq!(materials(mats, &mut entries), Some(7));
        let texture_of = |index: usize| u16_at(mats, entries[index].0 + MATERIAL_TEXTURE).unwrap();
        assert_eq!([2, 3, 4, 5, 6].map(texture_of), [0, 2, 3, 4, 5]);
        // A cell of state 2 at the last place, hidden; a state 0 shown.
        let pane = |name: &[u8]| {
            list.iter()
                .find(|section| section.name(extended) == name)
                .unwrap()
                .bytes(extended)
        };
        // The shadows: hidden, half transparent, black, 5 right and 5 down
        // of the held cell, each with the held cell's material of its state.
        for (state, name) in SHADOWS.iter().enumerate() {
            assert_eq!(u16_at(pane(name), PIC_MATERIAL), Some(2 + state as u16));
        }
        let shadow = pane(SHADOWS[1]);
        assert_eq!(shadow[PANE_FLAG] & 1, 0);
        assert_eq!(shadow[PANE_ALPHA], 0x80);
        assert_eq!(
            shadow[PIC_COLORS..PIC_COLORS + 16],
            [0, 0, 0, 0xff].repeat(4)[..]
        );
        assert_eq!(
            f32::from_le_bytes(shadow[PANE_X..PANE_X + 4].try_into().unwrap()),
            HELD_AT.0 + 5.0
        );
        assert_eq!(
            f32::from_le_bytes(shadow[PANE_Y..PANE_Y + 4].try_into().unwrap()),
            HELD_AT.1 - 5.0
        );
        assert_eq!(u16_at(shadow, PIC_MATERIAL), Some(3));
        let last = pane(CELLS[2][3]);
        assert_eq!(
            f32::from_le_bytes(last[PANE_X..PANE_X + 4].try_into().unwrap()),
            cell_x(3)
        );
        assert_eq!(u16_at(last, PIC_MATERIAL), Some(4));
        assert_eq!(last[PANE_FLAG] & 1, 0);
        assert_eq!(pane(CELLS[0][0])[PANE_FLAG] & 1, 1);
        assert_eq!(u16_at(pane(HANDS[1]), PIC_MATERIAL), Some(6));
        // The top line: "Gruppe: " right before the title, hidden, its text
        // right-aligned, ending at x -70; the title from x -70.
        let texts: Vec<Vec<u8>> = list
            .iter()
            .filter(|section| section.magic(extended) == b"txt1")
            .map(|section| section.name(extended).to_vec())
            .collect();
        assert_eq!(texts, [b"nf".to_vec(), b"n2".to_vec()]);
        let float =
            |pane: &[u8], at: usize| f32::from_le_bytes(pane[at..at + 4].try_into().unwrap());
        let label = pane(b"nf");
        assert_eq!(label[PANE_FLAG] & 1, 0);
        assert_eq!(label[TEXT_POSITION], TEXT_RIGHT);
        assert_eq!(float(label, PANE_X) + float(label, PANE_WIDTH) / 2.0, -70.0);
        let title = pane(b"n2");
        assert_eq!(title[PANE_FLAG] & 1, 1);
        assert_eq!(title[TEXT_POSITION], 0);
        assert_eq!(float(title, PANE_X) - float(title, PANE_WIDTH) / 2.0, -70.0);
        assert_eq!((TITLE, GROUP_LABEL), (0x33e, 0x34b));
    }

    #[test]
    fn another_layout_is_left_alone() {
        let mut room = vec![0u8; EXTENDED_ROOM];
        assert_eq!(extend(b"FLYT", &mut room), None);
        let mut original = layout();
        // A layout that has one of the names already.
        let at = original
            .windows(4)
            .position(|window| window == b"gx\0\0")
            .unwrap();
        original[at] = b'2';
        original[at + 1] = 0;
        assert_eq!(extend(&original, &mut room), None);
    }

    #[test]
    fn the_cursor_reaches_the_four_cells_from_the_boxes_and_the_buttons() {
        let mut original = [0u8; PLACES * 8];
        // The original's row ends and the button row, as at 003377fc.
        let mut set = |place: usize, links: [u8; 4]| {
            original[place * 8 + 4..place * 8 + 8].copy_from_slice(&links)
        };
        set(87, [74, 109, 86, 88]);
        set(90, [77, 112, 89, 78]);
        set(91, [78, 100, 99, 92]);
        set(99, [86, 108, 98, 91]);
        set(109, [87, 9, 105, 100]);
        set(112, [90, 12, 105, 100]);
        let mut extended = [0u8; EXTENDED_PLACES * 8];
        extend_places(&original, &mut extended);
        let links = |place: usize| extended[place * 8 + 4..place * 8 + 8].to_vec();
        assert_eq!(links(113), [87, 109, 99, 114]);
        assert_eq!(links(116), [90, 112, 115, 91]);
        assert_eq!(links(99), [86, 108, 98, 113]);
        assert_eq!(links(91), [78, 100, 116, 92]);
        assert_eq!(links(87), [74, 113, 86, 88]);
        assert_eq!(links(112), [116, 12, 105, 100]);
        assert_eq!(&extended[113 * 8..113 * 8 + 4], &id(b"2").to_le_bytes());
        assert_eq!((CURSOR, HAND), (0x344, 0x342));
    }
}

/// The real layout, from a file of the user's own console data given by
/// `BANK_LAYOUT`, run on its own (docs/building.md); no game file is in the
/// repository. `BANK_LAYOUT_OUT`, if given, names a file the extended layout
/// is written to, to look at.
#[cfg(test)]
mod real {
    extern crate std;
    use super::*;

    /// The extended layout's length (docs/internals.md, "Four more cells").
    const EXTENDED_SIZE: usize = 31_604;

    #[test]
    #[ignore = "needs BANK_LAYOUT, the layout from your own copy of the game"]
    fn the_original_layout_extends() {
        let path = std::env::var("BANK_LAYOUT").expect("BANK_LAYOUT names Turtle_lower1_3.bclyt");
        let original = std::fs::read(path).unwrap();
        assert_eq!(original.len(), ORIGINAL_SIZE);
        assert_eq!(offline_core::crc32(&original), ORIGINAL_CRC);
        let mut room = std::vec![0u8; EXTENDED_ROOM];
        let len = extend(&original, &mut room).unwrap();
        assert_eq!(len, EXTENDED_SIZE);
        if let Ok(out) = std::env::var("BANK_LAYOUT_OUT") {
            std::fs::write(out, &room[..len]).unwrap();
        }
    }
}
