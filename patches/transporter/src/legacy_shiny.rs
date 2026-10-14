//! The Easter egg of the Gen 1 Virtual Console conversion.
//!
//! Transporter 1.2 tested the wrong DV when it decided whether a Gen 1
//! Pokémon is shiny: Attack 10 and Defense with bit 1 set, where 1.3 and
//! later test Defense 10 and Attack with bit 1 set (Speed and Special 10 in
//! both). It also kept the random gender the Pokémon constructor draws, where
//! later versions derive it from the Attack DV. The seven combinations that
//! only the old rule accepts are shiny again here, with the constructor's
//! gender. Every other Pokémon converts as the original does; the one that
//! both rules accept (all four DVs 10) stays the original's shiny.
//!
//! The Gen 1 converter (`0024F428`) holds its source record of 33 bytes in
//! `r4`; byte `0x1B` holds Attack (high nibble) and Defense, byte `0x1C`
//! Speed and Special.

/// A Gen 1 record as the converter reads it.
pub const RECORD_SIZE: usize = 33;

const ATTACK_DEFENSE: usize = 0x1b;
const SPEED_SPECIAL: usize = 0x1c;

/// The original's rule (0024F5B8): Defense, Speed, Special 10, Attack bit 1.
fn correct(attack_defense: u8, speed_special: u8) -> bool {
    speed_special == 0xaa && attack_defense & 0x0f == 10 && attack_defense & 0x20 != 0
}

/// Transporter 1.2's rule: Attack, Speed, Special 10, Defense bit 1.
fn old(attack_defense: u8, speed_special: u8) -> bool {
    speed_special == 0xaa && attack_defense >> 4 == 10 && attack_defense & 0x02 != 0
}

/// Whether the Easter egg makes this Pokémon shiny with the random gender.
pub fn applies(record: &[u8; RECORD_SIZE]) -> bool {
    let (attack_defense, speed_special) = (record[ATTACK_DEFENSE], record[SPEED_SPECIAL]);
    old(attack_defense, speed_special) && !correct(attack_defense, speed_special)
}

/// Called from the build stub in `link.rs`, right before the constructor
/// call (0024F6B0). For a record the Easter egg applies to, the shiny mode of
/// the construction parameters, the word at `+0x0C` (1 never, 2 shiny; set
/// at 0024F610), asks for a shiny.
/// # Safety
/// `source` is the converter's source record, `params` its construction
/// parameters on the converter's stack.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_shiny_build(source: *const u8, params: *mut u32) {
    if applies(unsafe { &*source.cast::<[u8; RECORD_SIZE]>() }) {
        unsafe { params.add(3).write(2) };
    }
}

/// Called from the gender stub in `link.rs` (0024F8E0), where the original
/// replaces the constructor's gender by one from the Attack DV: 1 keeps the
/// constructor's gender.
/// # Safety
/// `source` is the converter's source record.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_shiny_keeps_gender(source: *const u8) -> u32 {
    u32::from(applies(unsafe { &*source.cast::<[u8; RECORD_SIZE]>() }))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// A record with these DVs: Attack, Defense, Speed, Special.
    fn record([attack, defense, speed, special]: [u8; 4]) -> [u8; RECORD_SIZE] {
        let mut record = [0x55; RECORD_SIZE];
        record[ATTACK_DEFENSE] = attack << 4 | defense;
        record[SPEED_SPECIAL] = speed << 4 | special;
        record
    }

    fn every_combination() -> impl Iterator<Item = [u8; 4]> {
        (0..=u16::MAX).map(|packed| {
            let [high, low] = packed.to_be_bytes();
            [high >> 4, high & 15, low >> 4, low & 15]
        })
    }

    #[test]
    fn the_rules_on_the_four_dvs() {
        for [a, d, s, c] in every_combination() {
            let [ad, ss] = [a << 4 | d, s << 4 | c];
            assert_eq!(correct(ad, ss), d == 10 && s == 10 && c == 10 && a & 2 != 0);
            assert_eq!(old(ad, ss), a == 10 && s == 10 && c == 10 && d & 2 != 0);
        }
    }

    #[test]
    fn seven_more_combinations_and_no_other_change() {
        let mut counts = [0; 4];
        let mut extra = Vec::new();
        for dvs in every_combination() {
            let [a, d, s, c] = dvs;
            let [ad, ss] = [a << 4 | d, s << 4 | c];
            counts[usize::from(correct(ad, ss)) * 2 + usize::from(old(ad, ss))] += 1;
            if applies(&record(dvs)) {
                extra.push(u16::from_be_bytes([ad, ss]));
            }
        }
        // Neither, only the old rule, only the correct one, both.
        assert_eq!(counts, [65_521, 7, 7, 1]);
        assert_eq!(
            extra,
            [0xa2aa, 0xa3aa, 0xa6aa, 0xa7aa, 0xabaa, 0xaeaa, 0xafaa]
        );
    }

    #[test]
    fn the_original_shinies_stay_the_originals() {
        assert!(!applies(&record([10, 10, 10, 10])));
        for attack in [2, 3, 6, 7, 11, 14, 15] {
            assert!(!applies(&record([attack, 10, 10, 10])));
        }
        assert!(!applies(&record([10, 2, 10, 11])));
        assert!(!applies(&record([10, 2, 11, 10])));
        assert!(applies(&record([10, 2, 10, 10])));
    }
}
