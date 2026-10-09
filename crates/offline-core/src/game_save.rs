//! The trainer of a Gen 6 or Gen 7 game save, read from the save file.
//!
//! Bank knows a game by its title. Two copies of one title are told apart by
//! the trainer: ID, secret ID and name never change in a save.
//!
//! The save file ends in a 512-byte footer: two secure values, the magic
//! `FEEB`, then one entry of 8 bytes per data block (u32 length, u16 id, u16
//! checksum). Each block starts on a multiple of 512, in table order, and the
//! footer follows the last one. One block has the length of the trainer
//! block; it starts with the trainer ID, the secret ID and the game's version
//! number, and holds the name further on. This was checked on saves of Alpha
//! Sapphire and Ultra Moon; for the other titles a table without exactly one
//! such block, or a block with another version number, gives no trainer.

pub const FOOTER_SIZE: usize = 0x200;
pub const NAME_SIZE: usize = 26;
const MAGIC: core::ops::Range<usize> = 0x10..0x14;
const TABLE: usize = 0x14;
const ENTRY: usize = 8;
const ALIGNMENT: u64 = 0x200;
const VERSION_AT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    Six,
    Seven,
}
impl Generation {
    /// Length of the trainer block.
    pub const fn trainer_len(self) -> usize {
        match self {
            Self::Six => 0x170,
            Self::Seven => 0xC0,
        }
    }
    const fn name_at(self) -> usize {
        match self {
            Self::Six => 0x48,
            Self::Seven => 0x38,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trainer {
    pub id: u16,
    pub secret_id: u16,
    /// As stored: 13 UTF-16 units.
    pub name: [u8; NAME_SIZE],
}

/// Offset of the trainer block in a save of `file_len` bytes with this
/// footer. `None` unless the table is whole, ends where the footer begins,
/// and holds exactly one block of the trainer block's length.
pub fn trainer_block(footer: &[u8], file_len: u64, generation: Generation) -> Option<u64> {
    if footer.len() != FOOTER_SIZE || footer[MAGIC] != *b"FEEB" {
        return None;
    }
    let (mut offset, mut found, mut count) = (0u64, None, 0);
    for entry in footer[TABLE..].chunks_exact(ENTRY) {
        let length = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        if length == 0 {
            break;
        }
        if length as usize == generation.trainer_len() {
            found = Some(offset);
            count += 1;
        }
        offset = offset.checked_add((u64::from(length) + ALIGNMENT - 1) & !(ALIGNMENT - 1))?;
    }
    if count != 1 || offset.checked_add(FOOTER_SIZE as u64) != Some(file_len) {
        return None;
    }
    found
}

/// The trainer in a trainer block of a game with this version number.
pub fn trainer(block: &[u8], generation: Generation, version: u8) -> Option<Trainer> {
    if block.len() != generation.trainer_len() || block[VERSION_AT] != version {
        return None;
    }
    let mut name = [0; NAME_SIZE];
    name.copy_from_slice(&block[generation.name_at()..generation.name_at() + NAME_SIZE]);
    Some(Trainer {
        id: u16::from_le_bytes([block[0], block[1]]),
        secret_id: u16::from_le_bytes([block[2], block[3]]),
        name,
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    /// A footer for blocks of these lengths, and the file length they give.
    fn footer(lengths: &[u32]) -> (Vec<u8>, u64) {
        let mut out = vec![0u8; FOOTER_SIZE];
        out[..16].fill(0xAB); // the two secure values
        out[MAGIC].copy_from_slice(b"FEEB");
        let mut end = 0u64;
        for (index, length) in lengths.iter().enumerate() {
            let at = TABLE + index * ENTRY;
            out[at..at + 4].copy_from_slice(&length.to_le_bytes());
            out[at + 4..at + 6].copy_from_slice(&(index as u16).to_le_bytes());
            out[at + 6..at + 8].copy_from_slice(&0xC0DEu16.to_le_bytes());
            end += (u64::from(*length) + 0x1ff) & !0x1ff;
        }
        (out, end + FOOTER_SIZE as u64)
    }

    #[test]
    fn the_trainer_block_is_found_where_the_checked_saves_have_it() {
        // The first blocks of Alpha Sapphire up to its trainer block, as far
        // as their rounded lengths go, and of Ultra Moon.
        let mut gen6 = vec![0x200u32; 17];
        gen6[0] = 0x2c8; // rounds up to 0x400
        gen6[1] = 0xb90; // rounds up to 0xc00
        let before: u64 = gen6.iter().map(|l| (u64::from(*l) + 0x1ff) & !0x1ff).sum();
        gen6.extend([0x170, 0x61c, 0x34ad0]);
        let (bytes, len) = footer(&gen6);
        assert_eq!(trainer_block(&bytes, len, Generation::Six), Some(before));
        assert_eq!(trainer_block(&bytes, len, Generation::Seven), None);

        let (bytes, len) = footer(&[0xe28, 0x7c, 0x14, 0xc0, 0x61c, 0x36600]);
        assert_eq!(trainer_block(&bytes, len, Generation::Seven), Some(0x1400));
        assert_eq!(trainer_block(&bytes, len, Generation::Six), None);
    }

    #[test]
    fn anything_but_one_whole_table_with_one_such_block_gives_nothing() {
        let (bytes, len) = footer(&[0x100, 0xc0, 0x300]);
        assert_eq!(trainer_block(&bytes, len, Generation::Seven), Some(0x200));
        // Two blocks of that length, or none.
        let (twice, twice_len) = footer(&[0xc0, 0x100, 0xc0]);
        assert_eq!(trainer_block(&twice, twice_len, Generation::Seven), None);
        let (none, none_len) = footer(&[0x100, 0x300]);
        assert_eq!(trainer_block(&none, none_len, Generation::Seven), None);
        // The blocks do not end where the footer begins.
        for wrong in [len - 0x200, len + 0x200, len + 1, 0] {
            assert_eq!(trainer_block(&bytes, wrong, Generation::Seven), None);
        }
        // No magic, or not a whole footer.
        let mut other = bytes.clone();
        other[0x10] ^= 1;
        assert_eq!(trainer_block(&other, len, Generation::Seven), None);
        assert_eq!(
            trainer_block(&bytes[..FOOTER_SIZE - 1], len, Generation::Seven),
            None
        );
        // A table that fills the footer to its last entry is read whole.
        let full = vec![0xc0u32; 1]
            .into_iter()
            .chain(core::iter::repeat_n(0x10, 58))
            .collect::<Vec<_>>();
        let (bytes, len) = footer(&full);
        assert_eq!(trainer_block(&bytes, len, Generation::Seven), Some(0));
    }

    #[test]
    fn the_trainer_is_read_only_from_a_block_of_the_expected_game() {
        for (generation, version, name_at) in
            [(Generation::Six, 26u8, 0x48), (Generation::Seven, 33, 0x38)]
        {
            let mut block = vec![0u8; generation.trainer_len()];
            block[..2].copy_from_slice(&54321u16.to_le_bytes());
            block[2..4].copy_from_slice(&12345u16.to_le_bytes());
            block[VERSION_AT] = version;
            for (index, unit) in "Serena".encode_utf16().enumerate() {
                block[name_at + index * 2..name_at + index * 2 + 2]
                    .copy_from_slice(&unit.to_le_bytes());
            }
            let found = trainer(&block, generation, version).unwrap();
            assert_eq!((found.id, found.secret_id), (54321, 12345));
            assert_eq!(&found.name[..12], &block[name_at..name_at + 12]);
            assert_eq!(found.name[12..], [0; 14]);
            // Another title's block, or a block of another length.
            assert_eq!(trainer(&block, generation, version + 1), None);
            assert_eq!(trainer(&block[1..], generation, version), None);
            // Each part tells two trainers apart.
            for at in [0, 2, name_at, name_at + 25] {
                let mut other = block.clone();
                other[at] ^= 1;
                assert_ne!(trainer(&other, generation, version), Some(found));
            }
        }
    }
}
