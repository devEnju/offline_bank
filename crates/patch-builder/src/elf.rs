//! Strict ELF32 reader for the fixed Bank linker contract.
//! Accepts fully linked ARM EABI5 images only; does not install or execute them.

use crate::{
    add, bank15, bytes, fail, le16, le32, placement::LinkedPayload, sha256, CheckedEdit, Error,
    Result,
};

const BOOTSTRAP: u32 = 0x00313910;
const BOOTSTRAP_END: u32 = 0x00314000;
const MAX_MEMORY: u32 = 0x00d05000; // keeps the eventual code image within IPS 16 MiB

// Internal AAPCS boundaries, not additional runtime hook exports. These exact
// bodies prevent the raw-inline-SVC regression that corrupted a Rust Result
// destination held in r12 during thread-capacity queries.
const KERNEL_WRAPPERS: [(&str, &[u32]); 5] = [
    (
        "bank_svc_close_handle",
        &[0xe92d4010, 0xef000023, 0xe8bd8010],
    ),
    (
        "bank_svc_wait_thread",
        &[0xe92d4010, 0xef000024, 0xe8bd8010],
    ),
    (
        "bank_svc_get_resource_limit",
        &[
            0xe92d4001, 0xef000038, 0xe59d2000, 0xe5821000, 0xe28dd004, 0xe49df004,
        ],
    ),
    (
        "bank_svc_get_resource_limit_values",
        &[0xe92d4010, 0xef000039, 0xe8bd8010],
    ),
    (
        "bank_svc_get_resource_current_values",
        &[0xe92d4010, 0xef00003a, 0xe8bd8010],
    ),
];

#[derive(Debug)]
pub struct PayloadElf {
    pub elf_sha256: [u8; 32],
    pub entry: u32,
    /// The profile's entry functions with their addresses, in its order.
    pub entries: Vec<(String, u32)>,
    pub bootstrap_entry: u32,
    pub executable_size: u32,
    pub memory_size: u32,
    bootstrap: Vec<u8>,
    image: Vec<u8>,
}
impl PayloadElf {
    /// The address of one of the profile's entry functions.
    pub fn export(&self, name: &str) -> Result<u32> {
        self.entries
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, address)| *address)
            .ok_or_else(|| Error(format!("missing ELF export {name}")))
    }
    pub fn linked_payload(&self) -> LinkedPayload<'_> {
        LinkedPayload {
            image: &self.image,
            linked_address: bank15::PAYLOAD_ADDRESS,
            entry_offset: self.entry - bank15::PAYLOAD_ADDRESS,
            executable_size: self.executable_size,
            memory_size: self.memory_size,
        }
    }
    /// Requires complete native-image hash validation when used by placement.
    pub fn bootstrap_edit(&self) -> CheckedEdit {
        CheckedEdit {
            offset: (BOOTSTRAP - bank15::CODE_BASE) as usize,
            expected: vec![0; self.bootstrap.len()],
            replacement: self.bootstrap.clone(),
        }
    }
    pub fn bootstrap_bytes(&self) -> &[u8] {
        &self.bootstrap
    }
    pub fn image_bytes(&self) -> &[u8] {
        &self.image
    }
}
#[derive(Clone, Copy)]
struct Segment {
    offset: u32,
    address: u32,
    file_size: u32,
    memory_size: u32,
    flags: u32,
}
impl Segment {
    fn end(self) -> Result<u32> {
        end(self.address, self.memory_size)
    }
    fn contains(self, address: u32, length: u32) -> bool {
        address >= self.address
            && u64::from(address) + u64::from(length)
                <= u64::from(self.address) + u64::from(self.memory_size)
    }
}
#[derive(Clone, Copy)]
struct Section {
    name: u32,
    kind: u32,
    flags: u32,
    address: u32,
    offset: u32,
    size: u32,
    link: u32,
    entry_size: u32,
}
fn end(address: u32, length: u32) -> Result<u32> {
    address
        .checked_add(length)
        .ok_or_else(|| Error("ELF address overflow".into()))
}
fn string(table: &[u8], offset: u32) -> Result<&str> {
    let tail = table
        .get(offset as usize..)
        .ok_or_else(|| Error("ELF string offset out of bounds".into()))?;
    let length = tail
        .iter()
        .position(|&v| v == 0)
        .ok_or_else(|| Error("unterminated ELF string".into()))?;
    core::str::from_utf8(&tail[..length]).map_err(|_| Error("ELF string is not UTF-8".into()))
}

/// Verifies geometry, symbol definitions, section/segment agreement, target ABI,
/// and absence of runtime relocations/dynamic linking/TLS/initializers.
/// This is not instruction-level proof of the runtime hooks.
/// `entries` are the entry functions of the patch's profile; the first must
/// be the entry of the ELF.
pub fn inspect_payload_elf(elf: &[u8], entries: &[&str]) -> Result<PayloadElf> {
    if entries.is_empty() {
        return fail("a profile names at least one entry function");
    }
    bytes(elf, 0, 52)?;
    if bytes(elf, 0, 9)? != b"\x7fELF\x01\x01\x01\0\0"
        || le16(elf, 16)? != 2
        || le16(elf, 18)? != 40
        || le32(elf, 20)? != 1
        || ![0x05000000, 0x05000200].contains(&le32(elf, 36)?)
        || le16(elf, 40)? != 52
        || le16(elf, 42)? != 32
        || le16(elf, 46)? != 40
    {
        return fail(
            "expected ELF32 little-endian ARM EABI5 static executable with base float ABI",
        );
    }
    let entry = le32(elf, 24)?;
    let phoff = le32(elf, 28)? as usize;
    let shoff = le32(elf, 32)? as usize;
    let phnum = usize::from(le16(elf, 44)?);
    let shnum = usize::from(le16(elf, 48)?);
    let names_index = usize::from(le16(elf, 50)?);
    if !(2..=8).contains(&phnum)
        || !(2..=4096).contains(&shnum)
        || names_index == 0
        || names_index >= shnum
    {
        return fail("unsupported ELF table counts or extended indexes");
    }
    bytes(elf, phoff, phnum * 32)?;
    bytes(elf, shoff, shnum * 40)?;
    let mut segments = Vec::new();
    let mut exidx_segments = Vec::new();
    for at in (phoff..phoff + phnum * 32).step_by(32) {
        let kind = le32(elf, at)?;
        let segment = Segment {
            offset: le32(elf, at + 4)?,
            address: le32(elf, at + 8)?,
            file_size: le32(elf, at + 16)?,
            memory_size: le32(elf, at + 20)?,
            flags: le32(elf, at + 24)?,
        };
        let align = le32(elf, at + 28)?;
        bytes(elf, segment.offset as usize, segment.file_size as usize)?;
        segment.end()?;
        if segment.address != le32(elf, at + 12)? || segment.file_size > segment.memory_size {
            return fail("ELF load/file address mismatch or file size exceeds memory");
        }
        match kind {
            1 => {
                if segment.memory_size == 0
                    || align != 0x1000
                    || segment.offset % align != segment.address % align
                    || ![5, 6].contains(&segment.flags)
                {
                    return fail("ELF LOAD must have a nonempty congruent page mapping with RX or RW permissions");
                }
                segments.push(segment);
            }
            0x70000001 => exidx_segments.push(segment),
            0x6474e551
                if segment.file_size == 0 && segment.memory_size == 0 && segment.flags == 6 => {}
            _ => return fail("unsupported ELF program header, dynamic loader, or TLS segment"),
        }
    }
    segments.sort_unstable_by_key(|s| s.address);
    if !(2..=3).contains(&segments.len()) {
        return fail("expected bootstrap, payload RX, and optional payload RW LOADs");
    }
    for pair in segments.windows(2) {
        if pair[0].end()? > pair[1].address {
            return fail("ELF LOAD mappings overlap");
        }
        let a = pair[0];
        let b = pair[1];
        if u64::from(a.offset) < u64::from(b.offset) + u64::from(b.file_size)
            && u64::from(b.offset) < u64::from(a.offset) + u64::from(a.file_size)
        {
            return fail("ELF LOAD file ranges overlap");
        }
    }
    let boot = segments[0];
    let rx = segments[1];
    if boot.address != BOOTSTRAP
        || boot.end()? > BOOTSTRAP_END
        || boot.flags != 5
        || boot.file_size != boot.memory_size
        || rx.address != bank15::PAYLOAD_ADDRESS
        || rx.flags != 5
        || rx.file_size != rx.memory_size
        || rx.memory_size % 0x1000 != 0
    {
        return fail("ELF does not match the reviewed bootstrap/payload RX layout");
    }
    let memory_end = if let Some(rw) = segments.get(2) {
        if rw.address != rx.end()? || rw.flags != 6 || rw.memory_size % 0x1000 != 0 {
            return fail("payload RW pages must immediately follow RX");
        }
        rw.end()?
    } else {
        rx.end()?
    };
    let memory_size = memory_end - rx.address;
    if memory_size > MAX_MEMORY {
        return fail("ELF payload exceeds IPS image capacity");
    }
    for segment in exidx_segments {
        if segment.flags != 4
            || segment.file_size != segment.memory_size
            || !rx.contains(segment.address, segment.memory_size)
            || segment.offset.checked_sub(rx.offset) != segment.address.checked_sub(rx.address)
        {
            return fail("ARM exception index segment is outside initialized RX");
        }
    }

    let mut sections = Vec::with_capacity(shnum);
    for at in (shoff..shoff + shnum * 40).step_by(40) {
        let section = Section {
            name: le32(elf, at)?,
            kind: le32(elf, at + 4)?,
            flags: le32(elf, at + 8)?,
            address: le32(elf, at + 12)?,
            offset: le32(elf, at + 16)?,
            size: le32(elf, at + 20)?,
            link: le32(elf, at + 24)?,
            entry_size: le32(elf, at + 36)?,
        };
        let alignment = le32(elf, at + 32)?;
        if alignment > 1
            && (!alignment.is_power_of_two()
                || section.flags & 2 != 0 && section.address % alignment != 0
                || section.kind != 8 && section.offset % alignment != 0)
        {
            return fail("invalid ELF section alignment");
        }
        if section.kind != 8 {
            bytes(elf, section.offset as usize, section.size as usize)?;
        }
        if section.flags & (0x400 | 0x800) != 0
            || [4, 6, 9, 11, 14, 15, 16, 17, 18].contains(&section.kind)
        {
            return fail("ELF contains relocations, dynamic data, TLS, constructors, or unsupported compression/groups");
        }
        if section.flags & 2 != 0 && section.size != 0 {
            let owner = segments
                .iter()
                .find(|s| s.contains(section.address, section.size))
                .ok_or_else(|| Error("allocated ELF section lies outside LOAD segments".into()))?;
            if section.flags & 1 != 0 && owner.flags != 6
                || section.flags & 4 != 0 && owner.flags != 5
                || section.flags & 5 == 5
                || section.kind == 8 && owner.flags != 6
            {
                return fail(
                    "ELF section permissions disagree with LOAD or executable BSS requested",
                );
            }
            if section.kind != 8
                && (section.offset.checked_sub(owner.offset)
                    != section.address.checked_sub(owner.address)
                    || u64::from(section.offset) + u64::from(section.size)
                        > u64::from(owner.offset) + u64::from(owner.file_size))
            {
                return fail("ELF section file mapping disagrees with LOAD");
            }
        }
        sections.push(section);
    }
    if bytes(elf, shoff, 40)?.iter().any(|&v| v != 0) || sections[names_index].kind != 3 {
        return fail("ELF null section or section name table is invalid");
    }
    let names = bytes(
        elf,
        sections[names_index].offset as usize,
        sections[names_index].size as usize,
    )?;
    let mut symbols = None;
    let mut attributes = 0;
    let mut bootstrap_sections = 0;
    let mut alloc_ranges = Vec::new();
    for section in &sections {
        let name = string(names, section.name)?;
        if section.kind == 2 && symbols.replace(*section).is_some() {
            return fail("multiple ELF symbol tables");
        }
        if section.kind == 0x70000003 {
            attributes += 1;
            check_attributes(bytes(elf, section.offset as usize, section.size as usize)?)?;
        }
        if section.flags & 2 != 0 && section.size != 0 {
            alloc_ranges.push((section.address, end(section.address, section.size)?));
        }
        if name == ".bank.bootstrap" {
            bootstrap_sections += 1;
        }
        if name == ".bank.bootstrap"
            && (section.address != boot.address
                || section.size != boot.file_size
                || section.flags & 7 != 6)
        {
            return fail("bootstrap section differs from bootstrap LOAD");
        }
    }
    alloc_ranges.sort_unstable();
    if alloc_ranges.windows(2).any(|w| w[0].1 > w[1].0)
        || attributes != 1
        || bootstrap_sections != 1
    {
        return fail("allocated ELF sections overlap or ARM attributes are missing/duplicated");
    }
    let symbols =
        symbols.ok_or_else(|| Error("ELF symbol table required for hook verification".into()))?;
    if symbols.entry_size != 16
        || symbols.size % 16 != 0
        || symbols.link as usize >= sections.len()
        || sections[symbols.link as usize].kind != 3
    {
        return fail("invalid ELF symbol table layout or linked strings");
    }
    let strings = sections[symbols.link as usize];
    let strings = bytes(elf, strings.offset as usize, strings.size as usize)?;
    let mut exports = std::collections::BTreeMap::new();
    let mut bounds = std::collections::BTreeMap::new();
    for symbol in bytes(elf, symbols.offset as usize, symbols.size as usize)?.chunks_exact(16) {
        let name = string(strings, le32(symbol, 0)?)?;
        let value = le32(symbol, 4)?;
        let size = le32(symbol, 8)?;
        let index = le16(symbol, 14)?;
        if index == 0 && (!name.is_empty() || value != 0 || size != 0) {
            return fail(format!("unresolved ELF symbol: {name}"));
        }
        if index != 0 && index != 0xfff1 && usize::from(index) >= sections.len() {
            return fail("unsupported ELF symbol section index");
        }
        if entries.contains(&name)
            || ["bank_bootstrap_startup", "bank_bootstrap_enable_rx"].contains(&name)
            || KERNEL_WRAPPERS
                .iter()
                .any(|(required, _)| *required == name)
        {
            let segment = if name.starts_with("bank_bootstrap") {
                boot
            } else {
                rx
            };
            if symbol[12] != 0x12
                || value % 4 != 0
                || size < 4
                || !segment.contains(value, size)
                || index == 0
                || index == 0xfff1
                || sections[usize::from(index)].flags & 6 != 6
                || value < sections[usize::from(index)].address
                || u64::from(value) + u64::from(size)
                    > u64::from(sections[usize::from(index)].address)
                        + u64::from(sections[usize::from(index)].size)
                || exports.insert(name.to_owned(), value).is_some()
            {
                return fail(
                    "required hook must be one defined global ARM function in initialized RX",
                );
            }
            if let Some((_, words)) = KERNEL_WRAPPERS
                .iter()
                .find(|(required, _)| *required == name)
            {
                if size as usize != words.len() * 4 {
                    return fail(format!("ELF kernel wrapper {name} has unexpected size"));
                }
                let offset = add(segment.offset as usize, (value - segment.address) as usize)?;
                for (index, expected) in words.iter().enumerate() {
                    let actual = le32(elf, add(offset, index * 4)?)?;
                    // Both canonical ARM encodings restore the sole saved PC
                    // and advance SP by four. No other instruction varies.
                    let equivalent_pop = name == "bank_svc_get_resource_limit"
                        && index == words.len() - 1
                        && actual == 0xe8bd8000;
                    if actual != *expected && !equivalent_pop {
                        return fail(format!(
                            "ELF kernel wrapper {name} has unaudited instructions"
                        ));
                    }
                }
            }
        }
        if [
            "__bank_payload_start",
            "__bank_payload_rx_end",
            "__bank_payload_rx_size",
            "__bank_payload_end",
            "__bank_original_app_init",
        ]
        .contains(&name)
            && bounds.insert(name.to_owned(), value).is_some()
        {
            return fail("duplicate ELF layout symbol");
        }
    }
    let get_export = |name: &str| {
        exports
            .get(name)
            .copied()
            .ok_or_else(|| Error(format!("missing ELF export {name}")))
    };
    for (name, _) in KERNEL_WRAPPERS {
        get_export(name)?;
    }
    let entries = entries
        .iter()
        .map(|name| Ok((name.to_string(), get_export(name)?)))
        .collect::<Result<Vec<_>>>()?;
    let bootstrap_entry = get_export("bank_bootstrap_startup")?;
    get_export("bank_bootstrap_enable_rx")?;
    if entry != entries[0].1 || bootstrap_entry != BOOTSTRAP {
        return fail("ELF entry/startup symbol differs from the reviewed entry contract");
    }
    for (name, expected) in [
        ("__bank_payload_start", rx.address),
        ("__bank_payload_rx_end", rx.end()?),
        ("__bank_payload_rx_size", rx.memory_size),
        ("__bank_payload_end", memory_end),
        ("__bank_original_app_init", 0x0010494c),
    ] {
        if bounds.get(name) != Some(&expected) {
            return fail(format!("ELF layout symbol mismatch: {name}"));
        }
    }
    let image_end = segments.last().unwrap();
    let image_length = end(image_end.address, image_end.file_size)? - rx.address;
    let mut image = vec![0; image_length as usize];
    for segment in &segments[1..] {
        let at = (segment.address - rx.address) as usize;
        image[at..at + segment.file_size as usize].copy_from_slice(bytes(
            elf,
            segment.offset as usize,
            segment.file_size as usize,
        )?);
    }
    Ok(PayloadElf {
        elf_sha256: sha256(elf),
        entry,
        entries,
        bootstrap_entry,
        executable_size: rx.memory_size,
        memory_size,
        image,
        bootstrap: bytes(elf, boot.offset as usize, boot.file_size as usize)?.to_vec(),
    })
}

fn uleb(data: &[u8], at: &mut usize) -> Result<u32> {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let byte = *data
            .get(*at)
            .ok_or_else(|| Error("truncated ARM attribute integer".into()))?;
        *at += 1;
        if shift == 28 && byte & 0xf0 != 0 {
            return fail("ARM attribute integer overflow");
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    fail("unterminated ARM attribute integer")
}
fn skip_string(data: &[u8], at: &mut usize) -> Result<()> {
    let text = string(data, *at as u32)?;
    *at = add(*at, text.len() + 1)?;
    Ok(())
}
fn check_attributes(data: &[u8]) -> Result<()> {
    if data.first() != Some(&b'A') {
        return fail("unsupported ARM attributes encoding");
    }
    let mut vendor_at = 1;
    let mut cpu_seen = false;
    let mut arm_seen = false;
    while vendor_at < data.len() {
        let length = le32(data, vendor_at)? as usize;
        if length < 11 {
            return fail("truncated ARM vendor attributes");
        }
        let vendor_end = add(vendor_at, length)?;
        let vendor = bytes(data, vendor_at, length)?;
        if string(vendor, 4)? != "aeabi" {
            return fail("unsupported ARM attribute vendor");
        }
        let mut sub_at = 10;
        while sub_at < vendor.len() {
            let start = sub_at;
            if uleb(vendor, &mut sub_at)? != 1 {
                return fail("only ARM file attributes are supported");
            }
            let sub_length = le32(vendor, sub_at)? as usize;
            sub_at = add(sub_at, 4)?;
            let sub_end = add(start, sub_length)?;
            if sub_end < sub_at || sub_end > vendor.len() {
                return fail("invalid ARM attribute subsection extent");
            }
            let attrs = &vendor[..sub_end];
            while sub_at < sub_end {
                let tag = uleb(attrs, &mut sub_at)?;
                if tag == 4 || tag == 5 || tag >= 32 && tag & 1 != 0 {
                    skip_string(attrs, &mut sub_at)?;
                    continue;
                }
                if tag == 32 {
                    return fail("unsupported ARM compatibility attribute");
                }
                let value = uleb(attrs, &mut sub_at)?;
                match tag {
                    6 => {
                        if ![1, 2, 3, 4, 5, 6, 7, 9].contains(&value) {
                            return fail("ELF requires unsupported ARM architecture");
                        }
                        cpu_seen = true;
                    }
                    8 => {
                        if value != 1 {
                            return fail("ELF must support ARM instructions");
                        }
                        arm_seen = true;
                    }
                    9 if value > 1 => return fail("Thumb-2 is unsupported by this payload target"),
                    10 if value > 2 => {
                        return fail("ELF requires unsupported floating-point hardware")
                    }
                    12 if value != 0 => return fail("NEON is unsupported by this payload target"),
                    28 if value != 0 => return fail("ELF requires VFP argument passing"),
                    34 if value != 0 => {
                        return fail("ELF permits unaligned accesses outside the reviewed target")
                    }
                    _ => {}
                }
            }
        }
        vendor_at = vendor_end;
    }
    if !cpu_seen || !arm_seen {
        return fail("ARM CPU/ISA attributes required");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checks with the entry functions of the offline patch.
    fn inspect(elf: &[u8]) -> Result<PayloadElf> {
        inspect_payload_elf(elf, bank15::offline::PROFILE.entries)
    }
    const SHOFF: usize = 0x2d00;
    const SYMOFF: usize = 0x2200;
    fn u16_at(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn u32_at(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn add_name(table: &mut Vec<u8>, name: &str) -> u32 {
        let at = table.len() as u32;
        table.extend_from_slice(name.as_bytes());
        table.push(0);
        at
    }
    fn section(data: &mut [u8], index: usize, fields: [u32; 10]) {
        for (word, value) in fields.into_iter().enumerate() {
            u32_at(data, SHOFF + index * 40 + word * 4, value);
        }
    }
    fn fixture() -> Vec<u8> {
        // Synthetic ELF layout only. No proprietary Bank bytes or executable
        // artifact is written by these tests.
        let mut elf = vec![0; 0x3000];
        elf[..9].copy_from_slice(b"\x7fELF\x01\x01\x01\0\0");
        u16_at(&mut elf, 16, 2);
        u16_at(&mut elf, 18, 40);
        u32_at(&mut elf, 20, 1);
        u32_at(&mut elf, 24, 0x3fb000);
        u32_at(&mut elf, 28, 52);
        u32_at(&mut elf, 32, SHOFF as u32);
        u32_at(&mut elf, 36, 0x05000200);
        for (at, value) in [(40, 52), (42, 32), (44, 3), (46, 40), (48, 9), (50, 7)] {
            u16_at(&mut elf, at, value);
        }
        for (index, fields) in [
            [1, 0x910, BOOTSTRAP, BOOTSTRAP, 8, 8, 5, 0x1000],
            [1, 0x1000, 0x3fb000, 0x3fb000, 0x1000, 0x1000, 5, 0x1000],
            [1, 0x2000, 0x3fc000, 0x3fc000, 4, 0x1000, 6, 0x1000],
        ]
        .into_iter()
        .enumerate()
        {
            for (word, value) in fields.into_iter().enumerate() {
                u32_at(&mut elf, 52 + index * 32 + word * 4, value);
            }
        }
        for at in [
            0x910, 0x914, 0x1000, 0x1004, 0x1008, 0x100c, 0x1010, 0x1014, 0x1018, 0x101c, 0x1020,
            0x1024, 0x1028, 0x102c, 0x1030,
        ] {
            u32_at(&mut elf, at, 0xe12fff1e); // synthetic ARM BX LR
        }
        for (index, (_, words)) in KERNEL_WRAPPERS.iter().enumerate() {
            for (word, value) in words.iter().enumerate() {
                u32_at(&mut elf, 0x1100 + index * 0x20 + word * 4, *value);
            }
        }
        elf[0x2000..0x2004].copy_from_slice(&[1, 2, 3, 4]);
        let mut names = vec![0];
        for (index, name, kind, flags, address, offset, size, link, align, entry_size) in [
            (1, ".bank.bootstrap", 1, 6, BOOTSTRAP, 0x910, 8, 0, 4, 0),
            (2, ".text", 1, 6, 0x3fb000, 0x1000, 0x1000, 0, 4, 0),
            (3, ".data", 1, 3, 0x3fc000, 0x2000, 4, 0, 4, 0),
            (4, ".bss", 8, 3, 0x3fc004, 0x2004, 0xffc, 0, 4, 0),
            (5, ".ARM.attributes", 0x70000003, 0, 0, 0x2100, 24, 0, 1, 0),
            (6, ".symtab", 2, 0, 0, SYMOFF as u32, 336, 8, 4, 16),
            (7, ".shstrtab", 3, 0, 0, 0x2400, 0, 0, 1, 0),
            (8, ".strtab", 3, 0, 0, 0x2500, 0, 0, 1, 0),
        ] {
            let name = add_name(&mut names, name);
            section(
                &mut elf,
                index,
                [
                    name, kind, flags, address, offset, size, link, 0, align, entry_size,
                ],
            );
        }
        u32_at(&mut elf, SHOFF + 7 * 40 + 20, names.len() as u32);
        elf[0x2400..0x2400 + names.len()].copy_from_slice(&names);
        let attributes = [
            b'A', 23, 0, 0, 0, b'a', b'e', b'a', b'b', b'i', 0, 1, 13, 0, 0, 0, 6, 6, 8, 1, 9, 1,
            28, 0,
        ];
        elf[0x2100..0x2118].copy_from_slice(&attributes);
        let mut strings = vec![0];
        for (index, (name, value, size, info, section_index)) in [
            ("bank_offline_next", 0x3fb000, 4, 0x12, 2),
            ("bank_offline_load", 0x3fb004, 4, 0x12, 2),
            ("bank_offline_save", 0x3fb008, 4, 0x12, 2),
            ("bank_offline_rewards", 0x3fb014, 4, 0x12, 2),
            ("bank_offline_timestamp", 0x3fb018, 4, 0x12, 2),
            ("bank_offline_dex_save_request", 0x3fb028, 4, 0x12, 2),
            ("bank_offline_dex_records_update", 0x3fb02c, 4, 0x12, 2),
            ("bank_offline_dex_records_finish", 0x3fb030, 4, 0x12, 2),
            ("bank_bootstrap_startup", BOOTSTRAP, 4, 0x12, 1),
            ("bank_bootstrap_enable_rx", BOOTSTRAP + 4, 4, 0x12, 1),
            ("__bank_payload_start", 0x3fb000, 0, 0x10, 0xfff1),
            ("__bank_payload_rx_end", 0x3fc000, 0, 0x10, 0xfff1),
            ("__bank_payload_rx_size", 0x1000, 0, 0x10, 0xfff1),
            ("__bank_payload_end", 0x3fd000, 0, 0x10, 0xfff1),
            ("__bank_original_app_init", 0x10494c, 0, 0x10, 0xfff1),
            ("bank_svc_close_handle", 0x3fb100, 12, 0x12, 2),
            ("bank_svc_wait_thread", 0x3fb120, 12, 0x12, 2),
            ("bank_svc_get_resource_limit", 0x3fb140, 24, 0x12, 2),
            ("bank_svc_get_resource_limit_values", 0x3fb160, 12, 0x12, 2),
            (
                "bank_svc_get_resource_current_values",
                0x3fb180,
                12,
                0x12,
                2,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let at = SYMOFF + (index + 1) * 16;
            u32_at(&mut elf, at, add_name(&mut strings, name));
            u32_at(&mut elf, at + 4, value);
            u32_at(&mut elf, at + 8, size);
            elf[at + 12] = info;
            u16_at(&mut elf, at + 14, section_index);
        }
        u32_at(&mut elf, SHOFF + 8 * 40 + 20, strings.len() as u32);
        elf[0x2500..0x2500 + strings.len()].copy_from_slice(&strings);
        elf
    }
    #[test]
    fn extracts_only_initialized_payload_and_checked_bootstrap_edit() {
        let elf = fixture();
        let parsed = inspect(&elf).unwrap();
        assert_eq!(parsed.entry, 0x3fb000);
        assert_eq!(parsed.export("bank_offline_timestamp"), Ok(0x3fb018));
        assert_eq!(parsed.entries.len(), 8);
        assert!(parsed.export("bank_offline_menu").is_err());
        assert_eq!(parsed.executable_size, 0x1000);
        assert_eq!(parsed.memory_size, 0x2000);
        assert_eq!(parsed.image_bytes().len(), 0x1004);
        assert_eq!(&parsed.image_bytes()[0x1000..], &[1, 2, 3, 4]);
        let edit = parsed.bootstrap_edit();
        assert_eq!(edit.offset, 0x213910);
        assert_eq!(edit.expected, vec![0; 8]);
        assert_eq!(edit.replacement, elf[0x910..0x918]);
        assert_eq!(parsed.linked_payload().memory_size, 0x2000);
    }
    #[test]
    fn every_kernel_wrapper_is_required_as_a_global_arm_rx_function() {
        for (index, (name, _)) in KERNEL_WRAPPERS.iter().enumerate() {
            let symbol = SYMOFF + (16 + index) * 16;
            let mut missing = fixture();
            u32_at(&mut missing, symbol, 0);
            assert!(inspect(&missing)
                .unwrap_err()
                .to_string()
                .contains(&format!("missing ELF export {name}")));
            for (value, info, section_index) in [
                (0x3fb100 + index as u32 * 0x20 + 1, 0x12, 2),
                (0x3fc000, 0x12, 3),
                (0x3fb100 + index as u32 * 0x20, 0x02, 2),
                (0x3fb100 + index as u32 * 0x20, 0x11, 2),
                (0x3fb100 + index as u32 * 0x20, 0x12, 1),
            ] {
                let mut invalid = fixture();
                u32_at(&mut invalid, symbol + 4, value);
                invalid[symbol + 12] = info;
                u16_at(&mut invalid, symbol + 14, section_index);
                assert!(inspect(&invalid).is_err(), "accepted invalid {name}");
            }
        }
    }

    #[test]
    fn kernel_wrappers_reject_missing_stack_preservation_and_wrong_svc() {
        for (index, (name, words)) in KERNEL_WRAPPERS.iter().enumerate() {
            let body = 0x1100 + index * 0x20;
            for (instruction, replacement) in [
                (0, 0xe1a00000),               // replacing the preserved frame with NOP
                (1, 0xef000032),               // an entirely different service
                (words.len() - 1, 0xe12fff1e), // BX LR leaks the saved stack
            ] {
                let mut invalid = fixture();
                u32_at(&mut invalid, body + instruction * 4, replacement);
                assert!(
                    inspect(&invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("unaudited instructions"),
                    "accepted corrupt {name}"
                );
            }
        }
        // Resource38 must restore the caller's output pointer from the stack,
        // not from a scratch register surviving the SVC.
        let mut invalid = fixture();
        u32_at(&mut invalid, 0x1140 + 2 * 4, 0xe1a0200c); // MOV r2,r12
        assert!(inspect(&invalid).is_err());
    }

    #[test]
    fn kernel_wrapper_extents_must_cover_exact_audited_bodies() {
        for (index, (_, words)) in KERNEL_WRAPPERS.iter().enumerate() {
            for size in [4, words.len() as u32 * 4 - 4, words.len() as u32 * 4 + 4] {
                let mut invalid = fixture();
                u32_at(&mut invalid, SYMOFF + (16 + index) * 16 + 8, size);
                assert!(inspect(&invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("unexpected size"));
            }
        }
        let mut equivalent = fixture();
        u32_at(&mut equivalent, 0x1140 + 5 * 4, 0xe8bd8000);
        assert!(inspect(&equivalent).is_ok());
    }

    #[test]
    fn timestamp_export_must_be_present_and_arm_aligned() {
        let mut missing = fixture();
        u32_at(&mut missing, SYMOFF + 5 * 16, 0); // remove timestamp's name
        assert!(inspect(&missing)
            .unwrap_err()
            .to_string()
            .contains("missing ELF export bank_offline_timestamp"));
        let mut thumb = fixture();
        u32_at(&mut thumb, SYMOFF + 5 * 16 + 4, 0x3fb019);
        assert!(inspect(&thumb).is_err());
    }
    #[test]
    fn dex_hooks_require_independent_arm_exports() {
        for symbol_index in 6..=8 {
            let mut missing = fixture();
            u32_at(&mut missing, SYMOFF + symbol_index * 16, 0);
            assert!(inspect(&missing)
                .unwrap_err()
                .to_string()
                .contains("missing ELF export bank_offline_"));
            let mut writable = fixture();
            u32_at(&mut writable, SYMOFF + symbol_index * 16 + 4, 0x3fc000);
            assert!(inspect(&writable).is_err());
        }
    }
    #[test]
    fn rejects_wrong_abi_relocations_tls_dynamic_and_bad_entries() {
        let elf = fixture();
        for (at, value) in [
            (36, 0x05000400),
            (24, 0x3fb001),
            (52, 2),
            (52 + 32 + 24, 7),
            (SHOFF + 6 * 40 + 4, 9),
            (SHOFF + 3 * 40 + 8, 0x403),
            (SHOFF + 3 * 40 + 4, 14),
            (SYMOFF + 16 + 4, 0x3fb001),
            (SYMOFF + 16 + 8, 0),
            (SYMOFF + 5 * 16 + 4, 0x3fc004),
        ] {
            let mut wrong = elf.clone();
            u32_at(&mut wrong, at, value);
            assert!(inspect(&wrong).is_err(), "unexpected acceptance at {at:x}");
        }
        let mut wrong = elf.clone();
        u16_at(&mut wrong, SYMOFF + 16 + 14, 0);
        assert!(inspect(&wrong).is_err());
        wrong = elf;
        wrong[0x2111] = 10; // CPU v7
        assert!(inspect(&wrong).is_err());
    }
    #[test]
    fn rejects_segment_section_overlaps_and_truncated_file_ranges() {
        let elf = fixture();
        for (at, value) in [
            (52 + 32 + 8, BOOTSTRAP),
            (52 + 64 + 8, 0x3fc004),
            (52 + 64 + 16, 0x1001),
            (SHOFF + 3 * 40 + 16, 0x2001),
            (SHOFF + 4 * 40 + 12, 0x3fc000),
            (SHOFF + 3 * 40 + 20, u32::MAX),
            (SHOFF + 6 * 40 + 24, 9999),
            (28, u32::MAX),
            (32, u32::MAX),
        ] {
            let mut wrong = elf.clone();
            u32_at(&mut wrong, at, value);
            assert!(inspect(&wrong).is_err(), "unexpected acceptance at {at:x}");
        }
    }
    #[test]
    fn arbitrary_truncation_and_attribute_corruption_never_panic() {
        let elf = fixture();
        for length in 0..elf.len() {
            // A prefix may contain all referenced bytes before trailing padding.
            let _ = inspect(&elf[..length]);
        }
        for length in [0, 1, 5, 10, 11, 12, 15, 17, 23] {
            assert!(check_attributes(&elf[0x2100..0x2100 + length]).is_err());
        }
        let mut attrs = elf[0x2100..0x2118].to_vec();
        attrs[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(check_attributes(&attrs).is_err());
        assert!(uleb(&[0xff; 5], &mut 0).is_err());
    }
    #[test]
    fn invalid_exception_segment_offset_is_an_error_not_underflow() {
        let mut elf = fixture();
        u16_at(&mut elf, 44, 4);
        for (word, value) in [0x70000001, 0, 0x3fb000, 0x3fb000, 16, 16, 4, 4]
            .into_iter()
            .enumerate()
        {
            u32_at(&mut elf, 52 + 3 * 32 + word * 4, value);
        }
        assert!(inspect(&elf).is_err());
        let mut elf = fixture();
        u16_at(&mut elf, SYMOFF + 16 + 14, 1); // payload function falsely assigned to bootstrap section
        assert!(inspect(&elf).is_err());
    }
}
