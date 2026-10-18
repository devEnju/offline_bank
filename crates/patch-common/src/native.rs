//! Objects of the original, as a hook reaches them.

/// The pointer at `base + offset`, if the original would itself dereference
/// it: word aligned and inside the process image or heap.
/// # Safety
/// `base + offset` must be four readable bytes.
pub unsafe fn follow(base: *const u8, offset: usize) -> Option<*mut u8> {
    let value = unsafe { base.add(offset).cast::<u32>().read() };
    if value & 3 != 0 || !(0x0010_0000..0x1000_0000).contains(&value) {
        return None;
    }
    Some(value as *mut u8)
}
