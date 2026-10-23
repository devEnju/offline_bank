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
/// original shows it where the server asked for that (`0025C964`).
pub const MESSAGE_OPEN_BANK: u32 = 3;
/// The original's message 5: "At least one Pokémon remains in the Transport
/// Box from your previous session. Please empty the Transport Box by using
/// Pokémon Bank before using Poké Transporter."
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
/// The original's message 2: "The saved data in your Pokémon game is
/// corrupted." One page in all ten languages; the reader shows it for a
/// damaged save.
pub const MESSAGE_CORRUPTED: u32 = 0x02;

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
    /// "The saved data in your Pokémon game is corrupted." in its place.
    Corrupted,
}

/// What is shown for `message`, when the last write to the game that failed
/// left `fault` (`cart::take_write_fault`) and the chosen cartridge's card
/// is in (`card_in`). The failure message stands for a note save that did
/// not count: the advice to try again, unless the card had no room for the
/// save's backup, or a save on the SD card may be half-written. The no-card
/// message stands for a save after the delivery that failed, or a card that
/// is gone: a save on the SD card, or a cartridge whose card is still in,
/// may be half-written. Every other message is the original's.
pub fn shown(message: u32, fault: WriteFault, card_in: bool) -> Shown {
    match (message, fault) {
        (MESSAGE_FAILURE, WriteFault::Full) => Shown::SdFull,
        (MESSAGE_FAILURE, WriteFault::Torn) => Shown::Corrupted,
        (MESSAGE_FAILURE, _) => Shown::SecondPage,
        (MESSAGE_NO_CARD, WriteFault::Full | WriteFault::Untouched | WriteFault::Torn) => {
            Shown::Corrupted
        }
        (MESSAGE_NO_CARD, WriteFault::Cartridge) if card_in => Shown::Corrupted,
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
        // converted, or one whose next save replaces the file.
        Refusal::NotReady => MESSAGE_OPEN_BANK,
    }
}

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
        message,
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
    /// indicator show it: message `0x20` of text file 39 ("Die Spielesoftware
    /// wird überprüft… Bitte weder das System ausschalten, noch Karte oder SD
    /// Card entfernen."), which the panel's own file 23 has not.
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
    /// The longest line about the HOME Menu with its label and its numbers
    /// that is carried from one message to the other: the line has 11 to 42
    /// units in the ten languages, as Bank's.
    const HOME_ROOM: usize = 128;
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
    /// first the line, which is copied out, then the failure message, which
    /// is cut and given the line as its next page, and handed to the dialog
    /// again. Without room for the label the numbers go alone, without room
    /// for the page the first part stays alone.
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
        let mut home = [0u16; HOME_ROOM];
        let mut home_len = 0;
        unsafe { show_still(ui, HOME_NOTICE) };
        if let Ok(Some(Text { room, len, .. })) = unsafe { shared::text(object) } {
            if len < HOME_ROOM {
                home[..len].copy_from_slice(&room[..len]);
                home_len =
                    numbers::put(&mut home, len, label.units(), first, second).unwrap_or(len);
            }
        }
        WHOLE.store(true, Ordering::Relaxed);
        unsafe { show_still(ui, MESSAGE_FAILURE) };
        WHOLE.store(false, Ordering::Relaxed);
        let Ok(Some(Text {
            object: text,
            room,
            len,
        })) = (unsafe { shared::text(object) })
        else {
            return Ok(());
        };
        let mut len = message::cut(&room[..len]);
        if home_len != 0 {
            len = numbers::append_page(room, len, &home[..home_len]).unwrap_or(len);
        }
        unsafe {
            shared::end(text, room, len);
            let hand: unsafe extern "aapcs" fn(*mut u8, *mut u8) = transmute(DIALOG_TEXT);
            hand(context, text);
        }
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
    use super::*;

    const FAULTS: [WriteFault; 5] = [
        WriteFault::None,
        WriteFault::Full,
        WriteFault::Untouched,
        WriteFault::Torn,
        WriteFault::Cartridge,
    ];

    #[test]
    fn only_the_failure_and_the_no_card_message_change_with_what_a_write_left() {
        for fault in FAULTS {
            for card_in in [false, true] {
                // The refusals and the closing message of a transfer are
                // shown whole.
                for message in [0, MESSAGE_OPEN_BANK, MESSAGE_BOX_NOT_EMPTY, 0x0a] {
                    assert_eq!(shown(message, fault, card_in), Shown::Whole);
                }
                // The note save that did not count.
                let expected = match fault {
                    WriteFault::Full => Shown::SdFull,
                    WriteFault::Torn => Shown::Corrupted,
                    _ => Shown::SecondPage,
                };
                assert_eq!(shown(MESSAGE_FAILURE, fault, card_in), expected);
                // A save after the delivery that failed, or a card that is
                // gone: 0x22 only where no save may be half-written.
                let expected = match fault {
                    WriteFault::None => Shown::Whole,
                    WriteFault::Cartridge if !card_in => Shown::Whole,
                    _ => Shown::Corrupted,
                };
                assert_eq!(shown(MESSAGE_NO_CARD, fault, card_in), expected);
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
