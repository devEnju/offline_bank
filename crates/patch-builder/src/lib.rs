//! Bounded inspection and development-patch construction with exact input checks.
//! Inspection establishes byte identity, not authenticity or patch compatibility.

pub mod artifacts;
pub mod bank15;
pub mod elf;
mod formats;
mod hash;
mod ips;
pub mod placement;
pub mod transporter15;

pub use artifacts::{build_development_patch, DevelopmentArtifacts};
pub use bank15::prepare_development_patch;
pub use formats::*;
pub use hash::{hex, sha256};
pub use ips::{build_ips, encode_arm_branch, CheckedEdit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}
fn bytes(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error("offset overflow".into()))?;
    data.get(offset..end).ok_or_else(|| {
        Error(format!(
            "truncated data: need {offset:#x}..{end:#x}, have {:#x} bytes",
            data.len()
        ))
    })
}
fn le16(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn le32(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
fn le64(data: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(bytes(data, at, 8)?.try_into().unwrap()))
}
fn be16(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(bytes(data, at, 2)?.try_into().unwrap()))
}
fn be32(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(data, at, 4)?.try_into().unwrap()))
}
fn be64(data: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_be_bytes(bytes(data, at, 8)?.try_into().unwrap()))
}
fn size(value: u64) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error("size exceeds host address space".into()))
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b)
        .ok_or_else(|| Error("offset overflow".into()))
}
fn align64(value: usize) -> Result<usize> {
    Ok(add(value, 63)? & !63)
}
fn ascii(data: &[u8]) -> String {
    data.iter()
        .take_while(|&&b| b != 0)
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}
