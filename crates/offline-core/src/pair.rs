//! A file that exists in two copies and holds nothing but its data.
//!
//! The main file and the transport box are kept this way, as the Bank file
//! is inside its own container (`store`). A copy has no label of its own:
//! the journal says which copy belongs to the current save and what its
//! checksum is, and a copy counts only if its bytes have that checksum. New
//! content is written into the copy that is not in use, made durable and
//! read back *before* the journal is changed to name it, so an interrupted
//! write leaves a copy nobody refers to.

use crate::{checksum::Crc32, Part, Slot, Storage};

#[derive(Debug, PartialEq, Eq)]
pub enum PairError<E> {
    Io(E),
    /// The parts given are not one whole copy.
    WrongLength,
    /// The copy does not have the checksum it is expected to have.
    Checksum,
    /// A copy just written does not read back as what was written.
    Readback,
}

pub struct Pair<S> {
    storage: S,
    len: u32,
}

impl<S: Storage> Pair<S> {
    /// `len` is the exact length of one copy; the two lie one after the
    /// other in `storage`, and on the console each is a file of its own.
    pub const fn new(storage: S, len: u32) -> Self {
        Self { storage, len }
    }
    pub fn into_inner(self) -> S {
        self.storage
    }
    /// Length of the two copies together.
    pub const fn file_len(len: u32) -> u64 {
        len as u64 * 2
    }
    fn offset(&self, slot: Slot) -> u64 {
        slot.index() * u64::from(self.len)
    }
    fn whole(&self, total: usize) -> Result<(), PairError<S::Error>> {
        if total == self.len as usize {
            Ok(())
        } else {
            Err(PairError::WrongLength)
        }
    }

    /// Reads the copy `part` names into `parts`, which together are one
    /// copy, and checks it on that pass.
    pub fn read(&mut self, part: Part, parts: &mut [&mut [u8]]) -> Result<(), PairError<S::Error>> {
        self.whole(parts.iter().map(|part| part.len()).sum())?;
        let mut at = self.offset(part.slot);
        let mut crc = Crc32::new();
        for bytes in parts.iter_mut() {
            self.storage.read(at, bytes).map_err(PairError::Io)?;
            crc.update(bytes);
            at += bytes.len() as u64;
        }
        if crc.finish() != part.crc {
            return Err(PairError::Checksum);
        }
        Ok(())
    }

    /// The checksum of a copy as it reads now.
    pub fn checksum(&mut self, slot: Slot) -> Result<u32, PairError<S::Error>> {
        let mut crc = Crc32::new();
        let mut buffer = [0; 512];
        let mut remaining = u64::from(self.len);
        let mut at = self.offset(slot);
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            self.storage
                .read(at, &mut buffer[..count])
                .map_err(PairError::Io)?;
            crc.update(&buffer[..count]);
            at += count as u64;
            remaining -= count as u64;
        }
        Ok(crc.finish())
    }

    /// Writes the copy `slot` from `parts`, which together are one copy,
    /// makes it durable and reads it back. Returns its checksum, for the
    /// journal. A copy that an earlier cut left unusable is discarded and
    /// written once more; it is never the copy in use.
    pub fn write(&mut self, slot: Slot, parts: &[&[u8]]) -> Result<u32, PairError<S::Error>> {
        self.whole(parts.iter().map(|part| part.len()).sum())?;
        let mut crc = Crc32::new();
        for bytes in parts {
            crc.update(bytes);
        }
        let crc = crc.finish();
        if self.put(slot, parts, crc).is_err() {
            self.storage
                .recreate(self.offset(slot), u64::from(self.len))
                .map_err(PairError::Io)?;
            self.put(slot, parts, crc)?;
        }
        Ok(crc)
    }

    fn put(&mut self, slot: Slot, parts: &[&[u8]], crc: u32) -> Result<(), PairError<S::Error>> {
        let mut at = self.offset(slot);
        for bytes in parts {
            self.storage.write(at, bytes).map_err(PairError::Io)?;
            at += bytes.len() as u64;
        }
        self.storage.sync().map_err(PairError::Io)?;
        if self.checksum(slot)? != crc {
            return Err(PairError::Readback);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    /// Fails the write that would exceed `budget` bytes, half written.
    struct Memory {
        bytes: Vec<u8>,
        budget: Option<usize>,
        recreated: usize,
    }
    impl Storage for Memory {
        type Error = ();
        fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), ()> {
            out.copy_from_slice(&self.bytes[at as usize..at as usize + out.len()]);
            Ok(())
        }
        fn write(&mut self, at: u64, bytes: &[u8]) -> Result<(), ()> {
            for (index, byte) in bytes.iter().enumerate() {
                if let Some(budget) = &mut self.budget {
                    if *budget == 0 {
                        return Err(());
                    }
                    *budget -= 1;
                }
                self.bytes[at as usize + index] = *byte;
            }
            Ok(())
        }
        fn sync(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn recreate(&mut self, at: u64, length: u64) -> Result<(), ()> {
            self.bytes[at as usize..(at + length) as usize].fill(0);
            self.recreated += 1;
            Ok(())
        }
    }
    const LEN: u32 = 7;
    fn pair(budget: Option<usize>) -> Pair<Memory> {
        Pair::new(
            Memory {
                bytes: vec![0; Pair::<Memory>::file_len(LEN) as usize],
                budget,
                recreated: 0,
            },
            LEN,
        )
    }

    #[test]
    fn a_copy_is_its_data_and_nothing_else() {
        let mut file = pair(None);
        let crc = file.write(Slot::B, &[b"abc", b"defg"]).unwrap();
        assert_eq!(crc, crate::crc32(b"abcdefg"));
        assert_eq!(file.checksum(Slot::B), Ok(crc));
        let (mut first, mut second) = ([0; 3], [0; 4]);
        file.read(Part { slot: Slot::B, crc }, &mut [&mut first, &mut second])
            .unwrap();
        assert_eq!((&first, &second), (b"abc", b"defg"));
        // The file is the two copies, one after the other.
        let bytes = file.into_inner().bytes;
        assert_eq!(bytes, b"\0\0\0\0\0\0\0abcdefg");
    }

    #[test]
    fn a_copy_counts_only_with_the_checksum_it_is_named_with() {
        let mut file = pair(None);
        let crc = file.write(Slot::A, &[b"abcdefg"]).unwrap();
        let mut out = [0; 7];
        for wrong in [
            Part {
                slot: Slot::A,
                crc: crc ^ 1,
            },
            Part { slot: Slot::B, crc },
        ] {
            assert_eq!(file.read(wrong, &mut [&mut out]), Err(PairError::Checksum));
        }
        // Parts that are not one whole copy are refused before any I/O.
        let part = Part { slot: Slot::A, crc };
        assert_eq!(
            file.read(part, &mut [&mut out[..6]]),
            Err(PairError::WrongLength)
        );
        assert_eq!(
            file.write(Slot::B, &[b"abcdefgh"]),
            Err(PairError::WrongLength)
        );
    }

    #[test]
    fn a_cut_at_any_byte_leaves_the_other_copy_as_it_was() {
        for budget in 0..7 {
            let mut file = pair(None);
            let current = file.write(Slot::A, &[b"current"]).unwrap();
            file.storage.budget = Some(budget);
            assert_eq!(file.write(Slot::B, &[b"another"]), Err(PairError::Io(())));
            file.storage.budget = None;
            let mut out = [0; 7];
            let part = Part {
                slot: Slot::A,
                crc: current,
            };
            file.read(part, &mut [&mut out]).unwrap();
            assert_eq!(&out, b"current");
            // The half-written copy has neither checksum, and is written
            // anew by the next try.
            let torn = file.checksum(Slot::B).unwrap();
            assert!(torn != current && torn != crate::crc32(b"another"));
            assert_eq!(
                file.write(Slot::B, &[b"another"]),
                Ok(crate::crc32(b"another"))
            );
        }
    }
}
