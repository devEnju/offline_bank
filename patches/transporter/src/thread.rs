//! What a wait on the worker thread's handle says (`hooks::step`). Only an
//! ended thread has left the stack the next job's thread is given.

/// The answer of `svcWaitSynchronization1` with timeout 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// The thread has ended.
    Ended,
    /// It is still running: ask again on the next frame.
    Running,
    /// The kernel refused the wait. Whether the thread has ended is not
    /// known, and asking again tells nothing more.
    Unknown,
}

pub fn wait(result: i32) -> Wait {
    if result == 0 {
        Wait::Ended
    } else if result as u32 & 0x3ff == 0x3fe {
        Wait::Running
    } else {
        Wait::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_success_is_an_ended_thread() {
        assert_eq!(wait(0), Wait::Ended);
        // Timeout: 0x09401BFE.
        assert_eq!(wait(0x0940_1bfe), Wait::Running);
        // An invalid handle: 0xD8E007F7.
        assert_eq!(wait(0xd8e0_07f7_u32 as i32), Wait::Unknown);
        assert_eq!(wait(1), Wait::Unknown);
    }
}
