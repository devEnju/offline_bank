# Poké Transporter Offline Patch

A Luma3DS patch for **Poké Transporter 1.5** (title `00040000000C9C00`, TMD version 5200) that works without Nintendo's servers and delivers into the transport box of the [offline Bank](bank.md).

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#transporter); how it is checked on a console is in [testing.md](testing.md#transporter-feature-by-feature).

## What changes

| Step | Original Transporter | Patched |
| --- | --- | --- |
| Connect, disconnect | Nintendo's servers | Skipped; wireless can stay off |
| Read the source game and convert the Pokémon | Transporter itself | Unchanged |
| Legality check | Server | None. Whatever Transporter itself accepts is transferred (eggs, for example, are still refused). |
| "Is Bank's transport box empty?" | Server, after a game was chosen | Reads `/mover.bin` and `/mover.alt.bin` in Bank's extdata, as soon as START is pressed on the title screen and before any game is looked for |
| Store the Pokémon in Bank | Server | Writes one delivery into the one of those two files that Bank is not using |
| Remove the Pokémon from the source game | Transporter itself | Unchanged |
| A transfer interrupted by a power failure | Sorted out with the server at the next session, with the same copy of the game: neither lost nor doubled | Nothing is left pending. The Pokémon are never lost; after a power cut between the two writes they can be in Bank and still in the game. |
| Source of a Gen 5 game | Cartridge | Cartridge and saves on the SD card ([below](#blackwhite-saves-on-the-sd-card)) |
| Language screen | Switches the language of the screens as long as the app is open | Chooses which language's games are listed ([below](#choosing-the-language-of-the-games)); the screens stay in the console's language |
| Notice that nicknames and OT names with prohibited words will be erased | Shown; the server erased them | Skipped. No name is ever changed offline. The other notices are unchanged. |
| HOME and sleep | Refused from connecting until disconnecting | Refused while files are read or written: from START until the game list is on screen, from choosing a game until the question, and during the transfer. They work on every screen that waits for you: the title screen, the game list, the notices, the question, and every message that waits for a button, the closing message of a transfer included. |
| Screens and messages | | The originals; no new text. One message gets two numbers that say why it came ([below](#transfers-and-refusals)). |

Gen 5 games produce Gen 6 Pokémon; Gen 1 and Gen 2 Virtual Console games produce Gen 7 Pokémon, as with the original service.

## Transfers and refusals

**Nothing is converted by the patch.** The original Transporter builds a complete Bank transport box in memory and used to upload it. The patch writes that box, unchanged, into Bank's file. The next time you open Bank, the Pokémon are in the transport box, in the positions they had in Box 1 of the source game.

**Order of the two writes.** The delivery is written, flushed, and read back first. Only then does the original remove the Pokémon from the source game. A power cut in between leaves the Pokémon in both places, never in neither. If the delivery cannot be written, the original failure message is shown and the game is not touched.

**One save before the question.** Right after the chosen game is read, and before any notice or question, the original saves that game once. It stores a small note of its own there, which the server used to sort out an interrupted transfer; offline the note is empty and nothing reads it. No Pokémon data changes in that save, and it happens also if you then answer "No".

**When Transporter refuses.** It asks about Bank when START is pressed on the title screen, before it looks for games. If Bank cannot take a delivery, it shows one of two messages of the original. After A the original's line "Press HOME to return to the HOME Menu" follows, and it stays: nothing can be transferred until Bank was opened, so nothing else reacts. Press HOME, close Transporter, and open Bank. No game has been read at that point.

| Message | When | What to do |
| --- | --- | --- |
| "At least one Pokémon remains in the Transport Box from your previous session. Please empty the Transport Box by using Pokémon Bank…" | The transport box in Bank still holds Pokémon, or an earlier delivery has not been picked up. | Open Bank, empty the transport box, Save and Quit. |
| "Your communication with Pokémon Bank did not complete correctly during your last session. Please open Pokémon Bank and perform the cleanup process…" | Bank's files are not ready for a delivery: Bank has an unfinished save, the offline Bank has never been started on this console, its Bank is from a version this Transporter does not continue, or Transporter cannot open or read Bank's data. | Open Bank. It finishes what is unfinished or says what is wrong. Then Save and Quit once. |

The second message is the original's for a session the server asked to clean up; there is no cleanup to perform by hand. Every other message of the original comes where it always did, for the game that was chosen.

**The numbers of the second message.** It stands for several causes, so two numbers say which: on the third line of its first page, and again under the line about the HOME Menu. They are Bank's numbers and mean what they mean there ([bank.md](bank.md#troubleshooting)). The first message has one cause and no numbers.

| Numbers | What Transporter found |
| --- | --- |
| `00000003` and one of the console's result codes | Bank's data or its transport box files do not open: the offline Bank was never started on this console, or its Bank is from a version this Transporter does not continue. |
| `00000003` and a small number | A transport box file has that size instead of the expected one. |
| `00000009 7A000201` | One of the two transport box files is missing or does not read, for example after a power cut. |
| `00000010 00000000` | Bank has a save that is not finished. |

Transporter never creates or deletes files and touches no Bank file other than `/mover.bin` and `/mover.alt.bin`; it only ever writes to the one of the two that Bank is not using. If that file is missing or unreadable, for example after a power cut, Transporter refuses; open Bank and Save and Quit once, which writes it anew.

**Bank and Transporter belong together.** A Bank that was last used with v0.2.1 keeps its transport box in a file of another name. This Transporter does not find it and refuses, and the Transporter of v0.2.1 refuses with a converted Bank. Update both, after converting the Bank with the migration that ships with release v0.3.0.

## Choosing the language of the games

The game list shows the games of **one language** only: the cartridge, the saves on the SD card and the Virtual Console titles alike. The original language screen, reached from the title screen, chooses that language. It no longer changes the language of the screens, which stay in the console's language.

- Until you choose, the games in the console's language are listed. A console set to a language no source game exists in (Chinese, Dutch, Portuguese, Russian) starts with English.
- The choice is not saved, as the original saved nothing either: it lasts until Transporter is closed.
- After a choice, the title screen shows it: its logo and its start prompt appear in the chosen language. Japanese, English and Korean have the same prompt and differ in the logo only. Everything else stays in the console's language.
- The list offers the six languages that are not chosen already, and a **Back** button below them that leaves the screen without changing anything, as B does. The two Chinese entries are gone, because no Gen 1, 2 or 5 game exists in Chinese.
- Japanese is confirmed like every other language; the kana/kanji step is gone, since the screens do not change.
- The Back button is built like the Back buttons of Pokémon Bank's menus, to which this screen belongs: the screen's own list button with the text of the game-selection screen's Back, in the language of the screens, and the return icon in its own colour. It is put together when the screen is opened from parts the original already has; no file of the application is replaced.
- The seven entries are spaced like the buttons of Bank's main menu, with a small gap between them, and centred on the screen.
- The language is the game's, not the save's: for a Gen 5 game the language letter of its game code, for a Virtual Console title the language of the installed title.

## Black/White saves on the SD card

Transporter looks for saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card and offers each one as its own entry in the game list, beside the cartridge and the Virtual Console titles. Only saves in the [chosen language](#choosing-the-language-of-the-games) are looked for. **The cartridge is preferred for its own game:** with, say, German chosen and a German White cartridge inserted, the White entry is the cartridge and a German White save on the SD card is not offered; saves of the other three games still are. Bank does the same with a game that is both inserted and installed: it uses the cartridge. A cartridge in another language than the chosen one is not listed.

Only the names GodMode9 gives a cartridge dump are accepted, in TWiLight Menu's default folder:

```text
SD:/roms/nds/saves/POKEMON_B_IRB?01_??.sav     Black
SD:/roms/nds/saves/POKEMON_W_IRA?01_??.sav     White
SD:/roms/nds/saves/POKEMON_B2_IRE?01_??.sav    Black 2
SD:/roms/nds/saves/POKEMON_W2_IRD?01_??.sav    White 2
```

- The first `?` is the language letter of the cartridge: `J` Japanese, `O` English, `F` French, `I` Italian, `D` German, `S` Spanish, `K` Korean. For example `POKEMON_W2_IRDD01_00.sav` is a German White 2 and is listed while German is chosen.
- `??` is the cartridge revision, `00` or `01`. If both exist for the same game and language, only `01` is used.
- Any other name, folder or spelling is ignored. A file that is not exactly 524,288 bytes is ignored.
- At most four saves are offered, one per game, and the whole list holds 40 games, as in the original.

What happens to the file:

- It is read and checked by the original code exactly as a cartridge's save would be; a damaged save is refused by the original.
- Before the first change in a session, the untouched save is copied to `<name>.sav.bak` beside it (one copy, replaced the next time). If that copy cannot be written, nothing is changed. The first change is the original's save right after the game is read, so the copy exists before any Pokémon leaves Box 1. It is a way back if a write to the file is cut off, independent of the two copies a Gen 5 save keeps of itself. The name ends in `.bak` so that no program takes it for a save.
- The Pokémon are removed from the file only after the delivery to Bank has been written and verified, as with a cartridge.
- The file is never created, resized or renamed.

Do not insert or remove a cartridge while a session is running; what is offered is decided when you leave the title screen, with the language chosen at that moment. If you do it anyway:

| After START you | What happens |
| --- | --- |
| Insert a cartridge | Nothing. It is not offered, and the list stays as it was until the next START. |
| Take the cartridge out | The session ends with the original's message that no Game Card is inserted. The original looks for a removed Game Card before each of its saves and stops instead of saving; the patch neither adds nor removes that check. |
| Take it out after the Pokémon were written to Bank and before the game is saved | The same message. The Pokémon are then in Bank and still in the game, as after a power cut at that moment ([Limits](#limits)). |

## Install

1. Install the [Bank patch](bank.md#install) and start Bank once, so that the transport box file exists.
2. Back up the source game's save and Bank's extdata `0x00000C9B`.
3. Make sure the installed Poké Transporter is version 1.5, the final update.
4. Copy both files from the same release or build:

   ```text
   SD:/luma/titles/00040000000C9C00/code.ips
   SD:/luma/titles/00040000000C9C00/exheader.bin
   ```

   They only work as a pair.
5. *Enable game patching* must be on in Luma's configuration.

## Troubleshooting

| What you see | Likely cause |
| --- | --- |
| Transporter does not start, or stops while loading. | `code.ips` and `exheader.bin` are not from the same release or build, or Transporter is not version 1.5. |
| START always answers "Pokémon remains in the Transport Box". | Open Bank, empty the transport box, and Save and Quit. |
| START always answers "did not complete correctly… open Pokémon Bank". | Open Bank and Save and Quit once. If Bank shows an error instead, that error is the cause ([bank.md](bank.md#troubleshooting)). If Bank opens and saves and the refusal stays, Transporter cannot open Bank's data on this setup: check that Luma3DS is v10.0 or later and that Bank and Transporter are from the same release, and otherwise please report it with the two numbers of the message ([above](#transfers-and-refusals)). |
| After a refusal only "Press HOME to return to the HOME Menu" is left. | Intended: nothing can be transferred until Bank was opened. Close Transporter from the HOME Menu. |
| The original failure message after confirming a transfer. | The delivery could not be written. The source game was not changed. |
| A game is not listed: cartridge, save on the SD card or Virtual Console title. | It is in another language than the one whose games are listed. Choose its language on the language screen. |
| A save on the SD card is not listed. | Its language is not the chosen one; a cartridge of the same game in that language is inserted; the name or folder differs from the pattern above; or the file is not 524,288 bytes. |
| The language screen does not change the language of the screens. | Intended: it chooses the games. The screens follow the console's language. |
| The last entry of the language list shows Chinese text instead of Back. | The screen's layout is not the one the patch knows, so it was left as it is. Transporter is not version 1.5, or its files are modified. The entry still works as Back. |
| A save on the SD card is reported as damaged. | The original's own check failed on the file's contents, as it would on a cartridge. |

**Reporting a problem.** Open an issue with the console model, Luma version, the release or package name, the source game, which screen was showing when it stopped, the two numbers if it showed any, and whether the source game still holds the Pokémon.

## Limits

- **A duplicate after a power cut.** The delivery is written before the Pokémon are removed from the game. If the power fails exactly between the two, they are in Bank and still in the game. The original prevented this with the server's help; the patch does not.
- **Wording of refusals.** The original app has no text for "Bank is not set up" or "Bank's data cannot be read". Everything that is not "Pokémon in the way" uses its "did not complete correctly… open Pokémon Bank" message, whose advice fits all of them; the two numbers on it tell them apart.
- **Access to Bank's data and to the SD card.** Transporter's own header grants neither. Luma3DS gives every application full filesystem access, which is what makes both work; this needs Luma3DS v10.0 or later.
- **Saves on the SD card.** Only revisions `00` and `01` are looked for, one save per game in the chosen language. The search adds a short moment to the loading screen.
- **Language of the screens.** It can no longer be changed inside Transporter; it is the console's.
