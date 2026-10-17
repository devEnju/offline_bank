//! The state a patch's hooks share, entered by one hook at a time.
//!
//! The original calls the hooks on its main thread, but one may run inside
//! another: the original's update of a task can call the task's busy poll,
//! which a patch replaces with the same hook. A hook that finds the state
//! taken does nothing (docs/internals.md, "Patches for Bank").
//!
//! The state holds pointers to the original's objects, which belong to the
//! main thread, so it is not `Send`. That it is used on that thread alone is
//! the promise of `new`: one thread takes it, and that thread is the one its
//! contents belong to. The guard cannot leave the thread that took it.
//!
//! ```compile_fail
//! fn send<T: Send>(_: T) {}
//! // SAFETY: taken on this thread only.
//! let state = unsafe { patch_common::exclusive::Exclusive::new(0) };
//! send(state.take());
//! ```
//! ```compile_fail
//! fn share<T: Sync>(_: &T) {}
//! // SAFETY: taken on this thread only.
//! let state = unsafe { patch_common::exclusive::Exclusive::new(0) };
//! share(&state.take());
//! ```

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Exclusive<T> {
    taken: AtomicBool,
    value: UnsafeCell<T>,
}
// SAFETY: `new`'s caller promises that the state is only ever taken on one
// thread, the one its contents belong to, and `Held`, the only way to the
// value, cannot be sent to or shared with another; `taken` allows one at a
// time on that thread. So the value, and whatever it refers to, is only used
// where it may be. A patch's worker
// thread has a mailbox of its own and is never given the value.
unsafe impl<T> Sync for Exclusive<T> {}

impl<T> Exclusive<T> {
    /// # Safety
    /// `take` is only ever called on one thread, and that thread is the one
    /// the contents of `value` belong to: whatever it holds or refers to that
    /// is not `Send` or not `Sync` may be used there and is used nowhere
    /// else while the state can be taken. For a patch's hooks this is
    /// Bank's main thread, which the original calls them on and which owns
    /// the original's objects the state points to.
    pub const unsafe fn new(value: T) -> Self {
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
            .map(|_| Held(self, PhantomData))
    }
}

/// The state while one hook holds it; dropping it lets the next one in. It
/// is neither `Send` nor `Sync`: it stays on the thread that took it.
pub struct Held<'a, T>(&'a Exclusive<T>, PhantomData<*mut ()>);
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
        // SAFETY: taken on this thread only.
        let state = unsafe { Exclusive::new(1) };
        let mut held = state.take().unwrap();
        *held += 1;
        assert!(state.take().is_none());
        drop(held);
        assert_eq!(*state.take().unwrap(), 2);
        assert!(state.take().is_some());
    }
}
