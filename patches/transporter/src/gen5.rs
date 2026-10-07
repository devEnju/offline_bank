//! Whether a stored Gen 5 box record holds a Pokémon.
//!
//! The original runs its checks on every Box 1 slot the server did not mark
//! "skip". Offline the patch has to say which slots are empty, and it decides
//! that as the games do: by the species number. The two stored fields that
//! look like a shortcut are not one: a slot the game set up and never used
//! has personality value 0 but checksum 4 (a blank record carries the
//! "genderless" flag), while a slot that was cleared has both zero.

/// A box record: personality value, flags, checksum, then four 32-byte blocks.
pub const RECORD_SIZE: usize = 136;

const FLAGS: usize = 4;
const CHECKSUM: usize = 6;
const DATA: usize = 8;
const BLOCK_WORDS: usize = 16;
/// Flag bit of a record whose data is stored plain and in block order ABCD;
/// the original tests the same bit before it decrypts (00245A28).
const PLAIN: u16 = 2;
/// Where block A, which starts with the species, is stored for each of the
/// 24 block orders. The order is `((personality >> 13) & 31) % 24`.
const BLOCK_A: [u8; 24] = [
    0, 0, 0, 0, 0, 0, 1, 1, 2, 3, 2, 3, 1, 1, 2, 3, 2, 3, 1, 1, 2, 3, 2, 3,
];

fn word(record: &[u8; RECORD_SIZE], offset: usize) -> u16 {
    u16::from_le_bytes([record[offset], record[offset + 1]])
}

/// The species number of the record, 0 for an empty slot.
pub fn species(record: &[u8; RECORD_SIZE]) -> u16 {
    if word(record, FLAGS) & PLAIN != 0 {
        return word(record, DATA);
    }
    let personality = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    let order = ((personality >> 13) & 31) % 24;
    let index = usize::from(BLOCK_A[order as usize]) * BLOCK_WORDS;
    // The data words are XORed with the high halves of an LCG stream seeded
    // with the checksum, one step per word.
    let mut seed = u32::from(word(record, CHECKSUM));
    for _ in 0..=index {
        seed = seed.wrapping_mul(0x41c6_4e6d).wrapping_add(0x6073);
    }
    word(record, DATA + index * 2) ^ (seed >> 16) as u16
}

pub fn holds_pokemon(record: &[u8; RECORD_SIZE]) -> bool {
    species(record) != 0
}

/// Called from the slot entry in `link.rs`.
/// # Safety
/// `record` points to one readable box record of the original's save copy.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_slot_holds(record: *const u8) -> u32 {
    u32::from(holds_pokemon(unsafe {
        &*record.cast::<[u8; RECORD_SIZE]>()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stored-block order for each of the 24 orders, written independently of
    /// `BLOCK_A`: entry `[order][position]` is the block stored there.
    const ORDERS: [&[u8; 4]; 24] = [
        b"ABCD", b"ABDC", b"ACBD", b"ACDB", b"ADBC", b"ADCB", b"BACD", b"BADC", b"BCAD", b"BCDA",
        b"BDAC", b"BDCA", b"CABD", b"CADB", b"CBAD", b"CBDA", b"CDAB", b"CDBA", b"DABC", b"DACB",
        b"DBAC", b"DBCA", b"DCAB", b"DCBA",
    ];

    /// Builds a stored record from plain blocks A..D the way the games do.
    fn stored(personality: u32, plain: &[[u16; BLOCK_WORDS]; 4]) -> [u8; RECORD_SIZE] {
        let mut record = [0u8; RECORD_SIZE];
        record[..4].copy_from_slice(&personality.to_le_bytes());
        let checksum = plain
            .iter()
            .flatten()
            .fold(0u16, |sum, &value| sum.wrapping_add(value));
        record[CHECKSUM..CHECKSUM + 2].copy_from_slice(&checksum.to_le_bytes());
        let order = ORDERS[(((personality >> 13) & 31) % 24) as usize];
        let mut seed = u32::from(checksum);
        for (position, &block) in order.iter().enumerate() {
            for (index, &value) in plain[usize::from(block - b'A')].iter().enumerate() {
                seed = seed.wrapping_mul(0x41c6_4e6d).wrapping_add(0x6073);
                let at = DATA + (position * BLOCK_WORDS + index) * 2;
                let stored = value ^ (seed >> 16) as u16;
                record[at..at + 2].copy_from_slice(&stored.to_le_bytes());
            }
        }
        record
    }

    fn blocks(species: u16) -> [[u16; BLOCK_WORDS]; 4] {
        let mut plain = [[0u16; BLOCK_WORDS]; 4];
        plain[0][0] = species;
        // Something in every block, so a wrong block is not read as zero.
        plain[1][0] = 0x1111;
        plain[2][0] = 0x2222;
        plain[3][0] = 0x3333;
        plain
    }

    #[test]
    fn a_cleared_slot_is_empty() {
        // All zero before encryption: personality 0, checksum 0.
        let record = stored(0, &[[0; BLOCK_WORDS]; 4]);
        assert_eq!(&record[..8], &[0; 8]);
        assert_ne!(&record[8..16], &[0; 8], "the data is still encrypted");
        assert!(!holds_pokemon(&record));
        assert!(!holds_pokemon(&[0; RECORD_SIZE]));
    }

    #[test]
    fn a_never_used_slot_is_empty() {
        // What the games write when they set a box up: only the genderless
        // flag (byte 0x40 of the record, block B) is set; checksum 4.
        let mut plain = [[0u16; BLOCK_WORDS]; 4];
        plain[1][(0x40 - DATA) / 2 - BLOCK_WORDS] = 4;
        let record = stored(0, &plain);
        assert_eq!(word(&record, CHECKSUM), 4);
        // The first bytes as found in a real Pokémon White save.
        assert_eq!(
            &record[8..16],
            &[0x19, 0x07, 0x08, 0xf4, 0x68, 0x54, 0xca, 0xe9]
        );
        assert!(!holds_pokemon(&record));
    }

    #[test]
    fn a_pokemon_is_found_in_every_block_order() {
        for order in 0..24u32 {
            // Two personality values per order, with other bits set as well.
            for personality in [order << 13, (order << 13) | 0xfffc_0000 | 0x1234 & 0x1fff] {
                let record = stored(personality, &blocks(491));
                assert_eq!(species(&record), 491, "order {order}");
                assert!(holds_pokemon(&record));
                let empty = stored(personality, &blocks(0));
                assert!(!holds_pokemon(&empty), "order {order}");
            }
        }
        // Orders 24..31 wrap around.
        assert_eq!(species(&stored(30 << 13, &blocks(7))), 7);
    }

    #[test]
    fn an_egg_counts_as_a_pokemon() {
        // The egg flag lives in block B; the species is still set, so the
        // original's own check refuses it with its message.
        let mut plain = blocks(175);
        plain[1][(0x38 - DATA) / 2 - BLOCK_WORDS + 1] = 0x4000;
        assert!(holds_pokemon(&stored(0x0001_e000, &plain)));
    }

    #[test]
    fn a_plain_record_is_read_without_decrypting() {
        let mut record = [0u8; RECORD_SIZE];
        record[FLAGS] = PLAIN as u8;
        record[CHECKSUM] = 0x55;
        assert!(!holds_pokemon(&record));
        record[DATA..DATA + 2].copy_from_slice(&25u16.to_le_bytes());
        assert_eq!(species(&record), 25);
    }
}
