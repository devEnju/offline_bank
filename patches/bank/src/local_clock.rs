//! Offline replacement for Bank's packed timestamp helper (001d3bf4).
//!
//! Native slot setters, group moves, and clears use this eight-byte value.
//! The online context is deliberately unused; local RTC conversion needs none.

/// Writes a native packed timestamp only after validating all calendar fields.
/// `read` receives zeroed words because native 0023a754 preserves upper flags.
///
/// # Safety
/// A non-null, four-byte-aligned `output` must identify eight writable bytes.
/// No concurrent access to those bytes is allowed. `online_context` is never
/// dereferenced and may be null. The reader must only write its supplied words.
pub unsafe fn timestamp_with(
    _online_context: *mut u8,
    output: *mut u8,
    read: impl FnOnce(&mut [u32; 2]),
) -> u32 {
    if output.is_null() || output as usize & 3 != 0 {
        return 0;
    }
    let mut words = [0; 2];
    read(&mut words);
    if !valid_date(words) {
        return 0;
    }
    // SAFETY: caller guarantees exclusive valid storage; alignment checked.
    unsafe {
        output.cast::<u32>().write(words[0]);
        output.add(4).cast::<u32>().write(words[1]);
    }
    1
}

/// Console-local calendar day of a native packed timestamp, for reward
/// accounting. Returns None for any value `timestamp_with` would reject.
pub fn local_day(words: [u32; 2]) -> Option<offline_core::rewards::Date> {
    if !valid_date(words) {
        return None;
    }
    let packed = u64::from(words[0]) | (u64::from(words[1]) << 32);
    offline_core::rewards::Date::new(
        ((packed >> 26) & 0x3fff) as u16,
        ((packed >> 22) & 15) as u8,
        ((packed >> 17) & 31) as u8,
    )
    .ok()
}

fn valid_date(words: [u32; 2]) -> bool {
    let packed = u64::from(words[0]) | (u64::from(words[1]) << 32);
    let year = ((packed >> 26) & 0x3fff) as u32;
    let month = ((packed >> 22) & 15) as u32;
    let day = ((packed >> 17) & 31) as u32;
    let hour = (packed >> 12) & 31;
    let minute = (packed >> 6) & 63;
    let second = packed & 63;
    if packed >> 40 != 0 || year == 0 || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: u64, month: u64, day: u64, hour: u64, minute: u64, second: u64) -> [u32; 2] {
        let packed = year << 26 | month << 22 | day << 17 | hour << 12 | minute << 6 | second;
        [packed as u32, (packed >> 32) as u32]
    }

    #[test]
    fn null_online_context_produces_exact_native_date_without_touching_neighbors() {
        let expected = date(2026, 10, 3, 12, 34, 56);
        let mut memory = [0xa5a5a5a5; 4];
        // Reproduces the crash condition: no initialized online context.
        let result = unsafe {
            timestamp_with(
                core::ptr::null_mut(),
                memory.as_mut_ptr().add(1).cast(),
                |words| {
                    assert_eq!(*words, [0, 0]);
                    *words = expected;
                },
            )
        };
        assert_eq!(result, 1);
        assert_eq!(memory, [0xa5a5a5a5, expected[0], expected[1], 0xa5a5a5a5]);
    }

    #[test]
    fn every_slot_of_a_group_can_stamp_without_an_online_owner() {
        let mut slots = [[0u32; 2]; 30];
        for (index, slot) in slots.iter_mut().enumerate() {
            let expected = date(2026, 10, 3, 12, 0, index as u64);
            // An invalid context is safe because the helper never follows it.
            let result = unsafe {
                timestamp_with(0x58usize as *mut u8, slot.as_mut_ptr().cast(), |words| {
                    *words = expected
                })
            };
            assert_eq!(result, 1);
            assert_eq!(*slot, expected);
        }
    }

    #[test]
    fn reward_day_ignores_time_of_day_and_rejects_invalid_clocks() {
        use offline_core::rewards::Date;
        let late = local_day(date(2026, 12, 31, 23, 59, 59)).unwrap();
        let early = local_day(date(2027, 1, 1, 0, 0, 0)).unwrap();
        assert_eq!(late, Date::new(2026, 12, 31).unwrap());
        assert_eq!(early, Date::new(2027, 1, 1).unwrap());
        // One second across midnight is one calendar day.
        assert_eq!(early.days_since(late), 1);
        assert_eq!(
            local_day(date(2028, 2, 29, 12, 0, 0)),
            Some(Date::new(2028, 2, 29).unwrap())
        );
        let mut flags = date(2026, 10, 3, 12, 0, 0);
        flags[1] |= 0x100;
        for invalid in [
            [0, 0],
            date(2026, 2, 29, 0, 0, 0),
            date(2026, 13, 1, 0, 0, 0),
            date(2026, 10, 3, 24, 0, 0),
            flags,
        ] {
            assert_eq!(local_day(invalid), None);
        }
    }

    #[test]
    fn invalid_output_does_not_read_clock() {
        for output in [core::ptr::null_mut(), core::ptr::dangling_mut::<u8>()] {
            assert_eq!(
                unsafe {
                    timestamp_with(core::ptr::null_mut(), output, |_| panic!("clock called"))
                },
                0
            );
        }
    }

    #[test]
    fn invalid_calendar_or_flags_preserve_previous_output() {
        let mut flags = date(2026, 10, 3, 12, 0, 0);
        flags[1] |= 0x100;
        for invalid in [
            [0, 0],
            date(2026, 13, 1, 0, 0, 0),
            date(2026, 2, 29, 0, 0, 0),
            date(2100, 2, 29, 0, 0, 0),
            date(2026, 4, 31, 0, 0, 0),
            date(2026, 10, 3, 24, 0, 0),
            date(2026, 10, 3, 12, 60, 0),
            date(2026, 10, 3, 12, 0, 60),
            flags,
        ] {
            let mut output = [0x11223344, 0x55667788];
            assert_eq!(
                unsafe {
                    timestamp_with(core::ptr::null_mut(), output.as_mut_ptr().cast(), |words| {
                        *words = invalid
                    })
                },
                0
            );
            assert_eq!(output, [0x11223344, 0x55667788]);
        }
        assert!(valid_date(date(2000, 2, 29, 23, 59, 59)));
        assert!(valid_date(date(2028, 2, 29, 0, 0, 0)));
    }
}
