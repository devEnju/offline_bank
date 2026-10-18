//! Bank's side of the worker (`patch_common::worker`): what a failure of a
//! job or of the worker is on Bank's error screen. Bank's SDK addresses are
//! `program::Bank15`.

use crate::fault::Fault;
use patch_common::worker::{Control, ControlKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerErrorKind {
    Operation,
    Busy,
    StaleJob,
    Resource,
    Stopped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerError {
    pub kind: WorkerErrorKind,
    fault: Fault,
    native: u32,
}
impl WorkerError {
    pub const fn fault(self) -> Fault {
        self.fault
    }
    pub const fn native_result(self) -> u32 {
        self.native
    }
    /// The two numbers of the error screen.
    pub const fn numbers(self) -> [u32; 2] {
        [self.fault as u32, self.native]
    }
    /// A job that failed (`Fault::with`).
    pub const fn operation(fault: Fault, native: u32) -> Self {
        Self {
            kind: WorkerErrorKind::Operation,
            fault,
            native,
        }
    }
    pub const fn control(kind: WorkerErrorKind) -> Self {
        Self {
            kind,
            fault: Fault::NativeObject,
            native: 0,
        }
    }
}
/// A failure of the worker itself: a native object fault with the native
/// result behind it; asking for the reply of no job is a re-entry.
impl From<Control> for WorkerError {
    fn from(control: Control) -> Self {
        let kind = match control.kind {
            ControlKind::Busy => WorkerErrorKind::Busy,
            ControlKind::StaleJob => WorkerErrorKind::StaleJob,
            ControlKind::Resource => WorkerErrorKind::Resource,
            ControlKind::Stopped => WorkerErrorKind::Stopped,
            ControlKind::NoJob => return Self::operation(Fault::Reentrant, 0),
        };
        Self {
            kind,
            fault: Fault::NativeObject,
            native: control.native,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_failures_are_the_numbers_they_were() {
        assert_eq!(
            WorkerError::from(Control::new(ControlKind::Busy)),
            WorkerError::control(WorkerErrorKind::Busy)
        );
        assert_eq!(
            WorkerError::from(Control::new(ControlKind::Stopped)).numbers(),
            [Fault::NativeObject as u32, 0]
        );
        let resource = WorkerError::from(Control {
            kind: ControlKind::Resource,
            native: 0xd860_0c06,
        });
        assert_eq!(resource.kind, WorkerErrorKind::Resource);
        assert_eq!(
            resource.numbers(),
            [Fault::NativeObject as u32, 0xd860_0c06]
        );
        assert_eq!(
            WorkerError::from(Control::new(ControlKind::NoJob)),
            WorkerError::operation(Fault::Reentrant, 0)
        );
    }
}
