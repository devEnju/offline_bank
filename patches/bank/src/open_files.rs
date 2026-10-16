//! The Bank's files between sessions (`runtime` carries this out). A session
//! ends with a job that closes them, so that nothing of them is open on the
//! start screen, where HOME works and Bank can be closed. The next session
//! begins once that job's reply is collected, and opens them as a first one
//! does.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files<J> {
    /// No job was given to the worker since they were last closed.
    Closed,
    /// A job was given to the worker since they were last closed.
    Used,
    /// The job that closes them, until its reply is collected.
    Closing(J),
}

impl<J: Copy> Files<J> {
    /// Whether to ask the worker to close them now: they were used, and no
    /// other job is out.
    pub fn to_close(&self, job_out: bool) -> bool {
        matches!(self, Self::Used) && !job_out
    }
    /// The worker's answer to the close: its job, or `None` if it refused.
    /// No other job is out, so nothing is reading or writing the files. A
    /// refused close hands nothing over: the files stay `Used`, and the next
    /// session's end asks again.
    pub fn asked(&mut self, close: Option<J>) {
        if let Some(job) = close {
            *self = Self::Closing(job);
        }
    }
    /// The close job to collect, if one is out.
    pub fn closing(&self) -> Option<J> {
        match *self {
            Self::Closing(job) => Some(job),
            _ => None,
        }
    }
    /// One collect of the close job; `out` while its reply is not there.
    /// Answers whether the next session may begin. What closing returned
    /// changes nothing: a file that could not be closed shows when the next
    /// session opens it.
    pub fn collected(&mut self, out: bool) -> bool {
        if !out {
            *self = Self::Closed;
        }
        !out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_files_are_closed_once_no_job_is_out() {
        assert!(!Files::<u32>::Closed.to_close(false));
        assert!(Files::<u32>::Used.to_close(false));
        assert!(!Files::<u32>::Used.to_close(true));
        assert!(!Files::Closing(7).to_close(false));
    }

    #[test]
    fn a_close_still_out_holds_the_next_session_back_until_its_reply() {
        let mut files = Files::Used;
        files.asked(Some(7));
        assert_eq!(files.closing(), Some(7));
        for _ in 0..3 {
            assert!(!files.collected(true));
            assert_eq!(files, Files::Closing(7));
        }
        assert!(files.collected(false));
        assert_eq!(files, Files::Closed);
        assert_eq!(files.closing(), None);
        assert!(!files.to_close(false));
    }

    #[test]
    fn a_refused_close_is_asked_again_at_the_next_end() {
        let mut files = Files::<u32>::Used;
        files.asked(None);
        assert_eq!(files, Files::Used);
        // Nothing is out to collect: the next session may begin.
        assert_eq!(files.closing(), None);
        assert!(files.to_close(false));
        files.asked(Some(8));
        assert_eq!(files, Files::Closing(8));
    }
}
