//! How long a loading screen stands before its work begins, and how long it
//! stays at least (docs/internals.md, "Loading screens").
//!
//! Every loading screen first stands idle for `IDLE`: the screen slides in
//! while nothing is read, written or built behind it. Its work begins after
//! that, and a screen whose work succeeds ends no earlier than `SHOWN` after
//! it went up. An error ends it as soon as it is found. The game list of
//! Transporter keeps its own minimum of 3 s instead.

/// How long a loading screen stands before its work begins, in ms.
pub const IDLE: u32 = 500;
/// How long a loading screen whose work succeeds stays at least, in ms.
pub const SHOWN: u32 = 1500;

/// When the loading screen that is up went up, in ms of the original's
/// clock (`now`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spinner {
    state: State,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// None went up.
    #[default]
    None,
    /// The original's set-up of the next task puts one up, on a frame of
    /// its own and maybe over several (`coming`).
    Coming,
    /// One went up at this time.
    Since(u32),
}
impl Spinner {
    pub const fn new() -> Self {
        Self { state: State::None }
    }
    /// A loading screen goes up at `now`.
    pub fn up(&mut self, now: u32) {
        self.state = State::Since(now);
    }
    /// The next task's set-up puts up a loading screen: its time is taken
    /// by `seen`, on the first frame the patch runs behind it. That is a
    /// frame or a few after the screen went up, so its waits are never
    /// shorter than they are to be. Until then everything waits.
    pub fn coming(&mut self) {
        self.state = State::Coming;
    }
    /// A frame of the patch behind the loading screen: a screen that was
    /// coming went up now.
    pub fn seen(&mut self, now: u32) {
        if self.state == State::Coming {
            self.up(now);
        }
    }
    /// How long the loading screen has been up at `now`, or `None` if none
    /// went up or its time is not taken yet.
    pub fn elapsed(&self, now: u32) -> Option<u32> {
        match self.state {
            State::Since(since) => Some(now.wrapping_sub(since)),
            State::None | State::Coming => None,
        }
    }
    /// Whether its work still waits at `now`.
    pub fn idle(&self, now: u32) -> bool {
        self.below(now, IDLE)
    }
    /// Whether its end still waits at `now`, after its work succeeded.
    pub fn held(&self, now: u32) -> bool {
        self.below(now, SHOWN)
    }
    fn below(&self, now: u32, time: u32) -> bool {
        match self.state {
            State::None => false,
            State::Coming => true,
            State::Since(since) => now.wrapping_sub(since) < time,
        }
    }
}

/// The original's clock in ms, as its own minimum times read it: `NOW`
/// copies the time the main loop keeps into an 8-byte moment, and
/// `MILLISECONDS` gives a moment in ms.
#[cfg(target_arch = "arm")]
pub fn now<P: crate::program::Program>() -> u32 {
    use core::mem::transmute;
    #[repr(C, align(8))]
    struct Moment([u32; 2]);
    let mut moment = Moment([0; 2]);
    unsafe {
        let now: unsafe extern "aapcs" fn(*mut Moment) = transmute(P::CLOCK_NOW);
        let milliseconds: unsafe extern "aapcs" fn(*const Moment) -> u32 =
            transmute(P::CLOCK_MILLISECONDS);
        now(&mut moment);
        milliseconds(&moment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_waits_half_a_second_and_the_end_one_and_a_half() {
        let mut spinner = Spinner::new();
        spinner.up(10_000);
        assert!(spinner.idle(10_000));
        assert!(spinner.idle(10_499));
        assert!(!spinner.idle(10_500));
        assert!(spinner.held(10_500));
        assert!(spinner.held(11_499));
        assert!(!spinner.held(11_500));
        assert_eq!(spinner.elapsed(11_500), Some(1_500));
    }

    #[test]
    fn without_a_loading_screen_nothing_waits() {
        let spinner = Spinner::new();
        assert!(!spinner.idle(0));
        assert!(!spinner.held(0));
        assert_eq!(spinner.elapsed(0), None);
    }

    #[test]
    fn a_new_loading_screen_counts_from_its_own_start() {
        let mut spinner = Spinner::new();
        spinner.up(1_000);
        assert!(!spinner.held(5_000));
        spinner.up(5_000);
        assert!(spinner.idle(5_000));
        assert!(spinner.held(6_000));
    }

    #[test]
    fn a_screen_the_original_puts_up_counts_from_the_first_frame_behind_it() {
        let mut spinner = Spinner::new();
        spinner.up(1_000);
        spinner.coming();
        // Until its time is taken everything waits.
        assert!(spinner.idle(9_000));
        assert!(spinner.held(9_000));
        assert_eq!(spinner.elapsed(9_000), None);
        spinner.seen(9_000);
        assert!(spinner.idle(9_499));
        assert!(!spinner.idle(9_500));
        assert!(!spinner.held(10_500));
        // Later frames take no new time.
        spinner.seen(10_000);
        assert_eq!(spinner.elapsed(10_500), Some(1_500));
    }

    #[test]
    fn a_screen_the_patch_puts_up_needs_no_frame_behind_it() {
        let mut spinner = Spinner::new();
        spinner.seen(5_000);
        assert!(!spinner.idle(5_000));
        spinner.coming();
        spinner.up(6_000);
        spinner.seen(7_000);
        assert_eq!(spinner.elapsed(7_000), Some(1_000));
    }

    #[test]
    fn the_clock_may_wrap() {
        let mut spinner = Spinner::new();
        spinner.up(u32::MAX - 100);
        assert!(spinner.idle(200));
        assert!(!spinner.idle(400));
        assert_eq!(spinner.elapsed(400), Some(501));
    }
}
