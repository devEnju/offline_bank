# Pokémon Bank Offline Patch

A Luma3DS patch for **Pokémon Bank 1.5** (title `00040000000C9B00`, TMD version 6272) that keeps your Pokémon on the SD card instead of on Nintendo's servers.

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#bank); how it is checked on a console is in [testing.md](testing.md#bank-feature-by-feature).

## What changes

| Original Bank | Patched |
| --- | --- |
| Connects, checks the account and pass, then shows the main menu. | No connection. Start screen → game scan → open the local Bank → **game selection**. There is no main menu; Back on game selection returns to the start screen. |
| Downloads the Bank from the server and uploads it on save. | Reads and writes local files on a background thread while the original loading screen keeps animating. |
| The server resolves interrupted saves. | A local journal does. An interruption leaves either the old or the new state, never a mix ([below](#if-save-and-quit-is-interrupted)). |
| First start: asks for a language, which cannot be changed later. | Never asks. Bank is always in the console's language ([below](#first-start)). |
| First start: shows the "Precaution for Use" notice (with the note for users under 18) and needs it accepted. | Not shown. It counts as accepted, and that is saved as the original saves it. |
| HOME and sleep are refused for the whole connected session. | Refused on every loading screen: from leaving the start screen until game selection appears, from choosing a game until its Bank is loaded, and during Save and Quit. They work everywhere else, and on every message that waits for a button, an error included; the unlock screen's number keyboard takes no HOME press, as in the original. |
| Moving Pokémon stamps a server-checked time. | The same stamp from the console clock. |
| Poké Miles accrue from stored Pokémon; rewards also involve server gifts. | Miles accrue locally ([below](#poké-miles)). The original welcome gift and redemption are kept. Server gifts and distributions are skipped. |

Not included: Pokémon Home, purchases, and event gifts.

For an existing Bank, the original warning about removing the Game Card or SD card stays on the same loading screen from START until game selection. The spinner continues while Bank scans the games and opens its local files.

If no compatible save can be read, Bank shows the original missing-save explanation and Pokédex reminder. A advances to the second page and A again closes the dialog and returns to the start screen. The Pokémon Home question is omitted. No offline Bank is opened or created. Insert a cartridge or make an installed save available, then press START again. A readable save's Pokédex eligibility is checked later, when you select that game, as in the original.

## First start

The original asks two things when it has no save data of its own yet. Both are gone:

- **Language.** Bank no longer asks and no longer follows an earlier choice: it is in the console's language at every start, as Poké Transporter is. No language is ever stored, and one stored earlier by the original is cleared. A Japanese console gets kana and kanji could only be chosen on the removed screen.
- **Precaution for Use.** The notice is not shown. Bank goes on as if you had accepted it and creates its save data in the same step, once.

**If you remove the patch later**, the original finds save data in which the notice is accepted and no language is chosen: it asks for the language and does not show the notice again. That also holds for save data in which the original had stored a language: the patched Bank clears it at its first start, so the original asks again.

This save data is Bank's own small save, not the extra data that holds your boxes.

**Brigette's welcome** is kept. The original plays it when no Bank exists yet, right before it creates one, and stores no mark for it. The patch does the same: it plays once, when the offline Bank is created, and never for a Bank that already exists. If you close Bank while she talks, no Bank has been created and the welcome plays at the next start.

## Poké Miles

**What you earn.** One Pokémon in the Bank earns one Mile in 30 days, counted by the hour: every full hour adds `Pokémon in the boxes ÷ 720` Miles. The part below a whole Mile is carried, so nothing is lost to rounding, however often you open Bank. This is the original rate, but the original dropped the remainder at every visit.

- Only the 100 regular boxes count. The game's boxes and the transport box do not.
- 30 Pokémon earn 1 Mile a day, 15 earn 1 Mile every two days, 3,000 earn 100 a day.
- An hour counts when the clock passes a full hour: saved at 10:55 and opened at 11:05 is one hour.
- The hours up to a Save and Quit are paid for the Pokémon that were in the boxes when the game was loaded. What you deposit or withdraw counts from that save on.
- There is one balance for the whole Bank. Opening one game and then another does not earn twice.
- The balance stops at the original limit of 65,535.

**The welcome gift.** Once per Bank, the original's welcome gives 100 Miles: its three messages after the game is loaded, then the choices as for any redemption.

- It comes after the first game selection of a Bank that has not had it. A Bank downloaded from the official server had it there.
- It becomes permanent with the next Save and Quit, together with the Miles. If you leave without saving, it comes again at the next opening, again with 100, never twice.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, comes first with the original's notice. If you go on to the boxes, the welcome waits for the next opening of a game that can take a gift.

**When it is saved.** Miles are added when the selected game has loaded and again at Save and Quit, and become permanent there, in the same step as your boxes. If you exit without saving, nothing changes and the same hours are credited next time. A new Bank starts earning with its first Save and Quit even if the 100 extra Miles had not been redeemed yet.

**The clock.** The date of the last Save and Quit and the day of the last redemption only move forward.

- Console date set back: saving still works, no Miles are earned, and the stored dates are kept. Earning resumes once the clock passes the stored date again.
- Console date set ahead: that time is paid once, because a console cannot tell a jump from a real absence. Back on the real date, nothing is earned until it catches up.

**Redeeming.** Unchanged original behaviour:

- Choices appear only with at least 10 Miles.
- Miles: the whole balance goes to the game. Gen 6 only, while the original refuses Miles for Gen 7 games with its own message.
- Battle Points: `Miles ÷ 10` go to the game and the remainder stays. Gen 6 and Gen 7.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, blocks a new claim.

**When the dialog comes.** The original talks about Miles at every opening while the balance is above zero. The patch is quieter (apart from the welcome gift, above):

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

- A game's Pokédex and adventure records are taken over for the first time when you **open the records screen** with that game while saving alone does not do it. They **replace that version's slot**. The other seven slots are untouched.
- Once a game is in its slot, **every Save and Quit with that game brings its Pokédex and records up to date**, without a visit to the records screen. This holds only for the same trainer.
- If the slot belongs to a different trainer, opening the screen takes nothing over. Choose that game's entry there and the original confirmation appears. Accepting replaces the slot, declining keeps it.
- What was taken over is stored with the next Save and Quit. If you leave without saving, it is not.

The original import copies and does not merge within a version.

## Where the Bank is stored

Eight files inside SD extdata archive `0x00000C9B`, the archive Bank already owns, in four pairs. The first three hold the data the original Bank kept on the server, cut into three pieces and otherwise unchanged.

| Files | Size of each | Each holds |
| --- | --- | --- |
| `/bank.bin`, `/bank.alt.bin` | 729,800 bytes | The 100 boxes: the Pokémon, each box's name, and what Bank keeps per slot. |
| `/main.bin`, `/main.alt.bin` | 30,466 bytes | Everything else: the group names, the Pokédex of all eight game versions with their trainer and adventure records, and the Miles. |
| `/mover.bin`, `/mover.alt.bin` | 6,990 bytes | The transport box: up to 30 Pokémon from Poké Transporter. |
| `/journal.bin`, `/journal.alt.bin` | 140 bytes | The record that ties the others together: which file of each pair is current, with its checksum, and whether a save is in progress. |

**Why pairs.** A save writes the file of each pair that is not in use and switches over only when everything is written. A power cut during a save leaves the files in use as they were, and the next start decides from the game's save which ones to keep ([below](#if-save-and-quit-is-interrupted)). `.alt` is not an older save: the two files take turns.

- Files are written only at Save and Quit, when they are first created, and when a start finishes an interrupted save. Loading and moving Pokémon never write.
- If a file Bank needs cannot be read, Bank shows its error screen and changes nothing. It never replaces what it cannot read with something empty.
- The transport box is filled by the [Transporter patch](transporter.md). You take Pokémon out by moving them into boxes and what you leave there stays.
- The files are inside encrypted extdata, not loose on the SD card. An edited save dropped onto the card does not work.

The formats are described in [internals.md](internals.md#storage).

## Install

1. Close Bank and all games. As a precaution you can back up your games' saves first.
2. Make sure the installed Pokémon Bank is version 1.5, the final update.
3. Copy both files from the same release or build. They only work as a pair.

   ```text
   SD:/luma/titles/00040000000C9B00/code.ips
   SD:/luma/titles/00040000000C9B00/exheader.bin
   ```

4. Hold SELECT while powering on, switch on *Enable game patching*, and save. Wireless can stay off.

The first start creates an empty Bank.

## Backing up the Bank

The boxes are in Bank's extdata, so a backup of Bank's *save* does not contain them. In [Checkpoint](https://github.com/BernardoGiordano/Checkpoint), switch to **extdata** mode (X button), select Pokémon Bank, and back up or restore from there.

## If Save and Quit is interrupted

If the power fails or the cartridge is pulled during Save and Quit, **start Bank again with the same game inserted, before you play that game.** Bank sees from the game's save how far the save got, finishes or undoes it, and opens as usual.

If the game's save cannot show it (the game was played and saved since, or it is not there), Bank keeps the side that cannot lose a Pokémon:

| The interrupted session | Bank keeps | At worst |
| --- | --- | --- |
| Only deposited Pokémon | the new boxes | The deposited Pokémon are in Bank and still in the game. |
| Only withdrew or released Pokémon | the old boxes | The withdrawn Pokémon are in the game and still in Bank. |
| Moved none between Bank and the game | the old boxes | A Miles claim counts twice. |
| Moved Pokémon both ways | nothing yet: it needs you [below](#a-save-that-waits-for-its-game) | |

None of this happens in normal use. It takes a rare event such as a power failure during the few seconds of Save and Quit.

### A save that waits for its game

> You previously used a different copy of this game. Please try again using \<name>'s game (Trainer ID No. \<number>).

The session's game is not there. Make it the one Bank finds and press START. Bank uses the cartridge if one of the game is inserted, otherwise the installed copy.

| The session was with | Do this |
| --- | --- |
| a cartridge | Insert that cartridge. |
| the installed copy | Take out the cartridge of the same game, or install the game again with its save. |
| either, but another trainer's game is found | Use the game the message names. |

### A save left to Transporter

> Your communication with Poké Transporter did not complete correctly during your last session.
> (A)
> Please open Poké Transporter and perform the cleanup process before using Pokémon Bank.

The game is there but was saved on since, so its save cannot show which boxes fit it. Open Poké Transporter and press START: it asks you ([transporter.md](transporter.md#a-save-left-to-transporter)). Until you answer, nothing changes.

### Unlock without the game

Only if your session's game will never be available again. Press START and hold **L, A and START** until the loading screen ends. The following screen then comes in place of the message. Enter the eight digits as shown.

> Please enter the unlock key: \<digits>

From then on it sends you to Poké Transporter, as above. **Cancel** changes nothing.

## A pulled Game Card or a full SD card

Bank shows the original's message and returns to its start screen after A. Nothing is lost and Bank does not have to be closed.

| Message | Do this |
| --- | --- |
| "Please confirm that your Game Card is properly inserted, and then try again." | Insert the cartridge and press START. |
| "Pokémon Bank could not be started due to a lack of space on your SD card." (first start), "The SD card is full." (Save and Quit) | Close Bank, free space, start again. Nothing was saved. |

## A game save that does not load

> The saved data in your Pokémon game is corrupted.

The chosen game's save failed the check of its checksums, so Bank does not load it. Nothing is written, and every other game can be chosen as usual. Start the game itself to see what it makes of its save.

The patch's changes cannot cause this. The game's save is written by the original Bank's own code, exactly as without the patch. What the patch adds is the check: the original did not check a save when loading it, and its next Save and Quit wrote a damaged one back with new checksums, which hid the damage. The patch checks first, so that nothing damaged reaches your boxes.

## Troubleshooting

| What you see | Do this |
| --- | --- |
| Bank does not start, or stops while loading. | Use `code.ips` and `exheader.bin` from the same release, with Bank 1.5. |
| A message about an interrupted save, a pulled Game Card, a full SD card or a corrupted save. | See the sections above. |
| "Press HOME to return to the HOME Menu" with two numbers under it. | Press HOME, close Bank and start it again. If it comes again, [report the numbers](../README.md#problems-and-feature-requests). |

**The error screen.** An error ends this run of Bank while stored data is kept. Bank first shows the original's sentence.

> The server did not receive the data.

Then it shows:

```text
Press (HOME) to return to the HOME Menu
v0.7.0
XXXXXXXX YYYYYYYY
```

The second line is the patch's version, with "-test" or "-download" behind it for those packages, or "draft-" and a commit for a build that is no release. The two numbers say which step failed and why ([errors.md](errors.md#every-active-first-error-number)). Poké Transporter shows the same screen with the same numbers.

**Starting over with an empty Bank.** This deletes every Pokémon in the offline Bank. Delete Bank's Extra Data in System Settings, or power off, put the SD card in a PC, delete the folder `Nintendo 3DS/<ID0>/<ID1>/extdata/00000000/00000C9B` (its contents are encrypted), and start Bank again.

## Limits

- **First start.** Two original steps run in the foreground and can cause short pauses: creating the extdata archive and formatting the default box names. If the power fails while the Bank is being created, the next start creates it again, welcome included.
- **Power cuts.** A cut during Save and Quit only reaches the file being written. Damage to the SD card itself is beyond what Bank can guard against, as for every save on the console.
- **Clock.** A console date set ahead is paid in Miles as if the time had passed.
