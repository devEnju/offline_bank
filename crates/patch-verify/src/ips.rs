//! Applies code.ips to the original as Luma3DS does, over added memory that
//! is not zero: every record bounded and disjoint, no record into the
//! original outside the allowed bytes, and every added byte written.

use crate::{fail, Result};

/// What fills the added memory before the IPS is applied.
pub const FILL: u8 = 0xa5;

#[derive(Debug)]
pub struct Expanded {
    pub patched: Vec<u8>,
    pub records: usize,
}

fn check(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        fail(message)
    }
}

/// `allowed[i]` says whether the IPS may write byte `i` of the original.
pub fn expand(ips: &[u8], original: &[u8], size: usize, allowed: &[bool]) -> Result<Expanded> {
    check(size >= original.len(), "IPS bounds")?;
    let mut patched = vec![FILL; size];
    patched[..original.len()].copy_from_slice(original);
    let mut covered = vec![false; size];
    check(ips.starts_with(b"PATCH"), "IPS magic")?;
    let byte = |at: usize| ips[at] as usize;
    let mut at = 5;
    let mut records = 0;
    loop {
        check(at + 3 <= ips.len(), "IPS truncated offset")?;
        if &ips[at..at + 3] == b"EOF" {
            at += 3;
            break;
        }
        let offset = byte(at) << 16 | byte(at + 1) << 8 | byte(at + 2);
        at += 3;
        check(at + 2 <= ips.len(), "IPS truncated length")?;
        let mut length = byte(at) << 8 | byte(at + 1);
        at += 2;
        let rle = length == 0;
        let mut value = 0;
        if rle {
            check(at + 3 <= ips.len(), "IPS truncated RLE")?;
            length = byte(at) << 8 | byte(at + 1);
            value = ips[at + 2];
            at += 3;
        }
        check(length > 0 && offset + length <= size, "IPS bounds")?;
        let range = offset..offset + length;
        check(
            !covered[range.clone()].contains(&true),
            "Overlapping IPS records",
        )?;
        if offset < original.len() {
            check(
                offset + length <= original.len(),
                "IPS native/extension boundary",
            )?;
            check(
                allowed[range.clone()].iter().all(|&a| a),
                "IPS writes an unapproved native region",
            )?;
        }
        covered[range.clone()].fill(true);
        if rle {
            patched[range].fill(value);
        } else {
            check(at + length <= ips.len(), "IPS truncated data")?;
            patched[range].copy_from_slice(&ips[at..at + length]);
            at += length;
        }
        records += 1;
    }
    check(at == ips.len(), "Unexpected IPS EOF trailer")?;
    check(
        covered[original.len()..].iter().all(|&c| c),
        "Uninitialized extension byte",
    )?;
    Ok(Expanded { patched, records })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(offset: usize, data: &[u8]) -> Vec<u8> {
        let mut out = vec![(offset >> 16) as u8, (offset >> 8) as u8, offset as u8];
        out.extend([(data.len() >> 8) as u8, data.len() as u8]);
        out.extend(data);
        out
    }

    fn ips(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"PATCH".to_vec();
        records.iter().for_each(|r| out.extend(r));
        out.extend(b"EOF");
        out
    }

    #[test]
    fn records_and_runs_apply_over_the_fill() {
        // At 6, a run of 2 bytes of 7.
        let run = vec![0, 0, 6, 0, 0, 0, 2, 7];
        let file = ips(&[record(1, &[9]), record(4, &[1, 2]), run]);
        let expanded = expand(&file, &[0; 4], 8, &[false, true, false, false]).unwrap();
        assert_eq!(expanded.patched, [0, 9, 0, 0, 1, 2, 7, 7]);
        assert_eq!(expanded.records, 3);
    }

    #[test]
    fn what_luma_would_not_do_is_refused() {
        let allowed = [false, true, false, false];
        let refused =
            |file: Vec<u8>, size: usize| expand(&file, &[0; 4], size, &allowed).unwrap_err();
        assert_eq!(
            refused(ips(&[record(0, &[1]), record(4, &[0])]), 5),
            "IPS writes an unapproved native region"
        );
        assert_eq!(
            refused(ips(&[record(4, &[1, 2]), record(5, &[3])]), 6),
            "Overlapping IPS records"
        );
        assert_eq!(
            refused(ips(&[record(4, &[1])]), 6),
            "Uninitialized extension byte"
        );
        assert_eq!(
            refused(ips(&[record(3, &[1, 2])]), 6),
            "IPS native/extension boundary"
        );
        assert_eq!(refused(ips(&[record(4, &[1, 2, 3])]), 6), "IPS bounds");
        let mut trailer = ips(&[record(4, &[1])]);
        trailer.push(0);
        assert_eq!(refused(trailer, 5), "Unexpected IPS EOF trailer");
        assert_eq!(refused(b"PATCX".to_vec(), 4), "IPS magic");
    }
}
