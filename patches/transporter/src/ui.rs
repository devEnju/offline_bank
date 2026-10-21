//! The original's screens as the hooks use them: the messages they show,
//! the general loading panel of the shared UI, and a pane's text from a
//! message file of the original. Every text is the original's.

use offline_core::transport::Refusal;

/// The original's message 3: "Your communication with Pokémon Bank did not
/// complete correctly during your last session. Please open Pokémon Bank
/// and perform the cleanup process before using Poké Transporter." The
/// original shows it where the server asked for that (`0025C964`).
pub const MESSAGE_OPEN_BANK: u32 = 3;
/// The original's message 5: "At least one Pokémon remains in the Transport
/// Box from your previous session. Please empty the Transport Box by using
/// Pokémon Bank before using Poké Transporter."
pub const MESSAGE_BOX_NOT_EMPTY: u32 = 5;

/// The original's message `0x0B`: "The server did not receive the data." and,
/// on its second page, "Please confirm that your Game Card is properly
/// inserted, and then try again." The transfer shows it when nothing was
/// delivered (sub-state `0x11`), the Bank step when the card is gone before
/// it saves its note.
pub const MESSAGE_FAILURE: u32 = 0x0b;

/// Whether the message `message` is shown without its first page. There is
/// no server any more, so the failure message keeps only its advice to check
/// the Game Card, the same in every language.
pub fn from_second_page(message: u32) -> bool {
    message == MESSAGE_FAILURE
}

/// The message that stops a session before it begins, for what Bank's
/// journal says (`transport::check`). Opening Bank shows or solves each
/// reason, so the message carries no numbers of its own.
pub fn refusal_message(refusal: Refusal) -> u32 {
    match refusal {
        // Pokémon are in the way: in Bank's box, or in a waiting delivery.
        Refusal::Occupied => MESSAGE_BOX_NOT_EMPTY,
        // No Bank yet, a Bank save that was not finished, or a file that
        // does not read: Bank before its first start, one not yet
        // converted, or one whose next save replaces the file.
        Refusal::NotReady => MESSAGE_OPEN_BANK,
    }
}

#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(target_arch = "arm")]
mod native {
    use core::mem::transmute;
    use patch_common::native::follow;

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
    pub const UI_CONTEXT: usize = 0x5c;
    const UI_HEAP: usize = 0x78;
    const CONTEXT_STRINGS: usize = 0x4;
    const CONTEXT_LAYOUTS: usize = 0x1c;
    const LOADING_SOUND: u32 = 0x0005_0011;
    const PANEL_PANE: u32 = 0x1d;
    const SPINNER_ANIMATION: u32 = 0;
    const TEXT_FRAME_PANE: u32 = 0x1e;
    const OTHER_FRAME_PANE: u32 = 0x25;
    const TEXT_PANE: u32 = 0x23;
    /// The panel's text, as the game scan of Bank and the game list's own
    /// indicator show it: message `0x20` of text file 39 ("Die Spielesoftware
    /// wird überprüft… Bitte weder das System ausschalten, noch Karte oder SD
    /// Card entfernen."), which the panel's own file 23 has not.
    const PANEL_TEXT_FILE: u32 = 39;
    const PANEL_TEXT_MESSAGE: u32 = 0x20;

    /// A text file of the original, opened as the original gives a screen its
    /// messages (`0022EF30`): `(size, heap)` allocates, `(10)` names the text
    /// archive of the screens' language, and `(memory, archive, file, heap, 1,
    /// 5)` opens the file. `(strings, layout, pane, messages, message)` copies a
    /// message into a text pane (`001A20C8` calls it with a screen's own
    /// messages). The object's second virtual function deletes it.
    const ALLOCATE: usize = 0x001e_6360;
    const MESSAGES_SIZE: u32 = 0x28;
    const TEXT_ARCHIVE: usize = 0x001e_2540;
    const OPEN_MESSAGES: usize = 0x001d_fb00;
    const SET_PANE_TEXT: usize = 0x0019_f7b0;

    /// Gives text pane `pane` of `layout` message `message` of text file `file`
    /// in the language of the screens: opens the file on `heap`, sets the
    /// pane's text from it, which copies the string, and deletes the file
    /// object again. `strings` are the work strings of the layout's screen.
    /// # Safety
    /// The arguments are live objects of the original, on the main thread.
    pub unsafe fn message_text(
        heap: *mut u8,
        strings: *mut u8,
        layout: *mut u8,
        pane: u32,
        file: u32,
        message: u32,
    ) {
        unsafe {
            let allocate: unsafe extern "aapcs" fn(u32, *mut u8) -> *mut u8 = transmute(ALLOCATE);
            let archive: unsafe extern "aapcs" fn(u32) -> u32 = transmute(TEXT_ARCHIVE);
            let open: unsafe extern "aapcs" fn(*mut u8, u32, u32, *mut u8, u32, u32) -> *mut u8 =
                transmute(OPEN_MESSAGES);
            let set: unsafe extern "aapcs" fn(*mut u8, *mut u8, u32, *mut u8, u32) =
                transmute(SET_PANE_TEXT);
            let memory = allocate(MESSAGES_SIZE, heap);
            if memory.is_null() {
                return;
            }
            let messages = open(memory, archive(10), file, heap, 1, 5);
            if messages.is_null() {
                return;
            }
            set(strings, layout, pane, messages, message);
            let delete: unsafe extern "aapcs" fn(*mut u8) =
                transmute(messages.cast::<*const usize>().read().add(1).read());
            delete(messages);
        }
    }

    /// The original's "show this message and wait": `(ui, message, 1)`, as
    /// the game search calls it for its own message 0 (`00246FFC`).
    const SHOW_MESSAGE: usize = 0x0019_b50c;
    /// `(messages, message, string)`: loads a message of the shared UI's
    /// text file into a string object (`001DBEB8`), as "show a message" does
    /// before it hands the string to its dialog (`0019B578`).
    const LOAD_MESSAGE: usize = 0x001d_beb8;

    /// Shows message `message` of the shared UI on `ui` and waits for an
    /// answer, as the game search shows its own.
    /// # Safety
    /// `ui` is the live shared UI, on the main thread.
    pub unsafe fn show(ui: *mut u8, message: u32) {
        let show: unsafe extern "aapcs" fn(*mut u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
        unsafe { show(ui, message, 1) };
    }

    /// The original's load of `message` into `string`, as "show a message"
    /// makes it; the original's result.
    /// # Safety
    /// The arguments are those of the original's call (`0019B578`).
    pub unsafe fn load_message(messages: *mut u8, message: u32, string: *mut u8) -> u32 {
        let load: unsafe extern "aapcs" fn(*mut u8, u32, *mut u8) -> u32 =
            unsafe { transmute(LOAD_MESSAGE) };
        unsafe { load(messages, message, string) }
    }

    /// Starts the general loading panel on `ui` with the text of the game
    /// scan, as the reader's set-up starts it.
    /// # Safety
    /// `ui` is the live shared UI with its context, on the main thread.
    pub unsafe fn loading(ui: *mut u8) {
        let start: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(PANEL_START) };
        unsafe {
            start(ui);
            panel_text(ui);
        }
    }

    /// The panel's text frame with message `0x20` of text file 39, as
    /// `0019C700` shows one of file 23, on the UI's heap (`[[ui + 0x78] + 4]`,
    /// as `0019C700` allocates).
    unsafe fn panel_text(ui: *mut u8) {
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
            let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(PANE_SHOWN);
            pane(context, 0, TEXT_FRAME_PANE, 1);
            pane(context, 0, OTHER_FRAME_PANE, 0);
            message_text(
                heap,
                strings,
                layout,
                TEXT_PANE,
                PANEL_TEXT_FILE,
                PANEL_TEXT_MESSAGE,
            );
        }
    }

    /// Ends the general loading panel on `ui`, as the question step ends it
    /// (`00247394`..`002473D8`): its sound, its spinner, then the shared UI's
    /// resting state, so that neither its text frame nor its text is left.
    /// # Safety
    /// `ui` is the live shared UI the panel was started on, on the main thread.
    pub unsafe fn stop_loading(ui: *mut u8) {
        let Some(context) = (unsafe { follow(ui, UI_CONTEXT) }) else {
            return;
        };
        unsafe {
            let stop: unsafe extern "aapcs" fn(u32, u32, i32) = transmute(STOP_SOUND);
            let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(PANE_SHOWN);
            let unbind: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
                transmute(UNBIND_ANIMATION);
            let hide: unsafe extern "aapcs" fn(*mut u8) = transmute(PANEL_HIDE);
            stop(LOADING_SOUND, 0, -1);
            pane(context, 0, PANEL_PANE, 0);
            unbind(context, 0, SPINNER_ANIMATION, 0);
            hide(ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_failure_message_loses_its_first_page() {
        assert!(from_second_page(MESSAGE_FAILURE));
        // The refusals, the no-card message 0x22, and the closing message
        // of a transfer are shown whole.
        for message in [0, MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY, 0x0a, 0x22] {
            assert!(!from_second_page(message), "{message:#x}");
        }
    }

    #[test]
    fn the_check_tells_pokemon_in_the_way_from_a_bank_that_is_not_ready() {
        // The "remains in the Transport Box" message only when one does.
        assert_eq!(refusal_message(Refusal::Occupied), MESSAGE_BOX_NOT_EMPTY);
        assert_eq!(refusal_message(Refusal::NotReady), MESSAGE_OPEN_BANK);
        // The worker reports "go on" as 0, which no message may be.
        assert_eq!((MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY), (3, 5));
    }
}
