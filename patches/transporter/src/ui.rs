//! The original's screens as the hooks use them: the messages they show,
//! the general loading panel of the shared UI, a pane's text from a message
//! file of the original, and the error screen, the same as Bank's. Every
//! text is the original's, and on the error screen the version above the
//! two numbers.

use crate::sdcard::WriteFault;
use offline_core::transport::Refusal;

/// The original's message 3: "Your communication with Pokémon Bank did not
/// complete correctly during your last session. Please open Pokémon Bank
/// and perform the cleanup process before using Poké Transporter." The
/// original shows it where the server asked for that (`0025C964`); the
/// patch only at START (`refusal_message`).
pub const MESSAGE_OPEN_BANK: u32 = 3;
/// The original's message 5: "At least one Pokémon remains in the Transport
/// Box from your previous session. Please empty the Transport Box by using
/// Pokémon Bank before using Poké Transporter." Only at START.
pub const MESSAGE_BOX_NOT_EMPTY: u32 = 5;

/// The original's message `0x0B`: "The server did not receive the data." and,
/// on its second page, "Please confirm that your Game Card is properly
/// inserted, and then try again." The transfer shows it when nothing was
/// delivered (sub-state `0x11`), the Bank task when the card is gone before
/// it saves its note.
pub const MESSAGE_FAILURE: u32 = 0x0b;

/// The original's message `0x22`: "No compatible Pokémon Game Card
/// detected." and "Please check that your Game Card is properly inserted
/// into your system's Game Card slot." The transfer shows it when the game
/// cannot be saved after the delivery or its card is gone.
pub const MESSAGE_NO_CARD: u32 = 0x22;

/// What "show a message" shows for `message` (`hooks::transporter_message_text`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// The message as the original loads it.
    Whole,
    /// Only what follows its first page: the failure message's advice to
    /// check the Game Card, as there is no server any more.
    SecondPage,
    /// "The SD card is full." in its place.
    SdFull,
    /// The error screen in its place in the same frame, with
    /// `Fault::GameWrite` and this second number (`hooks::saving`).
    Numbers(u32),
}

/// The game Pokémon are moved from, as a message about its save needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// A cartridge whose card is in, or a save on the SD card.
    CardIn,
    /// A cartridge whose card is gone, or a game not known.
    CardOut,
    /// A Virtual Console title, whose save the patch's hooks never see.
    VirtualConsole,
}

/// What is shown for `message`, when the last write to the game that failed
/// left `fault` with the console's `result` (`cart::take_write_fault`), the
/// game is `chosen`, `no_room` says that the last delivery did not happen
/// for lack of room on the SD card, and `note_save` says that the Bank task
/// shows it, after its note save.
///
/// The failure message stands for a note save that did not count or a
/// delivery that did not happen: nothing is in Bank, and the game holds its
/// Pokémon. A delivery or a save backup the SD card had no room for gets
/// "The SD card is full.". A note save that the SD card refused otherwise,
/// or that a write may have left half-written, or of a Virtual Console
/// title, which has no card to take out, is the error screen: the advice
/// about a Game Card does not help there, and the reader finds a damaged
/// save when the game is chosen again. Everything else is the advice to try
/// again: a cartridge, or a card that is gone.
///
/// The no-card message stands for a save after the delivery that failed or
/// a card that is gone: Bank holds the Pokémon, and the game may still hold
/// them. A card that is gone keeps it. A save that failed on the SD card, on
/// a cartridge whose card is still in, or of a Virtual Console title is the
/// error screen. Bank's own refusals are START's alone (`refusal_message`).
/// Every other message is the original's.
pub fn shown(
    message: u32,
    (fault, result): (WriteFault, u32),
    chosen: Chosen,
    no_room: bool,
    note_save: bool,
) -> Shown {
    let virtual_console = chosen == Chosen::VirtualConsole;
    match (message, fault) {
        (MESSAGE_FAILURE, _) if no_room => Shown::SdFull,
        (MESSAGE_FAILURE, WriteFault::Full) => Shown::SdFull,
        (MESSAGE_FAILURE, WriteFault::Untouched | WriteFault::Torn) if note_save => {
            Shown::Numbers(result)
        }
        (MESSAGE_FAILURE, WriteFault::None) if note_save && virtual_console => Shown::Numbers(0),
        (MESSAGE_FAILURE, _) => Shown::SecondPage,
        (MESSAGE_NO_CARD, WriteFault::Full | WriteFault::Untouched | WriteFault::Torn) => {
            Shown::Numbers(result)
        }
        (MESSAGE_NO_CARD, WriteFault::Cartridge) if chosen == Chosen::CardIn => {
            Shown::Numbers(result)
        }
        (MESSAGE_NO_CARD, WriteFault::None) if virtual_console => Shown::Numbers(0),
        _ => Shown::Whole,
    }
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
        // converted, or one whose next save replaces the file. A damaged
        // journal is the error screen (`storage_worker::check_reply`) and
        // never comes here; were it to, Bank says what is wrong.
        // A save left to the restore question never comes here
        // (`storage_worker::checked`); were it to, Bank says what to do.
        Refusal::NotReady | Refusal::Damaged | Refusal::Asked => MESSAGE_OPEN_BANK,
    }
}

/// The original's message 4, the loading text of its restore: "Communication
/// in your last session did not complete correctly. Performing cleanup
/// process now..."
pub const MESSAGE_CLEANUP: u32 = 4;

/// An object of the original a screen needs is not there, or not made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

#[cfg(target_arch = "arm")]
pub use native::*;

#[cfg(target_arch = "arm")]
mod native {
    use super::{Unavailable, MESSAGE_FAILURE};
    use crate::program::Transporter15;
    use core::{
        mem::transmute,
        sync::atomic::{AtomicBool, Ordering},
    };
    use patch_common::{
        native::follow,
        numbers::{self, Label},
        program::Program,
        ui::{self as shared, Text},
    };

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
    const PANE_SHOWN: usize = Transporter15::PANE_SHOWN;
    /// `0019C814(ui)`: hides the panel, its text frame and the dialog frame
    /// (panes `0x1D`, `0x1E`, `0x25`) and two panes of the second group, the
    /// state the shared UI's constructor leaves it in (`0024B778`). The
    /// question task ends the panel with it when no message follows
    /// (`002473D8`).
    const PANEL_HIDE: usize = 0x0019_c814;
    const TEXT_FRAME_PANE: u32 = 0x1e;
    const OTHER_FRAME_PANE: u32 = 0x25;
    const TEXT_PANE: u32 = 0x23;
    /// The panel's text, as the game scan of Bank and the game list's own
    /// indicator show it: message `0x20` of text file 39 ("Checking the Game
    /// Card... Don't turn off the power or remove the Game Card or SD card."),
    /// which the panel's own file 23 has not.
    const PANEL_TEXT_FILE: u32 = 39;
    const PANEL_TEXT_MESSAGE: u32 = 0x20;

    /// `(strings, layout, pane, messages, message)` copies a message into a
    /// text pane (`001A20C8` calls it with a screen's own messages).
    const SET_PANE_TEXT: usize = 0x0019_f7b0;

    /// Gives text pane `pane` of `layout` message `message` of text file `file`
    /// in the language of the screens: opens the file on `heap`, sets the
    /// pane's text from it, which copies the string, and deletes the file
    /// object again (`patch_common::ui::with_messages`). `strings` are the
    /// work strings of the layout's screen.
    /// # Safety
    /// The arguments are live objects of the original, on the main thread.
    pub unsafe fn message_text(
        heap: *mut u8,
        strings: *mut u8,
        layout: *mut u8,
        pane: u32,
        file: u32,
        message: u32,
    ) -> Result<(), Unavailable> {
        let set: unsafe extern "aapcs" fn(*mut u8, *mut u8, u32, *mut u8, u32) =
            unsafe { transmute(SET_PANE_TEXT) };
        unsafe {
            shared::with_messages::<Transporter15, _>(heap, file, |messages| {
                set(strings, layout, pane, messages, message)
            })
        }
        .ok_or(Unavailable)
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

    /// Shows message `message` of the shared UI on `ui` without waiting:
    /// `(ui, message, 0)`. Such a dialog does not end at its last page, and
    /// its page breaks wait for A by themselves, as Bank's flag 0
    /// (`00233FA8`, the same code as Bank's `0029749C`).
    unsafe fn show_still(ui: *mut u8, message: u32) {
        let show: unsafe extern "aapcs" fn(*mut u8, u32, u32) = unsafe { transmute(SHOW_MESSAGE) };
        unsafe { show(ui, message, 0) };
    }

    /// The shared UI's own string object (`ui + 0x8C`), in which "show a
    /// message" hands its text to the dialog (`0019B57C`); `(context,
    /// string)` hands a changed string to the dialog again, the call "show
    /// a message" ends its text set-up with (`0019C7D4`, at `0019B5B0`);
    /// `(context)` is one frame of a dialog (`0019B4BC`, the same code as
    /// Bank's `001d6600`).
    const UI_STRING: usize = 0x8c;
    const DIALOG_TEXT: usize = 0x0019_c7d4;
    const DIALOG_POLL: usize = 0x0019_b4bc;
    /// The original's message `0x27`, the start screen's line: "Press HOME to
    /// return to the HOME Menu". One line in every language of the original.
    const HOME_NOTICE: u32 = 0x27;
    /// The next load of the failure message is kept whole (`whole`), for
    /// the error screen's first part.
    static WHOLE: AtomicBool = AtomicBool::new(false);

    /// Whether the failure message being loaded is the error screen's, which
    /// keeps its first part (`stop`).
    pub fn whole() -> bool {
        WHOLE.load(Ordering::Relaxed)
    }

    /// The screen the patch ends on when nothing can go on, as Bank's
    /// (`bank_common::ui::stop`): one dialog that does not end, the first
    /// part of the failure message `0x0B` ("The server did not receive the
    /// data."), and after A the line about the HOME Menu with `label` on its
    /// second line and the two numbers on its third. Both are the
    /// original's, loaded into the UI's string object by "show a message":
    /// first the line, from which the page is made (`numbers::home_page`),
    /// then the failure message, which is cut and given the page, and handed
    /// to the dialog again. Where the page or the message with it does not
    /// fit, the line about the HOME Menu is shown alone, as the original has
    /// it: never a part of the screen.
    /// # Safety
    /// `ui` is the live shared UI, on the main thread, with no other dialog
    /// shown.
    pub unsafe fn stop(
        ui: *mut u8,
        label: &Label,
        first: u32,
        second: u32,
    ) -> Result<(), Unavailable> {
        let (context, object) = unsafe {
            (
                shared::context(ui).ok_or(Unavailable)?,
                follow(ui, UI_STRING).ok_or(Unavailable)?,
            )
        };
        unsafe { show_still(ui, HOME_NOTICE) };
        let Ok(Some(Text { room, len, .. })) = (unsafe { shared::text(object) }) else {
            return Ok(());
        };
        let Some(home) = numbers::home_page(&room[..len], label, first, second) else {
            return Ok(());
        };
        WHOLE.store(true, Ordering::Relaxed);
        unsafe { show_still(ui, MESSAGE_FAILURE) };
        WHOLE.store(false, Ordering::Relaxed);
        if let Ok(Some(Text {
            object: text,
            room,
            len,
        })) = unsafe { shared::text(object) }
        {
            if let Some(len) = numbers::error_pages(room, len, &home) {
                unsafe {
                    shared::end(text, room, len);
                    let hand: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(DIALOG_TEXT);
                    hand(context, text);
                }
                return Ok(());
            }
        }
        unsafe { show_still(ui, HOME_NOTICE) };
        Ok(())
    }

    /// One frame of the error screen's dialog: only kept alive, as Bank's
    /// task keeps its own; its pages turn inside the dialog.
    /// # Safety
    /// As `stop`, once the screen is up.
    pub unsafe fn keep_dialog(ui: *mut u8) {
        if let Some(context) = unsafe { shared::context(ui) } {
            let poll: unsafe extern "aapcs" fn(*mut u8) -> u32 = unsafe { transmute(DIALOG_POLL) };
            unsafe { poll(context) };
        }
    }

    /// The text file of the original's common server messages, and its
    /// message 0: "The SD card is full." and, on a line of its own, "You
    /// can't download data." Its first line is a sentence of its own in all
    /// ten languages.
    const SERVER_MESSAGES: u32 = 26;
    const SD_FULL: u32 = 0;

    /// Loads the first line of message 0 of text file 26, "The SD card is
    /// full.", into `string`, where "show a message" would have loaded its
    /// message from `messages`: the file is opened on the heap `messages`
    /// was made on (`+4`, `001DFB24`) as the original opens a screen's own
    /// (`patch_common::ui::with_messages`). `None`, with nothing loaded, if
    /// it cannot be opened.
    /// # Safety
    /// The arguments are those of the original's load (`0019B578`).
    pub unsafe fn sd_full(messages: *mut u8, string: *mut u8) -> Option<u32> {
        let heap = unsafe { follow(messages, 4) }?;
        let loaded = unsafe {
            shared::with_messages::<Transporter15, _>(heap, SERVER_MESSAGES, |file| {
                load_message(file, SD_FULL, string)
            })
        }?;
        unsafe { shared::keep_first_line(string) };
        Some(loaded)
    }

    /// The text of a loading panel: `0019C700(ui, message)` shows the text
    /// frame and puts message `message` of text file 23 into it, as the
    /// restore task does for its own panel (`00243FCC`).
    const PANEL_TEXT: usize = 0x0019_c700;

    /// Starts the general loading panel on `ui` with the restore's text,
    /// "Communication in your last session did not complete correctly.
    /// Performing cleanup process now..." (`MESSAGE_CLEANUP`), as the
    /// original's restore task starts it after its last Yes
    /// (`00243FBC`..`00243FCC`).
    /// # Safety
    /// `ui` is the live shared UI, on the main thread.
    pub unsafe fn cleanup(ui: *mut u8) {
        unsafe {
            let start: unsafe extern "aapcs" fn(*mut u8) = transmute(PANEL_START);
            start(ui);
            let text: unsafe extern "aapcs" fn(*mut u8, u32) = transmute(PANEL_TEXT);
            text(ui, super::MESSAGE_CLEANUP);
        }
    }

    /// Starts the general loading panel on `ui` with the text of the game
    /// scan, as the reader's set-up starts it. Without what the panel needs,
    /// or without its text, nothing of it stays on screen.
    /// # Safety
    /// `ui` is the live shared UI with its context, on the main thread.
    pub unsafe fn loading(ui: *mut u8) -> Result<(), Unavailable> {
        // What the panel and its text need, before anything is started.
        let (context, strings, layout, heap) = unsafe {
            let context = shared::context(ui).ok_or(Unavailable)?;
            let strings = shared::work_strings(context).ok_or(Unavailable)?;
            let layout = shared::layout(context, 0).ok_or(Unavailable)?;
            let heap = shared::heap(ui).ok_or(Unavailable)?;
            (context, strings, layout, heap)
        };
        let start: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(PANEL_START) };
        unsafe {
            start(ui);
            // The panel's text frame with message `0x20` of text file 39, as
            // `0019C700` shows one of file 23, on the UI's heap
            // (`[[ui + 0x78] + 4]`, as `0019C700` allocates).
            let pane: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transmute(PANE_SHOWN);
            pane(context, 0, TEXT_FRAME_PANE, 1);
            pane(context, 0, OTHER_FRAME_PANE, 0);
            let text = message_text(
                heap,
                strings,
                layout,
                TEXT_PANE,
                PANEL_TEXT_FILE,
                PANEL_TEXT_MESSAGE,
            );
            if text.is_err() {
                stop_loading(ui);
            }
            text
        }
    }

    /// Ends the general loading panel on `ui`, as the question task ends it
    /// (`00247394`..`002473D8`): its sound, its spinner, then the shared UI's
    /// resting state, so that neither its text frame nor its text is left.
    /// # Safety
    /// `ui` is the live shared UI the panel was started on, on the main thread.
    pub unsafe fn stop_loading(ui: *mut u8) {
        if unsafe { patch_common::ui::end_loading::<Transporter15>(ui) }.is_ok() {
            let hide: unsafe extern "aapcs" fn(*mut u8) = unsafe { transmute(PANEL_HIDE) };
            unsafe { hide(ui) };
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    const FAULTS: [WriteFault; 5] = [
        WriteFault::None,
        WriteFault::Full,
        WriteFault::Untouched,
        WriteFault::Torn,
        WriteFault::Cartridge,
    ];

    const RESULT: u32 = 0xc8a0_4554;

    #[test]
    fn only_the_failure_and_the_no_card_message_change_with_what_a_write_left() {
        for fault in FAULTS {
            for chosen in [Chosen::CardIn, Chosen::CardOut, Chosen::VirtualConsole] {
                for (no_room, note_save) in [(false, false), (false, true), (true, false)] {
                    let shown =
                        |message| shown(message, (fault, RESULT), chosen, no_room, note_save);
                    let case = std::format!("{fault:?} {chosen:?} {no_room} {note_save}");
                    // The refusals and the closing message of a transfer are
                    // shown whole.
                    for message in [0, MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY, 0x0a] {
                        assert_eq!(shown(message), Shown::Whole, "{case}");
                    }
                    // A delivery that did not happen, or the note save that
                    // did not count: nothing is in Bank.
                    let expected = match fault {
                        _ if no_room => Shown::SdFull,
                        WriteFault::Full => Shown::SdFull,
                        // The note save of a save on the SD card or of a
                        // Virtual Console title: the numbers.
                        WriteFault::Untouched | WriteFault::Torn if note_save => {
                            Shown::Numbers(RESULT)
                        }
                        WriteFault::None if note_save && chosen == Chosen::VirtualConsole => {
                            Shown::Numbers(0)
                        }
                        // A cartridge, or a card that is gone: try again.
                        _ => Shown::SecondPage,
                    };
                    assert_eq!(shown(MESSAGE_FAILURE), expected, "{case}");
                    // A save after the delivery that failed, or a card that
                    // is gone: Bank holds the Pokémon. The card's message
                    // only where the card is the reason, never Bank's
                    // refusal.
                    let expected = match (fault, chosen) {
                        (WriteFault::None, Chosen::VirtualConsole) => Shown::Numbers(0),
                        (WriteFault::None, _) => Shown::Whole,
                        (WriteFault::Cartridge, Chosen::CardIn) => Shown::Numbers(RESULT),
                        (WriteFault::Cartridge, _) => Shown::Whole,
                        _ => Shown::Numbers(RESULT),
                    };
                    assert_eq!(shown(MESSAGE_NO_CARD), expected, "{case}");
                }
            }
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
