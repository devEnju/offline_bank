//! The entry points called from the patched original code, with the
//! assembly stubs of the sites that need one. The original task calls an
//! entry every frame until it answers. The file work runs on the patch's one
//! worker thread (`storage_worker`), which the first START starts; the main
//! thread never waits for it. What is not expected ends the session on the
//! error screen, as in Bank (`Runtime::report_failure`).

use crate::{
    cart, navigation,
    panel::{self, Panel},
    program::Transporter15,
    restore::{self, Frame, Restore},
    sdcard::WriteFault,
    storage_worker::{
        checked, delivered, settled, Checked, Delivered, Job, Reply, Settled, TransporterRequests,
        TransporterWorker, DELIVER_DONE, DELIVER_FAILED, DELIVER_PENDING,
    },
    task::*,
    ui::{self, loading, stop_loading, Chosen, Shown, MESSAGE_FAILURE, MESSAGE_NO_CARD},
};
use bank_common::{fault::Fault, worker::WorkerError};
use core::{
    mem::transmute,
    slice,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use offline_core::sections::TRANSPORT_SLOTS;
use patch_common::{
    exclusive::Exclusive,
    native::{follow, read_word, write_word, Ended},
    numbers::Label,
    spinner::{self, Spinner},
    worker::{Polled, Requests},
};

/// What the version line says behind "Ver. 1.5".
const VERSION_SUFFIX: &[u8] = b"-offline";
/// What the error screen says above its two numbers: the version, as
/// Bank's offline patch says it (`patch_common::numbers::Label`).
const LABEL: Label = Label::of(b"");
/// A task's state, and the transfer's flag that it is to end (set by
/// `0025CDC0`; its update then selects sub-state `0x14`, `0024A0CC`).
const TASK_STATE: usize = 0x10;
const TRANSFER_ENDING: usize = 0x45;
/// Where the game list notes the time it began, in ms (`00245180`..
/// `0024524C`), for its minimum time of 3 s (`00244B24`).
const LIST_BEGAN: usize = 0x70;

/// The original updates the entries replace in their vtables and run
/// behind their own work: the game search, reading the chosen game, the
/// Bank task, the question and the transfer.
const SEARCH_UPDATE: usize = 0x0024_6f04;
const READ_UPDATE: usize = 0x0024_55d0;
const BANK_UPDATE: usize = 0x0024_8c68;
/// The Bank task's shared UI, on which it shows its messages (`00248E78`).
const BANK_UI: usize = 0x38;
const QUESTION_UPDATE: usize = 0x0024_7304;
const TRANSFER_UPDATE: usize = 0x0024_a0c4;
/// The game search's first state, which starts the cartridge scan; its
/// state that waits for the acknowledgement of a message and ends the
/// search (selected at `00247000`); and the outcome "none" it ends with
/// (`0024702C`), which the router answers with the disconnect task.
const SEARCH_START: u32 = 0;
const SEARCH_MESSAGE: u32 = 5;
const SEARCH_NONE: u8 = 2;
/// The restore task's update (`00243AF4`). The question's number in the
/// table at `0028E00C` (`+0x8F`), the answer (`+0x90`) and the flag of its
/// different-copy notice (`+0x8E`); the constructor clears neither of the
/// last two (`0019D45C`).
const RESTORE_UPDATE: usize = 0x0024_3af4;
const RESTORE_PAGE: usize = 0x8f;
const RESTORE_ANSWER: usize = 0x90;
const RESTORE_NOTICE: usize = 0x8e;

/// The panel the search started (`panel`). It is kept apart from `STATE`,
/// unlike Bank's scan panel, which only its own hook uses: the entries that
/// only end it or ask about it run from stubs inside the original's calls
/// and never depend on whether a hook holds the state.
static PANEL: Panel = Panel::new();

/// The worker and the job it has out, when the loading screen that is up
/// went up, and the error the session stopped with. Only the main thread
/// takes it.
struct Runtime {
    /// The worker and the job out, for the kind of job that submitted it.
    requests: TransporterRequests,
    spinner: Spinner,
    failure: Option<Failure>,
    /// Where the restore task stands (`transporter_restore`).
    restore: Restore,
}
/// An error the session stopped with: its two numbers, and whether its
/// screen is up.
#[derive(Clone, Copy)]
struct Failure {
    numbers: [u32; 2],
    shown: bool,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        requests: Requests::new(),
        spinner: Spinner::new(),
        failure: None,
        restore: Restore::Asking(None),
    })
};
/// An object the job needs is not what the hook expects, or the job's reply
/// belongs to another: as Bank's hooks answer one (`Fault::NativeObject`).
const NOT_NATIVE: WorkerError = Fault::NativeObject.with(0);

impl Runtime {
    /// One frame of `kind`: on the first the worker is started if it is not
    /// yet and `stage` hands it what the job needs, then its reply. `None`
    /// while it runs. A job whose worker cannot say whether it is still
    /// running stays out: what it holds stays lent, and the next frame asks
    /// again.
    unsafe fn job(
        &mut self,
        kind: Job,
        stage: impl FnOnce(&mut TransporterWorker) -> Option<()>,
    ) -> Option<Result<Reply, WorkerError>> {
        if !self.requests.is_out() {
            let worker = match unsafe { self.requests.worker() } {
                Ok(worker) => worker,
                Err(failed) => return Some(Err(failed)),
            };
            if stage(worker).is_none() {
                return Some(Err(NOT_NATIVE));
            }
            return match unsafe { self.requests.submit(kind, kind) } {
                Ok(()) => None,
                Err(failed) => Some(Err(failed)),
            };
        }
        // The check and the delivery belong to tasks of their own and never
        // overlap; a reply is only ever the job's that asked for it
        // (`Pending`).
        match self.requests.poll(kind) {
            Polled::Waiting => None,
            Polled::Reply(reply) => Some(Ok(reply)),
            Polled::Failed(failed) => Some(Err(failed)),
            Polled::Foreign => Some(Err(NOT_NATIVE)),
        }
    }
    /// The session stops with `error`: its screen comes up on the next
    /// frame of the task (`report_failure`).
    fn fail(&mut self, error: WorkerError) {
        self.fail_with(error.numbers());
    }
    /// The same with the two numbers of an error.
    fn fail_with(&mut self, numbers: [u32; 2]) {
        self.failure.get_or_insert(Failure {
            numbers,
            shown: false,
        });
    }
    /// One frame of a task after an error, as Bank's: the session is over,
    /// and the task never ends. A job that is still out is waited for
    /// first, with the loading screen up and HOME refused. Then the loading
    /// panel ends and the error screen comes up on `ui`, which lets HOME
    /// through, so that Transporter is closed from the HOME Menu; every
    /// later frame keeps it. False only for a screen that could not be
    /// shown: the task then ends as it would without one.
    /// # Safety
    /// `ui` is the live shared UI of the task, on the main thread.
    unsafe fn report_failure(&mut self, ui: *mut u8) -> bool {
        let Some(failure) = self.failure.as_mut() else {
            return false;
        };
        if failure.shown {
            unsafe { ui::keep_dialog(ui) };
            return true;
        }
        // A job that is still out holds what the task lent it.
        if !self.requests.settle() {
            return true;
        }
        failure.shown = true;
        let [first, second] = failure.numbers;
        unsafe {
            if let Some(panel) = PANEL.release() {
                stop_loading(panel as *mut u8);
            }
            stop_loading(ui);
        }
        refuse_home(false);
        unsafe { ui::stop(ui, &LABEL, first, second) }.is_ok()
    }
    /// One frame of the task whose shared UI is `ui`, if the session has
    /// stopped: true while the error screen holds it.
    /// # Safety
    /// As `report_failure`.
    unsafe fn stopped(
        &mut self,
        ui: Result<*mut u8, patch_common::native::NotNative>,
    ) -> Option<bool> {
        self.failure?;
        Some(match ui {
            Ok(ui) => unsafe { self.report_failure(ui) },
            Err(_) => false,
        })
    }
}

/// The original's clock in ms (`spinner::now`).
fn now() -> u32 {
    spinner::now::<Transporter15>()
}
/// Copies the transport box the original filled while reading the source
/// game into `staging`: its records, then its tags.
/// # Safety
/// `task` is a live task of the original, on the main thread.
unsafe fn stage_box(task: Task, staging: &mut [u8]) -> Option<()> {
    let (records, tags) = staging.split_at_mut(RECORDS_LEN);
    unsafe {
        let object = task.bank_pointer().ok()?;
        let accessor = follow(object, BANK_OBJECT_ACCESSOR)?;
        let body = follow(accessor, ACCESSOR_BODY)?;
        records.copy_from_slice(slice::from_raw_parts(body.add(RECORDS_OFFSET), RECORDS_LEN));
        tags.copy_from_slice(slice::from_raw_parts(
            body.add(TAGS_OFFSET),
            TRANSPORT_SLOTS,
        ));
    }
    Some(())
}

/// Replaces the one call of the original's router (`00242EF8`). The
/// original is asked first; its answer is changed where the offline flow
/// differs, and HOME and sleep are refused or let through by the kind of
/// the task that comes next (`navigation`). A manager or task that is not a
/// pointer, or a state a hook holds, ends the session: the disconnect task,
/// and the original is not asked.
/// # Safety
/// Called only from the patched call site, on the main thread, with the
/// live manager whose current task has ended.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_next(manager: *mut u8, current: u32) -> u32 {
    let Some(mut state) = STATE.take() else {
        return navigation::DISCONNECT;
    };
    let Some(Ended {
        outcome,
        cancelled,
        native,
    }) = (unsafe { Ended::route::<Transporter15>(manager, current) })
    else {
        return navigation::DISCONNECT;
    };
    let next = navigation::destination(current, outcome, native, cancelled);
    // A task that reads or writes refuses HOME and sleep from the moment it
    // is chosen; one that waits lets them through, unless a job is still out.
    if navigation::loads(next) {
        refuse_home(true);
    } else if !state.requests.is_out() {
        refuse_home(false);
    }
    // The task's set-up puts up its loading panel on a later frame; its
    // time is taken on the first frame of the patch behind it.
    if navigation::spins(next) {
        state.spinner.coming();
    }
    // The search's panel stays into the game list, until the list is on
    // screen; after anything else it ends here.
    if !navigation::takes_panel(next) {
        if let Some(ui) = PANEL.release() {
            unsafe { stop_loading(ui as *mut u8) };
        }
    }
    next
}

// The original's "show a message" function, its first instruction
// (0019B50C, was: push {r4-r8, lr}). r0 = ui, r1 = message, r2 = whether the
// message waits for an answer (`patch_common`'s `message_stub!`). Every
// message a task that reads or writes ends with comes through here.
patch_common::message_stub!(transporter_message_stub, "r2", "0x0019b510", message_waits);
/// The original's "show a message" function was entered with a message that
/// waits for an answer. Such a message is where a task that reads or writes
/// has nothing left to do but end, so the refusal the router entry began for
/// it ends here, unless a job is still out (`let_through`). The original's
/// own bit for a Virtual Console save that is being written is not this one
/// and stays as it is.
extern "aapcs" fn message_waits() {
    // A refusal or "Could not find a game" comes without the search's
    // panel, as the reader's messages come without its own.
    if let Some(ui) = PANEL.release() {
        unsafe { stop_loading(ui as *mut u8) };
    }
    let_through();
}
/// A screen that waits for the user: HOME and sleep go through again, unless
/// a job is still out. Every hook lets go of the state before it calls the
/// original or shows a message, so one that holds it is no such screen's.
fn let_through() {
    if STATE.take().is_none_or(|state| !state.requests.is_out()) {
        refuse_home(false);
    }
}

// Game list set-up, state 0: the call that starts its indicator (00244EAC,
// was: bl 002450F4, r0 = task), once the list's screen is built. While the
// search's panel is up (entry: 1) the list starts nothing: on inside
// 002450F4 with that function's own prologue at 00245180, which only notes
// when the list began and returns to 2:; the started entry then moves that
// time back to when the panel went up and returns to the caller. Otherwise
// the call.
patch_common::stub!(
    transporter_list_indicator_stub,
    [
        "push {{r0, lr}}",
        "bl {list_indicator}",
        "movs r12, r0",
        "pop {{r0, lr}}",
        "ldreq r12, =0x002450f4",
        "bxeq r12",
        "push {{r0, lr}}",
        "adr lr, 2f",
        "push {{r3-r11, lr}}",
        "mov r4, r0",
        "ldr r12, =0x00245180",
        "bx r12",
        "2:",
        "pop {{r0, lr}}",
        "b {list_started}",
    ],
    list_indicator = sym list_indicator,
    list_started = sym list_started,
);
/// The game list's set-up is about to start its own indicator on the list's
/// screen it has just built. Answers 1 while the search's panel is up: the
/// list then starts nothing and only notes when it began, for its minimum
/// time; 0: the list starts its own, as the original.
unsafe extern "aapcs" fn list_indicator(raw: *mut u8) -> u32 {
    if unsafe { Task::from_raw(raw, &[TASK_LIST]) }.is_err() {
        return 0;
    }
    u32::from(PANEL.list_starts() == panel::ListStart::Quiet)
}
/// The game list has noted when it began, while the search's panel is up:
/// it counts its minimum time from when the panel went up instead, so the
/// panel is not kept 3 s longer than the list needs. The search's 0.5 s
/// before its work count towards those 3 s.
/// # Safety
/// `task` is the live game-list task, on the main thread.
unsafe extern "aapcs" fn list_started(task: *mut u8) {
    let Ok(task) = (unsafe { Task::from_raw(task, &[TASK_LIST]) }) else {
        return;
    };
    let task = task.raw();
    let elapsed = STATE
        .take()
        .and_then(|state| state.spinner.elapsed(now()))
        .unwrap_or(0);
    unsafe {
        let began = task.add(LIST_BEGAN).cast::<u64>();
        began.write(panel::list_start(began.read(), elapsed));
    }
}

// Game list set-up, every frame: its check that restarts its indicator's
// sound (00244D40, was: ldr r0, [r5, #0x5c]; r4 = task, r5 = the list's
// screen; the check's own calls clobber r12 and the flags). While the
// search's panel is up (entry: 1) on behind the check (00244D8C); otherwise
// the instruction and the check.
patch_common::stub!(
    transporter_list_frame_stub,
    [
        "push {{r0-r3, r12, lr}}",
        "mov r0, r4",
        "bl {list_frame}",
        "cmp r0, #0",
        "pop {{r0-r3, r12, lr}}",
        "ldrne r12, =0x00244d8c",
        "bxne r12",
        "ldr r0, [r5, #0x5c]",
        "ldr r12, =0x00244d44",
        "bx r12",
    ],
    list_frame = sym list_frame,
);
/// A frame of the game list's set-up: 1 while the search's panel is up,
/// whose sound plays, so the list's sound check is skipped.
unsafe extern "aapcs" fn list_frame(raw: *mut u8) -> u32 {
    if unsafe { Task::from_raw(raw, &[TASK_LIST]) }.is_err() {
        return 0;
    }
    u32::from(PANEL.up())
}

// Game list, the first instruction of its update (002445EC, was: ldr r0,
// [r4, #0x38]): its set-up is done and it is on screen. r4 = task; the
// instruction is run here and every other register the original still uses
// is returned as it came.
patch_common::stub!(
    transporter_list_ready_stub,
    [
        "push {{r1-r3, r12, lr}}",
        "sub sp, sp, #4",
        "mov r0, r4",
        "bl {list_ready}",
        "add sp, sp, #4",
        "pop {{r1-r3, r12, lr}}",
        "ldr r0, [r4, #0x38]",
        "bx lr",
    ],
    list_ready = sym list_ready,
);
/// The game list is set up and on screen: every save has been read, and it
/// waits for the user. The panel that covered its loading ends here.
unsafe extern "aapcs" fn list_ready(raw: *mut u8) {
    if unsafe { Task::from_raw(raw, &[TASK_LIST]) }.is_err() {
        return;
    }
    if let Some(ui) = PANEL.release() {
        unsafe { stop_loading(ui as *mut u8) };
    }
    let_through();
}

/// Replaces the game search's update in its vtable (`002E6780`, was
/// `00246F04`), as Bank's scan entry replaces the scan's. In its first
/// state, before the original searches for games, it asks whether Bank can
/// take a delivery: the question the original put to the server after a
/// game was chosen. If not, the original's message for it is shown here and
/// the search goes to its own state that waits for it and ends as for "no
/// game found": back to the start screen. Every other frame is the
/// original's update.
/// # Safety
/// The original's task driver supplies the live game-search task, on the
/// main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_search(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_SEARCH]) }) else {
        // Finished, with nothing read or written, as Bank's task hooks
        // answer an unexpected task.
        return 1;
    };
    if unsafe { read_word(raw, TASK_STATE) } == SEARCH_START {
        match unsafe { check(task) } {
            Check::Pending => return 0,
            Check::Go => {}
            Check::Shown => {
                unsafe { write_word(raw, TASK_STATE, SEARCH_MESSAGE) };
                return 0;
            }
            Check::End => {
                task.set_outcome(SEARCH_NONE);
                return 1;
            }
            // The router entry answers it with the restore task, and ends
            // the search's panel (`navigation::destination`).
            Check::Ask => {
                task.set_outcome(navigation::ASKED);
                return 1;
            }
        }
    }
    unsafe { original(SEARCH_UPDATE, raw) }
}

/// What the check at START answers: come back next frame; go on with the
/// search; a message is on screen; end the search without one.
enum Check {
    Pending,
    Go,
    Shown,
    End,
    /// Bank left its save in progress to the restore question.
    Ask,
}
/// The check in the search's first state.
/// # Safety
/// `task` is the live game-search task, on the main thread.
unsafe fn check(task: Task) -> Check {
    // The loading panel goes up before anything is read, on a frame of its
    // own, as Bank's scan puts up its panel (`panel`). The worker's start
    // and the check follow once it has stood for 0.5 s (`spinner`).
    let Some(mut state) = STATE.take() else {
        return Check::Pending;
    };
    if let Some(held) = unsafe { state.stopped(task.pointer(TASK_UI)) } {
        return if held { Check::Pending } else { Check::End };
    }
    if !PANEL.up() {
        // No file work without its panel. Without what it needs of the
        // shared UI the search stops on the error screen, as Bank's hooks
        // do for an object that is not what they expect.
        let Ok(ui) = task.pointer(TASK_UI) else {
            return Check::End;
        };
        if unsafe { loading(ui) }.is_ok() {
            PANEL.start(ui as usize);
            state.spinner.up(now());
            return Check::Pending;
        }
        state.fail(NOT_NATIVE);
        return Check::Pending;
    }
    if !state.requests.is_out() && state.spinner.idle(now()) {
        return Check::Pending;
    }
    let answer = unsafe { state.job(Job::Check, |_| Some(())) };
    let message = match checked(answer) {
        Checked::Pending => return Check::Pending,
        Checked::Go => return Check::Go,
        Checked::Ask => return Check::Ask,
        Checked::Refused(message) => message,
        Checked::Failed(error) => {
            state.fail(error);
            return Check::Pending;
        }
    };
    drop(state);
    unsafe { show(task, message) }
}

/// Replace the updates of reading the chosen game (`002E66C4`, was
/// `002455D0`), the question (`002E67D0`, was `00247304`) and the transfer
/// (`002E6A9C`, was `0024A0C4`) in their vtables. While this frame waits for
/// the loading panel (`waits`) the update returns 0, as on a frame it has
/// nothing to do; otherwise the original's update runs.
/// # Safety
/// The original's task driver supplies the live task, on the main
/// thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_read(raw: *mut u8) -> u32 {
    unsafe { update(raw, navigation::READ, READ_UPDATE) }
}
/// # Safety
/// As `transporter_read`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_question(raw: *mut u8) -> u32 {
    unsafe { update(raw, navigation::QUESTION, QUESTION_UPDATE) }
}
/// # Safety
/// As `transporter_read`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_transfer(raw: *mut u8) -> u32 {
    unsafe { update(raw, navigation::TRANSFER, TRANSFER_UPDATE) }
}
/// One frame of the update of task `id`: none while it waits, else the
/// original's. An object that is not the task `id` is not touched: the
/// update answers finished, as the search's and Bank's task entries do.
unsafe fn update(raw: *mut u8, id: u32, update: usize) -> u32 {
    let expected = match id {
        navigation::READ => TASK_READ,
        navigation::QUESTION => TASK_QUESTION,
        _ => TASK_TRANSFER,
    };
    let Ok(task) = (unsafe { Task::from_raw(raw, &[expected]) }) else {
        return 1;
    };
    if id == navigation::TRANSFER {
        return unsafe { saving(task, TRANSFER_UI, update, false) };
    }
    if unsafe { waits(task, id) } {
        return 0;
    }
    unsafe { original(update, task.raw()) }
}
/// Replaces the Bank task's update in its vtable (`002E697C`, was
/// `00248C68`), the task that saves the original's note into the chosen
/// game (`saving`).
/// # Safety
/// The original's task driver supplies the live Bank task, on the main
/// thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_bank(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_BANK]) }) else {
        return 1;
    };
    unsafe { saving(task, BANK_UI, BANK_UPDATE, true) }
}

/// One frame of the update at `update` of a task that saves the chosen
/// game: the Bank task, which saves the original's note (`note`), and the
/// transfer, which waits for its loading panel first (`waits`). A failed
/// save the error screen stands for (`Shown::Numbers`), or a message that
/// cannot be loaded, is noted by the message-text entry when the original
/// shows its message, and this entry puts the error screen in its place on
/// the task's shared UI at `ui` in the same frame, before it is drawn. From
/// then on the task stays on it, as the search does.
/// # Safety
/// `task` is the live Bank task or transfer, on the main thread.
unsafe fn saving(task: Task, ui: usize, update: usize, note: bool) -> u32 {
    {
        let Some(mut state) = STATE.take() else {
            // Its state is a hook's: this frame waits, as Bank's hooks do.
            return 0;
        };
        if unsafe { state.stopped(task.pointer(ui)) } == Some(true) {
            return 0;
        }
    }
    if !note && unsafe { waits(task, navigation::TRANSFER) } {
        return 0;
    }
    // The failure message names the chosen game's kind (`ui::Chosen`).
    if let Ok(manager) = task.pointer(TASK_MANAGER) {
        MANAGER.store(manager as usize, Ordering::Relaxed);
    }
    DUE.store(0, Ordering::Relaxed);
    NOTE_SAVE.store(note, Ordering::Relaxed);
    let finished = unsafe { original(update, task.raw()) };
    NOTE_SAVE.store(false, Ordering::Relaxed);
    let first = DUE.swap(0, Ordering::Relaxed);
    if first == 0 {
        return finished;
    }
    let Some(mut state) = STATE.take() else {
        return finished;
    };
    state.fail_with([first, DUE_SECOND.load(Ordering::Relaxed)]);
    match unsafe { state.stopped(task.pointer(ui)) } {
        Some(true) => 0,
        // No screen to show it on: the task ends as without one.
        _ => finished,
    }
}

/// Replaces the restore task's update in its vtable (`002E6578`, was
/// `00243AF4`). The original asked the server first; offline the search
/// enters it only for a save in progress Bank left to its question
/// (`navigation::ASKED`). On its first frame the entry goes straight to
/// the question, the first of its table: "You can try restoring your data,
/// but you may lose the Pokémon currently being transported. Would you
/// still like to try restoring your data?" (23:0x1A). The original update
/// shows it with its Yes and No buttons and waits for the answer; once the
/// buttons are gone the entry takes over, where the original would ask its
/// second question or end. The loading panel with "Communication in your
/// last session did not complete correctly. Performing cleanup process
/// now..." (23:0x04) goes up, HOME and sleep are refused, and after 0.5 s
/// the worker settles the save as the answer chose (`restore::answered`): Yes
/// the save before, No the save after. The task ends once the panel has
/// stood for 1.5 s, and the disconnect task follows. A failure ends on the
/// error screen. What each frame does is `restore::Restore::frame`.
/// # Safety
/// The original's task driver supplies the live restore task, on the main
/// thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_restore(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_RESTORE]) }) else {
        return 1;
    };
    let Some(mut state) = STATE.take() else {
        // Its state is a hook's: this frame waits, as Bank's hooks do.
        return 0;
    };
    if let Some(held) = unsafe { state.stopped(task.pointer(RESTORE_UI)) } {
        return u32::from(!held);
    }
    let raw = task.raw();
    let at = unsafe { read_word(raw, TASK_STATE) };
    let answer = unsafe { raw.add(RESTORE_ANSWER).read() };
    match state.restore.frame(at, answer) {
        Frame::Begin => {
            unsafe {
                write_word(raw, TASK_STATE, restore::ASK);
                raw.add(RESTORE_PAGE).write(0);
                raw.add(RESTORE_ANSWER).write(0);
                raw.add(RESTORE_NOTICE).write(0);
            }
            drop(state);
            unsafe { original(RESTORE_UPDATE, raw) }
        }
        Frame::Original => {
            drop(state);
            unsafe { original(RESTORE_UPDATE, raw) }
        }
        Frame::Answered => {
            let Ok(ui) = task.pointer(RESTORE_UI) else {
                state.fail(NOT_NATIVE);
                return 0;
            };
            unsafe { ui::cleanup(ui) };
            state.spinner.up(now());
            refuse_home(true);
            0
        }
        Frame::Work(decision) => {
            if !state.requests.is_out() && state.spinner.idle(now()) {
                return 0;
            }
            match settled(unsafe { state.job(Job::Settle(decision), |_| Some(())) }) {
                Settled::Pending => 0,
                Settled::Failed(error) => {
                    state.fail(error);
                    0
                }
                Settled::Done => {
                    state.restore = Restore::Settled;
                    u32::from(!state.spinner.held(now()))
                }
            }
        }
        // The job is done; only the panel's time is left.
        Frame::Done => u32::from(!state.spinner.held(now())),
    }
}

/// The original update at `address`.
unsafe fn original(address: usize, raw: *mut u8) -> u32 {
    let update: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(address) };
    unsafe { update(raw) }
}

/// Whether this frame of the update of task `id` waits for its loading panel
/// (`panel::waits`). A transfer the original is ending is not held up. The
/// frame the question's first state runs, which takes down the panel of
/// the reading, lets HOME and sleep through.
/// # Safety
/// `task` is the live task `id`, on the main thread.
unsafe fn waits(task: Task, id: u32) -> bool {
    let task = task.raw();
    if id == navigation::TRANSFER && unsafe { task.add(TRANSFER_ENDING).read() } != 0 {
        return false;
    }
    let Some(mut state) = STATE.take() else {
        // Its state is a hook's: this frame waits, as Bank's hooks do.
        return true;
    };
    let now = now();
    // The panel of reading the game or of the transfer.
    state.spinner.seen(now);
    let task_state = unsafe { read_word(task, TASK_STATE) };
    let waits = panel::waits(
        id,
        task_state,
        state.spinner.idle(now),
        state.spinner.held(now),
    );
    if !waits && panel::ends_panel(id, task_state) && !state.requests.is_out() {
        refuse_home(false);
    }
    waits
}

/// Shows an original message the way the game search shows its own. It
/// waits for an answer, so HOME and sleep work on it (`message_waits`).
/// The search ends without one when the task's dialog owner is not a
/// pointer.
unsafe fn show(task: Task, message: u32) -> Check {
    let Ok(ui) = task.pointer(TASK_UI) else {
        return Check::End;
    };
    unsafe { ui::show(ui, message) };
    Check::Shown
}

/// The manager of the transfer, noted by the delivery entry, for what the
/// chosen game is (`ui::Chosen`).
static MANAGER: AtomicUsize = AtomicUsize::new(0);
/// The last delivery did not happen for lack of room on the SD card, for the
/// message the transfer shows next: taken by the message-text entry.
static DELIVERY_NO_ROOM: AtomicBool = AtomicBool::new(false);
/// The original update of the Bank task runs (`transporter_bank`): a
/// failure message shown now is the one after its note save.
static NOTE_SAVE: AtomicBool = AtomicBool::new(false);
/// The error screen is to take the place of the message the task just
/// showed, with these numbers; the first is never 0. Taken by `saving`.
static DUE: AtomicU32 = AtomicU32::new(0);
static DUE_SECOND: AtomicU32 = AtomicU32::new(0);

/// Notes `error` for the error screen that takes the place of the message
/// being loaded (`saving`).
fn due(error: WorkerError) {
    let [first, second] = error.numbers();
    DUE_SECOND.store(second, Ordering::Relaxed);
    DUE.store(first, Ordering::Relaxed);
}

/// Replaces the load of a message in the original's "show a message"
/// (`0019B578`): the original loads it, and what is shown follows from what
/// the last write to the game that failed left and what the delivery
/// answered (`ui::shown`): the failure message from its second page, "The
/// SD card is full." in its place, or the error screen. The error screen's
/// failure message is kept whole (`ui::whole`).
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_message_text(
    messages: *mut u8,
    message: u32,
    string: *mut u8,
) -> u32 {
    let shown = if ui::whole() {
        Shown::Whole
    } else {
        let fault = if matches!(message, MESSAGE_FAILURE | MESSAGE_NO_CARD) {
            cart::take_write_fault()
        } else {
            (WriteFault::None, 0)
        };
        let manager = MANAGER.load(Ordering::Relaxed) as *mut u8;
        let chosen = if patch_common::native::pointer(manager as u32).is_none() {
            Chosen::CardOut
        } else if unsafe { cart::chosen_virtual_console(manager) } {
            Chosen::VirtualConsole
        } else if unsafe { cart::chosen_card_missing(manager) } {
            Chosen::CardOut
        } else {
            Chosen::CardIn
        };
        let no_room = matches!(message, MESSAGE_FAILURE | MESSAGE_NO_CARD)
            && DELIVERY_NO_ROOM.swap(false, Ordering::Relaxed);
        let note_save = NOTE_SAVE.load(Ordering::Relaxed);
        ui::shown(message, fault, chosen, no_room, note_save)
    };
    match shown {
        Shown::SdFull => {
            if let Some(loaded) = unsafe { ui::sd_full(messages, string) } {
                return loaded;
            }
            // Its text cannot be loaded: the error screen, never another
            // message in its place.
            due(NOT_NATIVE);
        }
        // The task's entry puts the error screen in its place before the
        // frame is drawn (`saving`).
        Shown::Numbers(second) => due(Fault::GameWrite.with(second)),
        Shown::Whole | Shown::SecondPage => {}
    }
    let loaded = unsafe { ui::load_message(messages, message, string) };
    if shown != Shown::Whole {
        unsafe { patch_common::ui::keep_rest(string) };
    }
    loaded
}

// Transfer, sub-state 0 (0024A150, was: the start of creating the upload
// request). r4 = task; lr is free as above. Done: on to the original
// removal code. Pending: the original return, to come back next frame.
// Else: sub-state 0x11, the original failure message, through the store at
// 0024A4C0.
patch_common::stub!(
    transporter_deliver_stub,
    [
        "mov r0, r4",
        "bl {deliver}",
        "cmp r0, #{done}",
        "ldreq r12, =0x0024a274",
        "bxeq r12",
        "cmp r0, #{pending}",
        "ldreq r12, =0x0024a5dc",
        "bxeq r12",
        "mov r0, #0x11",
        "ldr r12, =0x0024a4c0",
        "bx r12",
    ],
    deliver = sym deliver,
    done = const DELIVER_DONE,
    pending = const DELIVER_PENDING,
);
/// Replaces the upload (`storage_worker::delivered`). Returns `DELIVER_DONE`
/// when Bank holds the delivery; `DELIVER_PENDING` while it is written;
/// `DELIVER_FAILED` when Bank holds none, for certain, because the card is
/// gone or the SD card had no room. Every other answer, a delivery Bank may
/// hold included, ends the session on the error screen, and the transfer
/// waits on it (`DELIVER_PENDING`). Only `DELIVER_DONE` lets the original
/// remove the Pokémon from the source game: after a delivery Bank may hold
/// they may be in both, a duplicate, never a loss.
///
/// Bank is written only while the game the Pokémon leave can be saved: for
/// a cartridge whose card is gone the answer is `DELIVER_FAILED` before
/// anything is written, as the original asks again before it saves the game.
/// # Safety
/// `task` is the live transfer task, on the main thread.
unsafe extern "aapcs" fn deliver(raw: *mut u8) -> u32 {
    let Ok(task) = (unsafe { Task::from_raw(raw, &[TASK_TRANSFER]) }) else {
        return DELIVER_FAILED;
    };
    let Some(mut state) = STATE.take() else {
        return DELIVER_PENDING;
    };
    if let Some(held) = unsafe { state.stopped(task.pointer(TRANSFER_UI)) } {
        return if held {
            DELIVER_PENDING
        } else {
            DELIVER_FAILED
        };
    }
    if !state.requests.is_out() {
        // A manager that is not a pointer is an object the hook does not
        // know: the error screen, as in Bank.
        let Ok(manager) = task.pointer(TASK_MANAGER) else {
            state.fail(NOT_NATIVE);
            return DELIVER_PENDING;
        };
        MANAGER.store(manager as usize, Ordering::Relaxed);
        DELIVERY_NO_ROOM.store(false, Ordering::Relaxed);
        if unsafe { cart::chosen_card_missing(manager) } {
            return DELIVER_FAILED;
        }
    }
    // The transport box goes to the worker in its staging buffer, copied
    // here: the worker touches no object of the original.
    let answer = unsafe {
        state.job(Job::Deliver, |worker| {
            stage_box(task, worker.staging_mut().ok()?)
        })
    };
    match delivered(answer) {
        Delivered::Pending => DELIVER_PENDING,
        Delivered::Done => DELIVER_DONE,
        Delivered::NoRoom => {
            DELIVERY_NO_ROOM.store(true, Ordering::Relaxed);
            DELIVER_FAILED
        }
        Delivered::Failed(error) => {
            state.fail(error);
            DELIVER_PENDING
        }
    }
}

/// Replaces the start screen's call that writes its version line
/// (`0024BAE8`): the line says which patch runs (`patch_common::ui`).
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_version_shown(
    context: *mut u8,
    group: u32,
    pane: u32,
    message: u32,
) {
    unsafe {
        patch_common::ui::version::<Transporter15>(context, group, pane, message, VERSION_SUFFIX)
    }
}
