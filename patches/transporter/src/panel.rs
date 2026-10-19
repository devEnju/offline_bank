//! When the loading panel after START goes up, and who ends it.
//!
//! After START the original searches for games (`00246F04`) without an
//! indicator. The game list's own indicator can only come once its set-up
//! has built the list's screen, which loads that screen's archives first
//! (`0024264C`). The patch reads Bank's files and looks for saves on the SD
//! card in the search, so the search shows the general loading panel of the
//! shared UI, which needs nothing loaded, with the text the list's indicator
//! shows. The search's first frame only starts it; the check follows on the
//! next. That one panel stays until the list is on screen:
//!
//! - the list's set-up does not start its own indicator; it only notes the
//!   time it began (`hooks::transporter_list_indicator`), and its check
//!   that restarts the indicator's sound is skipped while the panel is up
//!   (`hooks::transporter_list_frame`). Its end of loading stops the sound,
//!   which is the panel's, after at least 3 s, as it stops its own;
//! - the panel ends once the list is on screen (`transporter_list_ready`),
//!   before a message that waits (a refusal, "Could not find a game…"), or
//!   when the search ends in any step but the game list.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The panel the patch started: the UI it is on, or 0 while there is none,
/// and whether the game list has taken the search's place under it. Only
/// the main thread uses it.
pub struct Panel {
    ui: AtomicUsize,
    list: AtomicBool,
}

/// What the game list's set-up does where it would start its indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStart {
    /// The panel is up: start nothing, only note the time.
    Quiet,
    /// No panel: the list starts its own indicator, as the original.
    Own,
}

impl Panel {
    pub const fn new() -> Self {
        Self {
            ui: AtomicUsize::new(0),
            list: AtomicBool::new(false),
        }
    }
    /// True if the panel is to be started now on `ui`: when none is up.
    pub fn start(&self, ui: usize) -> bool {
        let started = ui != 0
            && self
                .ui
                .compare_exchange(0, ui, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok();
        if started {
            self.list.store(false, Ordering::Relaxed);
        }
        started
    }
    /// Whether the panel is up.
    pub fn up(&self) -> bool {
        self.ui.load(Ordering::Relaxed) != 0
    }
    /// The game list's set-up is about to start its indicator.
    pub fn list_starts(&self) -> ListStart {
        if self.up() {
            self.list.store(true, Ordering::Relaxed);
            ListStart::Quiet
        } else {
            ListStart::Own
        }
    }
    /// Whether the game list has taken the search's place under the panel.
    pub fn in_list(&self) -> bool {
        self.up() && self.list.load(Ordering::Relaxed)
    }
    /// The UI of the panel that is up, which is ended now, or `None`.
    pub fn release(&self) -> Option<usize> {
        self.list.store(false, Ordering::Relaxed);
        match self.ui.swap(0, Ordering::Relaxed) {
            0 => None,
            ui => Some(ui),
        }
    }
}
impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the panel stays when the search ends: into the game list, whose
/// loading it covers; after anything else the patch ends it.
pub fn handed_on(next: u32) -> bool {
    next == crate::navigation::LIST
}

/// The time the game list counts its minimum time from (`task + 0x70`, in
/// ms), once it has noted `noted` and the panel has been up for `elapsed`
/// ms: the panel's start, so that the list waits its 3 s from START, not
/// from its own beginning.
pub fn list_start(noted: u64, elapsed: u32) -> u64 {
    noted.saturating_sub(u64::from(elapsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{DISCONNECT, LIST, TITLE};

    const UI: usize = 0x0830_0000;

    #[test]
    fn the_check_starts_it_once_however_many_frames_it_takes() {
        let panel = Panel::new();
        // The frame that starts it does nothing else: the check comes back
        // next frame and is handed over then.
        assert!(panel.start(UI));
        for _ in 0..100 {
            assert!(!panel.start(UI));
        }
        assert!(panel.up());
    }

    #[test]
    fn the_list_starts_nothing_of_its_own_while_the_panel_is_up() {
        let panel = Panel::new();
        assert!(panel.start(UI));
        assert!(!panel.in_list());
        assert_eq!(panel.list_starts(), ListStart::Quiet);
        assert!(panel.in_list());
        // Every frame of the list's set-up skips its sound check while the
        // panel is up.
        for _ in 0..100 {
            assert!(panel.up());
        }
        // Once the list is on screen the panel ends, once.
        assert_eq!(panel.release(), Some(UI));
        assert_eq!(panel.release(), None);
        assert!(!panel.up());
        assert!(!panel.in_list());
    }

    #[test]
    fn without_the_panel_the_list_is_the_original() {
        let panel = Panel::new();
        assert_eq!(panel.list_starts(), ListStart::Own);
        assert!(!panel.up());
        assert!(!panel.in_list());
    }

    #[test]
    fn it_is_ended_once_and_the_next_start_starts_it_again() {
        let panel = Panel::new();
        assert_eq!(panel.release(), None);
        assert!(panel.start(UI));
        panel.list_starts();
        assert_eq!(panel.release(), Some(UI));
        assert!(panel.start(UI));
        assert!(!panel.in_list());
    }

    #[test]
    fn no_ui_no_panel() {
        let panel = Panel::new();
        assert!(!panel.start(0));
        assert_eq!(panel.release(), None);
    }

    #[test]
    fn the_list_counts_its_minimum_time_from_the_panels_start() {
        // Noted at 10 s, the panel up for 4 s: the list counts from 6 s, so
        // its 3 s are over when it is built.
        assert_eq!(list_start(10_000, 4_000), 6_000);
        assert_eq!(list_start(10_000, 0), 10_000);
        // Never before the clock began.
        assert_eq!(list_start(1_000, 4_000), 0);
        assert!(list_start(10_000, 500) > list_start(10_000, 2_000));
    }

    #[test]
    fn only_the_game_list_keeps_it() {
        assert!(handed_on(LIST));
        assert!(!handed_on(TITLE));
        assert!(!handed_on(DISCONNECT));
    }
}
