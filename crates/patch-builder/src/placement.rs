//! Checked pairing of expanded initialized data, an exheader, and an IPS stream.
//!
//! This is an address/byte-placement primitive, not approval of a runtime hook.
//! The linked payload must already have been checked for its ABI, relocations,
//! section permissions, and memory budget. No files or install profile are emitted.

use crate::{build_ips, fail, inspect_code, inspect_exheader, sha256, CheckedEdit, Error, Result};

const PAGE: u32 = 0x1000;
const MAX_IPS_IMAGE: usize = 0x1000000;
const APPLICATION_HEAP_BASE: u32 = 0x08000000;

/// Fingerprints must come from an independently reviewed input manifest.
#[derive(Debug, Clone, Copy)]
pub struct InputIdentity {
    pub code_sha256: [u8; 32],
    /// Hash of the entire supplied 0x400- or 0x800-byte exheader file.
    pub exheader_sha256: [u8; 32],
    pub program_id: u64,
    pub remaster_version: u16,
}

/// A linked, flat payload image. Zero-initialized tail is included in memory_size.
/// Executable/read-only pages precede writable data/BSS pages.
#[derive(Debug, Clone, Copy)]
pub struct LinkedPayload<'a> {
    pub image: &'a [u8],
    pub linked_address: u32,
    pub entry_offset: u32,
    pub executable_size: u32,
    pub memory_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadPlacement {
    pub original_code_size: usize,
    pub expanded_code_size: usize,
    pub original_bss_address: u32,
    pub original_bss_end: u32,
    pub payload_address: u32,
    pub payload_file_offset: usize,
    pub payload_entry: u32,
    pub executable_size: u32,
    pub writable_address: u32,
    pub writable_size: u32,
    pub memory_size: u32,
    pub expanded_data_size: u32,
}

/// An inseparable construction result: deploying the IPS without its exheader
/// does not provide the allocation this patch needs. The builder does not deploy.
#[derive(Debug, Clone)]
pub struct PreparedPlacement {
    layout: PayloadPlacement,
    exheader: Vec<u8>,
    ips: Vec<u8>,
    expanded_code_sha256: [u8; 32],
    payload_sha256: [u8; 32],
}

impl PreparedPlacement {
    pub fn layout(&self) -> PayloadPlacement {
        self.layout
    }
    pub fn exheader(&self) -> &[u8] {
        &self.exheader
    }
    pub fn ips(&self) -> &[u8] {
        &self.ips
    }
    pub fn expanded_code_sha256(&self) -> [u8; 32] {
        self.expanded_code_sha256
    }
    pub fn payload_sha256(&self) -> [u8; 32] {
        self.payload_sha256
    }
}

/// Preserves every original segment VA, initializes the former BSS to zero, and
/// places a linked payload on the first page after the original data+BSS extent.
/// Only checked edits within original text/read-only allocations are accepted.
///
/// The output exheader preserves the original compression flag, so Luma can still
/// decompress the installed .code before applying the IPS. Only data page count,
/// data byte size, and BSS size change. The entire added extent is explicitly
/// written by IPS, including zero gaps/tail; allocation-zeroing is not assumed.
///
/// Permission-changing bootstrap code must execute from original executable
/// memory before entering payload pages. RX permission, cache synchronization,
/// startup/native BSS handling, and resource limits remain runtime release gates.
pub fn prepare_expanded_data(
    code: &[u8],
    exheader: &[u8],
    identity: &InputIdentity,
    payload: &LinkedPayload<'_>,
    native_edits: &[CheckedEdit],
) -> Result<PreparedPlacement> {
    if exheader.len() != 0x400 && exheader.len() != 0x800 {
        return fail("Luma exheader must be exactly 0x400 or 0x800 bytes");
    }
    if sha256(code) != identity.code_sha256 || sha256(exheader) != identity.exheader_sha256 {
        return fail("code/exheader fingerprints do not match the reviewed input pair");
    }
    let header = inspect_exheader(exheader)?;
    if header.program_id != identity.program_id
        || header.remaster_version != identity.remaster_version
    {
        return fail("exheader program ID or remaster version differs from the input manifest");
    }
    let text_size = page_ceil(header.text.byte_len)?;
    let ro_size = page_ceil(header.rodata.byte_len)?;
    let data_size = page_ceil(header.data.byte_len)?;
    for (segment, rounded) in [
        (header.text, text_size),
        (header.rodata, ro_size),
        (header.data, data_size),
    ] {
        if segment.address % PAGE != 0 || segment.pages != rounded / PAGE || rounded == 0 {
            return fail("original segment address/page count does not match the loader layout");
        }
    }
    if address_add(header.text.address, text_size)? != header.rodata.address
        || address_add(header.rodata.address, ro_size)? != header.data.address
    {
        return fail("expanded-data placement requires contiguous original segment addresses");
    }
    inspect_code(code, Some(&header))?;
    if code.len() > MAX_IPS_IMAGE {
        return fail("original code exceeds IPS image range");
    }
    let data_file_offset = (text_size as usize)
        .checked_add(ro_size as usize)
        .ok_or_else(|| Error("code offset overflow".into()))?;
    let data_padding_offset = data_file_offset
        .checked_add(header.data.byte_len as usize)
        .ok_or_else(|| Error("data offset overflow".into()))?;
    if code[data_padding_offset..].iter().any(|&byte| byte != 0) {
        return fail("original data padding is not zero; refusing to turn it into initialized BSS");
    }
    let original_bss_address = address_add(header.data.address, header.data.byte_len)?;
    let original_bss_end = address_add(original_bss_address, header.bss_size)?;
    let payload_address = page_ceil(original_bss_end)?;
    if payload.image.is_empty() || payload.image.iter().all(|&byte| byte == 0) {
        return fail("payload image must contain actual initialized bytes");
    }
    if payload.linked_address != payload_address {
        return fail("payload link address differs from the first page after original BSS");
    }
    if payload.executable_size == 0
        || payload.executable_size % PAGE != 0
        || payload.memory_size == 0
        || payload.memory_size % PAGE != 0
        || payload.executable_size > payload.memory_size
        || payload.image.len() > payload.memory_size as usize
    {
        return fail("payload RX/RW extents must be page aligned, ordered, and contain the image");
    }
    let entry_end = payload
        .entry_offset
        .checked_add(4)
        .ok_or_else(|| Error("payload entry overflow".into()))?;
    if payload.entry_offset % 4 != 0
        || entry_end > payload.executable_size
        || entry_end as usize > payload.image.len()
    {
        return fail("payload entry must name an initialized ARM instruction in the RX range");
    }
    let payload_end = address_add(payload_address, payload.memory_size)?;
    if payload_end > APPLICATION_HEAP_BASE {
        return fail("expanded code would reach the application heap address range");
    }
    let expanded_data_size = payload_end
        .checked_sub(header.data.address)
        .ok_or_else(|| Error("payload precedes data segment".into()))?;
    let expanded_code_size = data_file_offset
        .checked_add(expanded_data_size as usize)
        .ok_or_else(|| Error("expanded code size overflow".into()))?;
    if expanded_code_size > MAX_IPS_IMAGE {
        return fail("expanded image exceeds the supported 24-bit IPS range");
    }
    let payload_file_offset = data_file_offset
        .checked_add((payload_address - header.data.address) as usize)
        .ok_or_else(|| Error("payload offset overflow".into()))?;
    if payload_file_offset < code.len() {
        return fail("payload overlaps original initialized code bytes");
    }
    for edit in native_edits {
        let end = edit
            .offset
            .checked_add(edit.expected.len())
            .ok_or_else(|| Error("native edit overflow".into()))?;
        if end > data_file_offset {
            return fail(
                "paired placement accepts edits only within original text/read-only allocations",
            );
        }
    }

    let mut replacement_exheader = exheader.to_vec();
    replacement_exheader[0x34..0x38].copy_from_slice(&(expanded_data_size / PAGE).to_le_bytes());
    replacement_exheader[0x38..0x3c].copy_from_slice(&expanded_data_size.to_le_bytes());
    replacement_exheader[0x3c..0x40].fill(0);
    let replacement_header = inspect_exheader(&replacement_exheader)?;

    // An explicit record covering the whole added extent also zeroes every byte
    // of the former BSS and payload BSS outside the original image.
    let mut base = code.to_vec();
    base.resize(expanded_code_size, 0);
    let mut extension = vec![0; expanded_code_size - code.len()];
    let payload_in_extension = payload_file_offset - code.len();
    extension[payload_in_extension..payload_in_extension + payload.image.len()]
        .copy_from_slice(payload.image);
    let mut edits = native_edits.to_vec();
    edits.push(CheckedEdit {
        offset: code.len(),
        expected: vec![0; extension.len()],
        replacement: extension,
    });
    // Hashing this synthetic zero-padded base is safe only after the independently
    // reviewed original code AND exheader pair passed their checks above.
    let ips = build_ips(&base, sha256(&base), &edits)?;
    for edit in &edits {
        base[edit.offset..edit.offset + edit.replacement.len()].copy_from_slice(&edit.replacement);
    }
    inspect_code(&base, Some(&replacement_header))?;
    Ok(PreparedPlacement {
        layout: PayloadPlacement {
            original_code_size: code.len(),
            expanded_code_size,
            original_bss_address,
            original_bss_end,
            payload_address,
            payload_file_offset,
            payload_entry: address_add(payload_address, payload.entry_offset)?,
            executable_size: payload.executable_size,
            writable_address: address_add(payload_address, payload.executable_size)?,
            writable_size: payload.memory_size - payload.executable_size,
            memory_size: payload.memory_size,
            expanded_data_size,
        },
        exheader: replacement_exheader,
        ips,
        expanded_code_sha256: sha256(&base),
        payload_sha256: sha256(payload.image),
    })
}

fn page_ceil(value: u32) -> Result<u32> {
    value
        .checked_add(PAGE - 1)
        .map(|v| v & !(PAGE - 1))
        .ok_or_else(|| Error("page rounding overflow".into()))
}
fn address_add(base: u32, length: u32) -> Result<u32> {
    base.checked_add(length)
        .ok_or_else(|| Error("ARM address overflow".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BANK_TITLE_ID;
    fn fixture() -> (Vec<u8>, Vec<u8>, InputIdentity) {
        let mut exheader = vec![0; 0x800];
        exheader[0x0d] = 1; // installed .code is compressed; preserve this flag
        exheader[0x0e..0x10].copy_from_slice(&6_u16.to_le_bytes());
        exheader[0x200..0x208].copy_from_slice(&BANK_TITLE_ID.to_le_bytes());
        for (offset, address, size) in [
            (0x10, 0x100000_u32, 0x1100_u32),
            (0x20, 0x102000, 0x600),
            (0x30, 0x103000, 0x501),
        ] {
            exheader[offset..offset + 4].copy_from_slice(&address.to_le_bytes());
            exheader[offset + 4..offset + 8]
                .copy_from_slice(&(page_ceil(size).unwrap() / PAGE).to_le_bytes());
            exheader[offset + 8..offset + 12].copy_from_slice(&size.to_le_bytes());
        }
        exheader[0x3c..0x40].copy_from_slice(&0x1abc_u32.to_le_bytes());
        let mut code = vec![0; 0x4000];
        code[..0x1100].fill(0x55);
        code[0x2000..0x2600].fill(0x66);
        code[0x3000..0x3501].fill(0x77);
        let identity = identity(&code, &exheader);
        (code, exheader, identity)
    }
    fn identity(code: &[u8], exheader: &[u8]) -> InputIdentity {
        InputIdentity {
            code_sha256: sha256(code),
            exheader_sha256: sha256(exheader),
            program_id: BANK_TITLE_ID,
            remaster_version: 6,
        }
    }
    fn payload(image: &[u8]) -> LinkedPayload<'_> {
        LinkedPayload {
            image,
            linked_address: 0x105000,
            entry_offset: 0,
            executable_size: 0x1000,
            memory_size: 0x2000,
        }
    }
    // Simulates Luma's explicit record bounds. Added memory starts as nonzero to
    // prove the IPS does not rely on kernel allocation clearing the new extent.
    fn apply_bounded(code: &mut [u8], ips: &[u8]) -> Result<()> {
        if !ips.starts_with(b"PATCH") {
            return fail("bad test IPS header");
        }
        let mut at = 5;
        while &ips[at..at + 3] != b"EOF" {
            let offset = usize::from(ips[at]) * 65536
                + usize::from(ips[at + 1]) * 256
                + usize::from(ips[at + 2]);
            let length = usize::from(u16::from_be_bytes([ips[at + 3], ips[at + 4]]));
            if offset + length > code.len() || length == 0 {
                return fail("Luma would reject out-of-range IPS record");
            }
            code[offset..offset + length].copy_from_slice(&ips[at + 5..at + 5 + length]);
            at += 5 + length;
        }
        assert_eq!(at + 3, ips.len());
        Ok(())
    }
    #[test]
    fn paired_extension_preserves_native_bytes_and_zeroes_every_new_gap() {
        let (code, exheader, identity) = fixture();
        let bytes = [0x1e, 0xff, 0x2f, 0xe1]; // ARM BX LR, synthetic payload only
        let prepared =
            prepare_expanded_data(&code, &exheader, &identity, &payload(&bytes), &[]).unwrap();
        let layout = prepared.layout();
        assert_eq!(layout.original_bss_address, 0x103501);
        assert_eq!(layout.original_bss_end, 0x104fbd);
        // ceil(0x104fbd) is 0x105000, so the supplied address must be checked.
        assert_eq!(layout.payload_address, 0x105000);
        let mut expanded = vec![0xa5; layout.expanded_code_size];
        expanded[..code.len()].copy_from_slice(&code);
        apply_bounded(&mut expanded, prepared.ips()).unwrap();
        assert_eq!(&expanded[..code.len()], code);
        assert!(expanded[code.len()..layout.payload_file_offset]
            .iter()
            .all(|&v| v == 0));
        assert_eq!(
            &expanded[layout.payload_file_offset..layout.payload_file_offset + 4],
            bytes
        );
        assert!(expanded[layout.payload_file_offset + 4..]
            .iter()
            .all(|&v| v == 0));
        assert_eq!(sha256(&expanded), prepared.expanded_code_sha256());
        assert_eq!(prepared.exheader()[0x0d], 1);
        for (at, &original) in exheader.iter().enumerate() {
            if !(0x34..0x40).contains(&at) {
                assert_eq!(prepared.exheader()[at], original);
            }
        }
        let changed = inspect_exheader(prepared.exheader()).unwrap();
        assert_eq!(changed.bss_size, 0);
        assert_eq!(changed.data.address, 0x103000);
        assert_eq!(
            changed.code_image_size().unwrap(),
            layout.expanded_code_size
        );
        assert!(apply_bounded(&mut code.clone(), prepared.ips()).is_err());
    }
    #[test]
    fn wrong_input_pair_is_rejected() {
        let (code, mut exheader, identity) = fixture();
        let bytes = [1; 4];
        exheader[0x700] ^= 1;
        assert!(prepare_expanded_data(&code, &exheader, &identity, &payload(&bytes), &[]).is_err());
        exheader[0x700] ^= 1;
        let mut wrong = identity;
        wrong.code_sha256[0] ^= 1;
        assert!(prepare_expanded_data(&code, &exheader, &wrong, &payload(&bytes), &[]).is_err());
        let mut wrong = identity;
        wrong.remaster_version = 0;
        assert!(prepare_expanded_data(&code, &exheader, &wrong, &payload(&bytes), &[]).is_err());
    }
    #[test]
    fn inconsistent_segment_pages_gaps_and_padding_are_rejected() {
        let (code, exheader, _) = fixture();
        let bytes = [1; 4];
        for offset in [0x14, 0x20, 0x30] {
            let mut broken = exheader.clone();
            broken[offset] ^= 1;
            assert!(prepare_expanded_data(
                &code,
                &broken,
                &identity(&code, &broken),
                &payload(&bytes),
                &[]
            )
            .is_err());
        }
        let mut broken = code.clone();
        broken[0x3fff] = 1;
        assert!(prepare_expanded_data(
            &broken,
            &exheader,
            &identity(&broken, &exheader),
            &payload(&bytes),
            &[]
        )
        .is_err());
    }
    #[test]
    fn linked_address_entry_and_rx_rw_boundaries_are_checked() {
        let (code, exheader, identity) = fixture();
        let bytes = [1; 4];
        let good = payload(&bytes);
        for bad in [
            LinkedPayload {
                linked_address: 0x103000,
                ..good
            },
            LinkedPayload {
                entry_offset: 1,
                ..good
            },
            LinkedPayload {
                entry_offset: 0x1000,
                ..good
            },
            LinkedPayload {
                executable_size: 1,
                ..good
            },
            LinkedPayload {
                memory_size: 0x800,
                ..good
            },
            LinkedPayload {
                executable_size: 0x3000,
                ..good
            },
            LinkedPayload {
                memory_size: 0xfffff000,
                ..good
            },
        ] {
            assert!(prepare_expanded_data(&code, &exheader, &identity, &bad, &[]).is_err());
        }
        assert!(
            prepare_expanded_data(&code, &exheader, &identity, &payload(&[0; 4]), &[]).is_err()
        );
    }
    #[test]
    fn native_edits_require_original_bytes_and_cannot_touch_bss() {
        let (code, exheader, identity) = fixture();
        let bytes = [1; 4];
        let valid = CheckedEdit {
            offset: 0x1100,
            expected: vec![0; 4],
            replacement: vec![1; 4],
        };
        assert!(prepare_expanded_data(
            &code,
            &exheader,
            &identity,
            &payload(&bytes),
            core::slice::from_ref(&valid)
        )
        .is_ok());
        let wrong = CheckedEdit {
            expected: vec![2; 4],
            ..valid.clone()
        };
        assert!(
            prepare_expanded_data(&code, &exheader, &identity, &payload(&bytes), &[wrong]).is_err()
        );
        let data = CheckedEdit {
            offset: 0x3ffc,
            ..valid
        };
        assert!(
            prepare_expanded_data(&code, &exheader, &identity, &payload(&bytes), &[data]).is_err()
        );
    }
    #[test]
    fn info_only_exheader_is_supported_without_touching_unrelated_fields() {
        let (code, mut exheader, _) = fixture();
        exheader.truncate(0x400);
        let bytes = [1; 4];
        let prepared = prepare_expanded_data(
            &code,
            &exheader,
            &identity(&code, &exheader),
            &payload(&bytes),
            &[],
        )
        .unwrap();
        assert_eq!(prepared.exheader().len(), 0x400);
    }
    #[test]
    fn reviewed_bank_geometry_places_payload_after_native_bss() {
        // Geometry only: synthetic zero bytes, no proprietary image fixture.
        let (_, mut exheader, _) = fixture();
        for (offset, address, size) in [
            (0x10, 0x100000_u32, 0x213910_u32),
            (0x20, 0x314000, 0x55370),
            (0x30, 0x36a000, 0x41acc),
        ] {
            exheader[offset..offset + 4].copy_from_slice(&address.to_le_bytes());
            exheader[offset + 4..offset + 8]
                .copy_from_slice(&(page_ceil(size).unwrap() / PAGE).to_le_bytes());
            exheader[offset + 8..offset + 12].copy_from_slice(&size.to_le_bytes());
        }
        exheader[0x3c..0x40].copy_from_slice(&0x4ee38_u32.to_le_bytes());
        let code = vec![0; 0x2ac000];
        let bytes = [0x1e, 0xff, 0x2f, 0xe1];
        let linked = LinkedPayload {
            linked_address: 0x3fb000,
            ..payload(&bytes)
        };
        let prepared =
            prepare_expanded_data(&code, &exheader, &identity(&code, &exheader), &linked, &[])
                .unwrap();
        assert_eq!(prepared.layout().original_bss_address, 0x3abacc);
        assert_eq!(prepared.layout().original_bss_end, 0x3fa904);
        assert_eq!(
            prepared.layout().payload_address,
            crate::bank15::PAYLOAD_ADDRESS
        );
        assert_eq!(
            prepared.layout().payload_file_offset,
            crate::bank15::PAYLOAD_FILE_OFFSET
        );
        assert_eq!(prepared.layout().expanded_code_size, 0x2fd000);
    }
}
