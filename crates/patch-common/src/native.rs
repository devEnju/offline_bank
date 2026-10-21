//! Objects of the original, as a hook reaches them.

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
}
