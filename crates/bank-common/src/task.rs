//! The original's task objects, as far as every patch needs them: reaching
//! what a task owns, its busy protection and outcome, the router, the HOME
//! and sleep mask, and the screen with two numbers; and the addresses of the
//! original that more than one module uses. Addresses used in one module are
//! named at its top. See docs/internals.md, "Tasks and routing" and "HOME
//! button and sleep".

#[cfg(target_arch = "arm")]
use core::mem::transmute;

/// Factory `002a5a7c` accepts IDs below 0x1e (`002a5a80`).
pub const TASK_COUNT: u32 = 0x1e;
/// Vtables of the tasks a patch replaces the update of: opening the Bank
/// (task 9), loading it (0x10), saving (7), reward claim (0xd) and reward
/// guard (0xc).
pub const TASK_SCAN: u32 = 0x0036_1ab8;
pub const TASK_OPEN: u32 = 0x0036_1cf0;
pub const TASK_LOAD: u32 = 0x0036_1ec8;
pub const TASK_SAVE: u32 = 0x0036_2028;
pub const TASK_REWARDS: u32 = 0x0036_19fc;
pub const TASK_REWARD_GUARD: u32 = 0x0036_1bd8;
/// `00233a6c(save manager)`: the game object of the selected game.
pub const SELECTED_GAME: usize = 0x0023_3a6c;
/// `0023a754(words)`: the console's date and time as the original's packed
/// timestamp. It keeps upper flag bits of the two words, so they start
/// zeroed.
pub const LOCAL_DATE: usize = 0x0023_a754;
/// The original's code. A function pointer read from an object of the
/// original must lie here.
pub const TEXT: core::ops::Range<u32> = 0x0010_0000..0x0031_3910;
/// Busy protection of a task: `0025c420(task, 1, 0)` begins it,
/// `0025c3e8(task)` ends it.
#[cfg(target_arch = "arm")]
const BEGIN_BUSY: usize = 0x0025_c420;
#[cfg(target_arch = "arm")]
const END_BUSY: usize = 0x0025_c3e8;

use patch_common::native::read_word;
pub use patch_common::native::NotNative;
impl From<NotNative> for crate::fault::Fault {
    fn from(_: NotNative) -> Self {
        Self::NativeObject
    }
}

/// A task of Bank 1.5 (`patch_common::native::Task`), with what only Bank's
/// tasks have in `BankTask`.
pub type Task = patch_common::native::Task<crate::program::Bank15>;

/// What Bank's tasks have beyond every task: the shared UI, kept at an
/// offset of each task's own, the busy protection, and the outcomes a
/// task update ends with.
pub trait BankTask {
    fn ui(self) -> Result<*mut u8, NotNative>;
    #[cfg(target_arch = "arm")]
    fn begin_busy(self);
    #[cfg(target_arch = "arm")]
    fn end_busy(self);
    /// Ends the task as done (4) or failed (3); the update answers 1.
    fn finish(self, success: bool) -> u32;
}
impl BankTask for Task {
    fn ui(self) -> Result<*mut u8, NotNative> {
        let offset = match unsafe { read_word(self.raw(), 0) } {
            TASK_LOAD | TASK_REWARD_GUARD => 0x38,
            TASK_SCAN | TASK_REWARDS => 0x40,
            _ => 0x3c,
        };
        self.pointer(offset)
    }
    #[cfg(target_arch = "arm")]
    fn begin_busy(self) {
        let begin: unsafe extern "aapcs" fn(*mut u8, u32, u32) = unsafe { transmute(BEGIN_BUSY) };
        unsafe {
            begin(self.raw(), 1, 0);
        }
    }
    #[cfg(target_arch = "arm")]
    fn end_busy(self) {
        if unsafe { self.raw().add(0x25).read() } != 0 {
            let end: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(END_BUSY) };
            unsafe {
                end(self.raw());
            }
        }
    }
    fn finish(self, success: bool) -> u32 {
        self.set_outcome(if success { 4 } else { 3 });
        1
    }
}

/// Refuses HOME and sleep, or lets them through again
/// (`patch_common::home`).
#[cfg(target_arch = "arm")]
pub fn refuse_home(refuse: bool) {
    patch_common::home::refuse::<crate::program::Bank15>(refuse);
}

// The original's "show a message" function, 001d6650(ui, kind, message,
// wait), as every patch but the download enters it (`patch_common`'s
// `message_stub!`). The original's own bit for a game save that is being
// written is not the one let through here and stays as it is.
#[cfg(target_arch = "arm")]
patch_common::message_stub!(bank_message_stub, "r3", "0x001d6654", message_waits);
#[cfg(target_arch = "arm")]
extern "aapcs" fn message_waits() {
    refuse_home(false);
}

/// The screen a patch ends on when nothing can go on (`ui::stop`): one
/// dialog with the two numbers, and after A the original's line "Press HOME
/// to return to the HOME Menu" with the version and package and the numbers
/// under it; or that line alone, for numbers that are a result and no error. It stays, as the
/// original's own message with that line does (task 0, 002a9204, message
/// 0x1C): its task shows it without a wait and never ends.
#[cfg(target_arch = "arm")]
pub struct Notice {
    shown: bool,
    label: crate::numbers::Label,
}
#[cfg(target_arch = "arm")]
impl Notice {
    /// `suffix` is the package's own, as "-test"; the offline patch has
    /// none.
    pub const fn new(suffix: &[u8]) -> Self {
        Self {
            shown: false,
            label: crate::numbers::Label::of(suffix),
        }
    }
    /// Shows `label` above the numbers instead of the package's own, for a
    /// result that says more than the package: the migration names the way
    /// the Bank was stored that it converted.
    pub fn relabel(&mut self, label: crate::numbers::Label) {
        self.label = label;
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
            refuse_home(false);
            task.end_busy();
            // Loading may still be animating; end its sound before the dialog.
            let _ = unsafe { crate::ui::stop_loading(ui) };
            unsafe { crate::ui::stop(ui, reason, &self.label, first, second) }
        };
        self.shown = shown.is_ok();
        match shown {
            Ok(()) => 0,
            Err(_) => task.finish(false),
        }
    }
}
