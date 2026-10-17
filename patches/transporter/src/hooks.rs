//! The entry points called from the patched original code. The original
//! task calls an entry every frame until it answers. The file work runs on
//! the patch's one worker thread (`storage_worker`), which the first START
//! starts; the main thread never waits for it.

use crate::{
    layout::*,
    navigation,
    storage_worker::{DeliveryWorker, Failed, Job, Reply},
};
use core::{mem::transmute, slice};
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
    unsafe { refuse_home(false) };
}

/// The game list is set up and on screen: every save has been read, and it
/// waits for the user.
/// # Safety
/// Called only from the stub of the patched site at 002445EC (link.rs), on
/// the main thread.
#[no_mangle]
pub unsafe extern "aapcs" fn transporter_list_ready() {
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
