//! Native container layout follows Project_CTR (CiaProcess, TmdProcess,
//! NcchProcess, ExHeaderProcess). No signature verification or decryption.
use crate::{
    add, align64, ascii, be16, be32, be64, bytes, fail, le16, le32, le64, sha256, size, Error,
    Result,
};

pub const BANK_TITLE_ID: u64 = 0x0004_0000_000c_9b00;

#[derive(Debug, Clone)]
pub struct CiaInspection {
    pub sha256: [u8; 32],
    pub byte_len: usize,
    pub title_id: u64,
    pub title_version: u16,
    pub content_size: usize,
    pub contents: Vec<ContentInspection>,
}

#[derive(Debug, Clone)]
pub struct ContentInspection {
    pub id: u32,
    pub index: u16,
    pub content_type: u16,
    pub byte_len: usize,
    pub expected_sha256: [u8; 32],
    pub sha256: Option<[u8; 32]>,
    pub file_offset: Option<usize>,
    pub ncch: Option<NcchInspection>,
}

#[derive(Debug, Clone)]
pub struct NcchInspection {
    pub partition_id: u64,
    pub program_id: u64,
    pub product_code: String,
    pub format_version: u16,
    pub byte_len: usize,
    pub encrypted: bool,
    pub exheader: Option<ExheaderInspection>,
    pub exheader_hash_matches: Option<bool>,
    pub code: Option<StoredCodeInspection>,
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub address: u32,
    pub pages: u32,
    pub byte_len: u32,
}

#[derive(Debug, Clone)]
pub struct ExheaderInspection {
    pub sha256: [u8; 32],
    pub name: String,
    pub remaster_version: u16,
    pub compressed_code: bool,
    pub program_id: u64,
    pub native_save_size: u64,
    pub text: Segment,
    pub rodata: Segment,
    pub data: Segment,
    pub bss_size: u32,
    pub stack_size: u32,
    /// Interpretation depends on the storage-access flags; do not infer permissions.
    pub extdata_id_raw: u64,
    pub storage_access_raw: [u8; 32],
    pub services: Vec<String>,
}

impl ExheaderInspection {
    pub fn code_image_size(&self) -> Result<usize> {
        let pages =
            u64::from(self.text.pages) + u64::from(self.rodata.pages) + u64::from(self.data.pages);
        size(pages * 4096)
    }
}

#[derive(Debug, Clone)]
pub struct StoredCodeInspection {
    pub byte_len: usize,
    pub sha256: [u8; 32],
    pub expected_sha256: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct CodeInspection {
    pub byte_len: usize,
    pub sha256: [u8; 32],
    pub expected_length: Option<usize>,
}

/// Reads identity without examining tickets, keys, or account identifiers.
/// Content hash mismatches are reported, not silently treated as authentic data.
pub fn inspect_cia(data: &[u8]) -> Result<CiaInspection> {
    bytes(data, 0, 0x2020)?;
    let header_len = le32(data, 0)? as usize;
    if header_len < 0x2020 {
        return fail("CIA header is shorter than its content bitmap");
    }
    bytes(data, 0, header_len)?;
    if le16(data, 4)? != 0 || le16(data, 6)? != 0 {
        return fail("unsupported CIA type or format version");
    }
    let cert_len = le32(data, 8)? as usize;
    let ticket_len = le32(data, 12)? as usize;
    let tmd_len = le32(data, 16)? as usize;
    let meta_len = le32(data, 20)? as usize;
    let content_size = size(le64(data, 24)?)?;
    let cert_at = align64(header_len)?;
    let ticket_at = align64(add(cert_at, cert_len)?)?;
    let tmd_at = align64(add(ticket_at, ticket_len)?)?;
    let content_at = align64(add(tmd_at, tmd_len)?)?;
    bytes(data, cert_at, cert_len)?;
    bytes(data, ticket_at, ticket_len)?;
    let tmd = bytes(data, tmd_at, tmd_len)?;
    bytes(data, content_at, content_size)?;
    // A footer is optional. With no footer, padding after contents is optional too.
    if meta_len != 0 {
        bytes(data, align64(add(content_at, content_size)?)?, meta_len)?;
    }
    let signature_len = match be32(tmd, 0)? {
        0x10000 | 0x10003 => 0x240,
        0x10001 | 0x10004 => 0x140,
        0x10002 | 0x10005 => 0x80,
        _ => return fail("unsupported TMD signature type"),
    };
    let body = bytes(
        tmd,
        signature_len,
        tmd.len()
            .checked_sub(signature_len)
            .ok_or_else(|| Error("truncated TMD signature".into()))?,
    )?;
    bytes(body, 0, 0x9c4)?;
    let title_id = be64(body, 0x4c)?;
    let title_version = be16(body, 0x9c)?;
    let count = usize::from(be16(body, 0x9e)?);
    bytes(body, 0x9c4, count * 0x30)?;
    let bitmap = bytes(data, 0x20, 0x2000)?;
    let mut seen = [false; 65536];
    let mut contents = Vec::with_capacity(count);
    let mut cursor = content_at;
    let content_end = add(content_at, content_size)?;
    for record in body[0x9c4..0x9c4 + count * 0x30].chunks_exact(0x30) {
        let id = be32(record, 0)?;
        let index = be16(record, 4)?;
        if seen[usize::from(index)] {
            return fail(format!("duplicate TMD content index {index}"));
        }
        seen[usize::from(index)] = true;
        let content_type = be16(record, 6)?;
        let byte_len = size(be64(record, 8)?)?;
        let expected_sha256 = bytes(record, 16, 32)?.try_into().unwrap();
        let included = bitmap[usize::from(index) / 8] & (0x80 >> (index % 8)) != 0;
        let (file_offset, digest, ncch) = if included {
            let end = add(cursor, byte_len)?;
            if end > content_end {
                return fail("TMD content exceeds CIA content section");
            }
            let content = bytes(data, cursor, byte_len)?;
            let ncch = if content_type & 1 == 0 && content.get(0x100..0x104) == Some(b"NCCH") {
                Some(inspect_ncch(content)?)
            } else {
                None
            };
            let result = (Some(cursor), Some(sha256(content)), ncch);
            cursor = end;
            result
        } else {
            (None, None, None)
        };
        contents.push(ContentInspection {
            id,
            index,
            content_type,
            byte_len,
            expected_sha256,
            sha256: digest,
            file_offset,
            ncch,
        });
    }
    if cursor != content_end {
        return fail("CIA content size disagrees with included TMD records");
    }
    for (index, &was_seen) in seen.iter().enumerate() {
        if bitmap[index / 8] & (0x80 >> (index % 8)) != 0 && !was_seen {
            return fail("CIA bitmap includes a content index absent from the TMD");
        }
    }
    Ok(CiaInspection {
        sha256: sha256(data),
        byte_len: data.len(),
        title_id,
        title_version,
        content_size,
        contents,
    })
}

pub fn inspect_exheader(data: &[u8]) -> Result<ExheaderInspection> {
    let header = bytes(data, 0, 0x400)?;
    let segment = |offset| -> Result<Segment> {
        let segment = Segment {
            address: le32(header, offset)?,
            pages: le32(header, offset + 4)?,
            byte_len: le32(header, offset + 8)?,
        };
        if u64::from(segment.byte_len) > u64::from(segment.pages) * 4096 {
            return fail("exheader segment is larger than its page allocation");
        }
        if u64::from(segment.address) + u64::from(segment.pages) * 4096 > (1_u64 << 32) {
            return fail("exheader segment overflows ARM address space");
        }
        Ok(segment)
    };
    let services = header[0x250..0x350]
        .chunks_exact(8)
        .filter(|name| name[0] != 0)
        .map(ascii)
        .collect();
    Ok(ExheaderInspection {
        sha256: sha256(header),
        name: ascii(&header[..8]),
        remaster_version: le16(header, 0x0e)?,
        compressed_code: header[0x0d] & 1 != 0,
        program_id: le64(header, 0x200)?,
        native_save_size: le64(header, 0x1c0)?,
        text: segment(0x10)?,
        rodata: segment(0x20)?,
        data: segment(0x30)?,
        bss_size: le32(header, 0x3c)?,
        stack_size: le32(header, 0x1c)?,
        extdata_id_raw: le64(header, 0x230)?,
        storage_access_raw: header[0x230..0x250].try_into().unwrap(),
        services,
    })
}

pub fn inspect_ncch(data: &[u8]) -> Result<NcchInspection> {
    bytes(data, 0, 0x200)?;
    if bytes(data, 0x100, 4)? != b"NCCH" {
        return fail("NCCH magic not found");
    }
    let units = 512_u64
        .checked_shl(u32::from(data[0x18e]))
        .ok_or_else(|| Error("invalid NCCH block-size exponent".into()))?;
    let scaled = |n: u32| -> Result<usize> {
        size(
            u64::from(n)
                .checked_mul(units)
                .ok_or_else(|| Error("NCCH size overflow".into()))?,
        )
    };
    let byte_len = scaled(le32(data, 0x104)?)?;
    if byte_len < 0x200 {
        return fail("NCCH declared size is smaller than its header");
    }
    let data = bytes(data, 0, byte_len)?;
    let encrypted = data[0x18f] & 4 == 0;
    let exheader_size = le32(data, 0x180)? as usize;
    let mut result = NcchInspection {
        partition_id: le64(data, 0x108)?,
        program_id: le64(data, 0x118)?,
        product_code: ascii(&data[0x150..0x160]),
        format_version: le16(data, 0x112)?,
        byte_len,
        encrypted,
        exheader: None,
        exheader_hash_matches: None,
        code: None,
    };
    if exheader_size != 0 {
        let exheader = bytes(data, 0x200, exheader_size)?;
        if !encrypted {
            if exheader_size != 0x400 {
                return fail("unsupported NCCH extended-header size");
            }
            result.exheader_hash_matches =
                Some(sha256(exheader).as_slice() == bytes(data, 0x160, 32)?);
            result.exheader = Some(inspect_exheader(exheader)?);
        }
    }
    // Validate all NCCH section boundaries even when encrypted.
    for offset in [0x190, 0x198, 0x1a0, 0x1b0] {
        let section_at = scaled(le32(data, offset)?)?;
        let section_len = scaled(le32(data, offset + 4)?)?;
        if section_len != 0 {
            if section_at < 0x200 {
                return fail("NCCH section overlaps header");
            }
            bytes(data, section_at, section_len)?;
        }
    }
    let exefs_len = scaled(le32(data, 0x1a4)?)?;
    if exefs_len != 0 && !encrypted {
        let exefs = bytes(data, scaled(le32(data, 0x1a0)?)?, exefs_len)?;
        bytes(exefs, 0, 0x200)?;
        let mut ranges = Vec::new();
        let mut names = Vec::new();
        for index in 0..10 {
            let entry = &exefs[index * 16..index * 16 + 16];
            if entry[0] == 0 {
                continue;
            }
            let name = ascii(&entry[..8]);
            if names.contains(&name) {
                return fail("duplicate ExeFS entry name");
            }
            names.push(name.clone());
            let start = add(0x200, le32(entry, 8)? as usize)?;
            let length = le32(entry, 12)? as usize;
            let file = bytes(exefs, start, length)?;
            ranges.push((start, add(start, length)?));
            if name == ".code" {
                result.code = Some(StoredCodeInspection {
                    byte_len: length,
                    sha256: sha256(file),
                    expected_sha256: bytes(exefs, 0x1e0 - index * 32, 32)?.try_into().unwrap(),
                });
            }
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return fail("overlapping ExeFS entries");
        }
    }
    Ok(result)
}

/// The caller must supply an already decompressed image. A bare binary has no
/// reliable magic that can establish whether it was decompressed correctly.
pub fn inspect_code(data: &[u8], exheader: Option<&ExheaderInspection>) -> Result<CodeInspection> {
    if data.is_empty() {
        return fail("empty code image");
    }
    let expected_length = exheader
        .map(ExheaderInspection::code_image_size)
        .transpose()?;
    if let Some(expected) = expected_length {
        if expected != data.len() {
            return fail(format!("code length {:#x} differs from exheader page total {expected:#x}; provide decompressed code", data.len()));
        }
    }
    Ok(CodeInspection {
        byte_len: data.len(),
        sha256: sha256(data),
        expected_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(sig: u32, signature_len: usize, included: bool) -> Vec<u8> {
        let tmd_at = 0x2040;
        let tmd_len = signature_len + 0x9c4 + 0x30;
        let content_at = align64(tmd_at + tmd_len).unwrap();
        let content_len = if included { 16 } else { 0 };
        let mut data = vec![0_u8; content_at + content_len];
        data[..4].copy_from_slice(&0x2020_u32.to_le_bytes());
        data[16..20].copy_from_slice(&(tmd_len as u32).to_le_bytes());
        data[24..32].copy_from_slice(&(content_len as u64).to_le_bytes());
        data[0x20] = if included { 0x80 } else { 0 };
        data[tmd_at..tmd_at + 4].copy_from_slice(&sig.to_be_bytes());
        let body = tmd_at + signature_len;
        data[body + 0x4c..body + 0x54].copy_from_slice(&BANK_TITLE_ID.to_be_bytes());
        data[body + 0x9e..body + 0xa0].copy_from_slice(&1_u16.to_be_bytes());
        data[body + 0x9c4 + 8..body + 0x9c4 + 16].copy_from_slice(&16_u64.to_be_bytes());
        let hash = sha256(&[0; 16]);
        data[body + 0x9c4 + 16..body + 0x9c4 + 48].copy_from_slice(&hash);
        data
    }

    #[test]
    fn parses_all_supported_tmd_signature_sizes() {
        for (sig, length) in [
            (0x10000, 0x240),
            (0x10001, 0x140),
            (0x10002, 0x80),
            (0x10003, 0x240),
            (0x10004, 0x140),
            (0x10005, 0x80),
        ] {
            let result = inspect_cia(&fixture(sig, length, true)).unwrap();
            assert_eq!(result.title_id, BANK_TITLE_ID);
            assert_eq!(
                result.contents[0].sha256,
                Some(result.contents[0].expected_sha256)
            );
        }
    }
    #[test]
    fn absent_tmd_content_does_not_consume_bytes() {
        let result = inspect_cia(&fixture(0x10004, 0x140, false)).unwrap();
        assert_eq!(result.contents[0].file_offset, None);
        assert_eq!(result.contents[0].sha256, None);
    }
    #[test]
    fn truncated_inputs_and_bad_sizes_are_rejected() {
        let data = fixture(0x10004, 0x140, true);
        for end in [0, 31, 0x201f, 0x2050, data.len() - 1] {
            assert!(inspect_cia(&data[..end]).is_err());
        }
        let mut huge = data.clone();
        huge[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(inspect_cia(&huge).is_err());
        let mut short_header = data.clone();
        short_header[..4].copy_from_slice(&0x20_u32.to_le_bytes());
        assert!(inspect_cia(&short_header).is_err());
        let mut wrong_sig = data.clone();
        wrong_sig[0x2040..0x2044].copy_from_slice(&0x99_u32.to_be_bytes());
        assert!(inspect_cia(&wrong_sig).is_err());
        let mut extra = data.clone();
        extra[0x20] |= 0x40;
        assert!(inspect_cia(&extra).is_err());
    }
    #[test]
    fn content_bit_order_uses_most_significant_bit_first() {
        let mut data = fixture(0x10004, 0x140, true);
        data[0x20] = 1;
        let record = 0x2040 + 0x140 + 0x9c4;
        data[record + 4..record + 6].copy_from_slice(&7_u16.to_be_bytes());
        assert_eq!(inspect_cia(&data).unwrap().contents[0].index, 7);
    }
    #[test]
    fn hash_mismatch_is_reported_without_authenticity_claim() {
        let mut data = fixture(0x10004, 0x140, true);
        *data.last_mut().unwrap() = 1;
        let result = inspect_cia(&data).unwrap();
        assert_ne!(
            result.contents[0].sha256,
            Some(result.contents[0].expected_sha256)
        );
    }
    #[test]
    fn exheader_and_code_size_checks() {
        let mut data = vec![0_u8; 0x400];
        data[0x14..0x18].copy_from_slice(&1_u32.to_le_bytes());
        data[0x18..0x1c].copy_from_slice(&100_u32.to_le_bytes());
        let info = inspect_exheader(&data).unwrap();
        assert_eq!(info.code_image_size().unwrap(), 4096);
        assert!(inspect_code(&[1; 4096], Some(&info)).is_ok());
        assert!(inspect_code(&[1; 100], Some(&info)).is_err());
        assert!(inspect_code(&[], None).is_err());
        data[0x18..0x1c].copy_from_slice(&4097_u32.to_le_bytes());
        assert!(inspect_exheader(&data).is_err());
    }
    #[test]
    fn ncch_bounds_and_encryption_gate() {
        let mut data = vec![0_u8; 0x600];
        data[0x100..0x104].copy_from_slice(b"NCCH");
        data[0x104..0x108].copy_from_slice(&3_u32.to_le_bytes());
        data[0x180..0x184].copy_from_slice(&0x400_u32.to_le_bytes());
        assert!(inspect_ncch(&data).unwrap().exheader.is_none());
        data[0x18f] = 4;
        assert!(inspect_ncch(&data).unwrap().exheader.is_some());
        data[0x18e] = 255;
        assert!(inspect_ncch(&data).is_err());
        data[0x18e] = 0;
        data[0x1a0..0x1a4].copy_from_slice(&3_u32.to_le_bytes());
        data[0x1a4..0x1a8].copy_from_slice(&1_u32.to_le_bytes());
        assert!(inspect_ncch(&data).is_err());
    }
    #[test]
    fn arbitrary_short_input_never_panics() {
        let mut seed = 1_u32;
        for len in 0..1024 {
            let mut data = vec![0; len];
            for byte in &mut data {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *byte = (seed >> 24) as u8;
            }
            assert!(inspect_cia(&data).is_err());
            assert!(inspect_ncch(&data).is_err());
            assert!(inspect_exheader(&data).is_err());
        }
    }
}
