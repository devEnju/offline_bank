//! Fingerprints for native game writes that replace selected byte ranges.
//!
//! The original main-file bytes supply all padding and trailing data. The native
//! writer seeks to aligned block offsets and writes each actual block length;
//! it does not initialize the gaps. Callers must use prepared native bytes after
//! checksum/signature generation and hold exclusive access to both sources.

use crate::{Fingerprint, Sha256, Sha256Error, Storage};

#[derive(Clone, Copy, Debug)]
pub struct Overlay<'a> {
    pub offset: u64,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageFingerprints {
    pub before: Fingerprint,
    pub after: Fingerprint,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError<ReadError, BlockError> {
    Read(ReadError),
    Block(BlockError),
    OutOfBounds,
    UnorderedOrOverlapping,
    Hash(Sha256Error),
}

/// Hashes the complete existing image and the image obtained by overlaying
/// ordered, nonoverlapping native blocks. It allocates no image buffer and never
/// writes or syncs storage. `file_len` must be the independently verified exact
/// length; short/missing reads are storage errors, not implicit zero bytes.
///
/// These hashes cover file bytes only. Native secure values and archive identity
/// must be verified separately before a transaction is considered complete.
/// The caller must revalidate unchanged input before starting the actual write.
pub fn fingerprint_images<'a, S, I, E>(
    source: &mut S,
    file_len: u64,
    overlays: I,
) -> Result<ImageFingerprints, ImageError<S::Error, E>>
where
    S: Storage,
    I: IntoIterator<Item = Result<Overlay<'a>, E>>,
{
    let mut before = Sha256::new();
    let mut after = Sha256::new();
    let mut cursor = 0u64;
    let mut buffer = [0u8; 512];
    for overlay in overlays {
        let overlay = overlay.map_err(ImageError::Block)?;
        let end = overlay
            .offset
            .checked_add(overlay.bytes.len() as u64)
            .filter(|end| *end <= file_len)
            .ok_or(ImageError::OutOfBounds)?;
        if overlay.offset < cursor {
            return Err(ImageError::UnorderedOrOverlapping);
        }
        hash_existing(
            source,
            &mut cursor,
            overlay.offset,
            &mut before,
            Some(&mut after),
            &mut buffer,
        )?;
        hash_existing(source, &mut cursor, end, &mut before, None, &mut buffer)?;
        after.update(overlay.bytes).map_err(ImageError::Hash)?;
    }
    hash_existing(
        source,
        &mut cursor,
        file_len,
        &mut before,
        Some(&mut after),
        &mut buffer,
    )?;
    Ok(ImageFingerprints {
        before: before.finalize(),
        after: after.finalize(),
    })
}

fn hash_existing<S: Storage, E>(
    source: &mut S,
    cursor: &mut u64,
    end: u64,
    before: &mut Sha256,
    mut after: Option<&mut Sha256>,
    buffer: &mut [u8; 512],
) -> Result<(), ImageError<S::Error, E>> {
    while *cursor < end {
        let count = (end - *cursor).min(buffer.len() as u64) as usize;
        let bytes = &mut buffer[..count];
        source.read(*cursor, bytes).map_err(ImageError::Read)?;
        before.update(bytes).map_err(ImageError::Hash)?;
        if let Some(hash) = after.as_mut() {
            hash.update(bytes).map_err(ImageError::Hash)?;
        }
        *cursor += count as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::convert::Infallible;
    use std::{vec, vec::Vec};

    struct Source {
        bytes: Vec<u8>,
        fail_at: Option<u64>,
    }
    impl Storage for Source {
        type Error = &'static str;
        fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Self::Error> {
            if self
                .fail_at
                .is_some_and(|at| offset + bytes.len() as u64 > at)
            {
                return Err("read failed");
            }
            let range = self
                .bytes
                .get(offset as usize..offset as usize + bytes.len())
                .ok_or("short read")?;
            bytes.copy_from_slice(range);
            Ok(())
        }
        fn write(&mut self, _: u64, _: &[u8]) -> Result<(), Self::Error> {
            panic!("fingerprinting must not write")
        }
        fn sync(&mut self) -> Result<(), Self::Error> {
            panic!("fingerprinting must not sync")
        }
    }
    fn digest(bytes: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(bytes).unwrap();
        hash.finalize()
    }
    fn source(length: usize) -> Source {
        Source {
            bytes: (0..length)
                .map(|i| (i.wrapping_mul(41).wrapping_add(7)) as u8)
                .collect(),
            fail_at: None,
        }
    }
    #[test]
    fn matches_explicit_image_and_preserves_alignment_gaps_and_tail() {
        let mut input = source(4099);
        let original = input.bytes.clone();
        let first = [0xa3; 37];
        let second = [0xbb; 521];
        let third = [0x17; 9];
        let mut expected = original.clone();
        expected[0..37].copy_from_slice(&first);
        expected[512..1033].copy_from_slice(&second);
        expected[1536..1545].copy_from_slice(&third);
        let overlays: [Result<_, Infallible>; 3] = [
            Ok(Overlay {
                offset: 0,
                bytes: &first,
            }),
            Ok(Overlay {
                offset: 512,
                bytes: &second,
            }),
            Ok(Overlay {
                offset: 1536,
                bytes: &third,
            }),
        ];
        let actual = fingerprint_images(&mut input, 4099, overlays).unwrap();
        assert_eq!(actual.before, digest(&original));
        assert_eq!(actual.after, digest(&expected));
        assert_eq!(input.bytes, original);
    }
    #[test]
    fn empty_overlay_keeps_identical_fingerprints_and_reads_entire_file() {
        let mut input = source(777);
        let actual = fingerprint_images(
            &mut input,
            777,
            core::iter::empty::<Result<Overlay<'_>, Infallible>>(),
        )
        .unwrap();
        assert_eq!(actual.before, digest(&input.bytes));
        assert_eq!(actual.before, actual.after);
    }
    #[test]
    fn rejects_overlap_and_range_overflow() {
        let mut input = source(16);
        let bad: [Result<_, Infallible>; 2] = [
            Ok(Overlay {
                offset: 4,
                bytes: &[1; 4],
            }),
            Ok(Overlay {
                offset: 7,
                bytes: &[2; 1],
            }),
        ];
        assert_eq!(
            fingerprint_images(&mut input, 16, bad),
            Err(ImageError::UnorderedOrOverlapping)
        );
        for offset in [15, u64::MAX] {
            let bad: [Result<_, Infallible>; 1] = [Ok(Overlay {
                offset,
                bytes: &[0; 2],
            })];
            assert_eq!(
                fingerprint_images(&mut input, 16, bad),
                Err(ImageError::OutOfBounds)
            );
        }
    }
    #[test]
    fn does_not_treat_failed_or_short_read_as_zero_padding() {
        let mut input = source(16);
        input.fail_at = Some(12);
        assert_eq!(
            fingerprint_images(
                &mut input,
                16,
                core::iter::empty::<Result<Overlay<'_>, Infallible>>()
            ),
            Err(ImageError::Read("read failed"))
        );
        input.fail_at = None;
        assert_eq!(
            fingerprint_images(
                &mut input,
                17,
                core::iter::empty::<Result<Overlay<'_>, Infallible>>()
            ),
            Err(ImageError::Read("short read"))
        );
    }
    #[test]
    fn propagates_native_block_iterator_error() {
        let mut input = Source {
            bytes: vec![0; 8],
            fail_at: None,
        };
        let bad: [Result<Overlay<'_>, _>; 1] = [Err("invalid native block")];
        assert_eq!(
            fingerprint_images(&mut input, 8, bad),
            Err(ImageError::Block("invalid native block"))
        );
    }
}
