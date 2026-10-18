//! SHA-256 of whole files (`offline_core::Sha256`), and hex text.

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = offline_core::Sha256::new();
    hash.update(data)
        .expect("a file in memory is shorter than SHA-256's limit");
    hash.finalize()
}

pub fn hex(data: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(data.len() * 2);
    for &byte in data {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_known_digest_in_hex() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
