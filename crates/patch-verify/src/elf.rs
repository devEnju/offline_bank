//! The linked payload as the check needs it: its three loaded segments
//! (start-up hook, payload code, payload data) and its symbols.

use crate::{fail, Result};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    pub offset: u32,
    pub address: u32,
    pub file_bytes: u32,
    pub memory_bytes: u32,
    pub flags: u32,
    pub alignment: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub value: u32,
    pub size: u32,
    pub info: u8,
}

pub struct Elf {
    /// Sorted by address: the start-up hook, the payload code, its data.
    pub loads: [Load; 3],
    pub symbols: HashMap<String, Symbol>,
    pub entry: u32,
}

impl Elf {
    pub fn boot(&self) -> &Load {
        &self.loads[0]
    }
    pub fn rx(&self) -> &Load {
        &self.loads[1]
    }
    pub fn rw(&self) -> &Load {
        &self.loads[2]
    }
}

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    match bytes.get(at..at.wrapping_add(2)) {
        Some(b) => Ok(u16::from_le_bytes([b[0], b[1]])),
        None => fail("data ends early"),
    }
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    match bytes.get(at..at.wrapping_add(4)) {
        Some(b) => Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        None => fail("data ends early"),
    }
}

fn c_string(bytes: &[u8], at: usize) -> Result<String> {
    let rest = bytes.get(at..).unwrap_or_default();
    match rest.iter().position(|&b| b == 0) {
        Some(end) => Ok(String::from_utf8_lossy(&rest[..end]).into_owned()),
        None => fail("Unterminated ELF string"),
    }
}

fn check(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        fail(message)
    }
}

pub fn read(elf: &[u8]) -> Result<Elf> {
    check(
        elf.len() >= 52 && elf[..4] == *b"\x7fELF" && elf[4] == 1 && elf[5] == 1,
        "ELF32 little-endian header",
    )?;
    check(
        u16_at(elf, 16)? == 2 && u16_at(elf, 18)? == 40,
        "ARM executable ELF",
    )?;
    let ph_offset = u32_at(elf, 28)? as usize;
    let ph_size = u16_at(elf, 42)? as usize;
    let ph_count = u16_at(elf, 44)? as usize;
    let sh_offset = u32_at(elf, 32)? as usize;
    let sh_size = u16_at(elf, 46)? as usize;
    let sh_count = u16_at(elf, 48)? as usize;
    check(ph_size == 32 && sh_size == 40, "ELF table sizes")?;
    let mut loads = Vec::new();
    for index in 0..ph_count {
        let at = ph_offset + index * ph_size;
        if u32_at(elf, at)? != 1 {
            continue;
        }
        let load = Load {
            offset: u32_at(elf, at + 4)?,
            address: u32_at(elf, at + 8)?,
            file_bytes: u32_at(elf, at + 16)?,
            memory_bytes: u32_at(elf, at + 20)?,
            flags: u32_at(elf, at + 24)?,
            alignment: u32_at(elf, at + 28)?,
        };
        check(
            load.address == u32_at(elf, at + 12)? && load.file_bytes <= load.memory_bytes,
            "ELF LOAD mapping",
        )?;
        check(
            load.offset as u64 + load.file_bytes as u64 <= elf.len() as u64
                && load.alignment == 0x1000,
            "ELF LOAD file range",
        )?;
        check(
            load.address % 0x1000 == load.offset % 0x1000,
            "ELF LOAD congruence",
        )?;
        loads.push(load);
    }
    loads.sort_by_key(|load| load.address);
    let Ok(loads) = <[Load; 3]>::try_from(loads) else {
        return fail("ELF LOADs are the start-up hook, the payload code and its data");
    };
    let mut symbols = HashMap::new();
    let mut tables = 0;
    for index in 0..sh_count {
        let at = sh_offset + index * sh_size;
        if u32_at(elf, at + 4)? != 2 {
            continue;
        }
        tables += 1;
        let first = u32_at(elf, at + 16)? as usize;
        let length = u32_at(elf, at + 20)? as usize;
        let link = u32_at(elf, at + 24)? as usize;
        let strings = u32_at(elf, sh_offset + link * sh_size + 16)? as usize;
        check(
            u32_at(elf, at + 36)? == 16 && length % 16 == 0,
            "ELF symbol entries",
        )?;
        for entry in (first..first + length).step_by(16) {
            let name = c_string(elf, strings + u32_at(elf, entry)? as usize)?;
            if !name.is_empty() {
                let info = *elf.get(entry + 12).ok_or("data ends early")?;
                let symbol = Symbol {
                    value: u32_at(elf, entry + 4)?,
                    size: u32_at(elf, entry + 8)?,
                    info,
                };
                symbols.insert(name, symbol);
            }
        }
    }
    check(tables == 1, "One ELF symbol table")?;
    Ok(Elf {
        loads,
        symbols,
        entry: u32_at(elf, 24)?,
    })
}
