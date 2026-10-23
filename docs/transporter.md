# Poké Transporter Offline Patch

A Luma3DS patch for **Poké Transporter 1.5** (title `00040000000C9C00`, TMD version 5200) that works without Nintendo's servers and delivers into the transport box of the [offline Bank](bank.md).

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#transporter); how it is checked on a console is in [testing.md](testing.md#transporter-feature-by-feature).

## What changes

| Original Transporter | Patched |
| --- | --- |
| Connects to Nintendo's servers at the start and disconnects at the end. | No connection. Wireless can stay off. |
| Reads the source game and converts the Pokémon itself. | Unchanged, apart from a small Easter egg in the Gen 1 Virtual Console games. |
| Gen 5 Pokémon come from a cartridge. | From a cartridge or from saves on the SD card ([below](#blackwhite-saves-on-the-sd-card)). |
| The server checks the Pokémon's legality. | No check. Whatever Transporter itself accepts is transferred (eggs, for example, are still refused). |
| After a game was chosen, asks the server whether Bank's transport box is empty. | Reads Bank's record of its files (`/journal.bin`, `/journal.alt.bin` in Bank's extdata) as soon as START is pressed on the start screen, before any game is looked for. |
| The server stores the Pokémon in Bank. | Writes one delivery into the file of the transport box that Bank is not using (`/mover.bin` or `/mover.alt.bin`), then notes in Bank's record that it waits ([below](#transfers-and-refusals)). |
| Removes the Pokémon from the source game. | Unchanged. |
| A transfer interrupted by a power failure is sorted out with the server at the next session, with the same copy of the game: neither lost nor doubled. | Nothing is left pending. The Pokémon are never lost; after a power cut between the two writes they can be in Bank and still in the game ([below](#limits)). |
| Screens and messages. | The originals; no new text. |
| The language screen switches the language of the screens as long as the app is open. | It chooses which language's games are listed ([below](#choosing-the-language-of-the-games)). The screens stay in the console's language. |
| Shows a notice that nicknames and OT names with prohibited words will be erased; the server erased them. | Not shown. No name is ever changed offline. The other notices are unchanged. |
| HOME and sleep are refused from connecting until disconnecting. | Refused while files are read or written: from START until the game list is on screen, from choosing a game until the question, and during the transfer. They work on every screen that waits for you: the start screen, the game list, the notices, the question, and every message that waits for a button, the closing message of a transfer included. |

Gen 5 games produce Gen 6 Pokémon; Gen 1 and Gen 2 Virtual Console games produce Gen 7 Pokémon, as with the original service.

## Transfers and refusals

**The patch converts nothing itself.** The original Transporter builds a complete Bank transport box in memory and used to upload it. The patch writes that box, unchanged, into Bank's file. The next time you open Bank, the Pokémon are in the transport box, in the positions they had in Box 1 of the source game.

**Order of the two writes.** The delivery is written, flushed, read back and noted in Bank's record first. Only then does the original remove the Pokémon from the source game. A power cut in between leaves the Pokémon in both places, never in neither.

| When | Bank | Source game | Message |
| --- | --- | --- | --- |
| The cartridge's Game Card is gone at "Yes", checked before anything is written | unchanged | unchanged | "Please confirm that your Game Card is properly inserted, and then try again." |
| The delivery cannot be written: the SD card refuses it | unchanged | unchanged | the error screen with two numbers ([below](#troubleshooting)) |
| Whether Bank received the delivery cannot be read, even when Bank's record is read again | may have the Pokémon | unchanged | "No compatible Pokémon Game Card detected…" |
| The cartridge is taken out after the delivery, before the game is saved | has the Pokémon | still has them | the same |
| The game cannot be saved after the delivery, with the cartridge in, a save on the SD card, or a Virtual Console title | has the Pokémon | may still have them, and its save may be damaged | "The saved data in your Pokémon game is corrupted." |
| Everything is written | has the Pokémon | without them | the original's success message |
| The patch itself cannot go on, for instance its background thread cannot start, or Bank's data cannot be opened at all | unchanged | unchanged | the error screen with two numbers ([below](#troubleshooting)) |

"Try again" comes only where nothing changed. The failure message's first sentence, "The server did not receive the data.", is shown only on the error screen: there is no server.

**One save before the question.** Right after the chosen game is read, and before any notice or question, the original saves that game once. It stores a small note of its own there, which the server used to sort out an interrupted transfer; offline the note is empty and nothing reads it. No Pokémon data changes in that save, and it happens also if you then answer "No".

**When Transporter refuses.** It asks about Bank every time START is pressed on the start screen, behind the loading screen that comes with the press, before it looks for games. If Bank cannot take a delivery, it ends the loading screen, shows one of two messages of the original and returns to the start screen. No game has been read at that point.

| Message | When | What to do |
| --- | --- | --- |
| "At least one Pokémon remains in the Transport Box from your previous session. Please empty the Transport Box by using Pokémon Bank…" | The transport box in Bank still holds Pokémon, or an earlier delivery has not been picked up. | Open Bank, empty the transport box, Save and Quit. |
| "Your communication with Pokémon Bank did not complete correctly during your last session. Please open Pokémon Bank and perform the cleanup process…" | Bank's files are not ready for a delivery: Bank has an unfinished save, the offline Bank has never been started on this console, its Bank is from a version this Transporter does not continue, or a file of Bank's is missing or unreadable after a power cut. | Open Bank. It finishes what is unfinished or says what is wrong. Then Save and Quit once. |

The second message is the original's for a session the server asked to clean up; there is no cleanup to perform by hand. Every other message of the original comes where it always did, for the game that was chosen.

Transporter never creates or deletes any of Bank's files (on the SD card it creates only the backups of Black/White saves, [below](#blackwhite-saves-on-the-sd-card)). Of Bank's files it touches four: it writes the file of the transport box that Bank is not using, and then the two files of Bank's record, to say that a delivery waits. Until the record says so, Bank sees no delivery, so a transfer that is cut off leaves none or the whole one. If one of the files it would write is missing or unreadable, for example after a power cut, Transporter refuses at START; open Bank and Save and Quit once, which writes it anew.

**Bank and Transporter belong together.** A Bank that was last used with an earlier release may keep its transport box in another form. This Transporter then refuses, and the Transporter of that release refuses with a converted Bank. Update both, after converting the Bank with the migration that ships with the release that changed the files.

## Choosing the language of the games

The game list shows the games of **one language** only: the cartridge, the saves on the SD card and the Virtual Console titles alike. The original language screen, reached from the start screen, chooses that language. It no longer changes the language of the screens, which stay in the console's language.

- Until you choose, the games in the console's language are listed. A console set to a language no source game exists in (Chinese, Dutch, Portuguese, Russian) starts with English.
- The choice is not saved, as the original saved nothing either: it lasts until Transporter is closed.
- After a choice, the start screen shows it: its logo and its start prompt appear in the chosen language. Japanese, English and Korean have the same prompt and differ in the logo only. Everything else stays in the console's language.
- The list offers the six languages that are not chosen already, and a **Back** button below them that leaves the screen without changing anything, as B does. The two Chinese entries are gone, because no Gen 1, 2 or 5 game exists in Chinese.
- Japanese is confirmed like every other language; the kana/kanji step is gone, since the screens do not change.
- The Back button is built like the Back buttons of Pokémon Bank's menus, to which this screen belongs: the screen's own list button with the text of the game-selection screen's Back, in the language of the screens, and the return icon in its own colour. It is put together when the screen is opened from parts the original already has; no file of the application is replaced.
- The seven entries are spaced like the buttons of Bank's main menu, with a small gap between them, and centred on the screen.
- The language is the game's, not the save's: for a Gen 5 game the language letter of its game code, for a Virtual Console title the language of the installed title.

## Black/White saves on the SD card

Transporter looks for saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card and offers each one as its own entry in the game list, beside the cartridge and the Virtual Console titles. Only saves in the [chosen language](#choosing-the-language-of-the-games) are looked for. **The cartridge is preferred for its own game:** with, say, German chosen and a German White cartridge inserted, the White entry is the cartridge and a German White save on the SD card is not offered; saves of the other three games still are. Bank does the same with a game that is both inserted and installed: it uses the cartridge. A cartridge in another language than the chosen one is not listed.

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

- The last two digits are the cartridge's revision: `01` for the Japanese Black 2 and White 2, `00` for every other one. A save under the other number is not listed; rename it to the name in the table.
- A save is listed only while its language is chosen: `POKEMON_W2_IRDD01_00.sav` is a German White 2.
- Any other name, folder or spelling is ignored. A file that is not exactly 524,288 bytes is ignored. A save of another game or language than its name says is ignored: a White save renamed to the Black name, or an English save renamed to the German name.
- At most four saves are offered, one per game, and the whole list holds 40 games, as in the original.

What happens to the file:

- It is read and checked by the original code exactly as a cartridge's save would be; a damaged save is refused by the original.
- Before the first change in a session, the untouched save is copied to `<name>.sav.bak` beside it (one copy, replaced the next time), and the copy is read back: it must hold what was read from the save. Every write to the save is read back too, and the save closed; a write the card does not hold fails as a cartridge write does. If that copy cannot be written and closed whole, nothing is changed; a write to the save that fails once bytes reached it shows "The saved data in your Pokémon game is corrupted."; if the SD card has no room for it, the original's "The SD card is full." says so, and Transporter returns to its start screen. The first change is the original's save right after the game is read, so the copy exists before any Pokémon leaves Box 1. It is a way back if a write to the file is cut off, independent of the two copies a Gen 5 save keeps of itself. The name ends in `.bak` so that no program takes it for a save.
- The Pokémon are removed from the file only after the delivery to Bank has been written and verified, as with a cartridge.
- The file is never created, resized or renamed.

Do not insert or remove a cartridge while a session is running; what is offered is decided when you leave the start screen, with the language chosen at that moment. If you do it anyway:

| After START you | What happens |
| --- | --- |
| Insert a cartridge | Nothing. It is not offered, and the list stays as it was until the next START. |
| Take the cartridge out before the list is on screen | The cartridge is not listed; the saves on the SD card and the Virtual Console titles are. |
| Take the cartridge out on the list | Only the cartridge's entry is concerned. Choosing it shows the original's message that no compatible Pokémon game is detected; a save of the same game on the SD card is not used in its place. Every other entry can be chosen and transferred as usual. |
| Take the cartridge out after choosing it, before the question | The session ends with the original's message that no compatible game is detected, or with the advice to check the Game Card. Nothing is written. The original looks for a removed Game Card before it reads or saves the game; the patch asks only for the cartridge, never for a save on the SD card. |
| Take it out on the question and answer "Yes" | Only the advice to check the Game Card. Nothing is written to Bank, and the game keeps its Pokémon. |
| Take it out after the Pokémon were written to Bank and before the game is saved | The original's message that no compatible Game Card is detected, and nothing after it. The Pokémon are then in Bank and still in the game, as after a power cut at that moment ([Limits](#limits)). |

## Install

1. Install the [Bank patch](bank.md#install) and start Bank once, so that the transport box file exists.
2. Close Transporter, Bank and all games. Back up the source game's save and Bank's extdata `0x00000C9B` together, as one dated set ([how](bank.md#backing-up-the-bank)).
3. Make sure the installed Poké Transporter is version 1.5, the final update.
4. Copy both files from the same release or build:

   ```text
   SD:/luma/titles/00040000000C9C00/code.ips
   SD:/luma/titles/00040000000C9C00/exheader.bin
   ```

   They only work as a pair.
5. *Enable game patching* must be on in Luma's configuration, as for Bank: hold SELECT while powering on, switch it on, and save. Wireless can stay off.

## Troubleshooting

| What you see | Likely cause |
| --- | --- |
| Transporter does not start, or stops while loading. | `code.ips` and `exheader.bin` are not from the same release or build, or Transporter is not version 1.5. |
| START always answers "Pokémon remains in the Transport Box". | Open Bank, empty the transport box, and Save and Quit. |
| START always answers "did not complete correctly… open Pokémon Bank". | Open Bank and Save and Quit once. If Bank shows an error instead, that error is the cause ([bank.md](bank.md#troubleshooting)). If Bank opens and saves and the refusal stays, check that Bank and Transporter are from the same release, and otherwise please report it. Where Transporter cannot open Bank's data at all, it shows the error screen instead. |
| "Please confirm that your Game Card is properly inserted" after confirming a transfer. | The cartridge's card was not inserted, or the delivery could not be written. The source game and Bank are unchanged; transfer again. |
| "No compatible Pokémon Game Card detected" after confirming a transfer. | The cartridge was taken out after the delivery, or whether Bank received the delivery could not be read. The Pokémon may be in Bank's transport box and still in the game: look in Bank before you transfer them again. |
| "The saved data in your Pokémon game is corrupted." after confirming a transfer. | Saving the game failed in the middle. The Pokémon may be in Bank and still in the game, and the game's save may be damaged: check it before you play. For a save on the SD card, `<name>.sav.bak` holds it as it was before the session. |
| "The SD card is full." after choosing a Black/White save on the SD card. | The card has no room for the save's backup (512 KiB). Nothing was written. Free space and start again. |
| "Press HOME to return to the HOME Menu" with two numbers under it. | The patch itself cannot go on, for instance its background thread cannot start, or something of Transporter it needs is not what it expects, or Bank's data cannot be opened at all or its record is damaged (`3`, `5`), which Bank could not repair either. It is Bank's error screen, with Bank's numbers ([bank.md](bank.md#troubleshooting)). Nothing was written to Bank or the game. Close Transporter from the HOME Menu and start it again; if it stays, please report the numbers. |
| A game is not listed: cartridge, save on the SD card or Virtual Console title. | It is in another language than the one whose games are listed. Choose its language on the language screen. |
| A save on the SD card is not listed. | Its language is not the chosen one; a cartridge of the same game in that language is inserted; the name or folder differs from the pattern above; the save is of another game or language than its name says; or the file is not 524,288 bytes. |
| The language screen does not change the language of the screens. | Intended: it chooses the games. The screens follow the console's language. |
| The last entry of the language list shows Chinese text instead of Back. | The screen's layout is not the one the patch knows, so it was left as it is. Transporter is not version 1.5, or its files are modified. The entry still works as Back. |
| A save on the SD card is reported as damaged. | The original's own check failed on the file's contents, as it would on a cartridge. |

**Reporting a problem.** Open an issue with the console model, Luma version, the release or package name, the source game, which screen was showing when it stopped, and whether the source game still holds the Pokémon.

## Limits

- **A duplicate after a power cut, by design.** The delivery is written before the Pokémon are removed from the game. If the power fails exactly between the two, or the game cannot be saved after the delivery, they are in Bank and still in the game. The original prevented this with the server's help. The patch keeps this order on purpose: it can duplicate a Pokémon but never lose one, the same choice Bank makes for an interrupted save that moved Pokémon one way.
- **Wording of refusals, by design.** The original app has no text for "Bank is not set up" or "Bank's data cannot be read". Everything that is not "Pokémon in the way" uses its "did not complete correctly… open Pokémon Bank" message, whose advice fits all of them. A refusal shows no error numbers: it concerns Bank's files, and Bank, once opened, solves it or says it with its own numbers. A failure of the patch itself (for instance no room for its thread) shows Bank's error screen with its two numbers and leaves Bank's files as they are. A delivery that fails leaves the game unchanged; Bank too, unless the delivery was written and its confirmation cannot be read even on a second look, in which case Bank may hold the Pokémon as well (a duplicate, never a loss), and the message says nothing about trying again.
- **Access to Bank's data and to the SD card.** Transporter's own header grants neither. Luma3DS gives every application full filesystem access, which is what makes both work; this needs Luma3DS v10.0 or later.
- **Saves on the SD card.** Only the one name per game and language in the table [above](#blackwhite-saves-on-the-sd-card) is looked for, one save per game in the chosen language. The search adds a short moment to the loading screen.
- **Language of the screens.** It can no longer be changed inside Transporter; it is the console's.
