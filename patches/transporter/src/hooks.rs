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
    ui::{end_panel, from_second_page, start_panel, MESSAGE_OPEN_BANK, UI_CONTEXT},
};
use core::{mem::transmute, slice};
use offline_core::sections::TRANSPORT_SLOTS;
use patch_common::{
    exclusive::Exclusive,
    native::follow,
    spinner::{self, Spinner},
    worker::JobId,
};

/// The original's "show this message and wait": `(ui, message, 1)`, as the
/// game search calls it for its own message 0 (`00246FFC`).
const SHOW_MESSAGE: usize = 0x0019_b50c;
/// `(messages, message, string)`: loads a message of the shared UI's text
/// file into a string object (`001DBEB8`), as "show a message" does before
/// it hands the string to its dialog (`0019B578`).
const LOAD_MESSAGE: usize = 0x001d_beb8;
/// What the version line says behind "Ver. 1.5".
const VERSION_SUFFIX: &[u8] = b"-offline";
/// The original's router, `get_next_state(manager, current)`, and what it
/// reads: the finished task at `manager + 0x10` with its outcome byte at
/// `+0x30`, and the manager's "cancelled" flag, which it clears.
const NEXT_STATE: usize = 0x0024_2ba0;
const MANAGER_TASK: usize = 0x10;
const MANAGER_CANCELLED: usize = 0x1a;
const TASK_OUTCOME: usize = 0x30;

/// A task's state, and the transfer's flag that it is to end (set by
/// `0025CDC0`; its update then selects sub-state `0x14`, `0024A0CC`).
const TASK_STATE: usize = 0x10;
const TRANSFER_ENDING: usize = 0x45;
/// Where the game list notes the time it began, in ms (`00245180`..
/// `0024524C`), for its minimum time of 3 s (`00244B24`).
const LIST_BEGAN: usize = 0x70;

/// Answers of the check entry, as its stub tests them: go on with the game
/// search; come back next frame; a message is on screen, so wait for it and
/// end the search, which leads back to the title screen; end the search at
/// once.
const SEARCH_GO: u32 = 0;
const SEARCH_PENDING: u32 = 1;
const SEARCH_SHOWN: u32 = 2;
const SEARCH_END: u32 = 3;
/// Answers of the delivery entry, as its stub tests them.
const DELIVER_FAILED: u32 = 0;
const DELIVER_DONE: u32 = 1;
const DELIVER_PENDING: u32 = 2;

/// The panel the search started (`panel`).
static PANEL: Panel = Panel::new();

/// The worker and the job it has out, and when the loading screen that is
/// up went up. Only the main thread takes it.
struct Runtime {
    worker: Option<TransporterWorker>,
    job: Option<(JobId, Job)>,
    spinner: Spinner,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        worker: None,
        job: None,
        spinner: Spinner::new(),
    })
};

impl Runtime {
    /// One frame of `kind`: on the first the worker is started if it is not
    /// yet and `stage` hands it what the job needs, then its reply. `None`
    /// while it runs. A job whose worker cannot say whether it is still
    /// running stays out: what it holds stays lent, and the next frame asks
    /// again.
    unsafe fn step(
        &mut self,
        kind: Job,
        stage: impl FnOnce(&mut TransporterWorker) -> Option<()>,
    ) -> Option<Result<Reply, Failed>> {
        let Some((id, out)) = self.job else {
            let worker = match unsafe { TransporterWorker::get(&mut self.worker) } {
                Ok(worker) => worker,
                Err(failed) => return Some(Err(failed)),
            };
            if stage(worker).is_none() {
                return Some(Err(Failed));
            }
            return match unsafe { worker.submit(kind) } {
                Ok(id) => {
                    self.job = Some((id, kind));
                    None
                }
                Err(failed) => Some(Err(failed)),
            };
        };
        // The check and the delivery are steps of their own and never
        // overlap; a reply is only ever the job's that asked for it.
        if out != kind {
            return Some(Err(Failed));
        }
        let worker = self.worker.as_mut()?;
        let mut job = Some(id);
        let polled = worker.collect(&mut job);
        if job.is_none() {
            self.job = None;
        }
        polled.transpose()
    }
}

/// The original's clock in ms (`spinner::now`).
fn now() -> u32 {
    spinner::now::<Transporter15>()
}
/// The transport box the original filled while reading the source game.
unsafe fn native_box(task: *const u8) -> Option<(&'static [u8], &'static [u8])> {
    unsafe {
        let manager = follow(task, TASK_MANAGER)?;
        let object = follow(manager, MANAGER_BANK_OBJECT)?;
        let accessor = follow(object, BANK_OBJECT_ACCESSOR)?;
        let body = follow(accessor, ACCESSOR_BODY)?;
        Some((
            slice::from_raw_parts(body.add(RECORDS_OFFSET), RECORDS_LEN),
            slice::from_raw_parts(body.add(TAGS_OFFSET), TRANSPORT_SLOTS),
        ))
    }
}

/// Replaces the one call of the original's router (`00242EF8`). The
/// original is asked first; its answer is changed where the offline flow
/// differs, and HOME and sleep are refused or let through by the kind of
/// the step that comes next (`navigation`).
/// # Safety
/// Called only from the patched call site, on the main thread, with the
/// live manager whose current task has ended.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_next(manager: *mut u8, current: u32) -> u32 {
    let original: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = unsafe { transmute(NEXT_STATE) };
    // Read before the original runs, which clears the flag.
    let cancelled = unsafe { manager.add(MANAGER_CANCELLED).read() } != 0;
    let outcome = unsafe { follow(manager, MANAGER_TASK) }
        .map(|task| unsafe { task.add(TASK_OUTCOME).read() });
    let native = unsafe { original(manager, current) };
    let Some(outcome) = outcome else {
        return native;
    };
    let next = navigation::destination(current, outcome, native, cancelled);
    refuse_home(navigation::loads(next));
    // The step's set-up puts up its loading panel on a later frame; its
    // time is taken on the first frame of the patch behind it.
    if navigation::spins(next) {
        if let Some(mut state) = STATE.take() {
            state.spinner.coming();
        }
    }
    // The search's panel stays into the game list, until the list is on
    // screen; after anything else it ends here.
    if !panel::handed_on(next) {
        if let Some(ui) = PANEL.release() {
            unsafe { end_panel(ui as *mut u8) };
        }
    }
    next
}

// The original's "show a message" function, its first instruction
// (0019B50C, was: push {r4-r8, lr}). r0 = ui, r1 = message, r2 = whether the
// message waits for an answer (`patch_common`'s `message_stub!`). Every
// message a step that reads or writes ends with comes through here.
patch_common::message_stub!(transporter_message_stub, "r2", "0x0019b510", message_waits);
/// The original's "show a message" function was entered with a message that
/// waits for an answer. Such a message is where a step that reads or writes
/// has nothing left to do but end, so the refusal the router entry began for
/// it ends here. The original's own bit for a Virtual Console save that is
/// being written is not this one and stays as it is.
extern "aapcs" fn message_waits() {
    // A refusal or "Could not find a game" comes without the search's
    // panel, as the reader's messages come without its own.
    if let Some(ui) = PANEL.release() {
        unsafe { end_panel(ui as *mut u8) };
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
extern "aapcs" fn list_indicator() -> u32 {
    u32::from(PANEL.list_starts() == panel::ListStart::Quiet)
}
/// The game list has noted when it began, while the search's panel is up:
/// it counts its minimum time from when the panel went up instead, so the
/// panel is not kept 3 s longer than the list needs. The search's 0.5 s
/// before its work count towards those 3 s.
/// # Safety
/// `task` is the live game-list task, on the main thread.
unsafe extern "aapcs" fn list_started(task: *mut u8) {
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
extern "aapcs" fn list_frame() -> u32 {
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
extern "aapcs" fn list_ready() {
    if let Some(ui) = PANEL.release() {
        unsafe { end_panel(ui as *mut u8) };
    }
    refuse_home(false);
}

// Game search, first step (00246F48, was: ldr r0, [r0], the start of the
// cartridge scan). r4 = task; lr is free, the original function saved its
// own. Pending: the search's "nothing this frame" return. Shown: a message
// is on screen; 00247000 selects the search's own step that waits for it
// and ends the search. End: its ending without a message. Else: the
// replaced instruction, with r0 as the search had loaded it, and on with the
// search.
patch_common::stub!(
    transporter_check_stub,
    [
        "mov r0, r4",
        "bl {check}",
        "cmp r0, #{pending}",
        "ldreq r12, =0x00247024",
        "bxeq r12",
        "cmp r0, #{shown}",
        "ldreq r12, =0x00247000",
        "bxeq r12",
        "cmp r0, #{end}",
        "ldreq r12, =0x0024702c",
        "bxeq r12",
        "ldr r0, =0x00329380",
        "ldr r0, [r0]",
        "ldr r12, =0x00246f4c",
        "bx r12",
    ],
    check = sym check,
    pending = const SEARCH_PENDING,
    shown = const SEARCH_SHOWN,
    end = const SEARCH_END,
);
/// Asks, when START was pressed and before the original searches for games,
/// whether Bank can take a delivery: the question the original put to the
/// server after a game was chosen. If not, the original's message for it is
/// shown here and the search ends as it does for "no game found": back to
/// the title screen.
/// # Safety
/// `task` is the live game-search task, on the main thread.
unsafe extern "aapcs" fn check(task: *mut u8) -> u32 {
    // The loading panel goes up before anything is read, on a frame of its
    // own, as Bank's scan puts up its panel (`panel`). The worker's start
    // and the check follow once it has stood for 0.5 s (`spinner`).
    let Some(mut state) = STATE.take() else {
        return SEARCH_PENDING;
    };
    if let Some(ui) = unsafe { follow(task, TASK_UI) } {
        if unsafe { follow(ui, UI_CONTEXT) }.is_some() && PANEL.start(ui as usize) {
            unsafe { start_panel(ui) };
            state.spinner.up(now());
            return SEARCH_PENDING;
        }
    }
    if state.job.is_none() && state.spinner.idle(now()) {
        return SEARCH_PENDING;
    }
    let answer = unsafe { state.step(Job::Check, |_| Some(())) };
    drop(state);
    // A worker that failed refuses as a file that does not read does.
    match answer.map(|reply| reply.unwrap_or(MESSAGE_OPEN_BANK)) {
        None => SEARCH_PENDING,
        Some(0) => SEARCH_GO,
        Some(message) => {
            if unsafe { show(task, message) } {
                SEARCH_SHOWN
            } else {
                SEARCH_END
            }
        }
    }
}

// The first instruction of three updates (was: their push): reading the
// chosen game (002455D0), the question (00247304) and the transfer
// (0024A0C4). r0 = task, lr = the caller's. While the entry answers 1 the
// update returns 0, as on a frame it has nothing to do; else the instruction
// is run here and the update goes on with every register as it came. The
// six pushed words keep the stack 8-byte aligned.
macro_rules! update_stub {
    ($name:ident, $step:expr, $push:literal, $resume:literal) => {
        patch_common::stub!(
            $name,
            [
                "push {{r0-r3, r12, lr}}",
                "mov r1, #{step}",
                "bl {update_waits}",
                "cmp r0, #0",
                "pop {{r0-r3, r12, lr}}",
                "movne r0, #0",
                "bxne lr",
                $push,
                concat!("ldr r12, =", $resume),
                "bx r12",
            ],
            update_waits = sym update_waits,
            step = const $step,
        );
    };
}
update_stub!(
    transporter_read_stub,
    navigation::READ,
    "push {{r4-r11, lr}}",
    "0x002455d4"
);
update_stub!(
    transporter_question_stub,
    navigation::QUESTION,
    "push {{r3-r11, lr}}",
    "0x00247308"
);
update_stub!(
    transporter_transfer_stub,
    navigation::TRANSFER,
    "push {{r4, r5, r6, lr}}",
    "0x0024a0c8"
);
/// The first instruction of the update of `step`: reading the chosen game,
/// the question, or the transfer. 1 while this frame waits for the loading
/// panel (`panel::waits`): the update returns 0 without running, as on a
/// frame it has nothing to do. 0 runs it. A transfer the original is ending
/// is not held up. The frame the question's first state runs, which takes
/// down the panel of the reading, lets HOME and sleep through.
/// # Safety
/// `task` is the live task of `step`, on the main thread.
unsafe extern "aapcs" fn update_waits(task: *mut u8, step: u32) -> u32 {
    if step == navigation::TRANSFER && unsafe { task.add(TRANSFER_ENDING).read() } != 0 {
        return 0;
    }
    let Some(mut state) = STATE.take() else {
        return 0;
    };
    let now = now();
    // The panel of reading the game or of the transfer.
    state.spinner.seen(now);
    let task_state = unsafe { task.add(TASK_STATE).cast::<u32>().read() };
    let waits = panel::waits(
        step,
        task_state,
        state.spinner.idle(now),
        state.spinner.held(now),
    );
    if !waits && panel::ends_panel(step, task_state) {
        refuse_home(false);
    }
    u32::from(waits)
}

/// Shows an original message the way the game search shows its own. It
/// waits for an answer, so HOME and sleep work on it (`message_waits`).
/// `false` when the task's dialog owner is not a pointer.
unsafe fn show(task: *const u8, message: u32) -> bool {
    let Some(ui) = (unsafe { follow(task, TASK_UI) }) else {
        return false;
    };
    let show: unsafe extern "aapcs" fn(*const u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
    unsafe { show(ui, message, 1) };
    true
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
    unsafe {
        let load: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) -> u32 = transmute(LOAD_MESSAGE);
        let loaded = load(messages, message, string);
        if from_second_page(message) {
            patch_common::ui::keep_rest(string);
        }
        loaded
    }
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
unsafe extern "aapcs" fn deliver(task: *mut u8) -> u32 {
    let Some(mut state) = STATE.take() else {
        return DELIVER_PENDING;
    };
    if state.job.is_none() {
        let Some(manager) = (unsafe { follow(task, TASK_MANAGER) }) else {
            return DELIVER_FAILED;
        };
        if unsafe { cart::chosen_card_missing(manager) } {
            return DELIVER_FAILED;
        }
    }
    // The transport box goes to the worker in its staging buffer, copied
    // here: the worker touches no object of the original.
    let answer = unsafe {
        state.step(Job::Deliver, |worker| {
            let (records, tags) = native_box(task)?;
            let staging = worker.staging_mut().ok()?;
            staging[..RECORDS_LEN].copy_from_slice(records);
            staging[RECORDS_LEN..].copy_from_slice(tags);
            Some(())
        })
    };
    match answer {
        None => DELIVER_PENDING,
        Some(Ok(1)) => DELIVER_DONE,
        Some(_) => DELIVER_FAILED,
    }
}

/// Replaces the title screen's call that writes its version line
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
