# Pokémon Bank Offline Patch

A Luma3DS patch for **Pokémon Bank 1.5** (title `00040000000C9B00`, TMD version 6272) that keeps your Pokémon on the SD card instead of on Nintendo's servers.

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#bank); how it is checked on a console is in [testing.md](testing.md#bank-feature-by-feature).

## What changes

| Original Bank | Patched |
| --- | --- |
| Connects, checks the account and pass, then shows the main menu. | No connection. Start screen → game scan → open the local Bank → **game selection**. There is no main menu; Back on game selection returns to the start screen. |
| Downloads the Bank from the server and uploads it on save. | Reads and writes local files on a background thread while the original loading screen keeps animating. |
| The server resolves interrupted saves. | A local journal does. An interruption leaves either the old or the new state, never a mix ([below](#if-save-and-quit-is-interrupted)). |
| Loading screens, messages, sounds. | The originals. |
| First start: asks for a language, which cannot be changed later. | Never asks. Bank is always in the console's language ([below](#first-start)). |
| First start: shows the "Precaution for Use" notice (with the note for users under 18) and needs it accepted. | Not shown. It counts as accepted, and that is saved as the original saves it. |
| HOME and sleep are refused for the whole connected session. | Refused on every loading screen: from leaving the start screen until game selection appears, from choosing a game until its Bank is loaded, and during Save and Quit. They work everywhere else, and on every message that waits for a button, an error included. |
| Moving Pokémon stamps a server-checked time. | The same stamp from the console clock. |
| Poké Miles accrue from stored Pokémon; rewards also involve server gifts. | Miles accrue locally ([below](#poké-miles)). The original redemption is kept. Server gifts and distributions are skipped. |

Not included: Pokémon HOME, purchases, event gifts, and import or export tools for save editors.

## First start

The original asks two things when it has no save data of its own yet. Both are gone:

- **Language.** Bank no longer asks and no longer follows an earlier choice: it is in the console's language at every start, as Poké Transporter is. No language is ever stored, and one stored earlier by the original is cleared. A Japanese console gets kana; kanji could only be chosen on the removed screen.
- **Precaution for Use.** The notice is not shown. Bank goes on as if you had accepted it and creates its save data in the same step ("Preparing Pokémon Bank for your use…"), once.

**If you remove the patch later**, the original finds save data in which the notice is accepted and no language is chosen: it asks for the language and does not show the notice again. That also holds for save data in which the original had stored a language: the patched Bank clears it at its first start, so the original asks again.

This save data is Bank's own small save, not the extra data that holds your boxes.

**Brigette's welcome** is kept. The original plays it when no Bank exists yet, right before it creates one, and stores no mark for it. The patch does the same: it plays once, when the offline Bank is created, and never for a Bank that already exists. If you close Bank while she talks, no Bank has been created and the welcome plays at the next start.

## Poké Miles

**What you earn.** One Pokémon in the Bank earns one Mile in 30 days, counted by the hour: every full hour adds `Pokémon in the boxes ÷ 720` Miles. The part below a whole Mile is carried, so nothing is lost to rounding, however often you open Bank. This is the original rate; the original dropped the remainder at every visit.

- Only the 100 regular boxes count. The game's boxes and the transport box do not.
- 30 Pokémon earn 1 Mile a day, 15 earn 1 Mile every two days, 3,000 earn 100 a day.
- An hour counts when the clock passes a full hour: saved at 10:55 and opened at 11:05 is one hour.
- The hours up to a Save and Quit are paid for the Pokémon that were in the boxes when the game was loaded. What you deposit or withdraw counts from that save on.
- There is one balance for the whole Bank. Opening Y and then Ultra Sun does not earn twice.
- The balance stops at the original limit of 65,535.

**When it is saved.** Miles are added when the selected game has loaded and again at Save and Quit, and become permanent there, in the same step as your boxes. If you exit without saving, nothing changes and the same hours are credited next time. A new Bank starts earning with its first Save and Quit.

**The clock.** The date of the last Save and Quit and the day of the last redemption only move forward.

- Console date set back: saving still works, no Miles are earned, and the stored dates are kept. Earning resumes once the clock passes the stored date again.
- Console date set ahead: that time is paid once, because a console cannot tell a jump from a real absence. Back on the real date, nothing is earned until it catches up.

**Redeeming.** Unchanged original behaviour:

- Choices appear only with at least 10 Miles.
- Miles: the whole balance goes to the game. Gen 6 only; the original refuses Miles for Gen 7 games with its own message.
- Battle Points: `Miles ÷ 10` go to the game and the remainder stays. Gen 6 and Gen 7.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, blocks a new claim.

**When the dialog comes.** The original talks about Miles at every opening while the balance is above zero. The patch is quieter:

| Balance | Redeemed today | What appears |
| --- | --- | --- |
| 0 | | Nothing. The boxes open directly. |
| any | yes | Nothing for the rest of the day, also when the balance passes 10 again. |
| 1 to 9 | no | The original message, without choices, at the first opening of a day. After a Save and Quit on that day nothing more, until the balance reaches 10 or the day changes. |
| 10 or more | no | The original message and the choices, at every opening until you redeem. |

- "The first opening of a day" means that the last Save and Quit was on an earlier day. If you leave without saving, the message comes again at the next opening.
- An offer you decline comes back at the next opening, with the Miles of the hours since.
- The original's notice about a present still waiting in a Gen 6 game, or about a full gift list in a Gen 7 game, only comes together with an offer of 10 or more, where it stands in the way of a redemption. Without an offer it is not shown.
- With the console date set back before a stored date, that date counts as today.

The patch adds one safety check: after a claim, the balance and the gift in the game must match what was shown. If not, the balance is restored and the session ends with error `00000013` without saving.

## Pokédex and adventure records

As in the original, with one addition for games the Bank already knows. The Bank keeps eight separate slots, one per game version (X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, Ultra Moon). The National Pokédex you see is the combination of all eight.

- A game's Pokédex and adventure records are taken over for the first time when you **open the records screen** with that game; saving alone does not do it. They **replace that version's slot**. The other seven slots are untouched.
- Once a game is in its slot, **every Save and Quit with that game brings its Pokédex and records up to date**, without a visit to the records screen. This holds only for the same trainer.
- If the slot belongs to a different trainer, opening the screen takes nothing over. Choose that game's entry there and the original confirmation appears. Accepting replaces the slot, declining keeps it.
- What was taken over is stored with the next Save and Quit. If you leave without saving, it is not.

The original import copies; it does not merge within a version.

## Where the Bank is stored

Eight files inside SD extdata archive `0x00000C9B`, the archive Bank already owns. They come in four pairs. The first three are the data the original Bank kept on the server, cut into three pieces and otherwise unchanged; nothing is added to them.

| Files | Size of each | Each holds |
| --- | --- | --- |
| `/bank.bin`, `/bank.alt.bin` | 729,800 bytes | One set of the 100 boxes: the Pokémon, each box's name, and what Bank keeps per slot. Nothing else. |
| `/main.bin`, `/main.alt.bin` | 30,466 bytes | One version of everything else Bank needs: the group names, the Pokédex of all eight game versions with their trainer and adventure records, and everything about Miles (balance, the date of the last Save and Quit, the part of a Mile below one, the day of the last redemption). |
| `/mover.bin`, `/mover.alt.bin` | 6,990 bytes | One version of the transport box: up to 30 Pokémon from Poké Transporter. |
| `/journal.bin`, `/journal.alt.bin` | 140 bytes | One copy of the record that ties the others together: which file of each pair belongs to the current save, with a checksum of each; whether a save is in progress, and with which game; and whether Pokémon from Poké Transporter wait to be shown. |

**Why pairs.** If the power fails while the console writes a file, that file can become unreadable as a whole. So Bank never writes into a file that holds something it still needs: a save goes into the file of each pair that is not in use, and only when everything is written does that file become the current one. The two files of a pair take turns. `.alt` means "the alternate of the two", not "the older one"; half the time it holds your current data.

- **They always belong to the same save.** One record names the file of each pair that belongs to it, and a file counts only if it has the checksum the record names. After a power cut, Bank goes on with the three files that record names.
- **Missing transport files are not an error while the transport box is empty.** The next save creates them.
- **A file that a power cut left unreadable is replaced.** It is always the one that was being written, never the one in use. Bank goes on with the other file of the pair, and the next save deletes the unreadable one and writes it anew.
- **A file of the wrong size is never touched.** Bank stops with an error whose second number is the size: error `3` for the files of the record and the boxes, which are opened first, and the error of the step that needed it for another file. This is also what happens with a Bank that was last used with an earlier release whose files have other sizes. Such a Bank is converted once, with the migration that ships with the release that changed the files; that release says how.
- **A damaged file in use stops Bank** (error `9`). That holds for the boxes, for the main file, and for the transport box while it holds Pokémon: Bank never replaces what it cannot read with something empty. Pokémon from Poké Transporter that wait and cannot be read are not shown, and Save and Quit is then refused (error `F`), so that nothing is saved over them. A power cut cannot cause it, since it only reaches the file being written.
- **Files are written only** at Save and Quit, when they are first created, and when a start has to finish or undo an interrupted save. Loading and moving Pokémon around never write anything.
- **The transport box is filled by the [Transporter patch](transporter.md).** Bank shows what was delivered; you take Pokémon out by moving them into boxes, and what you leave in the box stays there.
- These files live inside console-managed, encrypted extdata. They are not loose files on the SD card, and Bank's own 128 KiB save is not where the boxes are.
- Dropping an edited save onto the SD card does not work: the formats differ and the files' checksums must stay consistent.

### Backing up the Bank

Because the boxes are in Bank's extdata, a backup of Bank's *save* does not contain them. In [Checkpoint](https://github.com/BernardoGiordano/Checkpoint), select Pokémon Bank, switch from save to **extdata** mode (X button), and back up from there; restore the same way. Back up the games you moved Pokémon to or from at the same time, so that the set fits together.

The formats are described in [internals.md](internals.md#storage).

## Install

1. Close Bank and all games. Back up every connected game save and Bank's own save. When updating, back up Bank's extdata `0x00000C9B` with them, as one dated set ([how](#backing-up-the-bank)); before the first install it holds no data.
2. Make sure the installed Pokémon Bank is version 1.5, the final update.
3. Copy both files from the same release or build:

   ```text
   SD:/luma/titles/00040000000C9B00/code.ips
   SD:/luma/titles/00040000000C9B00/exheader.bin
   ```

   They only work as a pair.
4. Hold SELECT while powering on, switch on *Enable game patching*, and save. Wireless can stay off.

The first start creates an empty Bank.

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
| The game is not inserted, or only its other copy is there (see below) | There is nothing to look at. | Decides the same way if the session moved Pokémon one way only. A session that moved them both ways waits for its game ([below](#a-save-that-waits-for-its-game)). |

In the first two cases nothing is lost and nothing exists twice. **So after an interrupted Save and Quit, start Bank again with the same game inserted before you play it.**

When the game's save cannot say, Bank takes the boxes that cannot lose a Pokémon:

| The interrupted session | Bank | Worst case |
| --- | --- | --- |
| Only deposited Pokémon | Keeps its new boxes | The deposited Pokémon exist twice. |
| Only withdrew or released Pokémon | Goes back to its old boxes | The withdrawn Pokémon exist twice. |
| Moved none between Bank and the game | Goes back to its old boxes | Miles of a claim count twice. |
| Did both | Keeps its new boxes | If the power failed before step 2, the withdrawn Pokémon are lost and the deposited ones exist twice. |

Only the last row can lose Pokémon. Pokémon moved between Bank's own boxes, and a delivery from Poké Transporter taken in that session, count as neither deposited nor withdrawn.

### A save that waits for its game

When the interrupted session moved Pokémon both ways and its game is not there, Bank changes nothing and says which game it needs, with the original's own message for it:

> You previously used a different copy of this game. Please try again using (name)'s game (Trainer ID No. (number)).

Name and number are those of the game the session was made with; the number has five digits for X, Y, Omega Ruby and Alpha Sapphire and six for the later games. After A Bank is back on its start screen. Make that game the one Bank finds and press START: Bank then looks at its save and finishes or undoes the interrupted Save and Quit as in the first table. Bank does not have to be closed for this.

| The interrupted session was with | At START | The message names | What to do |
| --- | --- | --- | --- |
| a cartridge | the cartridge is not inserted | the cartridge's game | Insert the cartridge. |
| the installed copy | a cartridge of the same game is inserted, so Bank reads the cartridge and not the installed copy | the **installed copy's** game, not the cartridge's | Take the cartridge out. |
| the installed copy | the game is no longer installed | the installed copy's game | Install it again with its save. |

The message only comes for a Save and Quit that was cut off. An error in a running session, for example a cartridge pulled out during Save and Quit, ends that run of Bank with two numbers ([Troubleshooting](#troubleshooting)); nothing is waiting then. Start Bank again with the game in, and if the save was cut off, this section applies.

**The same game as a cartridge and installed.** Bank uses one copy of each game: the cartridge if it is inserted, otherwise the installed copy. If the session was with the cartridge and the cartridge is taken out, the installed copy takes its place with a save of its own, and the same happens the other way round when a cartridge is inserted beside an installed copy. Bank notices that it is the other kind of copy and treats the game as not inserted. This only concerns you if you have a game in both forms and have played both.

**After updating the patch.** A Save and Quit that was interrupted is finished by the version it was made with, or by one that records a save in progress the same way; a version that records it differently shows error `B` and changes nothing, whether its game is inserted or not. Put that version back, start Bank once, then switch. This also holds between v0.3.0 and the versions after it, which keep the game's name and trainer ID with a save in progress.

## Troubleshooting

An error ends this run of Bank. Stored data is kept. Bank has no general error text, so the patch shows the first sentence of the original's message for a failed save for every error ("The server did not receive the data.", in the console's language). The sentence about the server is not to be taken literally; it only says that something failed.

After A the original's line "Press HOME to return to the HOME Menu" appears with two lines under it, and it stays: nothing else reacts, because nothing can go on.

```text
Press HOME to return to the HOME Menu
v0.4.0
XXXXXXXX YYYYYYYY
```

The second line is the version of the patch, with a name behind it if it is not the normal package ("-test", "-migration", "-download"; "dev" for a package someone built without a version). Only the numbers tell what happened: the first says which step failed, the second why. Press HOME, close Bank, and start it again. Bank's files are closed before the error appears.

| First number | Step |
| --- | --- |
| `3`, `4` | Opening or creating the files |
| `5` | The stored data is not valid |
| `6` | Console clock |
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
| `7A00ffpp` | A problem with one of the smaller files. `ff`: 1 main, 2 transport. `pp`: 1 damaged or missing, 3 Pokémon from Poké Transporter wait but cannot be read, so nothing is saved over them, 4 not the format this version knows. |
| `8xxxxxxx`–`Fxxxxxxx` | The console's own filesystem result code. |

**Starting over with an empty Bank.** This deletes every Pokémon stored in the offline Bank.

1. Power off and put the SD card in a PC.
2. Delete the folder `Nintendo 3DS/<ID0>/<ID1>/extdata/00000000/00000C9B`. Its contents are encrypted, so you will not see the file names; delete the whole folder.
3. Start Bank again.

Deleting Bank's Extra Data in System Settings may not remove the files.

**Reporting a problem.** Open an issue with the console model, Luma version, the games involved, and a photo of any error screen, or its last two lines as text: the version and the two numbers. On an error, keep the SD card as it is.

## Limits

- **First start.** Creating the files runs in the background, but two original steps still run on the main thread and can cause short pauses: the game scan creates the extdata archive, and the default box and group names are formatted in one call. If the power fails while the Bank is being created ("Preparing Pokémon Bank for your use…"), the next start creates it again from the beginning, welcome included; nothing has to be deleted.
- **Backups.** The journal cannot repair a game save damaged mid-write, and cannot detect restoring a Bank backup and a game backup from different times. Always back up Bank's extdata and your games together.
- **Power cuts.** A cut during Save and Quit can only damage the file being written, which Bank does not need and replaces. It cannot protect against damage to the SD card's own file table; that holds for every save on the console.
- **Interrupted Save and Quit.** One case can lose Pokémon: see [If Save and Quit is interrupted](#if-save-and-quit-is-interrupted).
- **Clock.** A console date set ahead is paid as if the time had passed.
- **Errors are final for a run of Bank.** After the message only the line about the HOME Menu follows, with the two numbers. Close Bank from the HOME Menu and start it again. A save that waits for its game is not an error: it leads back to the start screen ([above](#a-save-that-waits-for-its-game)).
