/// CRC-32/ISO-HDLC (reflected polynomial 0xEDB88320).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut state = Crc32::new();
    state.update(bytes);
    state.finish()
}

/// The same checksum over data that comes in pieces.
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub fn new() -> Self {
        Self(u32::MAX)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u32::from(byte);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(self.0 & 1));
            }
        }
    }

    pub fn finish(self) -> u32 {
        !self.0
    }
}
