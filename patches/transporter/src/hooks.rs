//! The entry points called from the patched original code. The original
//! task calls an entry every frame until it answers. The file work runs on
//! the patch's one worker thread (`storage_worker`), which the first START
//! starts; the main thread never waits for it.

use crate::{
    layout::*,
    navigation,
    panel::{self, Panel},
    storage_worker::{DeliveryWorker, Failed, Job, Reply},
};
use core::{cell::UnsafeCell, mem::transmute, slice};
use offline_core::sections::TRANSPORT_SLOTS;
use patch_common::{exclusive::Exclusive, worker::JobId};

/// The original's "show this message and wait": `(ui, message, 1)`, as the
/// game search calls it for its own message 0 (`00246FFC`).
const SHOW_MESSAGE: usize = 0x0019_b50c;
/// The title screen's "put this message into its pane": `(context, group,
/// pane, message)`, as it calls it for its version line, message 0x29
/// (`0024BAE8`). It loads the message into the first of the context's two
/// work strings (`[context + 4]`), fills in its values into the second, and
/// hands the second to the pane (`0019F834` finds it in the layout of the
/// group, `[[context + 0x1C] + 8 * group]`; `002375D0` sets its text).
const PANE_MESSAGE: usize = 0x001d_33cc;
const PANE_FIND: usize = 0x0019_f834;
const PANE_TEXT: usize = 0x0023_75d0;
const CONTEXT_STRINGS: usize = 0x4;
const CONTEXT_LAYOUTS: usize = 0x1c;
/// What the version line says behind "Ver. 1.5".
const VERSION_SUFFIX: &[u8] = b"-offline";
/// The original's router, `get_next_state(manager, current)`, and what it
/// reads: the finished task at `manager + 0x10` with its outcome byte at
/// `+0x30`, and the manager's "cancelled" flag, which it clears.
const NEXT_STATE: usize = 0x0024_2ba0;
const MANAGER_TASK: usize = 0x10;
const MANAGER_CANCELLED: usize = 0x1a;
const TASK_OUTCOME: usize = 0x30;
/// The original's HOME and sleep block: `0022AEEC(bit)` sets a bit of its
/// activity mask and `0011A5FC(bit)` clears it. While the mask is not zero
/// the original refuses both, as it did from its connect step to its
/// disconnect step.
const BLOCK_SET: usize = 0x0022_aeec;
const BLOCK_CLEAR: usize = 0x0011_a5fc;
/// Set by the original (0010AD84) when HOME is pressed while the mask is
/// zero, and nothing else is done for the press then. Its main loop
/// (001044D0) carries the press out some frames later, whatever the mask
/// says by then.
const HOME_ACCEPTED: *mut u8 = 0x002f_49e9 as *mut u8;
/// The general loading panel of the shared UI (`ui`, its context at
/// `ui + 0x5C`), as the reader's set-up starts it (`00246A2C`):
/// `0019B340(ui)` plays the loop sound `0x50011`, shows the panel (pane
/// `0x1D`) and binds its spinner (animation 0); `0019C700(ui, message)`
/// shows its text frame (pane `0x1E`, with pane `0x25` hidden) and puts
/// message `message` of text file 23 into pane `0x23`. The reader ends it
/// before its messages (`0024621C`..`00246250`): `0019B6B0(0x50011, 0,
/// -1)` stops the sound, `0019C884(context, 0, pane, shown)` hides the
/// panel, `0019B604(context, 0, animation, 0)` unbinds the spinner.
const PANEL_START: usize = 0x0019_b340;
const STOP_SOUND: usize = 0x0019_b6b0;
const PANE_SHOWN: usize = 0x0019_c884;
const UNBIND_ANIMATION: usize = 0x0019_b604;
/// `0019C814(ui)`: hides the panel, its text frame and the dialog frame
/// (panes `0x1D`, `0x1E`, `0x25`) and two panes of the second group, the
/// state the shared UI's constructor leaves it in (`0024B778`). The
/// question step ends the panel with it when no message follows
/// (`002473D8`).
const PANEL_HIDE: usize = 0x0019_c814;
const UI_CONTEXT: usize = 0x5c;
const LOADING_SOUND: u32 = 0x0005_0011;
const PANEL_PANE: u32 = 0x1d;
const SPINNER_ANIMATION: u32 = 0;
const TEXT_FRAME_PANE: u32 = 0x1e;
const OTHER_FRAME_PANE: u32 = 0x25;
const TEXT_PANE: u32 = 0x23;
/// The panel's text, as the game scan of Bank and the game list's own
/// indicator show it: message `0x20` of text file 39 ("Die Spielesoftware
/// wird überprüft… Bitte weder das System ausschalten, noch Karte oder SD
/// Card entfernen."), which the panel's own file 23 has not. A text file is
/// opened as the original gives a screen its messages (`0022EF30`, see
/// `language`): `(size, heap)` allocates on the UI's heap (`[[ui + 0x78] +
/// 4]`, as `0019C700` allocates), `(10)` names the text archive of the
/// screens' language, `(memory, archive, file, heap, 1, 5)` opens the file,
/// `(strings, layout, pane, messages, message)` copies a message into a
/// pane, and the object's second virtual function deletes it.
const ALLOCATE: usize = 0x001e_6360;
const MESSAGES_SIZE: u32 = 0x28;
const TEXT_ARCHIVE: usize = 0x001e_2540;
const OPEN_MESSAGES: usize = 0x001d_fb00;
const SET_PANE_TEXT: usize = 0x0019_f7b0;
const UI_HEAP: usize = 0x78;
const PANEL_TEXT_FILE: u32 = 39;
const PANEL_TEXT_MESSAGE: u32 = 0x20;

/// The original's clock, as `0019B758` reads it: `00233854(&moment)` copies
/// the current time (`002F4E38`) into an 8-byte moment, `0025A77C(&moment)`
/// gives it in ms.
const NOW: usize = 0x0023_3854;
const MILLISECONDS: usize = 0x0025_a77c;
/// Where the game list notes the time it began, in ms (`00245180`..
/// `0024524C`), for its minimum time of 3 s (`00244B24`).
const LIST_BEGAN: usize = 0x70;

/// A moment of the original's clock.
#[repr(C, align(8))]
struct Moment(UnsafeCell<[u32; 2]>);
// SAFETY: only the main thread uses it.
unsafe impl Sync for Moment {}
impl Moment {
    /// Notes the current time.
    unsafe fn note(&self) {
        let now: unsafe extern "aapcs" fn(*mut [u32; 2]) = unsafe { transmute(NOW) };
        unsafe { now(self.0.get()) };
    }
    /// The noted time in ms.
    unsafe fn milliseconds(&self) -> u32 {
        let ms: unsafe extern "aapcs" fn(*const [u32; 2]) -> u32 =
            unsafe { transmute(MILLISECONDS) };
        unsafe { ms(self.0.get()) }
    }
}
/// When the search's panel went up.
static PANEL_SINCE: Moment = Moment(UnsafeCell::new([0; 2]));

/// The panel the search started (`panel`).
static PANEL: Panel = Panel::new();

/// The worker and the job it has out. Only the main thread takes it.
struct Runtime {
    worker: Option<DeliveryWorker>,
    job: Option<(JobId, Job)>,
}
// SAFETY: the original calls the hooks, the only ones to take it, on its
// main thread.
static STATE: Exclusive<Runtime> = unsafe {
    Exclusive::new(Runtime {
        worker: None,
        job: None,
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
        stage: impl FnOnce(&mut DeliveryWorker) -> Option<()>,
    ) -> Option<Result<Reply, Failed>> {
        let Some((id, out)) = self.job else {
            let worker = match unsafe { DeliveryWorker::get(&mut self.worker) } {
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

/// A pointer the original would itself dereference: word aligned and inside
/// the process image or heap.
unsafe fn follow(base: *const u8, offset: usize) -> Option<*const u8> {
    let value = unsafe { base.add(offset).cast::<u32>().read() };
    if value & 3 != 0 || !(0x0010_0000..0x1000_0000).contains(&value) {
        return None;
    }
    Some(value as *const u8)
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

/// Starts the general loading panel on `ui` with the text of the game
/// scan, as the reader's set-up starts it.
unsafe fn start_panel(ui: *const u8) {
    let start: unsafe extern "aapcs" fn(*const u8) = unsafe { transmute(PANEL_START) };
    unsafe {
        start(ui);
        panel_text(ui);
        PANEL_SINCE.note();
    }
}

/// The panel's text frame with message `0x20` of text file 39, as
/// `0019C700` shows one of file 23.
unsafe fn panel_text(ui: *const u8) {
    unsafe {
        let Some(context) = follow(ui, UI_CONTEXT) else {
            return;
        };
        let (Some(strings), Some(layouts), Some(heap)) = (
            follow(context, CONTEXT_STRINGS),
            follow(context, CONTEXT_LAYOUTS),
            follow(ui, UI_HEAP).and_then(|owner| follow(owner, 4)),
        ) else {
            return;
        };
        let Some(layout) = follow(layouts, 0) else {
            return;
        };
        let pane: unsafe extern "aapcs" fn(*const u8, u32, u32, u32) = transmute(PANE_SHOWN);
        pane(context, 0, TEXT_FRAME_PANE, 1);
        pane(context, 0, OTHER_FRAME_PANE, 0);
        let allocate: unsafe extern "aapcs" fn(u32, *const u8) -> *mut u8 = transmute(ALLOCATE);
        let archive: unsafe extern "aapcs" fn(u32) -> u32 = transmute(TEXT_ARCHIVE);
        let open: unsafe extern "aapcs" fn(*mut u8, u32, u32, *const u8, u32, u32) -> *mut u8 =
            transmute(OPEN_MESSAGES);
        let set: unsafe extern "aapcs" fn(*const u8, *const u8, u32, *mut u8, u32) =
            transmute(SET_PANE_TEXT);
        let memory = allocate(MESSAGES_SIZE, heap);
        if memory.is_null() {
            return;
        }
        let messages = open(memory, archive(10), PANEL_TEXT_FILE, heap, 1, 5);
        if messages.is_null() {
            return;
        }
        set(strings, layout, TEXT_PANE, messages, PANEL_TEXT_MESSAGE);
        let delete: unsafe extern "aapcs" fn(*mut u8) =
            transmute(messages.cast::<*const usize>().read().add(1).read());
        delete(messages);
    }
}

/// Ends the general loading panel on `ui`, as the question step ends it
/// (`00247394`..`002473D8`): its sound, its spinner, then the shared UI's
/// resting state, so that neither its text frame nor its text is left.
unsafe fn end_panel(ui: *const u8) {
    let Some(context) = (unsafe { follow(ui, UI_CONTEXT) }) else {
        return;
    };
    unsafe {
        let stop: unsafe extern "aapcs" fn(u32, u32, i32) = transmute(STOP_SOUND);
        let pane: unsafe extern "aapcs" fn(*const u8, u32, u32, u32) = transmute(PANE_SHOWN);
        let unbind: unsafe extern "aapcs" fn(*const u8, u32, u32, u32) =
            transmute(UNBIND_ANIMATION);
        let hide: unsafe extern "aapcs" fn(*const u8) = transmute(PANEL_HIDE);
        stop(LOADING_SOUND, 0, -1);
        pane(context, 0, PANEL_PANE, 0);
        unbind(context, 0, SPINNER_ANIMATION, 0);
        hide(ui);
    }
}

/// Sets or clears bit 1 of the original's activity mask through the
/// original functions.
///
/// The mask only refuses new HOME presses. One accepted just before it was
/// set would still be carried out, so setting the mask also takes that press
/// back: accepting it did nothing but set the one byte cleared here. From
/// then on the original refuses every press itself, and the HOME Menu, and
/// closing Transporter from it, never meet a file being read or written.
unsafe fn refuse_home(refuse: bool) {
    let address = if refuse { BLOCK_SET } else { BLOCK_CLEAR };
    let mark: unsafe extern "aapcs" fn(u32) = unsafe { transmute(address) };
    unsafe { mark(1) };
    if refuse {
        unsafe { HOME_ACCEPTED.write_volatile(0) };
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
    unsafe { refuse_home(navigation::loads(next)) };
    // The search's panel stays into the game list, until the list is on
    // screen; after anything else it ends here.
    if !panel::handed_on(next) {
        if let Some(ui) = PANEL.release() {
            unsafe { end_panel(ui as *const u8) };
        }
    }
    next
}

/// The original's "show a message" function was entered with a message that
/// waits for an answer (the stub of the patched site at 0019B50C, link.rs).
/// Such a message is where a step that reads or writes has nothing left to
/// do but end, so the refusal the router entry began for it ends here. The
/// original's own bit for a Virtual Console save that is being written is
/// not this one and stays as it is.
/// # Safety
/// Called only from that stub, on the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_message_waits() {
    // A refusal or "Could not find a game" comes without the search's
    // panel, as the reader's messages come without its own.
    if let Some(ui) = PANEL.release() {
        unsafe { end_panel(ui as *const u8) };
    }
    unsafe { refuse_home(false) };
}

/// The game list's set-up is about to start its own indicator on the list's
/// screen it has just built. Answers 1 while the search's panel is up: the
/// list then starts nothing and only notes when it began, for its minimum
/// time (link.rs); 0: the list starts its own, as the original.
/// # Safety
/// Called only from the stub of the patched site at 00244EAC (link.rs), on
/// the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_indicator() -> u32 {
    u32::from(PANEL.list_starts() == panel::ListStart::Quiet)
}

/// The game list has noted when it began, while the search's panel is up
/// (link.rs): it counts its minimum time from when the panel went up
/// instead, so the panel is not kept 3 s longer than the list needs.
/// # Safety
/// Called only from the stub of the patched site at 00244EAC (link.rs), on
/// the main thread, with the live game-list task.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_started(task: *mut u8) {
    unsafe {
        let now = Moment(UnsafeCell::new([0; 2]));
        now.note();
        let elapsed = now.milliseconds().wrapping_sub(PANEL_SINCE.milliseconds());
        let began = task.add(LIST_BEGAN).cast::<u64>();
        began.write(panel::list_start(began.read(), elapsed));
    }
}

/// A frame of the game list's set-up, before its check that restarts its
/// indicator's sound (`00244D40`). Answers 1 while the search's panel is up,
/// whose sound plays: the check is skipped (link.rs).
/// # Safety
/// Called only from the stub of the patched site at 00244D40 (link.rs), on
/// the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_frame() -> u32 {
    u32::from(PANEL.up())
}

/// The game list is set up and on screen: every save has been read, and it
/// waits for the user. The panel that covered its loading ends here.
/// # Safety
/// Called only from the stub of the patched site at 002445EC (link.rs), on
/// the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_ready() {
    if let Some(ui) = PANEL.release() {
        unsafe { end_panel(ui as *const u8) };
    }
    unsafe { refuse_home(false) };
}

/// Asks, when START was pressed and before the original searches for games,
/// whether Bank can take a delivery: the question the original put to the
/// server after a game was chosen. If not, the original's message for it is
/// shown here and the search ends as it does for "no game found": back to
/// the title screen.
/// # Safety
/// Called only from the stub of the patched site at 00246F48 (link.rs) with
/// the live game-search task in `r0`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_check(task: *mut u8) -> u32 {
    // The loading panel goes up before anything is read: on the first
    // frame, before the check is handed over (`panel`).
    if let Some(ui) = unsafe { follow(task, TASK_UI) } {
        if unsafe { follow(ui, UI_CONTEXT) }.is_some() && PANEL.start(ui as usize) {
            unsafe { start_panel(ui) };
        }
    }
    let answer = {
        let Some(mut state) = STATE.take() else {
            return SEARCH_PENDING;
        };
        unsafe { state.step(Job::Check, |_| Some(())) }
    };
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

/// Shows an original message the way the game search shows its own. It
/// waits for an answer, so HOME and sleep work on it
/// (`transporter_message_waits`). `false` when the task's dialog owner is
/// not a pointer.
unsafe fn show(task: *const u8, message: u32) -> bool {
    let Some(ui) = (unsafe { follow(task, TASK_UI) }) else {
        return false;
    };
    let show: unsafe extern "aapcs" fn(*const u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
    unsafe { show(ui, message, 1) };
    true
}

/// Replaces the upload. Returns `DELIVER_DONE` when the delivery is written,
/// flushed, and read back; `DELIVER_PENDING` while that is in progress; and
/// `DELIVER_FAILED` when nothing was delivered, in which case the original
/// must not remove anything from the source game.
/// # Safety
/// Called only from the stub of the patched site at 0024A150 (link.rs) with
/// the live task in `r0`.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_deliver(task: *mut u8) -> u32 {
    let Some(mut state) = STATE.take() else {
        return DELIVER_PENDING;
    };
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
/// (`0024BAE8`): the original call, then the same line with the patch's name
/// behind it, added to the original's own string in place and handed to the
/// pane with the original's two calls. Without room for it the line stays
/// the original's.
/// # Safety
/// Called only from that site, with the arguments of the replaced call.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_version(
    context: *const u8,
    group: u32,
    pane: u32,
    message: u32,
) {
    unsafe {
        let original: unsafe extern "aapcs" fn(*const u8, u32, u32, u32) = transmute(PANE_MESSAGE);
        original(context, group, pane, message);
        let Some(text) = follow(context, CONTEXT_STRINGS).and_then(|strings| follow(strings, 4))
        else {
            return;
        };
        let (Some(data), Some(layouts)) = (follow(text, 4), follow(context, CONTEXT_LAYOUTS))
        else {
            return;
        };
        let data = data.cast_mut().cast::<u16>();
        let room = usize::from(text.add(8).cast::<u16>().read());
        if text.add(0xc).read() == 0 {
            return;
        }
        let mut len = 0;
        while len < room && data.add(len).read() != 0 {
            len += 1;
        }
        if len == 0 || len + VERSION_SUFFIX.len() >= room {
            return;
        }
        for (index, &unit) in VERSION_SUFFIX.iter().enumerate() {
            data.add(len + index).write(u16::from(unit));
        }
        len += VERSION_SUFFIX.len();
        data.add(len).write(0);
        text.cast_mut().add(0xa).cast::<u16>().write(len as u16);
        let find: unsafe extern "aapcs" fn(u32, u32) -> *const u8 = transmute(PANE_FIND);
        let set: unsafe extern "aapcs" fn(*const u8, *const u8, i32) = transmute(PANE_TEXT);
        let layout = layouts.add(group as usize * 8).cast::<u32>().read();
        let found = find(layout, pane);
        if !found.is_null() {
            set(found, text, -1);
        }
    }
}
