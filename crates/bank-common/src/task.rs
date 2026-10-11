//! The original's task objects, as far as every patch needs them: reaching
//! what a task owns, its busy protection and outcome, the HOME and sleep
//! mask, and the screen with two numbers. See docs/internals.md, "Tasks and
//! routing" and "HOME button and sleep".

use core::{mem::transmute, ptr::NonNull};

/// Vtables of the tasks a patch replaces the update of: opening the Bank
/// (task 9), loading it (0x10), saving (7), reward claim (0xd) and reward
/// guard (0xc).
pub const TASK_OPEN: u32 = 0x0036_1cf0;
pub const TASK_LOAD: u32 = 0x0036_1ec8;
pub const TASK_SAVE: u32 = 0x0036_2028;
pub const TASK_REWARDS: u32 = 0x0036_19fc;
pub const TASK_REWARD_GUARD: u32 = 0x0036_1bd8;
/// Bank's file system session.
pub const FS_SESSION: *const u32 = 0x0039_0100 as *const u32;
/// Set by the original main loop (0010ba04) when HOME is pressed while the
/// activity mask is zero, and nothing else is done for the press then
/// (0010ba7c). The loop carries the press out once the current scene is
/// ready, which can be several frames later, whatever the mask says by then.
const HOME_ACCEPTED: *mut u8 = 0x0037_2989 as *mut u8;

/// A pointer or object of the original that is not what a hook expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotNative;

/// # Safety
/// `base + offset` must be four readable bytes of a live native object.
pub unsafe fn read_word(base: *mut u8, offset: usize) -> u32 {
    unsafe { base.add(offset).cast::<u32>().read() }
}
/// # Safety
/// `base + offset` must be four writable bytes of a live native object.
pub unsafe fn write_word(base: *mut u8, offset: usize, value: u32) {
    unsafe { base.add(offset).cast::<u32>().write(value) }
}
pub fn checked_pointer(value: u32) -> Result<*mut u8, NotNative> {
    if value == 0 || value & 3 != 0 {
        Err(NotNative)
    } else {
        Ok(value as *mut u8)
    }
}

#[derive(Clone, Copy)]
pub struct Task(NonNull<u8>);
impl Task {
    // Only trusted native hook callers may supply object pointers. These checks
    // detect a wrong hook/type; they are not a general memory-access validator.
    /// # Safety
    /// `raw` is the live task object the original's task driver passes to
    /// the hooked virtual function, on the main thread.
    pub unsafe fn from_raw(raw: *mut u8, expected: &[u32]) -> Result<Self, NotNative> {
        checked_pointer(raw as u32)?;
        if !expected.contains(&unsafe { read_word(raw, 0) }) {
            return Err(NotNative);
        }
        Ok(Self(NonNull::new(raw).ok_or(NotNative)?))
    }
    pub fn raw(self) -> *mut u8 {
        self.0.as_ptr()
    }
    pub fn pointer(self, offset: usize) -> Result<*mut u8, NotNative> {
        checked_pointer(unsafe { read_word(self.raw(), offset) })
    }
    pub fn bank_pointer(self) -> Result<*mut u8, NotNative> {
        let coordinator = self.pointer(8)?;
        checked_pointer(unsafe { read_word(coordinator, 0xcc) })
    }
    pub fn ui(self) -> Result<*mut u8, NotNative> {
        let offset = match unsafe { read_word(self.raw(), 0) } {
            TASK_LOAD | TASK_REWARD_GUARD => 0x38,
            TASK_REWARDS => 0x40,
            _ => 0x3c,
        };
        self.pointer(offset)
    }
    pub fn begin_busy(self) {
        let begin: unsafe extern "aapcs" fn(*mut u8, u32, u32) =
            unsafe { transmute(0x0025_c420usize) };
        unsafe {
            begin(self.raw(), 1, 0);
        }
    }
    pub fn end_busy(self) {
        if unsafe { self.raw().add(0x25).read() } != 0 {
            let end: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x0025_c3e8usize) };
            unsafe {
                end(self.raw());
            }
        }
    }
    pub fn finish(self, success: bool) -> u32 {
        unsafe {
            self.raw().add(0x30).write(if success { 4 } else { 3 });
        }
        1
    }
}

/// Sets or clears bit 1 of Bank's activity mask through the original
/// functions. While the mask is not zero the original main loop refuses the
/// HOME button and sleep, as it does during its own transfers.
///
/// The mask only refuses new HOME presses. One accepted just before it was
/// set would still be carried out, so setting the mask also takes that press
/// back: accepting it did nothing but set the one byte cleared here. From
/// then on the original refuses every press itself, and the HOME Menu, and
/// closing Bank from it, never meet a loading screen or a running job.
pub fn storage_activity(active: bool) {
    let address = if active {
        0x001d_4d90usize
    } else {
        0x0022_9eb4
    };
    let mark: unsafe extern "aapcs" fn(u32) = unsafe { transmute(address) };
    unsafe { mark(1) };
    if active {
        unsafe { HOME_ACCEPTED.write_volatile(0) };
    }
}

// The original's "show a message" function, 001d6650(ui, kind, message,
// wait), begins with `push {r4-r8, lr}`. A patch replaces that instruction
// by a branch to this entry, which runs it, lets HOME and sleep through if
// the message waits for an answer, and goes on inside the original.
//
// The patches refuse HOME and sleep by the task that comes next, for as
// long as it reads or writes. A message that waits is where such a task has
// nothing left to do but end, so the refusal the router hook began ends
// here. The original's own bit for a game save that is being written is not
// this one and stays as it is.
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".section .text.bank.message, \"ax\", %progbits",
    ".align 2",
    ".global bank_message_shown",
    ".type bank_message_shown, %function",
    "bank_message_shown:",
    "push {{r4, r5, r6, r7, r8, lr}}",
    "cmp r3, #0",
    "beq 1f",
    "push {{r0-r3, r12, lr}}",
    "bl {waits}",
    "pop {{r0-r3, r12, lr}}",
    "1:",
    "ldr r12, =0x001d6654",
    "bx r12",
    ".ltorg",
    ".size bank_message_shown, .-bank_message_shown",
    waits = sym message_waits,
);
extern "aapcs" fn message_waits() {
    storage_activity(false);
}
unsafe extern "aapcs" {
    /// # Safety
    /// Only the first instruction of 001d6650 may branch here, with that
    /// function's arguments and its caller's `lr`.
    pub fn bank_message_shown();
}

/// The screen a patch ends on when nothing can go on (`ui::stop`): one
/// dialog with the two numbers, and after A the original's line "Press HOME
/// to return to the HOME Menu" with the numbers under it; or that line
/// alone, for numbers that are a result and no error. It stays, as the
/// original's own message with that line does (task 0, 002a9204, message
/// 0x1C): its task shows it without a wait and never ends.
pub struct Notice {
    shown: bool,
}
impl Notice {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self { shown: false }
    }
    /// One frame of the task's update after an error. The result is the
    /// update's own: 0, as nothing ends this screen but closing Bank. Only a
    /// screen that could not be shown finishes the task as failed.
    pub fn stop(&mut self, task: Task, first: u32, second: u32) -> u32 {
        self.end(task, Some(crate::ui::FAILURE_NOTICE), first, second)
    }
    /// The same for a patch whose work is done and whose numbers are its
    /// result: the line about the HOME Menu with them, and nothing before.
    pub fn done(&mut self, task: Task, first: u32, second: u32) -> u32 {
        self.end(task, None, first, second)
    }
    fn end(&mut self, task: Task, reason: Option<u32>, first: u32, second: u32) -> u32 {
        let Ok(ui) = task.ui() else {
            task.end_busy();
            return task.finish(false);
        };
        let shown = if self.shown {
            // Each frame only keeps the dialog alive, as the original's
            // task does for its own; its pages turn inside the dialog.
            unsafe { crate::ui::acknowledged(ui) }.map(|_| ())
        } else {
            // Nothing more is read or written: HOME works on the error
            // screen, and closing Bank from the HOME Menu ends the task
            // the original's way.
            storage_activity(false);
            task.end_busy();
            // Loading may still be animating; end its sound before the dialog.
            let _ = unsafe { crate::ui::stop_loading(ui) };
            unsafe { crate::ui::stop(ui, reason, first, second) }
        };
        self.shown = shown.is_ok();
        match shown {
            Ok(()) => 0,
            Err(_) => task.finish(false),
        }
    }
}
