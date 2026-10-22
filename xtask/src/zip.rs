//! The release zips: deflate (RFC 1951) with the fixed Huffman codes and
//! matches found in a 32 KiB window, in a ZIP container (APPNOTE 6.3) of
//! files only, with CRC-32 from `offline-core`. Enough for the packages,
//! whose `code.ips` is mostly zero bytes, and for every unzip tool.

use offline_core::crc32;

/// Writes bits from the lowest up, as deflate packs them.
struct Bits {
    out: Vec<u8>,
    buffer: u64,
    count: u32,
}

impl Bits {
    fn put(&mut self, value: u32, bits: u32) {
        self.buffer |= (value as u64) << self.count;
        self.count += bits;
        while self.count >= 8 {
            self.out.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code, which deflate writes from its highest bit.
    fn code(&mut self, code: u32, bits: u32) {
        self.put(code.reverse_bits() >> (32 - bits), bits);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.buffer as u8);
        }
        self.out
    }
}

/// The fixed code of a literal/length symbol (RFC 1951, 3.2.6).
fn literal(bits: &mut Bits, symbol: u32) {
    match symbol {
        0..=143 => bits.code(0x30 + symbol, 8),
        144..=255 => bits.code(0x190 + symbol - 144, 9),
        256..=279 => bits.code(symbol - 256, 7),
        _ => bits.code(0xc0 + symbol - 280, 8),
    }
}

const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The last entry of `bases` not above `value`.
fn index(bases: &[u32], value: u32) -> usize {
    bases
        .iter()
        .rposition(|&base| base <= value)
        .expect("in range")
}

fn copy(bits: &mut Bits, length: u32, distance: u32) {
    let at = index(&LENGTH_BASE, length);
    literal(bits, 257 + at as u32);
    bits.put(length - LENGTH_BASE[at], LENGTH_EXTRA[at]);
    let at = index(&DISTANCE_BASE, distance);
    bits.code(at as u32, 5);
    bits.put(distance - DISTANCE_BASE[at], DISTANCE_EXTRA[at]);
}

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// How many earlier places with the same three bytes are tried.
const CHAIN: usize = 64;
const HASH_BITS: u32 = 15;

fn hash(data: &[u8], at: usize) -> usize {
    let value = (data[at] as u32) << 16 | (data[at + 1] as u32) << 8 | data[at + 2] as u32;
    (value.wrapping_mul(0x9e37_79b1) >> (32 - HASH_BITS)) as usize
}

/// `data` as one final deflate block with the fixed codes.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits {
        out: Vec::with_capacity(data.len() / 4 + 16),
        buffer: 0,
        count: 0,
    };
    bits.put(1, 1); // the final block
    bits.put(1, 2); // fixed codes
                    // The most recent place of each hash, and the one before each place.
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut previous = vec![usize::MAX; WINDOW];
    let insert = |head: &mut [usize], previous: &mut [usize], at: usize| {
        if at + MIN_MATCH <= data.len() {
            let h = hash(data, at);
            previous[at % WINDOW] = head[h];
            head[h] = at;
        }
    };
    let mut at = 0;
    while at < data.len() {
        let mut best = (0, 0);
        if at + MIN_MATCH <= data.len() {
            let limit = MAX_MATCH.min(data.len() - at);
            let mut candidate = head[hash(data, at)];
            for _ in 0..CHAIN {
                if candidate == usize::MAX || at - candidate > WINDOW {
                    break;
                }
                let length = data[candidate..]
                    .iter()
                    .zip(&data[at..at + limit])
                    .take_while(|(a, b)| a == b)
                    .count();
                if length > best.0 {
                    best = (length, at - candidate);
                    if length == limit {
                        break;
                    }
                }
                let next = previous[candidate % WINDOW];
                if next == usize::MAX || next >= candidate {
                    break;
                }
                candidate = next;
            }
        }
        if best.0 >= MIN_MATCH {
            copy(&mut bits, best.0 as u32, best.1 as u32);
            for each in at..at + best.0 {
                insert(&mut head, &mut previous, each);
            }
            at += best.0;
        } else {
            literal(&mut bits, data[at] as u32);
            insert(&mut head, &mut previous, at);
            at += 1;
        }
    }
    literal(&mut bits, 256);
    bits.finish()
}

/// A date and time as ZIP stores it (MS-DOS, local time unknown, two
/// seconds resolution).
#[derive(Clone, Copy)]
pub struct Stamp {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Stamp {
    fn time(self) -> u16 {
        (self.hour << 11 | self.minute << 5 | (self.second / 2)) as u16
    }
    fn date(self) -> u16 {
        ((self.year - 1980) << 9 | self.month << 5 | self.day) as u16
    }
}

/// A ZIP of `files` (path inside the zip with `/`, contents), in order.
pub fn zip(files: &[(String, Vec<u8>)], stamp: Stamp) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, contents) in files {
        let packed = deflate(contents);
        let crc = crc32(contents);
        let offset = out.len() as u32;
        let common = |header: &mut Vec<u8>| {
            header.extend(20u16.to_le_bytes()); // version needed: deflate
            header.extend(0u16.to_le_bytes()); // flags
            header.extend(8u16.to_le_bytes()); // deflate
            header.extend(stamp.time().to_le_bytes());
            header.extend(stamp.date().to_le_bytes());
            header.extend(crc.to_le_bytes());
            header.extend((packed.len() as u32).to_le_bytes());
            header.extend((contents.len() as u32).to_le_bytes());
            header.extend((name.len() as u16).to_le_bytes());
            header.extend(0u16.to_le_bytes()); // no extra field
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend(name.as_bytes());
        out.extend(&packed);
        directory.extend(0x0201_4b50u32.to_le_bytes());
        directory.extend(20u16.to_le_bytes()); // made by: MS-DOS, 2.0
        common(&mut directory);
        directory.extend(0u16.to_le_bytes()); // no comment
        directory.extend(0u16.to_le_bytes()); // disk
        directory.extend(0u16.to_le_bytes()); // internal attributes
        directory.extend(0u32.to_le_bytes()); // external attributes
        directory.extend(offset.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend(&directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // this disk
    out.extend(0u16.to_le_bytes()); // the directory's disk
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((directory.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // no comment
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decoder of fixed-code blocks only, to read back what `deflate`
    /// writes.
    fn inflate(data: &[u8]) -> Vec<u8> {
        let mut at = 0usize;
        let mut bit = |count: u32| {
            let mut value = 0;
            for i in 0..count {
                value |= ((data[at / 8] >> (at % 8)) as u32 & 1) << i;
                at += 1;
            }
            value
        };
        assert_eq!(bit(1), 1, "final block");
        assert_eq!(bit(2), 1, "fixed codes");
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Read a code from its highest bit: 7 bits, then 8, then 9.
            let mut code = 0;
            let mut length = 0;
            let symbol = loop {
                code = code << 1 | bit(1);
                length += 1;
                match (length, code) {
                    (7, 0..=0x17) => break code + 256,
                    (8, 0x30..=0xbf) => break code - 0x30,
                    (8, 0xc0..=0xc7) => break code - 0xc0 + 280,
                    (9, 0x190..=0x1ff) => break code - 0x190 + 144,
                    _ => assert!(length < 9, "no such code"),
                }
            };
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => return out,
                _ => {
                    let i = (symbol - 257) as usize;
                    let length = LENGTH_BASE[i] + bit(LENGTH_EXTRA[i]);
                    let mut d = 0;
                    for _ in 0..5 {
                        d = d << 1 | bit(1);
                    }
                    let distance = DISTANCE_BASE[d as usize] + bit(DISTANCE_EXTRA[d as usize]);
                    for _ in 0..length {
                        out.push(out[out.len() - distance as usize]);
                    }
                }
            }
        }
    }

    #[test]
    fn known_streams_match_zlib() {
        // What zlib writes for these with the fixed codes.
        assert_eq!(deflate(b""), [0x03, 0x00]);
        assert_eq!(deflate(b"a"), [0x4b, 0x04, 0x00]);
    }

    #[test]
    fn what_is_packed_unpacks() {
        let mut ips = b"PATCH".to_vec();
        ips.extend(vec![0; 100_000]);
        ips.extend((0..70_000u32).map(|i| (i * 7 % 251) as u8));
        ips.extend(b"PATCHPATCHPATCH and text, and text again");
        ips.extend(vec![0xa5; 40_000]);
        ips.extend((0..300u32).map(|i| (i ^ (i >> 3)) as u8));
        for data in [
            ips.as_slice(),
            b"",
            b"x",
            b"xyxyxyxyxy",
            &[0; 259],
            &[1; 1000],
        ] {
            assert_eq!(inflate(&deflate(data)), data);
        }
        assert!(deflate(&ips).len() < ips.len() / 2);
    }

    #[test]
    fn the_container_names_every_file_once() {
        let stamp = Stamp {
            year: 2026,
            month: 10,
            day: 22,
            hour: 13,
            minute: 5,
            second: 9,
        };
        let files = vec![
            ("README.txt".to_owned(), b"read me\r\n".to_vec()),
            ("luma/titles/X/code.ips".to_owned(), vec![0; 5000]),
        ];
        let zip = zip(&files, stamp);
        assert_eq!(&zip[..4], b"PK\x03\x04");
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([zip[end + 10], zip[end + 11]]), 2);
        let start = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
        assert_eq!(&zip[start..start + 4], b"PK\x01\x02");
        // The second local header follows the first file's packed data.
        let packed = u32::from_le_bytes(zip[18..22].try_into().unwrap()) as usize;
        let second = 30 + "README.txt".len() + packed;
        assert_eq!(&zip[second..second + 4], b"PK\x03\x04");
        assert_eq!(stamp.date(), (46 << 9 | 10 << 5 | 22) as u16);
    }
}
