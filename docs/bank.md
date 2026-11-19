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

The original asks two things when it has no save data of its own yet. Both are gone:

- **Language.** Bank no longer asks and no longer follows an earlier choice: it is in the console's language at every start, as Poké Transporter is. No language is ever stored, and one stored earlier by the original is cleared. A Japanese console therefore gets kana and kanji could only be chosen on the removed screen.
- **Precaution for Use.** The notice is not shown. Bank goes on as if you had accepted it and creates its save data in the same step, once.

**If you remove the patch later**, the original finds save data in which the notice is accepted and no language is chosen: it asks for the language and does not show the notice again. That also holds for save data in which the original had stored a language: the patched Bank clears it at its first start, so the original asks again.

This save data is Bank's own small save, not the extra data that holds your boxes.

**Brigette's welcome** is kept. The original plays it when no Bank exists yet, right before it creates one, and stores no mark for it. The patch does the same: it plays once, when the offline Bank is created, and never for a Bank that already exists. If you close Bank while she talks, no Bank has been created and the welcome plays at the next start.

## Poké Miles

**What you earn.** One Pokémon in the Bank earns one Mile in 30 days, counted by the hour: every full hour adds `Pokémon in the boxes ÷ 720` Miles. The part below a whole Mile is carried, so nothing is lost to rounding, however often you open Bank. This is the original rate, but the original dropped the remainder at every visit.

- Only the 100 regular boxes count. The game's boxes and the transport box do not.
- 30 Pokémon earn 1 Mile a day, 15 earn 1 Mile every two days, 3,000 earn 100 a day.
- An hour counts when the clock passes a full hour: saved at 10:55 and opened at 11:05 is one hour.
- The hours up to a save are paid for the Pokémon that were in the boxes when the game was loaded, or when the transfer screen's boxes appeared. What you deposit, withdraw or move counts from that save on.
- There is one balance for the whole Bank. Opening one game and then another does not earn twice.
- The balance stops at the original limit of 65,535.

**The welcome gift.** Once per Bank, the original's welcome gives 100 Miles: its three messages after the game is loaded, then the choices as for any redemption.

- It comes after the first game selection of a Bank that has not had it. A Bank downloaded from the official server had it there.
- It becomes permanent with the next Save and Quit, together with the Miles. If you leave without saving, it comes again at the next opening, again with 100, never twice.
- A Gen 6 gift still waiting in the game, or a full Gen 7 gift list, comes first with the original's notice. If you go on to the boxes, the welcome waits for the next opening of a game that can take a gift.

**When it is saved.** Miles are added when the selected game has loaded and again at Save and Quit, and become permanent there, in the same step as your boxes. The transfer screen does the same: Miles are added when its boxes appear, counted by the Pokémon in the Bank before any move, and again when it saves. So moving Pokémon into the Bank earns from that save on, never for the hours before it. If you exit without saving, nothing changes and the same hours are credited next time. A new Bank starts earning with its first Save and Quit even if the 100 extra Miles had not been redeemed yet.

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

- Files are written only at Save and Quit, at the transfer screen's save, when they are first created, and when a start finishes an interrupted save. Loading and moving Pokémon never write.
- If a file Bank needs cannot be read, Bank shows its error screen and changes nothing. It never replaces what it cannot read with something empty.
- The transport box is filled by the [Transporter patch](transporter.md). You take Pokémon out by moving them into boxes and what you leave there stays.
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

Press **Back** on game selection. The original's question for Pokémon HOME comes, without its words about Pokémon HOME: "Do you want to move the Pokémon in Pokémon Bank?", with the buttons "Begin the move" and "Cancel". Begin the move shows "Communicating with the Pokémon Bank server..." for a moment, then the box screen of the HOME transfer; Cancel goes back to the start screen. Nothing is sent anywhere: the screen manages your boxes on the SD card.

It shows your 100 boxes, and behind the last one four more: the **transport box** and three **offline boxes**, which are files on the SD card. The offline boxes are named as your boxes are by default, with their number: "Box 1" to "Box 3" in English ("Box: 1" in German, "Caja 1" in Spanish); the transport box reads "Transport Box", on a blue label as on the box screen. **START** changes the hand's colour, with the same keys and sound as on the box screen, while nothing is held:

| Hand | A on a box, then A on another | |
| --- | --- | --- |
| Red | Checks or unchecks it | The checked boxes are backed up when you finish. |
| Blue | Lifts it, then puts it there: the two boxes change places, with everything in them and their names | Move a box out of Bank into an offline box, or one back. |
| Green | Lifts its Pokémon, then puts them into the other box, each into the same place | The other box must hold no Pokémon; names stay. |

B puts a lifted box back, and the cursor goes back to its place with it. On a place it cannot go, A only plays the refusal sound and the box stays in the hand; if it would put Pokémon into the transport box (a box onto the transport box), it goes back to its place, the cursor with it, and then the refusal sound plays and "Cannot be transported there." shows until A or B. While a box is held, only A, B and moving the cursor do anything, as on the box screen: START, X, Y, L, R and the buttons on the touch screen do nothing. The green hand does not lift a checked box: A on it does nothing; uncheck it with the red hand first. The blue hand lifts it, and its check goes with it. The blue hand does not lift the transport box: move its Pokémon out with the green hand. A box moved with the blue hand takes its name and its category along; swapped with a box of the same category, the two change places in that category's order; moved out to an offline box, it leaves its category's place to the box of Bank that takes its spot; where it came from, a box of Bank that gets no name back gets its default one ("Bank 37"). A default name stays with its place: box 12's "Bank 12" moved to offline box 2 does not go along, and offline box 2 keeps "Box 2"; in the same way an offline box's "Box 1" stays in its cell. A box that brings another name, its own or a default from somewhere else, keeps it. The line at the top shows the title and the count of checked boxes, which counts the four cells too, out of 104. With the blue or green hand it shows the group of the box under the cursor as the box screen names it, "Group: <name>", and the box's place in it, "12/13"; for the transport box "1/1" and for an offline box its number of the three, each under the box's own name. While the blue hand holds a box, it shows the group and place the box would get where the cursor is. **L** with the blue hand moves the box under the cursor one place before in its group, with the box screen's sound for L; the first box goes to the end, and the others move up one. Held, it goes on at the box screen's speed until you let go; between its moves the other keys and buttons work as usual. The screen opens also for a Bank without Pokémon. The upper screen shows the box under the cursor, the four cells included; while the blue hand holds a box, that box until it is put down; while the green hand holds a box's Pokémon, they lie lifted over the box under the cursor, and their own box shows empty. Nothing can go into the transport box; its Pokémon leave it only with the green hand, into a box without Pokémon.

**When you finish**, press Y (DONE). B, while nothing is held, asks "Would you like to quit without saving?", whatever you changed: Yes goes to the start screen without saving anything, No back to the boxes.

| You have | Y asks | Yes | No |
| --- | --- | --- | --- |
| Checked a box | "Move these Pokémon?" | Writes everything | Back to the boxes |
| Changed boxes, none checked | "Would you like to save and quit?" | Writes everything | Back to the boxes |
| Changed nothing | "Quit moving Pokémon?" | Start screen | Back to the boxes |

Nothing counts as changed when every box shows what it showed when the screen opened: its Pokémon in their slots, its name, its category and its position. So boxes swapped with the blue hand and back, and empty boxes that ended up at each other's places, count as unchanged. A move with the green hand always counts, also when its Pokémon are moved back. After a write comes "Success! Changes saved." and the start screen.

**On the SD card**:

```text
/3ds/Pokémon Bank/
    mover1.bin .. mover3.bin         the offline boxes, written at the first save, with Pokémon or without
    20261109-140307/                 one folder per save that had a box checked; a save cut short or
                                     stopped by a full card can leave one with copies of boxes still in Bank
        bank21.bin                   box 21, copied: it stays in Bank (bank01 to bank99; box 100 is bank00)
        mover0.bin                   the transport box, copied
        mover2.bin                   offline box 2, copied; its cell holds an empty box afterwards
    20261116-091500-backup/          boxes START kept from an interrupted write (below)
    POKEMON_W_IRAD01_00.sav.bak      Poké Transporter's copy of a game's save on the SD card (transporter.md)
```

Each of the three cells always holds a box, as each of the 100 boxes does, with Pokémon or without. A checked offline box is **moved** into the session's folder: copied there, and its cell holds an empty box afterwards; a checked box of Bank is only copied. A check goes with its box, so a checked box of Bank moved onto a cell with the blue hand is moved out at the save as well: a way to free a box of Bank and keep its Pokémon in the session's folder. To bring a box back, copy its file (a file of a session's folder, also a copy of a box of Bank or of the transport box) to `/3ds/Pokémon Bank/` as `mover1.bin`, `mover2.bin` or `mover3.bin`, over the file of a cell without Pokémon, open the transfer screen and move it into Bank with the blue or the green hand. A box file keeps a box's Pokémon and its name, not its group or its place in the group: a box brought back takes those of the box in Bank whose spot it takes. A file that is not a whole box file stops the transfer screen with error `00000015` and is left as it is: a power cut while a file was written can leave one, in a session's folder or a `-backup` folder; delete it from the cell, it holds nothing usable.

Every write is safe against a power cut: the next START finishes it or undoes it, and no Pokémon is lost. While a save runs, the old box of each cell it writes is kept as `mover1.bak`.. in a folder `interrupted-XXXXXXXX-XXXXXXXX/`. A folder whose name starts with `interrupted-` is not resolved yet: leave it and its `.bak`, `.old` and `.new` files alone until the next START, which puts the old boxes back if the save did not go through. After START no `interrupted-` folder is left but the backup of a save that waits ([below](#a-save-left-to-transporter)). A finished folder is renamed to `YYYYMMDD-HHMMSS-backup/` and then emptied of what Bank holds; a folder of that name is resolved, and its files are yours to use: copy one to `/3ds/Pokémon Bank/` as the file of an empty cell, renamed to `mover1.bin`, `mover2.bin` or `mover3.bin`, to bring it back like any box file. Keep a `-backup` folder until you have found the Pokémon in it where you need them; it may hold their only copy. **Do not replace, restore or delete Bank's data (its extdata) while a folder `interrupted-…` is on the SD card**: START settles that folder by Bank's own record, which another Bank's data does not have. For the same reason, start Bank once before you copy a box file into a cell while such a folder is there: START may put the cell's old box back over it. A dated folder is always a new one: when a folder of that second is there already, as after the console's clock was set back, the next free second names it, so no box a folder keeps is ever written over. It is a backup of your boxes that does not need Checkpoint, and fully optional.

## Backing up the Bank

Bank keeps your boxes safe by itself: a power cut or a pulled Game Card during a save leaves them as before the save or as after it, and the next START finishes or undoes it ([below](#if-save-and-quit-is-interrupted)). No other tool is needed for that, and putting back an older copy of Bank's data in between is what can get in its way.

To keep a copy of Pokémon, check their boxes on the [transfer screen](#the-transfer-screen) and save: each checked box is copied into the session's folder, `/3ds/Pokémon Bank/YYYYMMDD-HHMMSS/`. To bring one back, copy its file to `/3ds/Pokémon Bank/` as the file of a cell without Pokémon, `mover1.bin`, `mover2.bin` or `mover3.bin`, and move it into a box on the transfer screen ([The transfer screen](#the-transfer-screen)). The copies lie on the same SD card as Bank: to keep them apart from it, copy the folder `/3ds/Pokémon Bank/` to a PC now and then.

The three offline boxes are no part of the Bank: they are files of the SD card, a way into and out of the Bank for Pokémon from elsewhere. Bank's own data, its extdata, holds the 100 boxes and the transport box; a Pokémon in an offline box is part of it only once moved into a box of the Bank.

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

**Your boxes are backed up first.** Before Bank shows this message, or the one that waits for your game, it copies every box whose Pokémon the interrupted session changed to your SD card, as it was before the session and as it was after:

```text
/3ds/Pokémon Bank/interrupted-XXXXXXXX-XXXXXXXX/
    bank30.old                       box 30 before the session
    bank30.new                       box 30 after it
    mover0.old  mover0.new           the transport box, if it changed
```

Once the session is settled, by your answer in Transporter or by Bank finding the game with the save before or after the session, the next START renames the folder to one of the date, `YYYYMMDD-HHMMSS-backup/`, and deletes there the side Bank holds, so the other side is left in it: after Yes in Transporter the `.new` boxes, after No the `.old` ones. They are the only copies of those boxes: keep the folder until you have found the Pokémon in it where you need them. To bring one back, copy it to `/3ds/Pokémon Bank/` as the file of a cell without Pokémon, `mover1.bin`, `mover2.bin` or `mover3.bin`, and move it in on the [transfer screen](#the-transfer-screen). Without room on the SD card for the backup, START shows "The SD card is full." and changes nothing: free some space and press START again.

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
