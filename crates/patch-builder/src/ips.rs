use crate::{bytes, fail, sha256, Error, Result};

/// A same-length replacement in a decompressed code image.
/// Expected bytes must come from a reviewed hook manifest, not the current input.
#[derive(Debug, Clone)]
pub struct CheckedEdit {
    pub offset: usize,
    pub expected: Vec<u8>,
    pub replacement: Vec<u8>,
}

/// Constructs a standard IPS stream only after validating the whole original
/// fingerprint and every expected byte. Does not extend the image or invent hooks.
/// The caller must obtain `expected_sha256` from an independently reviewed manifest.
pub fn build_ips(
    original: &[u8],
    expected_sha256: [u8; 32],
    edits: &[CheckedEdit],
) -> Result<Vec<u8>> {
    if sha256(original) != expected_sha256 {
        return fail("original code SHA-256 does not match the reviewed fingerprint");
    }
    if edits.is_empty() {
        return fail("refusing to build an empty IPS patch");
    }
    let mut edits: Vec<_> = edits.iter().collect();
    edits.sort_unstable_by_key(|edit| edit.offset);
    let mut previous_end = 0;
    for edit in &edits {
        if edit.expected.is_empty() || edit.expected.len() != edit.replacement.len() {
            return fail(
                "IPS edits must have nonempty, same-length expected and replacement bytes",
            );
        }
        let end = edit
            .offset
            .checked_add(edit.expected.len())
            .ok_or_else(|| Error("IPS edit offset overflow".into()))?;
        if edit.offset < previous_end {
            return fail("IPS edits overlap");
        }
        if end > 0x1000000 {
            return fail("IPS edit exceeds the supported 24-bit image range");
        }
        if bytes(original, edit.offset, edit.expected.len())? != edit.expected.as_slice() {
            return fail(format!(
                "original bytes differ at edit offset {:#x}",
                edit.offset
            ));
        }
        if edit.expected == edit.replacement {
            return fail("IPS edit does not change any bytes");
        }
        previous_end = end;
    }
    let mut patch = b"PATCH".to_vec();
    for edit in edits {
        let mut consumed = 0;
        while consumed < edit.replacement.len() {
            let offset = edit.offset + consumed;
            // 0x454f46 spells EOF, and cannot start an IPS record.
            if offset == 0x454f46 {
                return fail("IPS record offset is the reserved EOF marker; include a preceding verified byte");
            }
            let mut length = (edit.replacement.len() - consumed).min(65535);
            // Avoid creating the reserved marker at the start of a split record.
            if offset + length == 0x454f46 && consumed + length < edit.replacement.len() {
                length -= 1;
            }
            let offset_bytes = (offset as u32).to_be_bytes();
            patch.extend_from_slice(&offset_bytes[1..]);
            patch.extend_from_slice(&(length as u16).to_be_bytes());
            patch.extend_from_slice(&edit.replacement[consumed..consumed + length]);
            consumed += length;
        }
    }
    patch.extend_from_slice(b"EOF");
    Ok(patch)
}

/// Encodes an unconditional ARM-state B/BL. Thumb targets and out-of-range
/// branches require a separately reviewed veneer and are deliberately rejected.
pub fn encode_arm_branch(source_address: u32, target_address: u32, link: bool) -> Result<[u8; 4]> {
    if source_address & 3 != 0 || target_address & 3 != 0 {
        return fail("ARM branch addresses must be word aligned and in ARM state");
    }
    let pc = source_address
        .checked_add(8)
        .ok_or_else(|| Error("ARM PC address overflows".into()))?;
    let displacement = i64::from(target_address) - i64::from(pc);
    if !(-0x0200_0000..=0x01ff_fffc).contains(&displacement) {
        return fail("ARM branch target exceeds signed 24-bit instruction range");
    }
    let immediate = ((displacement >> 2) as u32) & 0x00ff_ffff;
    let opcode = if link { 0xeb00_0000 } else { 0xea00_0000 };
    Ok((opcode | immediate).to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(offset: usize, expected: &[u8], replacement: &[u8]) -> CheckedEdit {
        CheckedEdit {
            offset,
            expected: expected.to_vec(),
            replacement: replacement.to_vec(),
        }
    }
    fn apply(original: &[u8], patch: &[u8]) -> Vec<u8> {
        assert_eq!(&patch[..5], b"PATCH");
        let mut result = original.to_vec();
        let mut at = 5;
        while &patch[at..at + 3] != b"EOF" {
            let offset = usize::from(patch[at]) * 65536
                + usize::from(patch[at + 1]) * 256
                + usize::from(patch[at + 2]);
            let length = usize::from(u16::from_be_bytes([patch[at + 3], patch[at + 4]]));
            assert!(length > 0);
            result[offset..offset + length].copy_from_slice(&patch[at + 5..at + 5 + length]);
            at += 5 + length;
        }
        assert_eq!(at + 3, patch.len());
        result
    }
    #[test]
    fn exact_ips_encoding_and_application() {
        let original = b"abcdef";
        let patch = build_ips(original, sha256(original), &[edit(2, b"cd", b"XY")]).unwrap();
        assert_eq!(patch, b"PATCH\0\0\x02\0\x02XYEOF");
        assert_eq!(apply(original, &patch), b"abXYef");
    }
    #[test]
    fn fingerprint_and_expected_bytes_are_both_required() {
        assert!(build_ips(b"abcd", [0; 32], &[edit(0, b"a", b"z")]).is_err());
        assert!(build_ips(b"abcd", sha256(b"abcd"), &[edit(0, b"b", b"z")]).is_err());
    }
    #[test]
    fn overlap_bounds_empty_resize_and_noop_rejected() {
        let input = b"abcd";
        for edits in [
            vec![],
            vec![edit(1, b"bc", b"zz"), edit(2, b"c", b"q")],
            vec![edit(4, b"x", b"y")],
            vec![edit(0, b"", b"")],
            vec![edit(0, b"a", b"zz")],
            vec![edit(0, b"a", b"a")],
            vec![edit(usize::MAX, b"aa", b"zz")],
        ] {
            assert!(build_ips(input, sha256(input), &edits).is_err());
        }
    }
    #[test]
    fn unsorted_edits_are_deterministic() {
        let a = edit(0, b"a", b"q");
        let b = edit(3, b"d", b"z");
        let hash = sha256(b"abcd");
        assert_eq!(
            build_ips(b"abcd", hash, &[a.clone(), b.clone()]).unwrap(),
            build_ips(b"abcd", hash, &[b, a]).unwrap()
        );
    }
    #[test]
    fn large_edits_split_into_standard_records() {
        let input = vec![0; 70_000];
        let patch =
            build_ips(&input, sha256(&input), &[edit(0, &input, &vec![1; 70_000])]).unwrap();
        assert_eq!(apply(&input, &patch), vec![1; 70_000]);
        assert_eq!(patch.len(), 5 + 5 + 65535 + 5 + 4465 + 3);
    }
    #[test]
    fn eof_marker_start_is_rejected_and_split_avoids_it() {
        let input = vec![0; 0x454f46 + 10];
        let fingerprint = sha256(&input);
        assert!(build_ips(&input, fingerprint, &[edit(0x454f46, &[0], &[1])]).is_err());
        let offset = 0x454f46 - 65535;
        let expected = vec![0; 65545];
        let replacement = vec![1; 65545];
        let patch = build_ips(
            &input,
            fingerprint,
            &[edit(offset, &expected, &replacement)],
        )
        .unwrap();
        let applied = apply(&input, &patch);
        assert_eq!(&applied[offset..], replacement);
    }
    #[test]
    fn arm_branch_encoding_and_boundaries() {
        assert_eq!(
            encode_arm_branch(0x100000, 0x100008, false).unwrap(),
            [0, 0, 0, 0xea]
        );
        assert_eq!(
            encode_arm_branch(0x100000, 0x100000, true).unwrap(),
            [0xfe, 0xff, 0xff, 0xeb]
        );
        assert!(encode_arm_branch(0, 0x02000004, false).is_ok());
        assert!(encode_arm_branch(0, 0x02000008, false).is_err());
        assert!(encode_arm_branch(0x02000000, 8, false).is_ok());
        assert!(encode_arm_branch(0x02000000, 4, false).is_err());
        assert!(encode_arm_branch(0, 9, false).is_err());
        assert!(encode_arm_branch(2, 8, false).is_err());
        assert!(encode_arm_branch(0xffff_fffc, 0, false).is_err());
    }
}
