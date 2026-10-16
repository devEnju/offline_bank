//! When a job gets a worker thread of its own (`hooks::step`). The thread
//! ends on the stack the next job's thread is given, so the stack is given
//! again only after a wait on the thread's handle has shown it ended.

/// The phases of the worker, in `hooks::STATE`.
pub const IDLE: u32 = 0;
pub const RUNNING: u32 = 1;
/// The thread has stored its result; it may still be ending.
pub const DONE: u32 = 2;
/// A wait on a thread was refused, so it may still be on the stack: no
/// thread is created any more, and every job runs on the calling thread.
pub const INLINE: u32 = 3;

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

/// What a step of a job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Give the job a thread of its own; if none can be created, run it here.
    Start,
    /// Come back on the next frame.
    Pending,
    /// The thread's result is the job's: close its handle and go on in this
    /// phase.
    Collect(u32),
    /// Run the job here.
    Here,
}

/// The step in `phase`. `wait` waits on the thread's handle; it is asked
/// only once the thread has stored its result.
pub fn step(phase: u32, wait: impl FnOnce() -> i32) -> Step {
    match phase {
        IDLE => Step::Start,
        DONE => match self::wait(wait()) {
            Wait::Running => Step::Pending,
            Wait::Ended => Step::Collect(IDLE),
            Wait::Unknown => Step::Collect(INLINE),
        },
        INLINE => Step::Here,
        _ => Step::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: i32 = 0x0940_1bfe;
    const INVALID_HANDLE: i32 = 0xd8e0_07f7_u32 as i32;

    #[test]
    fn only_success_is_an_ended_thread() {
        assert_eq!(wait(0), Wait::Ended);
        assert_eq!(wait(TIMEOUT), Wait::Running);
        assert_eq!(wait(INVALID_HANDLE), Wait::Unknown);
        assert_eq!(wait(1), Wait::Unknown);
    }

    #[test]
    fn a_job_is_collected_once_its_thread_has_ended_and_the_next_gets_a_thread() {
        let never = || -> i32 { panic!("no thread to wait on") };
        assert_eq!(step(IDLE, never), Step::Start);
        assert_eq!(step(RUNNING, never), Step::Pending);
        assert_eq!(step(DONE, || TIMEOUT), Step::Pending);
        assert_eq!(step(DONE, || 0), Step::Collect(IDLE));
        assert_eq!(step(IDLE, never), Step::Start);
    }

    #[test]
    fn after_a_refused_wait_every_job_runs_here_and_no_thread_is_created() {
        assert_eq!(step(DONE, || INVALID_HANDLE), Step::Collect(INLINE));
        let mut phase = INLINE;
        for _ in 0..5 {
            let step = step(phase, || panic!("no thread to wait on"));
            assert_eq!(step, Step::Here);
            if let Step::Collect(next) = step {
                phase = next;
            }
        }
        assert_eq!(phase, INLINE);
    }
}
