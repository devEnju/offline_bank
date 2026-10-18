//! When the game scan's loading panel goes up.
//!
//! The original scan (task 3, update `002abe08`) mounts Bank's extdata in its
//! first state, on the main thread (`0029f12c`), and only then starts the
//! loading panel with message `0x20` ("Checking the Game Card...",
//! `002ac00c`, `002ac018`). The patch starts that panel itself, one frame
//! earlier, and those two calls of the original are gone (`nop`): the panel
//! is on screen before the mount, and it is started once.

/// The panel's text: message `0x20` of text file 39, as the original's.
pub const MESSAGE: u32 = 0x20;
/// The scan's first state: the mount, then (originally) the panel.
pub const PREPARE: u32 = 0;
/// Where the first state goes when the mount fails: message `0x26`, which
/// the original showed without the panel.
pub const MOUNT_FAILED: u32 = 0x19;

/// What one frame of the scan does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Start the panel and come back next frame; the original does not run.
    Show,
    /// The original's update.
    Original,
}

/// Whether the panel of this scan is up. The flag is set when the panel is
/// started and cleared when the scan has left its first state, which only a
/// new scan task enters again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Panel {
    shown: bool,
}
impl Panel {
    pub const fn new() -> Self {
        Self { shown: false }
    }
    /// The frame of a scan in `state`. `Show` marks the panel up.
    pub fn frame(&mut self, state: u32) -> Frame {
        if state == PREPARE && !self.shown {
            self.shown = true;
            Frame::Show
        } else {
            Frame::Original
        }
    }
    /// After the original's frame went from `before` to `after`: whether the
    /// panel has to end now, because the message that waits comes without it.
    pub fn ran(&mut self, before: u32, after: u32) -> bool {
        if after != PREPARE {
            self.shown = false;
        }
        before == PREPARE && after == MOUNT_FAILED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_comes_one_frame_before_the_first_state_and_only_once() {
        let mut panel = Panel::new();
        assert_eq!(panel.frame(PREPARE), Frame::Show);
        // The next frame runs the original's first state, behind the panel.
        assert_eq!(panel.frame(PREPARE), Frame::Original);
        assert!(!panel.ran(PREPARE, 1));
        for state in 1..0x26 {
            assert_eq!(panel.frame(state), Frame::Original);
            assert!(!panel.ran(state, state + 1));
        }
    }

    #[test]
    fn every_new_scan_gets_its_panel() {
        let mut panel = Panel::new();
        for _ in 0..3 {
            assert_eq!(panel.frame(PREPARE), Frame::Show);
            assert_eq!(panel.frame(PREPARE), Frame::Original);
            panel.ran(PREPARE, 1);
            panel.frame(1);
            panel.ran(1, 2);
        }
    }

    #[test]
    fn a_failed_mount_ends_the_panel_before_its_message() {
        let mut panel = Panel::new();
        assert_eq!(panel.frame(PREPARE), Frame::Show);
        assert_eq!(panel.frame(PREPARE), Frame::Original);
        assert!(panel.ran(PREPARE, MOUNT_FAILED));
        // Waiting for the message ends nothing more.
        assert!(!panel.ran(MOUNT_FAILED, MOUNT_FAILED));
    }
}
