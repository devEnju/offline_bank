//! Objects of the original, as a hook reaches them.

use core::ptr::NonNull;

/// A pointer or object of the original that is not what a hook expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotNative;

/// `raw` as an object of the original whose vtable is one of `expected`: a
/// pointer (`pointer`) whose first word is one of them. Every patch checks
/// an object a hook is given this way. It tells a wrong object at a hook's
/// site; it does not make arbitrary memory readable.
/// # Safety
/// A pointer `raw` must be four readable bytes.
pub unsafe fn object(raw: *mut u8, expected: &[u32]) -> Result<NonNull<u8>, NotNative> {
    pointer(raw as u32).ok_or(NotNative)?;
    let raw = NonNull::new(raw).ok_or(NotNative)?;
    if !expected.contains(&unsafe { raw.as_ptr().cast::<u32>().read() }) {
        return Err(NotNative);
    }
    Ok(raw)
}

/// `value` as a pointer of the original: not zero and word aligned. Every
/// patch checks a pointer it reads from an object of the original this way.
pub fn pointer(value: u32) -> Option<*mut u8> {
    (value != 0 && value & 3 == 0).then_some(value as *mut u8)
}

/// The pointer at `base + offset` (`pointer`).
/// # Safety
/// `base + offset` must be four readable bytes.
pub unsafe fn follow(base: *const u8, offset: usize) -> Option<*mut u8> {
    pointer(unsafe { base.add(offset).cast::<u32>().read() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_aligned_words_but_zero_are_pointers() {
        for value in [4, 0x0010_0000, 0x0036_1ac4, 0x1400_0000, 0xffff_fffc] {
            assert_eq!(pointer(value), Some(value as *mut u8));
        }
        for value in [0, 1, 2, 0x0010_0002, u32::MAX] {
            assert_eq!(pointer(value), None, "{value:#x}");
        }
    }

    #[test]
    fn only_an_object_with_an_expected_vtable_is_accepted_and_left_as_it_is() {
        let mut words = [0x002e_6618u32, 0x1234_5678];
        let raw = words.as_mut_ptr().cast::<u8>();
        let accepted = unsafe { object(raw, &[0x002e_6774, 0x002e_6618]) };
        assert_eq!(accepted.map(NonNull::as_ptr), Ok(raw));
        assert_eq!(unsafe { object(raw, &[0x002e_6774]) }, Err(NotNative));
        assert_eq!(unsafe { object(raw, &[]) }, Err(NotNative));
        assert_eq!(words, [0x002e_6618, 0x1234_5678]);
        let unaligned = unsafe { raw.add(1) };
        assert_eq!(unsafe { object(unaligned, &[0x002e_6618]) }, Err(NotNative));
        let null = core::ptr::null_mut();
        assert_eq!(unsafe { object(null, &[0x002e_6618]) }, Err(NotNative));
    }
}
