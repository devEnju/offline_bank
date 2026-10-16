//! The state a patch's hooks share, entered by one hook at a time.
//!
//! The original calls the hooks on its main thread, but one may run inside
//! another: the original's update of a task can call the task's busy poll,
//! which a patch replaces with the same hook. A hook that finds the state
//! taken does nothing (docs/internals.md, "Patches for Bank").

use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Exclusive<T> {
    taken: AtomicBool,
    value: UnsafeCell<T>,
}
// SAFETY: the value is reached only through `Held`, of which `taken` allows
// one at a time. A patch's worker thread has a mailbox of its own and is
// never given the value.
unsafe impl<T> Sync for Exclusive<T> {}

impl<T> Exclusive<T> {
    pub const fn new(value: T) -> Self {
        Self {
            taken: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }
    /// The state, or `None` while a hook holds it already.
    pub fn take(&self) -> Option<Held<'_, T>> {
        self.taken
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Held(self))
    }
}

/// The state while one hook holds it; dropping it lets the next one in.
pub struct Held<'a, T>(&'a Exclusive<T>);
impl<T> Deref for Held<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: see `Exclusive`.
        unsafe { &*self.0.value.get() }
    }
}
impl<T> DerefMut for Held<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: see `Exclusive`.
        unsafe { &mut *self.0.value.get() }
    }
}
impl<T> Drop for Held<'_, T> {
    fn drop(&mut self) {
        self.0.taken.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hook_at_a_time() {
        let state = Exclusive::new(1);
        let mut held = state.take().unwrap();
        *held += 1;
        assert!(state.take().is_none());
        drop(held);
        assert_eq!(*state.take().unwrap(), 2);
        assert!(state.take().is_some());
    }
}
