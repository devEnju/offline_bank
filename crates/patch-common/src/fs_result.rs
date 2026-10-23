//! What a result of the console's file system says, as the originals read
//! it. Both programs ask the same question of a failed write: was there no
//! room for it?

/// The module of a result that the file system gives (bits 10..17).
const FS_MODULE: u32 = 0x11;
/// The descriptions the file system gives when the card has no room
/// (bits 0..9).
const NO_SPACE: core::ops::RangeInclusive<u32> = 0xc8..=0xdb;

/// Whether `result` is a failure for lack of space, by the original's own
/// test: Bank's game scan, after it failed to create Bank's extdata
/// (`002abfa0`..`002abfd0`), takes a negative result of the file system
/// module with a description from `0xC8` to `0xDB` for it and shows its
/// message `0x26` ("Pokémon Bank could not be started due to a lack of space
/// on your SD card").
pub const fn lack_of_space(result: u32) -> bool {
    let description = result & 0x3ff;
    let module = (result >> 10) & 0xff;
    result & 0x8000_0000 != 0
        && module == FS_MODULE
        && description >= *NO_SPACE.start()
        && description <= *NO_SPACE.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A result of the file system with `description`, at the level and
    /// summary the console gives such a failure.
    const fn fs(description: u32) -> u32 {
        0xc860_0000 | FS_MODULE << 10 | description
    }

    #[test]
    fn a_full_card_is_what_the_original_takes_for_one() {
        assert_eq!(fs(0xd2), 0xc860_44d2);
        for description in 0xc8..=0xdb {
            assert!(lack_of_space(fs(description)), "{description:x}");
        }
        // The descriptions beside the range, another module, and a result
        // that is no failure are not.
        for result in [
            fs(0xc7),
            fs(0xdc),
            0xc860_4478, // a card that is not there
            0xd900_458b,
            fs(0xd2) & 0x7fff_ffff,
            fs(0xd2) & !(0xff << 10) | 0x12 << 10,
            0,
        ] {
            assert!(!lack_of_space(result), "{result:08x}");
        }
    }
}
