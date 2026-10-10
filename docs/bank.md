# Pokémon Bank Offline Patch

A Luma3DS patch for **Pokémon Bank 1.5** (title `00040000000C9B00`, TMD version 6272) that keeps your Pokémon on the SD card instead of on Nintendo's servers.

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#bank).

## What changes

| Original Bank | Patched |
| --- | --- |
| Connects, checks the account and pass, then shows the main menu. | No connection. Start screen → game scan → open the local Bank → **game selection**. There is no main menu; Back on game selection returns to the start screen. |
| Downloads the Bank from the server and uploads it on save. | Reads and writes local files on a background thread while the original loading screen keeps animating. |
| The server resolves interrupted saves. | A local journal does. An interruption leaves either the old or the new state, never a mix ([below](#if-save-and-quit-is-interrupted)). |
| Loading screens, messages, sounds. | The originals. |
| First start: asks for a language, which cannot be changed later. | Never asks. Bank is always in the console's language ([below](#first-start)). |
| First start: shows the "Precaution for Use" notice (with the note for users under 18) and needs it accepted. | Not shown. It counts as accepted, and that is saved as the original saves it. |
| HOME and sleep are refused for the whole connected session. | Refused on every loading screen: from leaving the start screen until game selection appears, from choosing a game until its Bank is loaded, and during Save and Quit. They work everywhere else. |
| Moving Pokémon stamps a server-checked time. | The same stamp from the console clock. |
| Poké Miles accrue from stored Pokémon; rewards also involve server gifts. | Miles accrue locally ([below](#poké-miles)). The original redemption is kept. Server gifts and distributions are skipped. |
| Pokédex and adventure records update when you visit the records screen. | They update at Save and Quit, with the original rules ([below](#pokédex-and-adventure-records)). |

Not included: Pokémon HOME, purchases, importing a Bank from Nintendo's servers, event gifts, and import or export tools for save editors.

## First start

The original asks two things when it has no save data of its own yet. Both are gone:

- **Language.** Bank no longer asks and no longer follows an earlier choice: it is in the console's language at every start, as Poké Transporter is. No language is ever stored, and one stored earlier by the original is cleared. A Japanese console gets kana; kanji could only be chosen on the removed screen.
- **Precaution for Use.** The notice is not shown. Bank goes on as if you had accepted it and creates its save data in the same step ("Preparing Pokémon Bank for your use…"), once.

**If you remove the patch later**, the original finds save data in which the notice is accepted and no language is chosen: it asks for the language and does not show the notice again. That also holds for save data in which the original had stored a language: the patched Bank clears it at its first start, so the original asks again.

This save data is Bank's own small save, not the extra data that holds your boxes.

**Brigette's welcome** is kept. The original plays it when no Bank exists yet, right before it creates one, and stores no mark for it. The patch does the same: it plays once, when the offline Bank is created, and never for a Bank that already exists. If you close Bank while she talks, no Bank has been created and the welcome plays at the next start.

## Poké Miles

**What you earn.** Each calendar day earns `Pokémon in the Bank at the last Save and Quit ÷ 30` Miles. The remainder is carried in thirtieths, so nothing is lost to rounding. This is the original rate, counted per calendar day instead of per hour.

- Only the 100 regular boxes count. The game's boxes and the transport box do not.
- New Miles are added to the balance already stored. Changing how many Pokémon you keep only changes what later days earn.
- There is one record for the whole Bank. Opening Y and then Ultra Sun on the same day does not earn twice.
- 15 Pokémon earn 1 Mile every two days. 30 Pokémon saved before midnight earn 1 Mile when opened after midnight.
- The balance stops at the original limit of 65,535.

**When it is saved.** Miles are shown when the selected game has loaded and become permanent at Save and Quit, in the same step as your boxes. If you exit without saving, nothing changes and the same days are credited next time. The first save of a new Bank only records the date and count; earning starts the next day.

**The clock.** The accounting date only moves forward.

- Console date set back: saving still works, no Miles are earned, and the stored date is kept. Earning resumes once the clock passes that date again.
- Console date set ahead: those days are paid once, because a console cannot tell a jump from a real absence. Back on the real date, nothing is earned until it catches up.

**Redeeming.** Unchanged original behaviour:

- Choices appear only with at least 10 Miles.
- Miles: the whole balance goes to the game. Gen 6 only; the original refuses Miles for Gen 7 games with its own message.
- Battle Points: `Miles ÷ 10` go to the game and the remainder stays. Gen 6 and Gen 7.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, blocks a new claim.

**When nothing is said.** The original talks about Miles whenever the balance is above zero. The patch goes straight to the boxes when it is still the day of the last Save and Quit and the balance is below 10. That is the usual state after redeeming Battle Points, which leaves a remainder. In that case the game's notice about a present still waiting in it is skipped as well, since nothing new could be sent. With 10 or more Miles, or on any later day, everything appears as in the original; on a later day that means once, until the next Save and Quit.

The patch adds one safety check: after a claim, the balance and the gift in the game must match what was shown. If not, the balance is restored and the session ends with error `00000013` without saving.

## Pokédex and adventure records

The patch calls the original import functions, so the rules are Bank's own. The Bank keeps eight separate slots, one per game version (X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, Ultra Moon). The National Pokédex you see is the combination of all eight.

- Saving with a game **replaces that version's slot** with the game's Pokédex and adventure records. The other seven slots are untouched.
- If the slot belongs to a different trainer, the original confirmation appears. Accepting replaces the slot, declining keeps it.

The original import copies; it does not merge within a version.

## Where the Bank is stored

Ten files inside SD extdata archive `0x00000C9B`, the archive Bank already owns. They come in five pairs:

| Files | Size of each | Each holds |
| --- | --- | --- |
| `/bank.bin`, `/bank.alt.bin` | 730,474 bytes | One set of the 100 boxes with their names, groups, and per-slot data. |
| `/journal.bin`, `/journal.alt.bin` | 192 bytes | One copy of the record that says which set of boxes is current and whether a save is in progress. |
| `/dex.bin`, `/dex.alt.bin` | 29,888 bytes | One version of the Pokédex of all eight game versions and their trainer and adventure records. |
| `/mover.bin`, `/mover.alt.bin` | 7,056 bytes | One version of the transport box: up to 30 Pokémon from Poké Transporter. |
| `/rewards.bin`, `/rewards.alt.bin` | 80 bytes | One version of everything about Miles: balance, date, saved count, fraction. |

**Why pairs.** If the power fails while the console writes a file, that file can become unreadable as a whole. So Bank never writes into a file that holds something it still needs: a save goes into the file of each pair that is not in use, and only when everything is written does that file become the current one. The two files of a pair take turns. `.alt` means "the alternate of the two", not "the older one"; half the time it holds your current data.

- **They always belong to the same save.** After a power cut, Bank uses the versions of the smaller files that match whichever Bank save survived.
- **A missing file is not an error.** No Pokédex files mean an empty Pokédex, no transport files an empty transport box, no rewards files zero Miles. The next save creates them.
- **A file that a power cut left unreadable is replaced.** It is always the one that was being written, never the one in use. Bank goes on with the other file of the pair, and the next save deletes the unreadable one and writes it anew.
- **A file of the wrong size is never touched.** Bank stops with error `3` and the size as second number. This is also what happens with a Bank stored by version 0.2.1 or earlier, whose `/bank.bin` is twice as large: it has to be converted first ([Updating from 0.2.1 or earlier](#updating-from-021-or-earlier)).
- **A damaged Pokédex or Miles file in use does not stop Bank either.** The Pokédex then starts empty and the Miles at zero. Each game writes its part of the Pokédex and its records again at its next Save and Quit. A damaged transport box in use does stop Bank (error `9`), because it can hold Pokémon.
- **Files are written only** at Save and Quit, when they are first created, and when a start has to finish or undo an interrupted save. Loading and moving Pokémon around never write anything.
- **The transport box is filled by the [Transporter patch](transporter.md).** Bank shows what was delivered; you take Pokémon out by moving them into boxes, and what you leave in the box stays there.
- These files live inside console-managed, encrypted extdata. They are not loose files on the SD card, and Bank's own 128 KiB save is not where the boxes are.
- Dropping an edited save onto the SD card does not work: the formats differ and the files' checksums must stay consistent.

### Backing up the Bank

Because the boxes are in Bank's extdata, a backup of Bank's *save* does not contain them. In [Checkpoint](https://github.com/BernardoGiordano/Checkpoint), select Pokémon Bank, switch from save to **extdata** mode (X button), and back up from there; restore the same way. Back up the games you moved Pokémon to or from at the same time, so that the set fits together.

The formats are described in [internals.md](internals.md#storage).

## Updating from 0.2.1 or earlier

Versions up to 0.2.1 kept the Bank in four files. A power cut during Save and Quit could leave such a Bank unreadable, which is why the layout changed. An existing Bank is converted once, with a separate **migration package** that does nothing else:

1. Start Bank once with the version you have and make sure it opens. An interrupted Save and Quit has to be finished by that version.
2. Back up Bank's extdata and your game saves ([how](#backing-up-the-bank)).
3. Install the migration package (the same two files, same folder), start Pokémon Bank and press START. It does not open the Bank. After a loading screen it shows two numbers:

   | Numbers | Meaning |
   | --- | --- |
   | `0000600D 00000001` | Converted. |
   | `0000600D 00000002` | Already converted; nothing was changed. |
   | `0000600D 00000000` | No Bank was found. |
   | `00000BAD 00000007` | A Save and Quit is unfinished. Go back to step 1. |
   | `00000BA1` to `00000BA4` | A step failed; the second number says why. Starting the migration package again is safe. |

4. Install the current Bank and Transporter patches. Both have to be updated together: an older Transporter does not find the transport box of a converted Bank and refuses to transfer.

Until the conversion is complete the old files are untouched, and a power cut at any point is picked up by starting the migration package again. It needs about 1.5 MB of free space on the SD card while it runs. After the conversion, versions up to 0.2.1 cannot open the Bank any more; the backup from step 2 is the way back.

## Install

1. Close Bank and all games. Back up every connected game save, Bank's own save, and Bank's extdata `0x00000C9B` as one dated set ([how](#backing-up-the-bank)).
2. Make sure the installed Pokémon Bank is version 1.5, the final update.
3. Copy both files from the same release or build:

   ```text
   SD:/luma/titles/00040000000C9B00/code.ips
   SD:/luma/titles/00040000000C9B00/exheader.bin
   ```

   They only work as a pair.
4. Hold SELECT while powering on, switch on *Enable game patching*, and save. Wireless can stay off.

The first start creates an empty Bank.

## Checking that it works

### Start and navigation

| Do this | Expected |
| --- | --- |
| First start. | Original opening message, moving spinner, loading sound, then game selection. The main menu never appears. Short pauses are possible (see [Limits](#limits)). |
| Back on game selection. | Start screen. |
| Select a game, Back on its confirmation. | Game selection again. |
| Select a game, cancel at the reward prompt; again, leave the boxes without saving. | Start screen. Nothing saved. |
| Move Pokémon both ways, rename a box, Save and Quit, restart. | Everything persisted in Bank and in the game. No missing or duplicate Pokémon. |
| Press HOME and close the lid right after leaving the start screen, while the Bank loads after selecting a game, and during Save and Quit. | Nothing happens while a loading screen is shown. On the start screen, game selection, in the boxes, and on an error screen both work as usual. |
| No usable game present. | The original "no game" notice once, then the start screen. |

### Poké Miles

The first Save and Quit only records the date and your Pokémon count N.

| Do this | Expected |
| --- | --- |
| Next day (or console date +1), open and select a game. | `N ÷ 30` Miles more than before, rounded down. |
| Decline, withdraw some Pokémon, Save and Quit, reopen the same day. | Balance unchanged. |
| Same day, a different game. | Same balance. No second credit. |
| Fewer than 10 Miles, first opening on a later day than the last Save and Quit (whether or not Miles were added). | Original messages, no redemption choices. Again at each opening until a Save and Quit on that day. |
| Fewer than 10 Miles, reopened the same day after Save and Quit (for example the remainder after redeeming Battle Points). | No reward dialog; the boxes open directly. Also no "present waiting" notice from a Gen 6 game. |
| 10 or more Miles saved without redeeming, reopened the same day. | The redemption choices appear as before. |
| 10 or more, Gen 6: Miles one day, Battle Points another. | Miles: balance 0. BP: `Miles ÷ 10` to the game, remainder kept. The gift waits in the game after Save and Quit. |
| 10 or more, Gen 7. | Miles refused by the original message; BP arrives in the gift list. |
| Redeem, then leave without saving. | Balance as before the claim; no gift in the game. |
| Set the date back a day, open, Save and Quit. | Save works. No Miles. After returning to the real date, none earned twice. |

### Pokédex

| Do this | Expected |
| --- | --- |
| Save with Y, then with Ultra Sun. | A species known only to Y and one known only to Ultra Sun both show in the National Pokédex. |
| Same version, different trainer (for example two Omega Ruby saves). | The original replacement prompt. Accept: that version's entries and records become the new save's. Decline: unchanged. |

## If Save and Quit is interrupted

Save and Quit writes to two places, always in this order:

| Step | What is written |
| --- | --- |
| 1 | Bank's new boxes, beside the old ones, and a mark that a save is in progress |
| 2 | The game's save |
| 3 | The mark is removed |

Until step 3 Bank still holds both sets of boxes. If the power fails, the next start finds the mark and looks at the game's save to see how far the save got:

| The game's save at the next start | What that means | Bank |
| --- | --- | --- |
| Unchanged | The power failed before step 2 was finished. | Goes back to its old boxes. Nothing was moved. |
| The one Bank wrote | The power failed after step 2. | Keeps its new boxes. Everything was moved. |
| Something else: the game was played and saved, a new game was started on it, or it is another cartridge of the game | The save no longer shows how far it got. | Decides by what the session moved (next table) and opens. |
| The game is not inserted, or only its other copy is there (see below) | There is nothing to look at. | Decides the same way if the session moved Pokémon one way only. A session that moved them both ways waits for its game (error `7`). |

In the first two cases nothing is lost and nothing exists twice. **So after an interrupted Save and Quit, start Bank again with the same game inserted before you play it.**

When the game's save cannot say, Bank takes the boxes that cannot lose a Pokémon:

| The interrupted session | Bank | Worst case |
| --- | --- | --- |
| Only deposited Pokémon | Keeps its new boxes | The deposited Pokémon exist twice. |
| Only withdrew or released Pokémon | Goes back to its old boxes | The withdrawn Pokémon exist twice. |
| Moved none between Bank and the game | Goes back to its old boxes | Miles of a claim count twice. |
| Did both | Keeps its new boxes | If the power failed before step 2, the withdrawn Pokémon are lost and the deposited ones exist twice. |

Only the last row can lose Pokémon. Pokémon moved between Bank's own boxes, and a delivery from Poké Transporter taken in that session, count as neither deposited nor withdrawn.

**The same game as a cartridge and installed.** Bank uses one copy of each game: the cartridge if it is inserted, otherwise the installed copy. If the session was with the cartridge and the cartridge is taken out, the installed copy takes its place with a save of its own, and the same happens the other way round when a cartridge is inserted beside an installed copy. Bank notices that it is the other kind of copy and treats the game as not inserted. This only concerns you if you have a game in both forms and have played both.

**After updating the patch.** A Save and Quit that was interrupted under another version of the patch is finished by that version only; any other shows error `B` and changes nothing. Put that version back, start Bank once, then switch.

## Troubleshooting

An error ends the session; restart Bank afterwards. Stored data is kept. Bank has no general error text, so the patch shows the first sentence of the original's message for a failed save for every error ("The server did not receive the data.", in the console's language) with two numbers on the third line: `XXXXXXXX YYYYYYYY`. Only the numbers tell what happened; the sentence about the server is not to be taken literally. The first number says which step failed, the second why.

| First number | Step |
| --- | --- |
| `3`, `4` | Opening or creating the files |
| `5` | The stored data is not valid |
| `6` | Console clock |
| `7` | An interrupted Save and Quit that moved Pokémon both ways is waiting for its game. Insert that game and start Bank again ([details](#if-save-and-quit-is-interrupted)). |
| `9` | Loading |
| `A`–`E` | Game save |
| `F` | Preparing the save |
| `10`, `11` | Finishing or undoing an interrupted save |
| `12`, `13` | Rewards |

| Second number | Meaning |
| --- | --- |
| Small number, for example `00176BF0` | A file exists with that size instead of the expected one. |
| `78000000` | Bank's own filesystem session was not ready. |
| `710000oo` | A call succeeded but returned no handle (`oo`: 0 open archive, 3 open file). |
| `720000oo` | A short read or write (`oo`: 5 read, 6 write). |
| `77000000` | Bank's filesystem session changed between two opens. |
| `79000000` | A file that is needed exists but cannot be opened. |
| `7A00ffpp` | A problem with one of the smaller files. `ff`: 1 Pokédex, 2 transport, 3 rewards. `pp`: 1 damaged, 3 a waiting delivery would be overwritten. |
| `8xxxxxxx`–`Fxxxxxxx` | The console's own filesystem result code. |

**Starting over with an empty Bank.** This deletes every Pokémon stored in the offline Bank.

1. Power off and put the SD card in a PC.
2. Delete the folder `Nintendo 3DS/<ID0>/<ID1>/extdata/00000000/00000C9B`. Its contents are encrypted, so you will not see the file names; delete the whole folder.
3. Start Bank again.

Deleting Bank's Extra Data in System Settings may not remove the files.

**Reporting a problem.** Open an issue with the console model, Luma version, the release or package name, the games involved, and a photo of any error screen. On an error, keep the SD card as it is.

## Limits

- **First start.** Creating the files runs in the background, but two original steps still run on the main thread and can cause short pauses: the game scan creates the extdata archive, and the default box and group names are formatted in one call. If the power fails while the Bank is being created ("Preparing Pokémon Bank for your use…"), the next start creates it again from the beginning, welcome included; nothing has to be deleted.
- **Backups.** The journal cannot repair a game save damaged mid-write, and cannot detect restoring a Bank backup and a game backup from different times. Always back up Bank's extdata and your games together.
- **Power cuts.** A cut during Save and Quit can only damage the file being written, which Bank does not need and replaces. It cannot protect against damage to the SD card's own file table; that holds for every save on the console.
- **Interrupted Save and Quit.** One case can lose Pokémon: see [If Save and Quit is interrupted](#if-save-and-quit-is-interrupted).
- **Clock.** A console date set ahead is paid as if the days had passed.
- **Errors are final for the session.** After an error with the two numbers, restart Bank. Until then, pressing START shows the same error again and opens nothing.
