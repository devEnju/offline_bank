# Pokémon Bank Offline Patch

A Luma3DS patch for **Pokémon Bank 1.5** (title `00040000000C9B00`, TMD version 6272) that keeps your Pokémon on the SD card instead of on Nintendo's servers.

## What changes

| Original Bank | Patched |
| --- | --- |
| Connects, checks the account and pass, then shows the main menu. | No connection. Start screen and after that **game selection**. There is no main menu; Back on game selection opens the [transfer screen](#the-transfer-screen), a box manager. |
| Downloads the Bank from the server and uploads it on save. | Reads and writes local files on a background thread while the original loading screen keeps animating. |
| The server resolves interrupted saves. | A local journal does. An interruption leaves either the old or the new state, never a mix ([below](#if-save-and-quit-is-interrupted)). |
| First start: asks for a language, which cannot be changed later. | Never asks. Bank is always in the console's language ([below](#first-start)). |
| First start: shows the "Precaution for Use" notice (with the note for users under 18) and needs it accepted. | Not shown. It counts as accepted, and that is saved as the original saves it. |
| HOME and sleep are refused for the whole connected session. | Refused only while files are read or written: from START until game selection, from choosing a game until its boxes open, and during Save and Quit. They work on every screen that waits for you, error messages included, except the unlock screen's number keyboard, as in the original. |
| Moving Pokémon stamps a server-checked time. | The same stamp from the console clock. |
| Poké Miles accrue from stored Pokémon; rewards also involve server gifts. | Miles accrue locally ([below](#poké-miles)). The original welcome gift and redemption are kept. Server gifts and distributions are skipped. |

## First start

The original asks two things when it has no save data of its own yet (Bank's small save, not the extdata with your boxes). Both are gone:

- **Language.** Bank is always in the console's language, as Poké Transporter is. No language is stored, and one the original stored is cleared. A Japanese console gets kana; kanji could only be chosen on the removed screen.
- **Precaution for Use.** Not shown. It counts as accepted and is saved as the original saves it.

**If you remove the patch later**, the original asks for the language again and does not show the notice.

**Brigette's welcome** plays once, when the Bank is created, as in the original. If you close Bank while she talks, it plays again at the next start.

## Poké Miles

**What you earn.** One Pokémon in the 100 boxes earns one Mile in 30 days: every full hour adds `Pokémon ÷ 720` Miles, and the part below a whole Mile is carried. The rate is the original's, which dropped that part at every visit.

- The game's boxes and the transport box do not count.
- 30 Pokémon earn 1 Mile a day, 15 earn 1 Mile every two days, 3,000 earn 100 a day.
- An hour counts when the clock passes a full hour: saved at 10:55 and opened at 11:05 is one hour.
- The hours up to a save are paid for the Pokémon that were in the boxes when the game was loaded, or when the transfer screen's boxes appeared. What you deposit, withdraw or move counts from that save on.
- There is one balance for the whole Bank. Opening one game and then another does not earn twice.
- The balance stops at the original limit of 65,535.

**The welcome gift.** Once per Bank, the original's welcome gives 100 Miles: its three messages after the game is loaded, then the choices as for any redemption.

- It comes after the first game selection of a Bank that has not had it. A Bank downloaded from the official server had it there.
- It becomes permanent with the next Save and Quit, together with the Miles. If you leave without saving, it comes again at the next opening, again with 100, never twice.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, comes first with the original's notice. If you go on to the boxes, the welcome waits for the next opening of a game that can take a gift.

**When it is saved.** Miles are added when the game has loaded (or the transfer screen's boxes appear) and again at the save, and become permanent with your boxes. Leaving without saving changes nothing: the same hours count next time. A new Bank earns from its first save on.

**The clock.** The date of the last save (Save and Quit, or the transfer screen's) and the day of the last redemption only move forward.

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
| 1 to 9 | no | The original message, without choices, at the first opening of a day. After a save on that day nothing more, until the balance reaches 10 or the day changes. |
| 10 or more | no | The original message and the choices, at every opening until you redeem. |

- "The first opening of a day" means that the last save (Save and Quit, or the transfer screen's) was on an earlier day. If you leave without saving, the message comes again at the next opening.
- An offer you decline comes back at the next opening, with the Miles of the hours since.
- The notice about a gift still waiting in a Gen 6 game, or a full gift list in a Gen 7 game, comes only with an offer of 10 or more.
- With the console date set back before a stored date, that date counts as today.

The patch adds one safety check: after a claim, the balance and the gift in the game must match what was shown. If not, the balance is restored and the session ends with error `00000013` without saving.

## Pokédex and adventure records

As in the original, with one addition. The Bank keeps one slot per game version (X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, Ultra Moon); the National Pokédex you see combines all eight.

- A game is taken over the first time you **open the records screen** with it. It replaces its version's slot; the others stay.
- After that, **every Save and Quit with that game** updates its slot, for the same trainer.
- A slot of another trainer is replaced only if you choose that game on the records screen and accept the original's confirmation.
- Nothing is stored until the next Save and Quit.

## Where the Bank is stored

Eight files in four pairs, in Bank's own SD extdata archive `0x00000C9B`. The first three pairs hold what the original kept on the server, cut into three pieces and otherwise unchanged.

| Files | Size of each | Each holds |
| --- | --- | --- |
| `/bank.bin`, `/bank.alt.bin` | 729,800 bytes | The 100 boxes: the Pokémon, each box's name, and what Bank keeps per slot. |
| `/main.bin`, `/main.alt.bin` | 30,466 bytes | Everything else: the group names, the Pokédex of all eight game versions with their trainer and adventure records, and the Miles. |
| `/mover.bin`, `/mover.alt.bin` | 6,990 bytes | The transport box: up to 30 Pokémon from Poké Transporter. |
| `/journal.bin`, `/journal.alt.bin` | 140 bytes | The record that ties the others together: which file of each pair is current, with its checksum, and whether a save is in progress. |

**Why pairs.** A save writes the file of each pair that is not in use and switches over only when everything is written, so a power cut leaves the files in use as they were ([below](#if-save-and-quit-is-interrupted)). `.alt` is not an older save: the two take turns.

- Files are written only at Save and Quit, at the transfer screen's save, when they are first created, and when a start finishes an interrupted save. Loading and moving Pokémon never write.
- If a file Bank needs cannot be read, Bank shows its error screen and changes nothing. It never replaces what it cannot read with something empty.
- The transport box is filled by the [Transporter patch](transporter.md). What you do not move out of it stays there.
- The files are inside encrypted extdata, not loose on the SD card. An edited save dropped onto the card does not work.

The formats are described in [internals.md](internals.md#storage).

## Install

Install both patches as the [README](../README.md#install) says. Bank's two files go here, both from the same release or build:

```text
SD:/luma/titles/00040000000C9B00/code.ips
SD:/luma/titles/00040000000C9B00/exheader.bin
```

The first start creates an empty Bank.

## The transfer screen

Press **Back** on game selection. The original's question comes, without its words about Pokémon HOME: "Do you want to move the Pokémon in Pokémon Bank?". "Begin the move" shows "Communicating with the Pokémon Bank server..." for a moment, then the box screen of the HOME transfer; "Cancel" goes back to the start screen. Nothing is sent anywhere.

The screen shows your 100 boxes and, behind the last one, the **transport box** ("Transport Box", on a blue label) and three **offline boxes**: files on the SD card, named as default boxes are ("Box 1" to "Box 3" in English). It opens also for a Bank without Pokémon. **START** changes the hand while nothing is held:

| Hand | A on a box, then A on another | |
| --- | --- | --- |
| Red | Checks or unchecks it. | Checked boxes are backed up at the save. |
| Blue | Swaps the two boxes, with their Pokémon, names, checks and places in their groups. | Not the transport box. **L** moves the box under the cursor one place up in its group (the first to the end); held, it repeats. |
| Green | Moves its Pokémon into the other box, each into the same slot. | The other box must hold no Pokémon. Not a checked box. Names stay. |

- B puts a held box back. Where it cannot go, A plays the refusal sound; onto the transport box it goes back, and "Cannot be transported there." shows.
- While something is held, only A, B and the cursor work.
- Nothing goes into the transport box. Its Pokémon leave only with the green hand.
- A box keeps a name it brings along. A default name ("Bank 12", "Box 2") stays with its place, and a box of Bank left without a name gets its default.
- The top line counts the checked boxes, out of 104. With the blue or green hand it names the shown box's group and place ("Group: <name>", "12/13").

**When you finish**, press Y (DONE). B, while nothing is held, asks "Would you like to quit without saving?": Yes goes to the start screen, No back to the boxes.

| You have | Y asks | Yes | No |
| --- | --- | --- | --- |
| Checked a box | "Move these Pokémon?" | Writes everything | Back to the boxes |
| Changed boxes, none checked | "Would you like to save and quit?" | Writes everything | Back to the boxes |
| Changed nothing | "Quit moving Pokémon?" | Start screen | Back to the boxes |

A box counts as changed if its Pokémon, name, group or position differ from when the screen opened; boxes swapped and back are unchanged. Putting Pokémon down with the green hand always counts. After a write comes "Success! Changes saved." and the start screen.

### On the SD card

```text
/3ds/Pokémon Bank/
    mover1.box .. mover3.box         the offline boxes, written at the first save
    20261109-140307/                 one folder per save with a box checked
        bank21.box                   box 21, copied: it stays in Bank (box 100 is bank00)
        mover0.box                   the transport box, copied
        mover2.box                   offline box 2, moved: its cell is empty afterwards
    20261116-091500-backup/          boxes START kept from an interrupted save (below)
    POKEMON_W_IRAD01_00.sav.bak      Poké Transporter's copy of a game's save (transporter.md)
```

- A checked box of Bank is copied; a checked offline box is moved. So a box of Bank swapped onto a cell and checked leaves Bank and is kept in the folder.
- **To bring a box back**, copy its file to `/3ds/Pokémon Bank/` as `mover1.box`, `mover2.box` or `mover3.box`, over a cell without Pokémon, and move it in. It takes the group and place of the box whose spot it takes.
- A file that is not a whole box file stops the screen with error `00000015`. A cut or a full card while writing can leave one; delete it.

**An interrupted save.** The next START finishes or undoes it. It copies the offline boxes with Pokémon that the save changed into `YYYYMMDD-HHMMSS-backup/`, as they were (`mover2.old.box`) and as they became (`mover2.new.box`), unless nothing had changed yet. One of them may be the only copy: keep the folder until you have found its Pokémon.

- Cut at the very end of a save: only `.old` may be there, as the new box is in its cell.
- "The SD card is full.": the next START does it, after you freed space.
- A folder named with 16 letters and digits: START was cut. Leave it; the next START renames it.
- A file that can no longer be read is deleted. Where Pokémon may be gone, the error screen shows `00000015 00000000` once.
- No folder is written over: if its name is taken, the next free second names it.

## Backing up the Bank

Bank needs no other tool to get through a power cut or a pulled Game Card ([below](#if-save-and-quit-is-interrupted)); putting back an older copy of Bank's data is what can get in its way.

To keep a copy of Pokémon, check their boxes on the [transfer screen](#the-transfer-screen) and save: they go into `/3ds/Pokémon Bank/YYYYMMDD-HHMMSS/` ([On the SD card](#on-the-sd-card)). The file of a checked offline box is then the only copy of its Pokémon, so keep it. To keep copies off the SD card, copy `/3ds/Pokémon Bank/` to a PC now and then.

The offline boxes are files of the SD card, not part of Bank's extdata: a Pokémon in one is in the Bank only once moved into a box.

### Backups and restoring

Bank cannot notice what is changed from outside, so these five rules are yours to follow:

1. Back up and restore Bank and `mover1.box`, `mover2.box` and `mover3.box` together.
2. After Transporter asks its question, open Bank and press START before you restore anything.
3. Only delete a `-backup` or session folder once you have checked that you no longer need its boxes.
4. Copy a box file only into an empty offline box, and only while Bank is closed.
5. Don't bring a box back while its Pokémon are still in Bank.

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

**Your boxes are backed up first.** Before showing this message or the one that waits for your game, Bank keeps every box the session changed, as it was before and after, inside its own data. Once you answer in Transporter, the next START writes the side not chosen to the SD card: after Yes the `.new.box` files, after No the `.old.box` ones. If Bank finds the game with its save from before or after the session instead, that decides it and nothing is written.

```text
/3ds/Pokémon Bank/YYYYMMDD-HHMMSS-backup/
    bank30.new.box                   box 30 after the session
    mover0.new.box                   the transport box, if it changed
```

They can be the only copies: keep the folder until you have found their Pokémon. To bring one back, see [On the SD card](#on-the-sd-card). If START says "The SD card is full.", nothing is lost: free some space and press START again.

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

The chosen game's save failed its checksum check, so Bank does not load it and writes nothing; every other game works as usual. Start the game itself to see what it makes of its save.

The patch cannot cause this: the game's save is written by the original's own code. The check is the patch's: the original loaded a damaged save and wrote it back with new checksums, which hid the damage.

## Troubleshooting

| What you see | Do this |
| --- | --- |
| Bank does not start, or stops while loading. | Use `code.ips` and `exheader.bin` from the same release, with Bank 1.5. |
| After START: "Could not find saved data for a game that is compatible with Pokémon Bank.", "You must also advance in the game to the point where you have received a Pokédex." | No game's save could be read. Insert a cartridge or make an installed game's save available, then press START again. Nothing was opened or created. Whether a game has its Pokédex is checked when you choose it, as in the original. |
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

The second line is the patch's version, with "-test" or "-download" behind it for those packages, or "draft-" and a commit for a build that is no release. The two numbers say which step failed and why: give them when you [report a problem](../README.md#problems-and-feature-requests). Poké Transporter shows the same screen with the same numbers. For developers, [errors.md](errors.md#every-active-first-error-number) lists what each number means.

**Starting over with an empty Bank.** This deletes every Pokémon in the offline Bank. Delete Bank's Extra Data in System Settings, or power off, put the SD card in a PC, delete the folder `Nintendo 3DS/<ID0>/<ID1>/extdata/00000000/00000C9B` (its contents are encrypted), and start Bank again.

## Limits

- **First start.** Two original steps run in the foreground and can cause short pauses: creating the extdata archive and formatting the default box names. If the power fails while the Bank is being created, the next start creates it again, welcome included.
- **Power cuts.** A cut during Save and Quit only reaches the file being written. Damage to the SD card itself is beyond what Bank can guard against, as for every save on the console.
- **Clock.** A console date set ahead is paid in Miles as if the time had passed.

## For developers

How the patch is built is in [building.md](building.md), how it works inside in [internals.md](internals.md#bank), and how it is checked on a console in [testing.md](testing.md#bank-feature-by-feature).
