//! Single-owner native worker and a release/acquire command mailbox.
//!
//! The main thread never waits for I/O or joins an active worker. Native SDK
//! 002320d4 enters through001211e4, which initializes native TLS before Rust.
//! The worker owns storage; the UI owns native game objects and all rendering.

pub use crate::storage_worker::{GameEvidence, Job, Reply, WorkerError, WorkerErrorKind};
#[cfg(any(test, target_arch = "arm"))]
use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU32, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobId(u32);

#[cfg(any(test, target_arch = "arm"))]
const IDLE: u32 = 0;
#[cfg(any(test, target_arch = "arm"))]
const WRITING: u32 = 1;
#[cfg(any(test, target_arch = "arm"))]
const QUEUED: u32 = 2;
#[cfg(any(test, target_arch = "arm"))]
const RUNNING: u32 = 3;
#[cfg(any(test, target_arch = "arm"))]
const PUBLISHING: u32 = 4;
#[cfg(any(test, target_arch = "arm"))]
const DONE: u32 = 5;
#[cfg(any(test, target_arch = "arm"))]
const CONSUMING: u32 = 6;

#[cfg(any(test, target_arch = "arm"))]
struct Mailbox<T: Copy, R: Copy> {
    state: AtomicU32,
    sequence: AtomicU32,
    cancel: AtomicU32,
    command: UnsafeCell<MaybeUninit<T>>,
    result: UnsafeCell<MaybeUninit<R>>,
}
// SAFETY: all accesses to command/result are guarded by unique atomic state
// transitions. Release publication and acquire consumption cover their bytes.
#[cfg(any(test, target_arch = "arm"))]
unsafe impl<T: Copy + Send, R: Copy + Send> Sync for Mailbox<T, R> {}
#[cfg(any(test, target_arch = "arm"))]
impl<T: Copy, R: Copy> Mailbox<T, R> {
    const fn new() -> Self {
        Self {
            state: AtomicU32::new(IDLE),
            sequence: AtomicU32::new(0),
            cancel: AtomicU32::new(0),
            command: UnsafeCell::new(MaybeUninit::uninit()),
            result: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
    fn idle(&self) -> bool {
        self.state.load(Ordering::Acquire) == IDLE
    }
    fn submit(&self, command: T) -> Result<JobId, WorkerError> {
        self.state
            .compare_exchange(IDLE, WRITING, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| WorkerError::control(WorkerErrorKind::Busy))?;
        let Some(next) = self.sequence.load(Ordering::Relaxed).checked_add(1) else {
            self.state.store(IDLE, Ordering::Release);
            return Err(WorkerError::control(WorkerErrorKind::StaleJob));
        };
        self.cancel.store(0, Ordering::Relaxed);
        self.sequence.store(next, Ordering::Relaxed);
        unsafe {
            (*self.command.get()).write(command);
        }
        self.state.store(QUEUED, Ordering::Release);
        Ok(JobId(next))
    }
    fn take(&self) -> Option<(JobId, T)> {
        self.state
            .compare_exchange(QUEUED, RUNNING, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        let id = JobId(self.sequence.load(Ordering::Relaxed));
        Some((id, unsafe { (*self.command.get()).assume_init_read() }))
    }
    fn finish(&self, id: JobId, result: R) -> Result<(), WorkerError> {
        if self.sequence.load(Ordering::Acquire) != id.0 {
            return Err(WorkerError::control(WorkerErrorKind::StaleJob));
        }
        self.state
            .compare_exchange(RUNNING, PUBLISHING, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| WorkerError::control(WorkerErrorKind::Busy))?;
        if self.sequence.load(Ordering::Relaxed) != id.0 {
            self.state.store(RUNNING, Ordering::Release);
            return Err(WorkerError::control(WorkerErrorKind::StaleJob));
        }
        unsafe {
            (*self.result.get()).write(result);
        }
        self.state.store(DONE, Ordering::Release);
        Ok(())
    }
    fn poll(&self, id: JobId) -> Result<Option<R>, WorkerError> {
        if self.sequence.load(Ordering::Acquire) != id.0 {
            return Err(WorkerError::control(WorkerErrorKind::StaleJob));
        }
        match self
            .state
            .compare_exchange(DONE, CONSUMING, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => {
                if self.sequence.load(Ordering::Relaxed) != id.0 {
                    self.state.store(DONE, Ordering::Release);
                    return Err(WorkerError::control(WorkerErrorKind::StaleJob));
                }
                let result = unsafe { (*self.result.get()).assume_init_read() };
                self.state.store(IDLE, Ordering::Release);
                Ok(Some(result))
            }
            Err(IDLE | WRITING) => Err(WorkerError::control(WorkerErrorKind::StaleJob)),
            Err(_) => Ok(None),
        }
    }
    fn request_cancel(&self, id: JobId) -> Result<(), WorkerError> {
        if self.sequence.load(Ordering::Acquire) != id.0 {
            return Err(WorkerError::control(WorkerErrorKind::StaleJob));
        }
        if !matches!(self.state.load(Ordering::Acquire), QUEUED | RUNNING) {
            return Err(WorkerError::control(WorkerErrorKind::Busy));
        }
        self.cancel.store(id.0, Ordering::Release);
        Ok(())
    }
    fn cancelled(&self, id: JobId) -> bool {
        self.cancel.load(Ordering::Acquire) == id.0
    }
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::storage_worker::StorageWorker;
    use core::{marker::PhantomData, mem::transmute, sync::atomic::AtomicBool};
    use offline_core::native_blob::BLOB_SIZE;

    const STACK_SIZE: usize = 0x8000;
    const GUARD_SIZE: usize = 64;
    const GUARD_BYTE: u8 = 0xa7;
    #[repr(C, align(8))]
    struct Stack([u8; STACK_SIZE]);
    #[repr(C)]
    struct Thread {
        handle: u32,
        joined: u8,
        detached: u8,
        padding: [u8; 2],
    }
    #[repr(C, align(4))]
    struct Event([u32; 2]);
    struct Shared {
        mailbox: Mailbox<Job, Result<Reply, WorkerError>>,
        staging: UnsafeCell<[u8; BLOB_SIZE]>,
        stack: UnsafeCell<Stack>,
        event: UnsafeCell<Event>,
        thread: UnsafeCell<Thread>,
        claimed: AtomicBool,
    }
    // SAFETY: staging ownership moves with the mailbox state. The singleton
    // main-thread BankWorker is the only producer. The SDK thread alone owns
    // StorageWorker and accesses staging only while its job is RUNNING.
    unsafe impl Sync for Shared {}
    static SHARED: Shared = Shared {
        mailbox: Mailbox::new(),
        staging: UnsafeCell::new([0; BLOB_SIZE]),
        stack: UnsafeCell::new(Stack([0; STACK_SIZE])),
        event: UnsafeCell::new(Event([0; 2])),
        thread: UnsafeCell::new(Thread {
            handle: 0,
            joined: 0,
            detached: 0,
            padding: [0; 2],
        }),
        claimed: AtomicBool::new(false),
    };

    /// Main-thread owner of the one worker. Drop deliberately never joins or
    /// frees an active native thread; process lifetime owns its static memory.
    pub struct BankWorker {
        current: Option<JobId>,
        cancellable: bool,
        closing: bool,
        closed: bool,
        thread: PhantomData<*mut ()>,
    }
    impl BankWorker {
        /// # Safety
        /// Requires the exact reviewed executable after native OS/FS setup.
        /// Call once from the UI thread. Keep the initialized native FS service
        /// and application manager alive until worker Close has completed or
        /// process exit. An idle worker touches only its private event and does
        /// not access native owners while the application tears down.
        pub unsafe fn start() -> Result<Self, WorkerError> {
            SHARED
                .claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .map_err(|_| WorkerError::control(WorkerErrorKind::Busy))?;
            let result = unsafe { start_thread() };
            if let Err(error) = result {
                // No worker exists after a failed Result from SDK create.
                if error.kind != WorkerErrorKind::Stopped {
                    SHARED.claimed.store(false, Ordering::Release);
                }
                return Err(error);
            }
            Ok(Self {
                current: None,
                cancellable: false,
                closing: false,
                closed: false,
                thread: PhantomData,
            })
        }
        /// # Safety
        /// Keep every borrowed game archive and frozen prepared block alive and
        /// immutable until poll returns a terminal result. Protect the owning
        /// native task against teardown/unmount. No native writer may overlap
        /// worker access to that game's main file. Staged bytes are exclusively
        /// transferred to the worker until completion is collected.
        pub unsafe fn submit(&mut self, job: Job) -> Result<JobId, WorkerError> {
            if self.closing || self.closed {
                return Err(WorkerError::control(WorkerErrorKind::Stopped));
            }
            if self.current.is_some() {
                return Err(WorkerError::control(WorkerErrorKind::Busy));
            }
            if thread_exited()? {
                return Err(WorkerError::control(WorkerErrorKind::Stopped));
            }
            let id = SHARED.mailbox.submit(job)?;
            self.current = Some(id);
            self.cancellable = job.cancellable();
            self.closing = matches!(job, Job::Close);
            signal();
            Ok(id)
        }
        pub fn poll(&mut self, id: JobId) -> Result<Option<Reply>, WorkerError> {
            if self.current != Some(id) {
                return Err(WorkerError::control(WorkerErrorKind::StaleJob));
            }
            if let Some(result) = SHARED.mailbox.poll(id)? {
                self.current = None;
                if !stack_guard_valid() {
                    return Err(WorkerError::control(WorkerErrorKind::Resource));
                }
                if self.closing {
                    // poll_closed separately waits for the SDK epilogue to exit.
                    self.closed = true;
                }
                return result.map(Some);
            }
            if thread_exited()? {
                // Completion can race the first mailbox check. Collect it if
                // the worker published immediately before SDK thread exit.
                if let Some(result) = SHARED.mailbox.poll(id)? {
                    self.current = None;
                    self.closed = true;
                    return result.map(Some);
                }
                // Only confirmed kernel termination releases borrowed native
                // owners. Leave a nonterminal mailbox poisoned/inaccessible.
                self.current = None;
                self.closed = true;
                return Err(WorkerError::control(WorkerErrorKind::Stopped));
            }
            Ok(None)
        }
        /// Cooperative cancellation only discards read-only results. A running
        /// IPC completes normally. Mutation jobs always finish their protocol.
        pub fn cancel(&mut self, id: JobId) -> Result<bool, WorkerError> {
            if self.current != Some(id) {
                return Err(WorkerError::control(WorkerErrorKind::StaleJob));
            }
            if !self.cancellable {
                return Ok(false);
            }
            SHARED.mailbox.request_cancel(id)?;
            Ok(true)
        }
        /// True until a terminal result has been collected. Errors such as a
        /// stale job ID never authorize releasing borrowed native owners.
        pub fn active(&self) -> bool {
            self.current.is_some()
        }
        pub fn staging(&self) -> Result<&[u8], WorkerError> {
            self.ensure_idle()?;
            Ok(unsafe { &*SHARED.staging.get() })
        }
        pub fn staging_mut(&mut self) -> Result<&mut [u8], WorkerError> {
            self.ensure_idle()?;
            Ok(unsafe { &mut *SHARED.staging.get() })
        }
        fn ensure_idle(&self) -> Result<(), WorkerError> {
            if self.current.is_some() || !SHARED.mailbox.idle() {
                return Err(WorkerError::control(WorkerErrorKind::Busy));
            }
            Ok(())
        }
        /// Checks room for the retained native game-writer thread immediately
        /// before launching it. Does not raise resource limits or block on I/O.
        pub fn ensure_writer_capacity(&self) -> Result<(), WorkerError> {
            thread_capacity(1)
        }
        /// Nonblocking cleanup after Close. Returns false until SDK exit; the
        /// static stack is never recycled while a thread could still use it.
        pub fn poll_closed(&mut self) -> Result<bool, WorkerError> {
            if !self.closed {
                return Ok(false);
            }
            if !thread_exited()? {
                return Ok(false);
            }
            let thread = unsafe { &mut *SHARED.thread.get() };
            if thread.handle != 0 {
                close_handle(thread.handle)?;
                thread.handle = 0;
            }
            Ok(true)
        }
    }
    unsafe extern "aapcs" fn worker_entry(_: *mut u8) {
        let mut storage = StorageWorker::new();
        loop {
            if let Some((id, job)) = SHARED.mailbox.take() {
                let cancelled = job.cancellable() && SHARED.mailbox.cancelled(id);
                let mut result = if cancelled {
                    Err(WorkerError::control(WorkerErrorKind::Cancelled))
                } else {
                    storage.run(job, unsafe { &mut *SHARED.staging.get() })
                };
                if job.cancellable() && SHARED.mailbox.cancelled(id) {
                    result = Err(WorkerError::control(WorkerErrorKind::Cancelled));
                }
                let closing = matches!(job, Job::Close);
                if SHARED.mailbox.finish(id, result).is_err() {
                    return;
                }
                if closing {
                    return;
                }
            } else {
                wait();
            }
        }
    }
    fn signal() {
        let signal: unsafe extern "aapcs" fn(*mut Event) = unsafe { transmute(0x00234a00usize) };
        unsafe {
            signal(SHARED.event.get());
        }
    }
    fn wait() {
        let wait: unsafe extern "aapcs" fn(*mut Event) = unsafe { transmute(0x00231e70usize) };
        unsafe {
            wait(SHARED.event.get());
        }
    }
    unsafe fn start_thread() -> Result<(), WorkerError> {
        thread_capacity(2)?; // this worker plus the retained native game writer
        let stack = SHARED.stack.get().cast::<u8>();
        unsafe {
            core::ptr::write_bytes(stack, GUARD_BYTE, GUARD_SIZE);
        }
        let init: unsafe extern "aapcs" fn(*mut Event, u32) -> u32 =
            unsafe { transmute(0x00235c6cusize) };
        unsafe {
            init(SHARED.event.get(), 0);
        }
        // Native delegate: copies one context pointer onto the supplied stack,
        // invokes entry(context), and has a no-op destructor. SDK001211e4 owns
        // TLS initialization and ExitThread; last argument0 retains our stack.
        let descriptor = [4u32, 0x0012117c, 0x00121198, 0x0012118c];
        let context: *mut u8 = core::ptr::null_mut();
        type Create = unsafe extern "aapcs" fn(
            *mut Thread,
            *const u32,
            usize,
            *const *mut u8,
            *mut u8,
            u32,
            i32,
            u32,
        ) -> i32;
        let create: Create = unsafe { transmute(0x002320d4usize) };
        let code = unsafe {
            create(
                SHARED.thread.get(),
                descriptor.as_ptr(),
                worker_entry as *const () as usize,
                &context,
                stack.add(STACK_SIZE),
                31,
                -2,
                0,
            )
        };
        if code < 0 {
            return Err(WorkerError::resource(code as u32));
        }
        // SDK success must supply a real handle. A missing handle cannot safely
        // authorize another creation attempt or any reuse of the static stack.
        if unsafe { (*SHARED.thread.get()).handle } == 0 {
            return Err(WorkerError::control(WorkerErrorKind::Stopped));
        }
        Ok(())
    }
    fn stack_guard_valid() -> bool {
        let bottom = SHARED.stack.get().cast::<u8>();
        (0..GUARD_SIZE).all(|i| unsafe { bottom.add(i).read_volatile() } == GUARD_BYTE)
    }
    fn thread_exited() -> Result<bool, WorkerError> {
        let handle = unsafe { (*SHARED.thread.get()).handle };
        if handle == 0 {
            return Ok(true);
        }
        // An ordinary AAPCS call makes every syscall scratch register a
        // compiler-visible clobber. Timeout zero keeps UI polling nonblocking.
        let result = unsafe { crate::kernel::wait_thread(handle, 0) };
        if result == 0 {
            Ok(true)
        } else if result as u32 & 0x3ff == 0x3fe {
            Ok(false)
        } else {
            Err(WorkerError::resource(result as u32))
        }
    }
    fn close_handle(handle: u32) -> Result<(), WorkerError> {
        let result = unsafe { crate::kernel::close_handle(handle) };
        if result < 0 {
            Err(WorkerError::resource(result as u32))
        } else {
            Ok(())
        }
    }
    fn thread_capacity(additional: i64) -> Result<(), WorkerError> {
        crate::kernel::check_thread_capacity(additional).map_err(|error| match error {
            crate::kernel::CapacityError::Native(code) => WorkerError::resource(code),
            crate::kernel::CapacityError::Unavailable => {
                WorkerError::control(WorkerErrorKind::Resource)
            }
        })
    }
}
#[cfg(target_arch = "arm")]
pub use arm::BankWorker;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    #[test]
    fn staging_remains_owned_until_completion_is_consumed() {
        let mailbox = Mailbox::<u32, u32>::new();
        let id = mailbox.submit(7).unwrap();
        assert!(!mailbox.idle());
        assert_eq!(mailbox.poll(id), Ok(None));
        assert!(mailbox.submit(9).is_err());
        assert_eq!(mailbox.take(), Some((id, 7)));
        assert!(mailbox.take().is_none());
        mailbox.finish(id, 14).unwrap();
        assert!(!mailbox.idle());
        assert!(mailbox.submit(9).is_err());
        assert_eq!(mailbox.poll(id), Ok(Some(14)));
        assert!(mailbox.idle());
        assert!(mailbox.poll(id).is_err());
    }
    #[test]
    fn stale_job_and_cancel_cannot_affect_next_request() {
        let mailbox = Mailbox::<u32, u32>::new();
        let first = mailbox.submit(1).unwrap();
        mailbox.request_cancel(first).unwrap();
        assert!(mailbox.cancelled(first));
        mailbox.take().unwrap();
        mailbox.finish(first, 1).unwrap();
        mailbox.poll(first).unwrap();
        let next = mailbox.submit(2).unwrap();
        assert_ne!(first, next);
        assert!(!mailbox.cancelled(next));
        assert!(mailbox.request_cancel(first).is_err());
        assert!(mailbox.finish(first, 99).is_err());
        assert!(mailbox.poll(first).is_err());
        assert_eq!(mailbox.take(), Some((next, 2)));
    }
    #[test]
    fn slow_worker_does_not_block_producer_polling() {
        let mailbox = Arc::new(Mailbox::<u32, u32>::new());
        let ready = Arc::new(Barrier::new(2));
        let finish = Arc::new(Barrier::new(2));
        let id = mailbox.submit(42).unwrap();
        let worker = {
            let (mailbox, ready, finish) = (mailbox.clone(), ready.clone(), finish.clone());
            thread::spawn(move || {
                let (id, request) = mailbox.take().unwrap();
                ready.wait();
                finish.wait();
                mailbox.finish(id, request + 1).unwrap();
            })
        };
        ready.wait();
        for _ in 0..10_000 {
            assert_eq!(mailbox.poll(id), Ok(None));
        }
        mailbox.request_cancel(id).unwrap();
        assert!(mailbox.cancelled(id));
        finish.wait();
        worker.join().unwrap();
        assert_eq!(mailbox.poll(id), Ok(Some(43)));
    }
    #[test]
    fn release_publication_preserves_full_results_across_many_jobs() {
        let mailbox = Arc::new(Mailbox::<u32, [u32; 32]>::new());
        let worker = {
            let mailbox = mailbox.clone();
            thread::spawn(move || {
                for expected in 1..=2048u32 {
                    let (id, request) = loop {
                        if let Some(work) = mailbox.take() {
                            break work;
                        }
                        thread::yield_now();
                    };
                    assert_eq!(request, expected);
                    mailbox
                        .finish(id, core::array::from_fn(|i| request ^ i as u32))
                        .unwrap();
                }
            })
        };
        for sequence in 1..=2048u32 {
            let id = mailbox.submit(sequence).unwrap();
            let reply = loop {
                if let Some(reply) = mailbox.poll(id).unwrap() {
                    break reply;
                }
                thread::yield_now();
            };
            assert_eq!(reply, core::array::from_fn(|i| sequence ^ i as u32));
        }
        worker.join().unwrap();
    }
    #[test]
    fn mutation_commands_do_not_accept_cooperative_discard() {
        assert!(!Job::Initialize.cancellable());
        assert!(!Job::Close.cancellable());
        assert!(Job::Open { session: 1 }.cancellable());
    }
    #[test]
    fn sequence_exhaustion_never_reuses_an_old_id() {
        let mailbox = Mailbox::<u32, u32>::new();
        mailbox.sequence.store(u32::MAX, Ordering::Relaxed);
        assert!(mailbox.submit(1).is_err());
        assert!(mailbox.idle());
    }
}
