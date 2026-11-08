# Poké Transporter Offline Patch

A Luma3DS patch for **Poké Transporter 1.5** (title `00040000000C9C00`, TMD version 5200) that works without Nintendo's servers and delivers into the transport box of the [offline Bank](bank.md).

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#transporter); how it is checked on a console is in [testing.md](testing.md#transporter-feature-by-feature).

## What changes

| Original Transporter | Patched |
| --- | --- |
| Connects to Nintendo's servers at the start and disconnects at the end. | No connection. Wireless can stay off. |
| Reads the source game and converts the Pokémon itself. | Unchanged, apart from a small Easter egg for the Gen 1 Virtual Console games. |
| Gen 5 Pokémon come from a cartridge. | From a cartridge or from saves on the SD card ([below](#blackwhite-saves-on-the-sd-card)). |
| The server checks the Pokémon's legality. | No additional checks. Whatever Transporter itself accepts is transferred (some illegal Pokémon and eggs, for example, are still refused). |
| After a game was chosen, asks the server whether Bank's transport box is empty. | Reads Bank's record of its files (`/journal.bin`, `/journal.alt.bin` in Bank's extdata) as soon as START is pressed on the start screen, before any game is looked for. |
| The server stores the Pokémon in Bank. | Writes one delivery into the file of the transport box that Bank is not using (`/mover.bin` or `/mover.alt.bin`), then notes in Bank's record that it waits ([below](#transfers)). |
| Removes the Pokémon from the source game. | Unchanged. |
| A transfer interrupted by a power failure is sorted out with the server at the next session, with the same copy of the game: neither lost nor doubled. | Nothing is left pending. The Pokémon are never lost; after a power cut between the two writes they can be in Bank and still in the game ([below](#limits)). |
| The language screen switches the language of the screens as long as the app is open. | It chooses which language's games are listed ([below](#choosing-the-language-of-the-games)). The screens stay in the console's language. |
| Shows a notice that nicknames and OT names with prohibited words will be erased; the server erased them. | Not shown. No name is ever changed offline. The other notices are unchanged. |
| HOME and sleep are refused from connecting until disconnecting. | Refused while files are read or written: from START until the game list is on screen, from choosing a game until the question, and during the transfer. They work on every screen that waits for you: the start screen, the game list, the notices, the question, and every message that waits for a button, the closing message of a transfer included. |

Gen 5 games produce Gen 6 Pokémon while Gen 1 and Gen 2 Virtual Console games produce Gen 7 Pokémon, as with the original service.

## Transfers

- **The patch converts nothing itself.** The original builds Bank's transport box and used to upload it, while the patch writes that box, unchanged, into Bank's file. Next time you open Bank, the Pokémon are in its transport box, where they were in Box 1 of the source game.
- **Bank first, then the game.** The delivery is written and checked in Bank before the original removes the Pokémon from the game. An interruption in between can leave them in both places, never in neither.
- **One save before the question.** Right after the chosen game is read, the original saves it once, with a small note the server used to read. No Pokémon change in that save, and it happens also if you then answer No.
- **Bank's files.** Transporter writes only the transport box file Bank is not using and Bank's record, and creates or deletes none of them. Until the record is written, Bank sees no delivery.
- **Transporter at START.** It checks that Bank can take a delivery, before it looks for games ([below](#when-transporter-refuses)).

## Choosing the language of the games

The game list shows the games of **one language** only: the cartridge, the saves on the SD card and the Virtual Console titles alike. The original language screen, reached from the start screen, chooses that language. It no longer changes the language of the screens, which stay in the console's language.

- Until you choose, the games in the console's language are listed. A console set to a language no source game exists in (Chinese, Dutch, Portuguese, Russian) starts with English.
- The choice is not saved, as the original saved nothing either: it lasts until Transporter is closed.
- After a choice, the start screen shows it: its logo and its start prompt appear in the chosen language. Japanese, English and Korean have the same prompt and differ in the logo only. Everything else stays in the console's language.
- The list offers the six languages that are not chosen already, and a **Back** button below them that leaves the screen without changing anything, as B does. The two Chinese entries are gone, because no Gen 1, 2 or 5 game exists in Chinese.
- Japanese is confirmed like every other language and the kana/kanji step is gone, since the screens do not change.
- The Back button is built like the Back buttons of Pokémon Bank's menus, to which this screen belongs: the screen's own list button with the text of the game-selection screen's Back, in the language of the screens, and the return icon in its own colour. It is put together when the screen is opened from parts the original already has.
- The seven entries are spaced like the buttons of Bank's main menu, with a small gap between them, and centred on the screen.
- The language is the game's, not the save's: for a Gen 5 game the language letter of its game code, for a Virtual Console title the language of the installed title.

## Black/White saves on the SD card

Transporter looks for saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card and offers each one as its own entry in the game list, beside the cartridge and the Virtual Console titles. Only saves in the [chosen language](#choosing-the-language-of-the-games) are looked for. **The cartridge is preferred for its own game:** with, say, German chosen and a German White cartridge inserted, the White entry is the cartridge and a German White save on the SD card is not offered while saves of the other three games still are. Bank does the same with a game that is both inserted and installed: it uses the cartridge. A cartridge in another language than the chosen one is not listed.

Only the names GodMode9 gives a cartridge dump are accepted, in TWiLight Menu's default folder `SD:/roms/nds/saves/`. Each game has exactly one name per language:

| Language | Black | White | Black 2 | White 2 |
| --- | --- | --- | --- | --- |
| Japanese | `POKEMON_B_IRBJ01_00.sav` | `POKEMON_W_IRAJ01_00.sav` | `POKEMON_B2_IREJ01_01.sav` | `POKEMON_W2_IRDJ01_01.sav` |
| English | `POKEMON_B_IRBO01_00.sav` | `POKEMON_W_IRAO01_00.sav` | `POKEMON_B2_IREO01_00.sav` | `POKEMON_W2_IRDO01_00.sav` |
| French | `POKEMON_B_IRBF01_00.sav` | `POKEMON_W_IRAF01_00.sav` | `POKEMON_B2_IREF01_00.sav` | `POKEMON_W2_IRDF01_00.sav` |
| Italian | `POKEMON_B_IRBI01_00.sav` | `POKEMON_W_IRAI01_00.sav` | `POKEMON_B2_IREI01_00.sav` | `POKEMON_W2_IRDI01_00.sav` |
| German | `POKEMON_B_IRBD01_00.sav` | `POKEMON_W_IRAD01_00.sav` | `POKEMON_B2_IRED01_00.sav` | `POKEMON_W2_IRDD01_00.sav` |
| Spanish | `POKEMON_B_IRBS01_00.sav` | `POKEMON_W_IRAS01_00.sav` | `POKEMON_B2_IRES01_00.sav` | `POKEMON_W2_IRDS01_00.sav` |
| Korean | `POKEMON_B_IRBK01_00.sav` | `POKEMON_W_IRAK01_00.sav` | `POKEMON_B2_IREK01_00.sav` | `POKEMON_W2_IRDK01_00.sav` |

- The last two digits are the cartridge's revision: `01` for the Japanese Black 2 and White 2, `00` for every other one. A save under the other number is not listed. Rename it to the name in the table.
- A save is listed only while its language is chosen: `POKEMON_W2_IRDD01_00.sav` is a German White 2.
- Any other name, folder or spelling is ignored. A file that is not exactly 524,288 bytes is ignored. A save of another game or language than its name says is ignored: a White save renamed to the Black name, or an English save renamed to the German name.
- At most four saves are offered, one per game, and the whole list holds 40 games, as in the original.

What happens to the file:

- The original reads and checks it as it would a cartridge's save.
- Before the first change in a session, the untouched save is copied to `<name>.sav.bak` beside it (one copy, replaced the next time) and read back. If the copy cannot be written whole, nothing changes.
- Every write is read back. The Pokémon are removed only after the delivery to Bank is written and checked.
- The file is never created, resized or renamed.

## Install

1. Install the [Bank patch](bank.md#install) and start Bank once.
2. Close Transporter, Bank and all games. As a precaution you can back up your games' saves first.
3. Make sure the installed Poké Transporter is version 1.5, the final update.
4. Copy both files from the same release or build. They only work as a pair. Use Bank and Transporter from the same release.

   ```text
   SD:/luma/titles/00040000000C9C00/code.ips
   SD:/luma/titles/00040000000C9C00/exheader.bin
   ```

5. *Enable game patching* must be on in Luma's configuration, as for Bank. Wireless can stay off.

## When Transporter refuses

At START, before any game is read, Transporter may show one of the original's messages and return to its start screen:

| Message | Do this |
| --- | --- |
| "At least one Pokémon remains in the Transport Box from your previous session. Please empty the Transport Box by using Pokémon Bank…" | Open Bank, move the Pokémon out of the transport box, Save and Quit. |
| "Your communication with Pokémon Bank did not complete correctly during your last session. Please open Pokémon Bank and perform the cleanup process…" | Open Bank and Save and Quit once. There is no cleanup to do by hand: Bank finishes what is unfinished (an interrupted save, a file a power cut left unreadable, a Bank not started yet) or shows what is wrong. |

## If a transfer is interrupted

Transporter writes Bank first, so nothing is lost: at worst a Pokémon is in Bank and still in the game.

| What you see | Bank and the game | Do this |
| --- | --- | --- |
| "Please confirm that your Game Card is properly inserted, and then try again." | Unchanged | Insert the cartridge and transfer again. |
| "The SD card is full." | Unchanged | Free space and transfer again. |
| "No compatible Pokémon Game Card detected…" | The Pokémon are in Bank and still in the game. | Open Bank and look before you transfer again. |
| The power failed | The Pokémon may be in Bank and still in the game. | Open Bank and look before you transfer again. |
| The error screen after confirming the transfer | The Pokémon may be in Bank and still in the game. | Open Bank and look before you transfer again. |

Do not insert or remove a cartridge while a session runs. A cartridge inserted after START is listed only at the next START and one taken out before the transfer gets the original's message while nothing is written.

## A save left to Transporter

This only comes after Bank has sent you here with "Your communication with Poké Transporter did not complete correctly during your last session…" ([bank.md](bank.md#a-save-left-to-transporter)). Press START and Transporter asks:

> You can try restoring your data, but you may lose the Pokémon currently being transported. Would you still like to try restoring your data?

**Yes** gives Bank its boxes from before the interrupted session, **No** those from after it. Then, after either answer, the original's final warning:

> This is your final warning. The Pokémon you are trying to send could be lost forever. Are you sure you want to try to restore your data?

Its **Yes** applies your first answer, and Bank opens normally again. Its **No** changes nothing, and the next START asks again. Closing Transporter on either question changes nothing, too. If the error screen comes instead, your answer was not applied, and the next START asks again.

Look in your game first:

| In your game now | First answer |
| --- | --- |
| The Pokémon you put **into** Bank are still in your boxes | Yes |
| The Pokémon you took **out of** Bank are in your boxes | No |
| Neither, or you are not sure | Back up Bank's extdata first ([how](bank.md#backing-up-the-bank)), then answer. A wrong answer can be undone by restoring Bank, and choosing the other option. |

## A game save that does not load

> The saved data in your Pokémon game is corrupted.

The original's check of the chosen game failed, so nothing is transferred and nothing is written. Start the game itself to see what it makes of its save. For a save on the SD card, `<name>.sav.bak` also holds it as it was before its last session: rename it to `<name>.sav` in place of the old file.

The patch's changes cannot cause this. The check and the message are the original Transporter's own, and so is every save of a game: the one before the question and the one that removes the transferred Pokémon are written by the original's own code, exactly as without the patch. For a save on the SD card the patch only carries those bytes to the file, after copying it to `<name>.sav.bak`, and reads each write back.

## Troubleshooting

| What you see | Do this |
| --- | --- |
| Transporter does not start, or stops while loading. | Use `code.ips` and `exheader.bin` from the same release, with Transporter 1.5. |
| A refusal at START, a message after a transfer, the restore question, or a corrupted save. | See the sections above. |
| A game is not listed. | Choose its language on the language screen ([above](#choosing-the-language-of-the-games)). |
| A save on the SD card is not listed. | Check its language, name, folder and size ([above](#blackwhite-saves-on-the-sd-card)). A cartridge of the same game takes its place. |
| The language screen does not change the language of the screens. | Intended: it chooses the games. |
| The last entry of the language list shows Chinese text instead of Back. | Transporter is not version 1.5, or its files are modified. The entry still works as Back. |
| "Press HOME to return to the HOME Menu" with two numbers under it. | Press HOME, close Transporter and start it again. If it comes again, open Bank, which may show the cause, or [report the numbers](../README.md#problems-and-feature-requests). |

The error screen is Bank's, with the same numbers ([bank.md](bank.md#troubleshooting), [errors.md](errors.md#every-active-first-error-number)).

## Limits

- **A duplicate, by design.** If the power fails between the delivery and the game's save, or the game cannot be saved after it, the Pokémon are in Bank and still in the game. The original prevented this with the server. The patch can duplicate a Pokémon but never lose one, as Bank does.
- **Wording of refusals.** The original has no text for "Bank is not set up" or "Bank's data cannot be read", so its "open Pokémon Bank" message stands for all of them. A refusal comes only at START and shows no numbers. Opening Bank once solves it or shows its own.
- **Saves on the SD card.** Only the one name per game and language [above](#blackwhite-saves-on-the-sd-card) is looked for.
