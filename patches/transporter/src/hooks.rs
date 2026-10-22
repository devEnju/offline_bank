//! The entry points called from the patched original code, with the
//! assembly stubs of the sites that need one. The original task calls an
//! entry every frame until it answers. The file work runs on the patch's one
//! worker thread (`storage_worker`), which the first START starts; the main
//! thread never waits for it.

use crate::{
    cart, navigation,
    panel::{self, Panel},
    program::Transporter15,
    storage_worker::{Failed, Job, Reply, TransporterWorker},
    task::*,
    ui::{self, from_second_page, loading, refusal_message, stop_loading, MESSAGE_OPEN_BANK},
};
use core::{mem::transmute, slice};
use offline_core::sections::TRANSPORT_SLOTS;
use patch_common::{
    exclusive::Exclusive,
    native::{follow, read_word, write_word, Ended},
    spinner::{self, Spinner},
    worker::{Pending, Polled},
};

/// What the version line says behind "Ver. 1.5".
const VERSION_SUFFIX: &[u8] = b"-offline";
/// A task's state, and the transfer's flag that it is to end (set by
/// `0025CDC0`; its update then selects sub-state `0x14`, `0024A0CC`).
const TASK_STATE: usize = 0x10;
const TRANSFER_ENDING: usize = 0x45;
/// Where the game list notes the time it began, in ms (`00245180`..
/// `0024524C`), for its minimum time of 3 s (`00244B24`).
const LIST_BEGAN: usize = 0x70;

/// The original updates the entries replace in their vtables and run
/// behind their own work: the game search, reading the chosen game, the
/// question and the transfer.
const SEARCH_UPDATE: usize = 0x0024_6f04;
const READ_UPDATE: usize = 0x0024_55d0;
const QUESTION_UPDATE: usize = 0x0024_7304;
const TRANSFER_UPDATE: usize = 0x0024_a0c4;
/// The game search's first state, which starts the cartridge scan; its
/// state that waits for the acknowledgement of a message and ends the
/// search (selected at `00247000`); and the outcome "none" it ends with
/// (`0024702C`), which the router answers with the disconnect task.
const SEARCH_START: u32 = 0;
const SEARCH_MESSAGE: u32 = 5;
const SEARCH_NONE: u8 = 2;
/// Answers of the delivery entry, as its stub tests them.
const DELIVER_FAILED: u32 = 0;
const DELIVER_DONE: u32 = 1;
const DELIVER_PENDING: u32 = 2;

/// The panel the search started (`panel`). It is kept apart from `STATE`,
/// unlike Bank's scan panel, which only its own hook uses: the entries that
/// only end it or ask about it run from stubs inside the original's calls
/// and never depend on whether a hook holds the state.
static PANEL: Panel = Panel::new();

/// The worker and the job it has out, and when the loading screen that is
/// up went up. Only the main thread takes it.
struct Runtime {
    worker: Option<TransporterWorker>,
    /// The job out, for the kind of job that submitted it.
    job: Pending<Job>,
    spinner: Spinner,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        worker: None,
        job: Pending::new(),
        spinner: Spinner::new(),
    })
};

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
    ) -> Option<Result<Reply, Failed>> {
        if !self.job.is_out() {
            let worker = match unsafe { TransporterWorker::get(&mut self.worker) } {
                Ok(worker) => worker,
                Err(failed) => return Some(Err(failed)),
            };
            if stage(worker).is_none() {
                return Some(Err(Failed));
            }
            return match unsafe { worker.submit(kind) } {
                Ok(id) => {
                    self.job.submitted(id, kind);
                    None
                }
                Err(failed) => Some(Err(failed)),
            };
        }
        // The check and the delivery belong to tasks of their own and never
        // overlap; a reply is only ever the job's that asked for it
        // (`Pending`).
        let worker = self.worker.as_mut()?;
        match self.job.poll(kind, |job| worker.collect(job)) {
            Polled::Waiting => None,
            Polled::Reply(reply) => Some(Ok(reply)),
            Polled::Failed(failed) => Some(Err(failed)),
            Polled::Foreign => Some(Err(Failed)),
        }
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
    } else if !state.job.is_out() {
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
/// it ends here. The original's own bit for a Virtual Console save that is
/// being written is not this one and stays as it is.
extern "aapcs" fn message_waits() {
    // A refusal or "Could not find a game" comes without the search's
    // panel, as the reader's messages come without its own.
    if let Some(ui) = PANEL.release() {
        unsafe { stop_loading(ui as *mut u8) };
    }
    refuse_home(false);
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
    refuse_home(false);
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
    if !PANEL.up() {
        // No file work without its panel: without one the search refuses
        // as a worker that fails does.
        let ui = task.pointer(TASK_UI).ok();
        if let Some(ui) = ui.filter(|&ui| unsafe { loading(ui) }.is_ok()) {
            PANEL.start(ui as usize);
            state.spinner.up(now());
            return Check::Pending;
        }
        drop(state);
        return unsafe { show(task, MESSAGE_OPEN_BANK) };
    }
    if !state.job.is_out() && state.spinner.idle(now()) {
        return Check::Pending;
    }
    let answer = unsafe { state.job(Job::Check, |_| Some(())) };
    drop(state);
    // A worker that failed refuses as a file that does not read does.
    let message = match answer {
        None => return Check::Pending,
        Some(Ok(Reply::Ready)) => return Check::Go,
        Some(Ok(Reply::Refused(refusal))) => refusal_message(refusal),
        Some(_) => MESSAGE_OPEN_BANK,
    };
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
    if unsafe { waits(task, id) } {
        return 0;
    }
    unsafe { original(update, task.raw()) }
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
    if !waits && panel::ends_panel(id, task_state) {
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

/// Replaces the load of a message in the original's "show a message"
/// (`0019B578`): the original loads it, and the failure message then keeps
/// only what follows its first page (`ui::from_second_page`).
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_message_text(
    messages: *mut u8,
    message: u32,
    string: *mut u8,
) -> u32 {
    let loaded = unsafe { ui::load_message(messages, message, string) };
    if from_second_page(message) {
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
/// Replaces the upload. Returns `DELIVER_DONE` when the delivery is written,
/// flushed, and read back; `DELIVER_PENDING` while that is in progress; and
/// `DELIVER_FAILED` when nothing was delivered, in which case the original
/// must not remove anything from the source game.
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
    if !state.job.is_out() {
        let Ok(manager) = task.pointer(TASK_MANAGER) else {
            return DELIVER_FAILED;
        };
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
    match answer {
        None => DELIVER_PENDING,
        Some(Ok(Reply::Delivered)) => DELIVER_DONE,
        Some(_) => DELIVER_FAILED,
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
