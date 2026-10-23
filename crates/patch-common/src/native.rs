//! Objects of the original, as a hook reaches them.

use crate::program::Program;
#[cfg(target_arch = "arm")]
use core::mem::transmute;
use core::{marker::PhantomData, ptr::NonNull};

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

/// `address`, read from an object of the original, as a function of program
/// `P`: word aligned and inside its code (`Program::TEXT`). Every patch
/// checks a function pointer it reads from an object of the original this
/// way before it calls it.
pub fn code<P: Program>(address: u32) -> Option<usize> {
    within(&P::TEXT, address)
}
fn within(text: &core::ops::Range<u32>, address: u32) -> Option<usize> {
    (address & 3 == 0 && text.contains(&address)).then_some(address as usize)
}

/// The word at `base + offset`.
/// # Safety
/// `base + offset` must be four readable bytes of a live native object.
pub unsafe fn read_word(base: *mut u8, offset: usize) -> u32 {
    unsafe { base.add(offset).cast::<u32>().read() }
}
/// Writes the word at `base + offset`.
/// # Safety
/// `base + offset` must be four writable bytes of a live native object.
pub unsafe fn write_word(base: *mut u8, offset: usize, value: u32) {
    unsafe { base.add(offset).cast::<u32>().write(value) }
}

/// A task that has ended, as a router hook sees it, and what the
/// original's router answered for it.
#[derive(Clone, Copy)]
pub struct Ended {
    /// The ended task's byte `+0x30`.
    pub outcome: u8,
    /// The manager's flag `+0x1a`, which the original's router clears.
    pub cancelled: bool,
    /// The original's router's answer.
    pub native: u32,
}
#[cfg(target_arch = "arm")]
impl Ended {
    /// Reads the ended task of `manager` (`+0x10`) and the manager's flag,
    /// then asks the original's router (`Program::ROUTER`). `None` for a
    /// task ID its factory does not make (`Program::TASK_COUNT`), or a
    /// manager or task that is not a pointer; the original is not asked
    /// then. Both programs' managers and tasks keep these at the same
    /// offsets.
    /// # Safety
    /// Called from the router's call site of `P` with its arguments, on the
    /// main thread.
    pub unsafe fn route<P: Program>(manager: *mut u8, current: u32) -> Option<Self> {
        if current >= P::TASK_COUNT {
            return None;
        }
        pointer(manager as u32)?;
        let task = unsafe { follow(manager, 0x10) }?;
        // Read before the router runs, which clears the flag.
        let cancelled = unsafe { manager.add(0x1a).read() } != 0;
        let outcome = unsafe { task.add(TASK_OUTCOME).read() };
        let router: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = unsafe { transmute(P::ROUTER) };
        Some(Self {
            outcome,
            cancelled,
            native: unsafe { router(manager, current) },
        })
    }
}

/// The pointer at `base + offset` (`pointer`). Every patch reads a pointer
/// from an object of the original this way.
/// # Safety
/// `base + offset` must be four readable bytes.
pub unsafe fn follow(base: *const u8, offset: usize) -> Option<*mut u8> {
    pointer(unsafe { base.add(offset).cast::<u32>().read() })
}

/// Where a task keeps its manager, and the manager the Bank data object;
/// the same in both programs.
pub const TASK_MANAGER: usize = 0x8;
pub const MANAGER_BANK_OBJECT: usize = 0xcc;
/// The byte a task ends with, which the router reads (`Ended`).
pub const TASK_OUTCOME: usize = 0x30;

/// A task of the original of program `P`, as a hook is given it: its
/// vtable checked (`object`), and every pointer it leads to checked before
/// it is followed.
pub struct Task<P>(NonNull<u8>, PhantomData<fn() -> P>);
impl<P> Clone for Task<P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P> Copy for Task<P> {}
impl<P: Program> Task<P> {
    /// The task a hook is given, if its vtable is one of `expected`.
    /// # Safety
    /// `raw` is the live task object the patched site passes, on the main
    /// thread.
    pub unsafe fn from_raw(raw: *mut u8, expected: &[u32]) -> Result<Self, NotNative> {
        unsafe { object(raw, expected) }.map(|raw| Self(raw, PhantomData))
    }
    pub fn raw(self) -> *mut u8 {
        self.0.as_ptr()
    }
    /// The pointer the task holds at `offset` (`follow`).
    pub fn pointer(self, offset: usize) -> Result<*mut u8, NotNative> {
        unsafe { follow(self.raw(), offset) }.ok_or(NotNative)
    }
    /// The manager's Bank data object, if its vtable is the one its
    /// constructor installs (`Program::BANK_VTABLE`), before anything of it
    /// is read.
    pub fn bank_pointer(self) -> Result<*mut u8, NotNative> {
        let manager = self.pointer(TASK_MANAGER)?;
        let bank = unsafe { follow(manager, MANAGER_BANK_OBJECT) }.ok_or(NotNative)?;
        unsafe { object(bank, &[P::BANK_VTABLE]) }.map(NonNull::as_ptr)
    }
    /// Ends the task with `outcome`, which the router reads.
    pub fn set_outcome(self, outcome: u8) {
        unsafe { self.raw().add(TASK_OUTCOME).write(outcome) }
    }
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
    fn only_aligned_words_inside_the_text_are_code() {
        let text = 0x0010_0000..0x0028_d1ac;
        for address in [0x0010_0000, 0x0019_b50c, 0x0028_d1a8] {
            assert_eq!(within(&text, address), Some(address as usize));
        }
        for address in [
            0,
            0x000f_fffc,
            0x0019_b50e,
            0x0028_d1ac,
            0x0036_4000,
            u32::MAX,
        ] {
            assert_eq!(within(&text, address), None, "{address:#x}");
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
