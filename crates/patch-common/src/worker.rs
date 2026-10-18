//! Single-owner native worker and a release/acquire command mailbox.
//!
//! The main thread never waits for I/O or joins an active worker. The thread
//! is created through the program's SDK (`Program::CREATE_THREAD`), whose
//! trampoline initializes native TLS before Rust runs. The worker owns
//! storage; the UI owns native game objects and all rendering.
//!
//! What the worker does is a patch's own: its `Service` names the jobs, runs
//! them, and says what a failure is. The thread, the mailbox and the
//! handing over of the staging buffer are the same for every patch of either
//! program.

#[cfg(any(test, target_arch = "arm"))]
use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU32, Ordering},
};

// SDK CreateThread adds 0x20 to priorities in 0..=0x20 before SVC 08, in
// both programs. Keep the desired kernel priority separate from the SDK
// argument.
#[cfg(any(test, target_arch = "arm"))]
const WORKER_KERNEL_PRIORITY: u32 = 0x31;
#[cfg(any(test, target_arch = "arm"))]
const WORKER_SDK_PRIORITY: u32 = WORKER_KERNEL_PRIORITY - 0x20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobId(u32);

/// What went wrong with the worker itself, not with a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// A job is out already, or the mailbox is not free.
    Busy,
    /// The job asked for is not the one out.
    StaleJob,
    /// No room for the thread, a native failure, or a broken stack guard.
    Resource,
    /// The thread is gone, or never was.
    Stopped,
    /// Asked for the reply of no job.
    NoJob,
}
/// A worker failure: its kind and the native result behind it, 0 if none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub kind: ControlKind,
    pub native: u32,
}
impl Control {
    pub const fn new(kind: ControlKind) -> Self {
        Self { kind, native: 0 }
    }
    #[cfg(target_arch = "arm")]
    const fn resource(native: u32) -> Self {
        Self {
            kind: ControlKind::Resource,
            native,
        }
    }
}

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
            command: UnsafeCell::new(MaybeUninit::uninit()),
            result: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
    fn idle(&self) -> bool {
        self.state.load(Ordering::Acquire) == IDLE
    }
    fn submit(&self, command: T) -> Result<JobId, Control> {
        self.state
            .compare_exchange(IDLE, WRITING, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| Control::new(ControlKind::Busy))?;
        let Some(next) = self.sequence.load(Ordering::Relaxed).checked_add(1) else {
            self.state.store(IDLE, Ordering::Release);
            return Err(Control::new(ControlKind::StaleJob));
        };
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
    fn finish(&self, id: JobId, result: R) -> Result<(), Control> {
        if self.sequence.load(Ordering::Acquire) != id.0 {
            return Err(Control::new(ControlKind::StaleJob));
        }
        self.state
            .compare_exchange(RUNNING, PUBLISHING, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| Control::new(ControlKind::Busy))?;
        if self.sequence.load(Ordering::Relaxed) != id.0 {
            self.state.store(RUNNING, Ordering::Release);
            return Err(Control::new(ControlKind::StaleJob));
        }
        unsafe {
            (*self.result.get()).write(result);
        }
        self.state.store(DONE, Ordering::Release);
        Ok(())
    }
    fn poll(&self, id: JobId) -> Result<Option<R>, Control> {
        if self.sequence.load(Ordering::Acquire) != id.0 {
            return Err(Control::new(ControlKind::StaleJob));
        }
        match self
            .state
            .compare_exchange(DONE, CONSUMING, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => {
                if self.sequence.load(Ordering::Relaxed) != id.0 {
                    self.state.store(DONE, Ordering::Release);
                    return Err(Control::new(ControlKind::StaleJob));
                }
                let result = unsafe { (*self.result.get()).assume_init_read() };
                self.state.store(IDLE, Ordering::Release);
                Ok(Some(result))
            }
            Err(IDLE | WRITING) => Err(Control::new(ControlKind::StaleJob)),
            Err(_) => Ok(None),
        }
    }
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use crate::program::Program;
    use core::{marker::PhantomData, mem::transmute, sync::atomic::AtomicBool};

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

    /// What one patch runs on the worker thread.
    pub trait Service: Sized + 'static {
        /// The program the patch is for.
        type Program: Program;
        type Job: Copy;
        type Reply: Copy;
        /// A failed job, and what a failure of the worker itself becomes.
        type Error: Copy + From<Control>;
        /// The buffer the main thread hands over with a job.
        type Staging: AsRef<[u8]> + AsMut<[u8]> + 'static;
        /// Threads the process must have room for when the worker starts:
        /// the worker itself and any the patch starts while it exists.
        const THREADS: i64;
        /// The statics of this payload's one worker.
        fn shared() -> &'static Shared<Self>;
        /// The state the worker thread owns, made on that thread.
        fn start() -> Self;
        /// Runs one job to its end; nothing stops a job half way.
        fn run(&mut self, job: Self::Job, staging: &mut [u8]) -> Result<Self::Reply, Self::Error>;
    }

    /// The mailbox, staging buffer and thread of a payload's one worker. A
    /// patch holds it in a static and names it in `Service::shared`.
    pub struct Shared<S: Service> {
        mailbox: Mailbox<S::Job, Result<S::Reply, S::Error>>,
        staging: UnsafeCell<S::Staging>,
        stack: UnsafeCell<Stack>,
        event: UnsafeCell<Event>,
        thread: UnsafeCell<Thread>,
        claimed: AtomicBool,
    }
    // SAFETY: staging ownership moves with the mailbox state. The singleton
    // main-thread Worker is the only producer. The SDK thread alone owns the
    // Service and accesses staging only while its job is RUNNING.
    unsafe impl<S: Service> Sync for Shared<S> {}
    impl<S: Service> Shared<S> {
        /// The statics of a worker whose staging buffer starts as `staging`.
        pub const fn new(staging: S::Staging) -> Self {
            Self {
                mailbox: Mailbox::new(),
                staging: UnsafeCell::new(staging),
                stack: UnsafeCell::new(Stack([0; STACK_SIZE])),
                event: UnsafeCell::new(Event([0; 2])),
                thread: UnsafeCell::new(Thread {
                    handle: 0,
                    joined: 0,
                    detached: 0,
                    padding: [0; 2],
                }),
                claimed: AtomicBool::new(false),
            }
        }
    }

    /// Main-thread owner of the one worker. The thread is started once and
    /// waits for jobs until the process ends; no job ends it, and Drop never
    /// joins or frees it. Process lifetime owns its static memory.
    pub struct Worker<S: Service> {
        current: Option<JobId>,
        thread: PhantomData<*mut ()>,
        service: PhantomData<S>,
    }
    impl<S: Service> Worker<S> {
        /// # Safety
        /// Requires the exact reviewed executable after native OS/FS setup.
        /// Call once from the UI thread. Keep the initialized native FS service
        /// and application manager alive until process exit. An idle worker
        /// touches only its private event and does
        /// not access native owners while the application tears down.
        pub unsafe fn start() -> Result<Self, S::Error> {
            S::shared()
                .claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .map_err(|_| Control::new(ControlKind::Busy))?;
            let result = unsafe { start_thread::<S>() };
            if let Err(error) = result {
                // No worker exists after a failed Result from SDK create.
                if error.kind != ControlKind::Stopped {
                    S::shared().claimed.store(false, Ordering::Release);
                }
                return Err(error.into());
            }
            Ok(Self {
                current: None,
                thread: PhantomData,
                service: PhantomData,
            })
        }
        /// The worker in `slot`, started on first use.
        /// # Safety
        /// As `start`.
        pub unsafe fn get(slot: &mut Option<Self>) -> Result<&mut Self, S::Error> {
            if slot.is_none() {
                *slot = Some(unsafe { Self::start() }?);
            }
            slot.as_mut()
                .ok_or(Control::new(ControlKind::Stopped).into())
        }
        /// # Safety
        /// Keep everything the job borrows from the original alive and
        /// immutable until poll returns a terminal result, and protect the
        /// owning native task against teardown. Staged bytes are exclusively
        /// transferred to the worker until completion is collected.
        pub unsafe fn submit(&mut self, job: S::Job) -> Result<JobId, S::Error> {
            if self.current.is_some() {
                return Err(Control::new(ControlKind::Busy).into());
            }
            if thread_exited::<S>()? {
                return Err(Control::new(ControlKind::Stopped).into());
            }
            let id = S::shared().mailbox.submit(job)?;
            self.current = Some(id);
            signal::<S>();
            Ok(id)
        }
        pub fn poll(&mut self, id: JobId) -> Result<Option<S::Reply>, S::Error> {
            if self.current != Some(id) {
                return Err(Control::new(ControlKind::StaleJob).into());
            }
            if let Some(result) = S::shared().mailbox.poll(id)? {
                self.current = None;
                if !stack_guard_valid::<S>() {
                    return Err(Control::new(ControlKind::Resource).into());
                }
                return result.map(Some);
            }
            if thread_exited::<S>()? {
                // The thread is gone, which no job asks for. Collect a result
                // it published immediately before that.
                if let Some(result) = S::shared().mailbox.poll(id)? {
                    self.current = None;
                    return result.map(Some);
                }
                // Only confirmed kernel termination releases borrowed native
                // owners. Leave a nonterminal mailbox poisoned/inaccessible.
                self.current = None;
                return Err(Control::new(ControlKind::Stopped).into());
            }
            Ok(None)
        }
        /// One frame of the job in `job`: its reply once it is there, which
        /// clears `job`. A poll that is uncertain while the job is still out
        /// never ends it: everything the job borrowed stays lent, and the
        /// next frame asks again.
        pub fn collect(&mut self, job: &mut Option<JobId>) -> Result<Option<S::Reply>, S::Error> {
            let id = job.ok_or(Control::new(ControlKind::NoJob))?;
            match self.poll(id) {
                Ok(None) => Ok(None),
                Err(_) if self.active() => Ok(None),
                result => {
                    *job = None;
                    result
                }
            }
        }
        /// True until a terminal result has been collected. Errors such as a
        /// stale job ID never authorize releasing borrowed native owners.
        pub fn active(&self) -> bool {
            self.current.is_some()
        }
        pub fn staging(&self) -> Result<&[u8], S::Error> {
            self.ensure_idle()?;
            Ok(unsafe { (*S::shared().staging.get()).as_ref() })
        }
        pub fn staging_mut(&mut self) -> Result<&mut [u8], S::Error> {
            self.ensure_idle()?;
            Ok(unsafe { (*S::shared().staging.get()).as_mut() })
        }
        fn ensure_idle(&self) -> Result<(), Control> {
            if self.current.is_some() || !S::shared().mailbox.idle() {
                return Err(Control::new(ControlKind::Busy));
            }
            Ok(())
        }
        /// Checks room for `additional` threads immediately before the patch
        /// starts them. Does not raise resource limits or block on I/O.
        pub fn ensure_thread_capacity(&self, additional: i64) -> Result<(), S::Error> {
            Ok(thread_capacity(additional)?)
        }
    }
    unsafe extern "aapcs" fn worker_entry<S: Service>(_: *mut u8) {
        let shared = S::shared();
        let mut service = S::start();
        loop {
            if let Some((id, job)) = shared.mailbox.take() {
                let result = service.run(job, unsafe { (*shared.staging.get()).as_mut() });
                if shared.mailbox.finish(id, result).is_err() {
                    return;
                }
            } else {
                wait::<S>();
            }
        }
    }
    fn signal<S: Service>() {
        let signal: unsafe extern "aapcs" fn(*mut Event) =
            unsafe { transmute(<S::Program as Program>::EVENT_SIGNAL) };
        unsafe {
            signal(S::shared().event.get());
        }
    }
    fn wait<S: Service>() {
        let wait: unsafe extern "aapcs" fn(*mut Event) =
            unsafe { transmute(<S::Program as Program>::EVENT_WAIT) };
        unsafe {
            wait(S::shared().event.get());
        }
    }
    unsafe fn start_thread<S: Service>() -> Result<(), Control> {
        thread_capacity(S::THREADS)?;
        let shared = S::shared();
        let stack = shared.stack.get().cast::<u8>();
        unsafe {
            core::ptr::write_bytes(stack, GUARD_BYTE, GUARD_SIZE);
        }
        let init: unsafe extern "aapcs" fn(*mut Event, u32) -> u32 =
            unsafe { transmute(<S::Program as Program>::EVENT_INIT) };
        unsafe {
            init(shared.event.get(), 0);
        }
        // Native delegate: copies one context pointer onto the supplied stack,
        // invokes entry(context), and has a no-op destructor. The SDK's
        // trampoline owns TLS initialization and ExitThread; last argument 0
        // retains our stack.
        let delegate = <S::Program as Program>::DELEGATE;
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
        let create: Create = unsafe { transmute(<S::Program as Program>::CREATE_THREAD) };
        let entry: unsafe extern "aapcs" fn(*mut u8) = worker_entry::<S>;
        let code = unsafe {
            create(
                shared.thread.get(),
                delegate.as_ptr(),
                entry as *const () as usize,
                &context,
                stack.add(STACK_SIZE),
                WORKER_SDK_PRIORITY,
                -2,
                0,
            )
        };
        if code < 0 {
            return Err(Control::resource(code as u32));
        }
        // SDK success must supply a real handle. A missing handle cannot safely
        // authorize another creation attempt or any reuse of the static stack.
        if unsafe { (*shared.thread.get()).handle } == 0 {
            return Err(Control::new(ControlKind::Stopped));
        }
        Ok(())
    }
    fn stack_guard_valid<S: Service>() -> bool {
        let bottom = S::shared().stack.get().cast::<u8>();
        (0..GUARD_SIZE).all(|i| unsafe { bottom.add(i).read_volatile() } == GUARD_BYTE)
    }
    fn thread_exited<S: Service>() -> Result<bool, Control> {
        let handle = unsafe { (*S::shared().thread.get()).handle };
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
            Err(Control::resource(result as u32))
        }
    }
    fn thread_capacity(additional: i64) -> Result<(), Control> {
        crate::kernel::check_thread_capacity(additional).map_err(|error| match error {
            crate::kernel::CapacityError::Native(code) => Control::resource(code),
            crate::kernel::CapacityError::Unavailable => Control::new(ControlKind::Resource),
        })
    }
}
#[cfg(target_arch = "arm")]
pub use arm::{Service, Shared, Worker};

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn the_sdk_argument_maps_to_a_kernel_priority_below_the_main_thread() {
        assert_eq!(WORKER_SDK_PRIORITY, 0x11);
        assert!((0..=0x1f).contains(&WORKER_SDK_PRIORITY));
        assert_eq!(WORKER_SDK_PRIORITY + 0x20, WORKER_KERNEL_PRIORITY);
        assert_eq!(WORKER_KERNEL_PRIORITY, 0x30 + 1);
    }
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
    fn a_stale_job_cannot_affect_the_next_request() {
        let mailbox = Mailbox::<u32, u32>::new();
        let first = mailbox.submit(1).unwrap();
        mailbox.take().unwrap();
        mailbox.finish(first, 1).unwrap();
        mailbox.poll(first).unwrap();
        let next = mailbox.submit(2).unwrap();
        assert_ne!(first, next);
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
    fn sequence_exhaustion_never_reuses_an_old_id() {
        let mailbox = Mailbox::<u32, u32>::new();
        mailbox.sequence.store(u32::MAX, Ordering::Relaxed);
        assert!(mailbox.submit(1).is_err());
        assert!(mailbox.idle());
    }
}
