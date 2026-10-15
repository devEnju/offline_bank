use offline_core::{crc32, native_blob::BLOB_SIZE, sections::BANK_SIZE, Crc32};
use std::{hint::black_box, time::Duration, time::Instant};

// The original implementation, kept independent of the production table.
struct Bitwise(u32);

impl Bitwise {
    fn new() -> Self {
        Self(u32::MAX)
    }

    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u32::from(byte);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(self.0 & 1));
            }
        }
    }

    fn finish(self) -> u32 {
        !self.0
    }
}

fn bitwise(bytes: &[u8]) -> u32 {
    let mut state = Bitwise::new();
    state.update(bytes);
    state.finish()
}

fn random_bytes(length: usize) -> Vec<u8> {
    let mut state = 0x92d6_8ca2u32;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

const LENGTHS: &[usize] = &[
    0, 1, 2, 31, 32, 63, 64, 135, 136, 140, 255, 256, 511, 512, 513, 4095, 4096, 4097, BANK_SIZE,
    BLOB_SIZE,
];

#[test]
fn every_one_and_two_byte_input_matches_bitwise() {
    for byte in u8::MIN..=u8::MAX {
        assert_eq!(crc32(&[byte]), bitwise(&[byte]), "byte {byte}");
    }
    for word in u16::MIN..=u16::MAX {
        let bytes = word.to_le_bytes();
        assert_eq!(crc32(&bytes), bitwise(&bytes), "word {word}");
    }
}

#[test]
fn repeated_patterns_match_bitwise_through_full_body_size() {
    for byte in [0, 0xff, 0x55, 0xaa] {
        let bytes = vec![byte; BLOB_SIZE];
        for &length in LENGTHS {
            assert_eq!(
                crc32(&bytes[..length]),
                bitwise(&bytes[..length]),
                "byte {byte}, length {length}"
            );
        }
    }
}

#[test]
fn random_inputs_and_offset_subslices_match_bitwise() {
    let bytes = random_bytes(BLOB_SIZE + 3);
    for offset in 0..4 {
        for &length in LENGTHS {
            let input = &bytes[offset..offset + length];
            assert_eq!(
                crc32(input),
                bitwise(input),
                "offset {offset}, length {length}"
            );
        }
    }
}

#[test]
fn incremental_updates_match_at_every_short_split() {
    let bytes = random_bytes(1025);
    let expected = bitwise(&bytes);
    for split in 0..=bytes.len() {
        let mut state = Crc32::new();
        state.update(&[]);
        state.update(&bytes[..split]);
        state.update(&[]);
        state.update(&bytes[split..]);
        state.update(&[]);
        assert_eq!(state.finish(), expected, "split {split}");
    }
    assert_eq!(Crc32::new().finish(), 0);
    let mut empty = Crc32::default();
    empty.update(&[]);
    assert_eq!(empty.finish(), 0);
}

#[test]
fn incremental_updates_match_for_full_body_and_uneven_chunks() {
    let bytes = random_bytes(BLOB_SIZE);
    let expected = bitwise(&bytes);
    assert_eq!(crc32(&bytes), expected);
    for chunk_size in [1, 3, 31, 511, 512, 513, 4096] {
        let mut state = Crc32::new();
        for chunk in bytes.chunks(chunk_size) {
            state.update(chunk);
            state.update(&[]);
        }
        assert_eq!(state.finish(), expected, "chunk size {chunk_size}");
    }

    let mut state = Crc32::new();
    let mut rest = bytes.as_slice();
    for length in [1, 511, 0, 513, 31, 4096, 3] {
        let (chunk, tail) = rest.split_at(length);
        state.update(chunk);
        rest = tail;
    }
    state.update(rest);
    assert_eq!(state.finish(), expected);
}

fn table_streamed(bytes: &[u8]) -> u32 {
    let mut state = Crc32::new();
    for chunk in bytes.chunks(512) {
        state.update(chunk);
    }
    state.finish()
}

fn bitwise_streamed(bytes: &[u8]) -> u32 {
    let mut state = Bitwise::new();
    for chunk in bytes.chunks(512) {
        state.update(chunk);
    }
    state.finish()
}

fn timed_batch(checksum: fn(&[u8]) -> u32, bytes: &[u8], iterations: usize) -> Duration {
    let checksum = black_box(checksum);
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(checksum(black_box(bytes)));
    }
    start.elapsed()
}

#[test]
#[ignore = "manual host throughput measurement; run with --release --ignored --nocapture"]
fn crc32_throughput() {
    let bytes = random_bytes(BLOB_SIZE);
    type Checksum = fn(&[u8]) -> u32;
    let cases: [(&str, usize, Checksum, Checksum); 5] = [
        ("journal", 136, crc32, bitwise),
        ("read chunk", 512, crc32, bitwise),
        ("Bank file", BANK_SIZE, crc32, bitwise),
        ("native body", BLOB_SIZE, crc32, bitwise),
        (
            "body / 512-byte updates",
            BLOB_SIZE,
            table_streamed,
            bitwise_streamed,
        ),
    ];
    for (name, length, table, reference) in cases {
        let input = &bytes[..length];
        assert_eq!(table(input), reference(input));
        // Equal work for each algorithm; allocation and generation are outside timing.
        let iterations = (16 * 1024 * 1024 / length).max(1);
        timed_batch(table, input, iterations);
        timed_batch(reference, input, iterations);
        let mut table_times = Vec::new();
        let mut reference_times = Vec::new();
        for sample in 0..7 {
            // Alternate order to reduce bias from changing host conditions.
            if sample % 2 == 0 {
                table_times.push(timed_batch(table, input, iterations));
                reference_times.push(timed_batch(reference, input, iterations));
            } else {
                reference_times.push(timed_batch(reference, input, iterations));
                table_times.push(timed_batch(table, input, iterations));
            }
        }
        table_times.sort_unstable();
        reference_times.sort_unstable();
        let table_seconds = table_times[3].as_secs_f64();
        let reference_seconds = reference_times[3].as_secs_f64();
        let mib = (length * iterations) as f64 / (1024.0 * 1024.0);
        println!(
            "{name}: {length} bytes, table {:.1} MiB/s, bitwise {:.1} MiB/s, {:.2}x faster (median of 7)",
            mib / table_seconds,
            mib / reference_seconds,
            reference_seconds / table_seconds
        );
    }
}
