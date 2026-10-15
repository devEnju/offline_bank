const fn table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut state = index as u32;
        let mut bit = 0;
        while bit < 8 {
            state = (state >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(state & 1));
            bit += 1;
        }
        table[index] = state;
        index += 1;
    }
    table
}

// One read-only table per payload, evaluated at compile time.
static TABLE: [u32; 256] = table();

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
            self.0 = (self.0 >> 8) ^ TABLE[((self.0 ^ u32::from(byte)) & 0xff) as usize];
        }
    }

    pub fn finish(self) -> u32 {
        !self.0
    }
}
