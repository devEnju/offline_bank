//! Allocation-free streaming SHA-256 for complete-save fingerprints.
//! Algorithm: [FIPS 180-4](https://csrc.nist.gov/pubs/fips/180-4/upd1/final).
//! A digest identifies bytes; save identity, extent, write order, and secure-value
//! handling remain the native adapter's responsibility.

/// The SHA-256 bit-length field cannot represent this input length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sha256Error {
    LengthOverflow,
}

/// Streaming SHA-256 with fixed memory use. Cloning preserves a shared prefix.
/// No allocation or native service calls are used.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            length: 0,
        }
    }

    /// Number of input bytes accepted so far, excluding padding.
    pub const fn len(&self) -> u64 {
        self.length
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Appends bytes in logical file order. On length overflow, leaves this
    /// state unchanged. Chunk boundaries do not affect the resulting digest.
    pub fn update(&mut self, mut bytes: &[u8]) -> Result<(), Sha256Error> {
        let added = u64::try_from(bytes.len()).map_err(|_| Sha256Error::LengthOverflow)?;
        let new_length = self
            .length
            .checked_add(added)
            .filter(|&n| n <= u64::MAX / 8)
            .ok_or(Sha256Error::LengthOverflow)?;
        let buffered = (self.length % 64) as usize;
        self.length = new_length;

        if buffered != 0 {
            let amount = bytes.len().min(64 - buffered);
            self.buffer[buffered..buffered + amount].copy_from_slice(&bytes[..amount]);
            if buffered + amount < 64 {
                return Ok(());
            }
            compress(&mut self.state, &self.buffer);
            bytes = &bytes[amount..];
        }
        let mut blocks = bytes.chunks_exact(64);
        for block in &mut blocks {
            compress(&mut self.state, block);
        }
        let remainder = blocks.remainder();
        self.buffer[..remainder.len()].copy_from_slice(remainder);
        Ok(())
    }

    /// Consumes the state and returns the conventional 32-byte SHA-256 digest.
    pub fn finalize(mut self) -> [u8; 32] {
        let buffered = (self.length % 64) as usize;
        self.buffer[buffered] = 0x80;
        if buffered >= 56 {
            self.buffer[buffered + 1..].fill(0);
            compress(&mut self.state, &self.buffer);
            self.buffer[..56].fill(0);
        } else {
            self.buffer[buffered + 1..56].fill(0);
        }
        // update rejects lengths whose bit count cannot fit in this field.
        self.buffer[56..].copy_from_slice(&(self.length * 8).to_be_bytes());
        compress(&mut self.state, &self.buffer);
        let mut digest = [0; 32];
        for (chunk, word) in digest.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }
}
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut words = [0_u32; 64];
    for (word, chunk) in words.iter_mut().take(16).zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes(chunk.try_into().unwrap());
    }
    for i in 16..64 {
        let a = words[i - 15];
        let b = words[i - 2];
        let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
        let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
        words[i] = words[i - 16]
            .wrapping_add(s0)
            .wrapping_add(words[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(K[i])
            .wrapping_add(words[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_from_hex(text: &str) -> [u8; 32] {
        assert_eq!(text.len(), 64);
        let mut digest = [0; 32];
        for (out, chunk) in digest.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let high = (chunk[0] as char).to_digit(16).unwrap();
            let low = (chunk[1] as char).to_digit(16).unwrap();
            *out = (high * 16 + low) as u8;
        }
        digest
    }

    #[test]
    fn published_vectors_match_at_every_split_point() {
        for (message, expected) in [
            ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            ("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"),
            ("abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu", "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"),
        ] {
            let message = message.as_bytes();
            for split in 0..=message.len() {
                let mut hash = Sha256::new();
                assert!(hash.is_empty());
                hash.update(&message[..split]).unwrap();
                hash.update(&[]).unwrap();
                hash.update(&message[split..]).unwrap();
                assert_eq!(hash.len(), message.len() as u64);
                assert_eq!(hash.finalize(), digest_from_hex(expected));
            }
        }
    }

    #[test]
    fn million_a_vector_streams_without_whole_input_allocation() {
        let block = [b'a'; 1000];
        let mut hash = Sha256::default();
        for _ in 0..1000 {
            hash.update(&block).unwrap();
        }
        assert_eq!(hash.len(), 1_000_000);
        assert_eq!(
            hash.finalize(),
            digest_from_hex("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
        );
    }

    #[test]
    fn binary_boundaries_match_independent_platform_sha256() {
        // Independently computed with .NET SHA256 on 2026-10-03.
        let mut input = [0; 10003];
        for (index, byte) in input.iter_mut().enumerate() {
            *byte = (index * 37 + 11) as u8;
        }
        for (length, expected) in [
            (
                55,
                "2900465fcb533e05a158fd2b3be0e5e3b03740d83060aa3580e0d98a96bf2384",
            ),
            (
                56,
                "31454ff48ef36af2f08fd511bdc37d9d5855ac23e992e5ff5445cb6b7674a674",
            ),
            (
                63,
                "5f6401b96532c36de4e65beec0409b69b1d181864c8009b7a04f43e5d56350d1",
            ),
            (
                64,
                "94eb5de4943613fd048dc93393ab06877405faa39c11f53e9386083339833e7e",
            ),
            (
                65,
                "fc518669b6eb4b4dd91827ecacef86689c725bd5bab888fd3b26dbb196eec954",
            ),
            (
                127,
                "0fe729ff19257bd6fec853acc2ea355f6b34b58e6c0f684c3e188fcdfcd9baae",
            ),
            (
                128,
                "0aedd4856f8eba0963627336ad5144a9a7dbe12498e6066f0165fc97d8ddee4c",
            ),
            (
                129,
                "4f1757ae4bffbae86d775b831765b75af154d52f7deaa46dd378051a2d3ad57f",
            ),
            (
                10003,
                "17f0109972faab856dd64bcc3f30885622fd9d91af104ba08b291c0042ab60f2",
            ),
        ] {
            for chunk_size in [1, 3, 7, 55, 56, 63, 64, 65, 127, 512, 4096, 10003] {
                let mut hash = Sha256::new();
                for chunk in input[..length].chunks(chunk_size) {
                    hash.update(chunk).unwrap();
                }
                assert_eq!(hash.finalize(), digest_from_hex(expected));
            }
        }
    }

    #[test]
    fn cloning_prefix_supports_separate_save_observations() {
        let mut prefix = Sha256::new();
        prefix.update(b"a").unwrap();
        let mut before = prefix.clone();
        before.update(b"bc").unwrap();
        let mut after = prefix;
        after.update(b"").unwrap();
        assert_eq!(
            before.finalize(),
            digest_from_hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            after.finalize(),
            digest_from_hex("ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb")
        );
    }

    #[test]
    fn length_overflow_is_rejected_without_modifying_state() {
        let mut hash = Sha256::new();
        // Exercise arithmetic at the limit without processing exabytes.
        hash.length = u64::MAX / 8;
        hash.buffer.fill(0x5a);
        let previous = hash.clone();
        assert_eq!(hash.update(b"x"), Err(Sha256Error::LengthOverflow));
        assert_eq!(hash.length, previous.length);
        assert_eq!(hash.buffer, previous.buffer);
        assert_eq!(hash.state, previous.state);
        assert!(hash.update(&[]).is_ok());
        assert_eq!(hash.length, previous.length);
    }
}
