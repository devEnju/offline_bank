//! Save-only import of the selected game's native Pokédex and adventure records.
//!
//! Native 0026de9c builds the trainer/record context. Native 0026d8ec already
//! provides a replacement prompt for a different trainer. The three hooks below
//! enter that prompt only after Save and Quit; ordinary records visits are native.
//! No function in this module writes a game file or the offline Bank store.

use offline_core::{crc32, native_blob::BLOB_SIZE};

pub const RECORDS_OFFSET: usize = 0xAD61C;
pub const RECORD_SIZE: usize = 0x44;
pub const DEX_OFFSET: usize = 0xAD83C;
pub const DEX_SIZE: usize = 0x7260;
pub const VALIDITY_OFFSET: usize = DEX_OFFSET + 0x7254;
const FAMILY_OFFSETS: [usize; 9] = [
    8, 0x6A8, 0xD48, 0x1F18, 0x30E8, 0x4060, 0x4FE0, 0x5F58, 0x6F18,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DexError {
    NativeObject,
    UnsupportedFormat,
    ChangedSelection,
    ConfirmationRequired,
    UnexpectedPromptState,
}

#[cfg(any(test, target_arch = "arm"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    Empty,
    Same,
    Different,
}

#[cfg(any(test, target_arch = "arm"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    None,
    Accepted,
    Declined,
}

#[cfg(any(test, target_arch = "arm"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Import {
    Copy,
    Preserve,
}

#[cfg(any(test, target_arch = "arm"))]
fn import_policy(identity: Identity, decision: Decision) -> Result<Import, DexError> {
    if decision == Decision::Declined {
        return Ok(Import::Preserve);
    }
    match (identity, decision) {
        (Identity::Empty | Identity::Same, _) | (_, Decision::Accepted) => Ok(Import::Copy),
        (Identity::Different, Decision::None) => Err(DexError::ConfirmationRequired),
        _ => unreachable!(),
    }
}

#[cfg(any(test, target_arch = "arm"))]
fn native_prompt_choice_consumed(step: u8, pane_mode: u8) -> bool {
    step == 8 || (step == 3 && pane_mode == 0)
}

/// Diagnostic values for the actual native regions, without storing Pokémon or
/// trainer data in logs. Each family digest includes its records and Dex bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub validity_mask: u8,
    pub dex_crc32: u32,
    pub family_crc32: [u32; 8],
}

pub fn evidence(body: &[u8]) -> Result<Evidence, DexError> {
    if body.len() != BLOB_SIZE {
        return Err(DexError::NativeObject);
    }
    let mut families = [0; 8];
    for (index, value) in families.iter_mut().enumerate() {
        let records = RECORDS_OFFSET + index * RECORD_SIZE;
        let dex = DEX_OFFSET + FAMILY_OFFSETS[index];
        let end = DEX_OFFSET + FAMILY_OFFSETS[index + 1];
        // Keep both CRCs in one unambiguous fixed-size input.
        let mut parts = [0; 8];
        parts[..4].copy_from_slice(&crc32(&body[records..records + RECORD_SIZE]).to_le_bytes());
        parts[4..].copy_from_slice(&crc32(&body[dex..end]).to_le_bytes());
        *value = crc32(&parts);
    }
    Ok(Evidence {
        validity_mask: body[VALIDITY_OFFSET],
        dex_crc32: crc32(&body[DEX_OFFSET..DEX_OFFSET + DEX_SIZE]),
        family_crc32: families,
    })
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::native_bank::NativeBank;
    use core::{cell::UnsafeCell, mem::transmute};

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Key {
        manager: u32,
        bank: u32,
        game: u32,
        family: u8,
        name: [u16; 13],
        trainer_id: u32,
        extra_id: u32,
    }

    #[derive(Clone, Copy)]
    enum PromptPhase {
        Opening,
        Showing,
        Waiting,
        Closing,
        Leaving,
    }

    #[derive(Clone, Copy)]
    struct Gate {
        parent: u32,
        key: Key,
        phase: PromptPhase,
    }

    #[derive(Clone, Copy)]
    struct State {
        gate: Option<Gate>,
        decision: Option<(Key, Decision)>,
        failure: Option<DexError>,
        before_import: Option<Evidence>,
        after_import: Option<Evidence>,
        after_restore: Option<Evidence>,
    }
    struct MainThread(UnsafeCell<State>);
    // SAFETY: every entry point is restricted to the native UI thread. The
    // background storage worker receives frozen byte copies, never this state.
    unsafe impl Sync for MainThread {}
    static STATE: MainThread = MainThread(UnsafeCell::new(State {
        gate: None,
        decision: None,
        failure: None,
        before_import: None,
        after_import: None,
        after_restore: None,
    }));

    fn read_state() -> State {
        // SAFETY: main-thread-only API; copying avoids holding a borrow across
        // native calls and their callbacks.
        unsafe { *STATE.0.get() }
    }
    fn write_state(state: State) {
        unsafe { *STATE.0.get() = state }
    }
    unsafe fn word(base: *mut u8, offset: usize) -> u32 {
        unsafe { base.add(offset).cast::<u32>().read() }
    }
    fn pointer(value: u32) -> Result<*mut u8, DexError> {
        if value == 0 || value & 3 != 0 {
            Err(DexError::NativeObject)
        } else {
            Ok(value as *mut u8)
        }
    }
    unsafe fn helpers(bank: *mut u8) -> Result<(*mut u8, *mut u8), DexError> {
        pointer(bank as u32)?;
        {
            let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
            let body = view.snapshot().map_err(|_| DexError::NativeObject)?;
            if body.as_bytes()[DEX_OFFSET + 4..DEX_OFFSET + 8] != [0; 4] {
                return Err(DexError::UnsupportedFormat);
            }
        }
        let records = pointer(unsafe { word(bank, 0xBB52C) })?;
        let dex = pointer(unsafe { word(records, 8) })?;
        if unsafe { word(records, 4) } != bank as u32 || unsafe { word(dex, 4) } != bank as u32 {
            return Err(DexError::NativeObject);
        }
        for offset in (8..=0x24).step_by(4) {
            pointer(unsafe { word(dex, offset) })?;
        }
        Ok((records, dex))
    }
    unsafe fn current() -> Result<(*mut u8, *mut u8), DexError> {
        let application = pointer(unsafe { (0x003A_B90C as *const u32).read() })?;
        let manager = pointer(unsafe { word(application, 0x1C) })?;
        let bank = pointer(unsafe { word(manager, 0xCC) })?;
        Ok((manager, bank))
    }

    /// Only the fields populated by native 0026de9c are used. That function
    /// neither requires a constructed UI nor accesses its uninitialized fields.
    #[repr(C, align(4))]
    struct Selection {
        words: [u32; 0x94 / 4],
    }
    impl Selection {
        unsafe fn capture(manager: *mut u8, bank: *mut u8) -> Result<Self, DexError> {
            let (actual_manager, actual_bank) = unsafe { current() }?;
            if (manager, bank) != (actual_manager, actual_bank) {
                return Err(DexError::ChangedSelection);
            }
            unsafe { helpers(bank) }?;
            let get_game: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
                unsafe { transmute(0x0023_3A6Cusize) };
            let game = unsafe { get_game(manager) };
            pointer(game as u32)?;
            if !(1..=8).contains(&unsafe { game.add(8).read() }) {
                return Err(DexError::NativeObject);
            }
            let mut selected = Self {
                words: [0; 0x94 / 4],
            };
            // Unmapped trainer game versions must not default to family X.
            unsafe { selected.raw().add(0x4C).write(0xFF) };
            let collect: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x0026_DE9Cusize) };
            unsafe { collect(selected.raw()) };
            if unsafe { selected.raw().add(0x4C).read() } >= 8
                || unsafe { word(selected.raw(), 0x48) } != unsafe { word(bank, 0xBB52C) }
            {
                return Err(DexError::NativeObject);
            }
            pointer(unsafe { word(selected.raw(), 0x40) })?;
            Ok(selected)
        }
        fn raw(&mut self) -> *mut u8 {
            self.words.as_mut_ptr().cast()
        }
        unsafe fn key(&mut self, manager: *mut u8, bank: *mut u8) -> Key {
            let get_game: unsafe extern "aapcs" fn(*mut u8) -> *mut u8 =
                unsafe { transmute(0x0023_3A6Cusize) };
            let mut name = [0; 13];
            for (index, unit) in name.iter_mut().enumerate() {
                *unit = unsafe { self.raw().add(0x4E + index * 2).cast::<u16>().read() };
            }
            Key {
                manager: manager as u32,
                bank: bank as u32,
                game: unsafe { get_game(manager) } as u32,
                family: unsafe { self.raw().add(0x4C).read() },
                name,
                trainer_id: unsafe { word(self.raw(), 0x68) },
                extra_id: unsafe { word(self.raw(), 0x6C) },
            }
        }
        unsafe fn identity(&mut self) -> Identity {
            unsafe { identity_of(self.raw()) }
        }
        unsafe fn import(&mut self) {
            let records = unsafe { word(self.raw(), 0x48) } as *mut u8;
            let family = unsafe { self.raw().add(0x4C).read() } as u32;
            let identity: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8, u32, u32) =
                unsafe { transmute(0x002B_8E00usize) };
            let copy_records: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) =
                unsafe { transmute(0x001E_1BA8usize) };
            let copy_dex: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) =
                unsafe { transmute(0x001E_1B94usize) };
            unsafe {
                identity(
                    records,
                    family,
                    self.raw().add(0x4E),
                    word(self.raw(), 0x68),
                    word(self.raw(), 0x6C),
                );
                copy_records(records, family, self.raw().add(0x70));
                copy_dex(records, family, word(self.raw(), 0x40) as *mut u8);
            }
        }
    }

    unsafe fn identity_of(context: *mut u8) -> Identity {
        let records = unsafe { word(context, 0x48) } as *mut u8;
        let family = unsafe { context.add(0x4C).read() } as u32;
        let exists: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
            unsafe { transmute(0x001E_1E58usize) };
        let matches: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8, u32, u32) -> u32 =
            unsafe { transmute(0x002C_B7B8usize) };
        if unsafe { exists(records, family) } == 0 {
            Identity::Empty
        } else if unsafe {
            matches(
                records,
                family,
                context.add(0x4E),
                word(context, 0x68),
                word(context, 0x6C),
            )
        } != 0
        {
            Identity::Same
        } else {
            Identity::Different
        }
    }

    /// Reloads all eight native family objects and invalidates aggregate caches.
    /// Call after a validated persisted-body restore, before returning to native
    /// views. This also discards decisions belonging to the previous session.
    ///
    /// # Safety
    /// Main UI thread only; bank must be the live, exclusively accessible native
    /// object for the verified binary. No storage worker may access it.
    pub unsafe fn refresh_after_restore(bank: *mut u8) -> Result<(), DexError> {
        let (_, dex) = unsafe { helpers(bank) }?;
        let reload: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002B_9A3Cusize) };
        let invalidate: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002B_9B90usize) };
        unsafe {
            reload(dex);
            invalidate(dex);
        }
        let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
        let restored = evidence(
            view.snapshot()
                .map_err(|_| DexError::NativeObject)?
                .as_bytes(),
        )?;
        let mut state = read_state();
        state.gate = None;
        state.decision = None;
        state.failure = None;
        state.after_restore = Some(restored);
        write_state(state);
        Ok(())
    }

    /// Discards prompt decisions of a previous session without touching the
    /// Bank. For a freshly created Bank whose live body is already current.
    ///
    /// # Safety
    /// Call only on Bank's UI thread while no other Dex hook is executing.
    pub unsafe fn reset_session() {
        let mut state = read_state();
        state.gate = None;
        state.decision = None;
        state.failure = None;
        write_state(state);
    }

    /// Last save-import and restore digests for development diagnostics.
    ///
    /// # Safety
    /// Call only on Bank's UI thread while no other Dex hook is executing.
    pub unsafe fn diagnostics() -> (Option<Evidence>, Option<Evidence>, Option<Evidence>) {
        let state = read_state();
        (state.before_import, state.after_import, state.after_restore)
    }

    /// Imports records and Dex immediately before freezing the Bank snapshot.
    /// A declined replacement keeps the prior family. An unconfirmed mismatch
    /// fails before a journal or game write can start.
    ///
    /// # Safety
    /// Main UI thread only, after the native Save-and-Quit confirmation gate.
    /// manager and bank must be the selected session's exclusively accessible
    /// native objects, with no outstanding game/storage worker access.
    pub unsafe fn sync_before_save(manager: *mut u8, bank: *mut u8) -> Result<(), DexError> {
        let mut state = read_state();
        if let Some(error) = state.failure {
            return Err(error);
        }
        if state.gate.is_some() {
            return Err(DexError::UnexpectedPromptState);
        }
        let mut selected = unsafe { Selection::capture(manager, bank) }?;
        let key = unsafe { selected.key(manager, bank) };
        let decision = match state.decision {
            None => Decision::None,
            Some((previous, choice)) if previous == key => choice,
            Some(_) => return Err(DexError::ChangedSelection),
        };
        let action = import_policy(unsafe { selected.identity() }, decision)?;
        let before = {
            let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
            evidence(
                view.snapshot()
                    .map_err(|_| DexError::NativeObject)?
                    .as_bytes(),
            )?
        };
        if action == Import::Copy {
            unsafe { selected.import() };
        }
        let after = {
            let view = unsafe { NativeBank::from_raw(bank) }.map_err(|_| DexError::NativeObject)?;
            evidence(
                view.snapshot()
                    .map_err(|_| DexError::NativeObject)?
                    .as_bytes(),
            )?
        };
        // Native import is selected-family-only. Stop publication if an ABI or
        // address error ever changes another family's records or Dex bytes.
        for index in 0..8 {
            if index != key.family as usize
                && before.family_crc32[index] != after.family_crc32[index]
            {
                return Err(DexError::NativeObject);
            }
        }
        if action == Import::Preserve && before != after {
            return Err(DexError::NativeObject);
        }
        state.before_import = Some(before);
        state.after_import = Some(after);
        write_state(state);
        Ok(())
    }

    unsafe fn request(parent: *mut u8) -> Result<bool, DexError> {
        let (manager, bank) = unsafe { current() }?;
        let mut selected = unsafe { Selection::capture(manager, bank) }?;
        let key = unsafe { selected.key(manager, bank) };
        let mut state = read_state();
        state.failure = None;
        state.decision = None;
        state.gate = None;
        if unsafe { selected.identity() } != Identity::Different {
            write_state(state);
            return Ok(false);
        }
        state.gate = Some(Gate {
            parent: parent as u32,
            key,
            phase: PromptPhase::Opening,
        });
        write_state(state);
        // Same native route as BoxGUI choice6. State0 detects the mismatch;
        // native mode3 then displays the existing accept/decline prompt.
        unsafe {
            parent.add(0x40).write(5);
            parent.add(0x11C).write(0);
            parent.add(0x120).cast::<u32>().write(0);
            parent.add(0x124).write(1);
        }
        Ok(true)
    }

    /// # Safety
    /// Verified BoxGUI child-completion slot0033d30c supplies its live parent on
    /// the main thread, after the native Save-and-Quit choice has completed.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_offline_dex_save_request(parent: *mut u8) {
        let original: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002A_7578usize) };
        if unsafe { parent.add(0x5A).read() } == 1 {
            match unsafe { request(parent) } {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    let mut state = read_state();
                    state.failure = Some(error);
                    write_state(state);
                    // Continue to task7, whose save entry reports this failure
                    // before any snapshot, game write, or store publication.
                }
            }
        }
        unsafe { original(parent) };
    }

    /// # Safety
    /// Verified records-task vtable003600cc supplies its initialized task on the
    /// main UI thread. Ordinary visits have no gate and call the original once.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_offline_dex_records_update(task: *mut u8) -> u32 {
        let original: unsafe extern "aapcs" fn(*mut u8) -> u32 =
            unsafe { transmute(0x0026_D8ECusize) };
        let callback: unsafe extern "aapcs" fn(*mut u8, u32) =
            unsafe { transmute(0x0026_D7F0usize) };
        let mut state = read_state();
        let Some(mut gate) = state.gate else {
            return unsafe { original(task) };
        };
        if unsafe { word(task, 0x18) } != gate.parent + 0x11C {
            return unsafe { original(task) };
        }
        if unsafe { task.add(0x4C).read() } != gate.key.family
            || unsafe { word(task, 0x48) } != unsafe { word(gate.key.bank as *mut u8, 0xBB52C) }
        {
            state.failure = Some(DexError::ChangedSelection);
            state.decision = Some((gate.key, Decision::Declined));
            gate.phase = PromptPhase::Leaving;
            unsafe { callback(task.add(0x14), 10) };
        }
        let step = unsafe { task.add(0x46).read() };
        if matches!(gate.phase, PromptPhase::Showing | PromptPhase::Waiting) {
            if step == 6 || step == 7 {
                state.decision = Some((
                    gate.key,
                    if step == 6 {
                        Decision::Accepted
                    } else {
                        Decision::Declined
                    },
                ));
                gate.phase = PromptPhase::Closing;
            } else if step >= 9 {
                state.decision = Some((gate.key, Decision::Declined));
                gate.phase = PromptPhase::Leaving;
            }
        }
        state.gate = Some(gate);
        write_state(state);
        let result = unsafe { original(task) };
        let step = unsafe { task.add(0x46).read() };
        state = read_state();
        if matches!(gate.phase, PromptPhase::Showing | PromptPhase::Waiting) {
            // The native update itself dispatches UI callbacks before its
            // switch. Yes/No can therefore pass through step6/7 in this call.
            // Step8, or the restored mode0 menu, proves native handling ended.
            let ui = unsafe { word(task, 0x34) } as *mut u8;
            let mode = unsafe { ui.add(0x84).read() };
            if native_prompt_choice_consumed(step, mode) {
                if unsafe { task.add(0x44).read() } != gate.key.family {
                    state.failure = Some(DexError::ChangedSelection);
                }
                let choice = if unsafe { identity_of(task) } == Identity::Same {
                    Decision::Accepted
                } else {
                    Decision::Declined
                };
                state.decision = Some((gate.key, choice));
                gate.phase = PromptPhase::Closing;
            } else if step >= 9 {
                state.decision = Some((gate.key, Decision::Declined));
                gate.phase = PromptPhase::Leaving;
            }
        }
        if step == 3 {
            match gate.phase {
                PromptPhase::Opening => {
                    // Select the actual game family; mode3 exposes only native
                    // accept/decline controls, not the other family rows.
                    unsafe { callback(task.add(0x14), u32::from(gate.key.family) + 2) };
                    gate.phase = PromptPhase::Showing;
                }
                PromptPhase::Showing => gate.phase = PromptPhase::Waiting,
                PromptPhase::Closing => {
                    unsafe { callback(task.add(0x14), 10) };
                    gate.phase = PromptPhase::Leaving;
                }
                _ => {}
            }
        }
        state.gate = Some(gate);
        write_state(state);
        result
    }

    /// # Safety
    /// Verified BoxGUI records-completion slot0033d34c supplies its live parent
    /// on the UI thread after the records child has completed native cleanup.
    #[no_mangle]
    pub unsafe extern "aapcs" fn bank_offline_dex_records_finish(parent: *mut u8) {
        let mut state = read_state();
        let Some(gate) = state.gate.filter(|gate| gate.parent == parent as u32) else {
            let original: unsafe extern "aapcs" fn(*mut u8) =
                unsafe { transmute(0x002A_71A8usize) };
            unsafe { original(parent) };
            return;
        };
        if state.decision.is_none() {
            state.decision = Some((gate.key, Decision::Declined));
        }
        state.gate = None;
        write_state(state);
        let finish_save: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(0x002A_7578usize) };
        unsafe {
            parent.add(0x5A).write(1);
            finish_save(parent);
        }
    }
}
#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;

    fn fixture() -> std::vec::Vec<u8> {
        let mut body = vec![0; BLOB_SIZE];
        body[0x15C..0x15E].copy_from_slice(&2u16.to_le_bytes());
        body[0x15E..0x160].copy_from_slice(&100u16.to_le_bytes());
        body
    }

    #[test]
    fn another_trainer_requires_confirmation_and_decline_never_imports() {
        assert_eq!(
            import_policy(Identity::Different, Decision::None),
            Err(DexError::ConfirmationRequired)
        );
        assert_eq!(
            import_policy(Identity::Different, Decision::Accepted),
            Ok(Import::Copy)
        );
        for identity in [Identity::Empty, Identity::Same, Identity::Different] {
            assert_eq!(
                import_policy(identity, Decision::Declined),
                Ok(Import::Preserve)
            );
        }
        assert_eq!(
            import_policy(Identity::Empty, Decision::None),
            Ok(Import::Copy)
        );
        assert_eq!(
            import_policy(Identity::Same, Decision::None),
            Ok(Import::Copy)
        );
    }

    #[test]
    fn prompt_dispatch_can_consume_yes_or_no_within_one_native_update() {
        // The same native step3 means either waiting for Yes/No (mode3), or
        // finished fading back to the menu (mode0). Only the latter can close.
        assert!(!native_prompt_choice_consumed(3, 3));
        assert!(!native_prompt_choice_consumed(4, 3));
        assert!(!native_prompt_choice_consumed(5, 3));
        assert!(native_prompt_choice_consumed(8, 3));
        assert!(native_prompt_choice_consumed(3, 0));
        assert!(!native_prompt_choice_consumed(1, 0));
    }

    #[test]
    fn cancelled_session_does_not_publish_its_dex_and_decline_preserves_all_bytes() {
        let committed = fixture();
        let before = evidence(&committed).unwrap();
        let mut live = committed.clone();
        let action = import_policy(Identity::Different, Decision::Declined).unwrap();
        if action == Import::Copy {
            live[DEX_OFFSET + FAMILY_OFFSETS[1]] = 7;
        }
        assert_eq!(live, committed);
        live[DEX_OFFSET + FAMILY_OFFSETS[1]] = 7;
        live[VALIDITY_OFFSET] |= 2;
        assert_ne!(evidence(&live).unwrap(), before);
        // Cancellation reloads committed data, retaining no uncommitted import.
        live.copy_from_slice(&committed);
        assert_eq!(evidence(&live).unwrap(), before);
    }
}
