# Testing on a console

The PC tests ([building.md](building.md#tests)) cover the storage format and every rule of a save, including a power cut at each write on a simulated card. What they cannot show is the console itself: its file system, the original programs, and a real power cut. This page lists the checks that need a console, as steps and what must be seen.

**This page is the baseline.** The checks listed here are what was tested overall, on one Nintendo 2DS, and the "must show" and "expected" columns say how the patches are supposed to behave. A result that differs from them is a fault, on any console.

No run is logged here one by one: the page does not say which check was made with which package or on which date, and no other model has been tested.

## Before you start

1. **Back up** Bank's extdata and the saves of the games you will use, with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint) ([how](bank.md#backing-up-the-bank)). Some runs leave the Bank and a game in a state that only the backup undoes.
2. **Use Pokémon you can lose** or have twice. A few runs lose or duplicate one on purpose.
3. **Switching packages.** Each package is a folder with `code.ips` and `exheader.bin`; a folder always holds the latest build of its kind. Copy both to `SD:/luma/titles/00040000000C9B00/` for Bank, or `SD:/luma/titles/00040000000C9C00/` for Transporter, replacing the two files there.
4. **Finish what a package began.** A Save and Quit that the test package left in progress must be settled with that package (the plain "Use Pokémon Bank"), or the backup restored, before another package is tried.

| Package | Built by | Folder |
| --- | --- | --- |
| Bank | `Build-BankPatch.ps1` | `build/binary/bank/` |
| Transporter | `Build-TransporterPatch.ps1` | `build/binary/transporter/` |
| Test package | `Build-BankTestPatches.ps1` | `build/test/` |
| Migration, when a release has one | `Build-BankMigrationPatch.ps1` | `build/migration/` |
| Download, while the official service runs | `Build-BankDownloadPatch.ps1` | `build/download/` |

Every package says what it is on its start screen: "Ver. 1.5-offline" for Bank and for Transporter, "Ver. 1.5-migration", "Ver. 1.5-download", "Ver. 1.5-test". What the test package does is in [building.md](building.md#test-builds-for-an-interrupted-save).

## Which tests after which change

| What changed | Run |
| --- | --- |
| Anything, before a release | [Normal use](#normal-use) |
| How or where files are stored, or the file access | [Normal use](#normal-use), [A power cut during a save](#a-power-cut-during-a-save) |
| The rules for an interrupted save | [An interrupted Save and Quit](#an-interrupted-save-and-quit) |
| Bank's screens and navigation, Poké Miles, or the Pokédex | The matching part of [Bank, feature by feature](#bank-feature-by-feature) |
| The Transporter patch, or the transport box files | Run 3 of [Normal use](#normal-use), and the matching part of [Transporter, feature by feature](#transporter-feature-by-feature); the HOME rows of [Transfers](#transfers) in any case |
| How Transporter hands a box over to Bank | [A power cut during a transfer](#a-power-cut-during-a-transfer) as well |
| How or where files are stored, with a migration for it | [Updating from the version before](#updating-from-the-version-before) as well |
| A migration patch | [Updating from the version before](#updating-from-the-version-before) |
| The download patch | [The download from the official server](#the-download-from-the-official-server) |

## Normal use

With the normal Bank and Transporter packages.

| Run | Do | Must show |
| --- | --- | --- |
| 1 | Start Bank on a console whose Bank extdata is empty | The welcome, then an empty Bank |
| 2 | Pick a game, move Pokémon both ways, Save and Quit. Open the same game again in the same session, move again, Save and Quit a second time. Close Bank and start it again. Once with an installed game and once with a cartridge, if you have both. | Everything where it was put, in Bank and in the game; nothing twice; Poké Miles as expected |
| 3 | Transfer from a Generation 5 game with Transporter. Back on its title screen, press START again. Then open Bank, take the Pokémon out of the transport box, Save and Quit. Start Transporter again and press START. | The transfer completes. The second START answers with the "Pokémon remains in the Transport Box" message and returns to the title screen, without a game list. In Bank the Pokémon are there once. Afterwards START leads to the game list again. |
| 4 | Start a game, press HOME, and start Bank from the HOME Menu without closing the game first. Press START, pick a game, look at the boxes, and leave; once with Save and Quit and once without saving. Back on the start screen, press HOME. | The HOME Menu appears, both times |

## Bank, feature by feature

With the normal Bank package. What each feature is meant to do is in [bank.md](bank.md).

### Start and navigation

| Do this | Expected |
| --- | --- |
| First start. | Original opening message, moving spinner, loading sound, then game selection. The main menu never appears. Short pauses are possible (see [Limits](bank.md#limits)). |
| Back on game selection. | Start screen. |
| Select a game, Back on its confirmation. | Game selection again. |
| Select a game, cancel at the reward prompt; again, leave the boxes without saving. | Start screen. Nothing saved. |
| Move Pokémon both ways, rename a box, Save and Quit, restart. | After the loading screen the success sound and "Success! Changes saved.", as on the unpatched Bank; A leads to the start screen. Everything persisted in Bank and in the game. No missing or duplicate Pokémon. |
| Press HOME and close the lid right after leaving the start screen, while the Bank loads after selecting a game, and during Save and Quit. | Nothing happens while a loading screen is shown. On the start screen, game selection, in the boxes, and on an error screen both work as usual. |
| No usable game present. | The original "no game" notice once, then the start screen. |
| The same error with the console set to Japanese, Korean or Chinese. | The second and third line read the same as in English: the letters, digits, `.` and `-` are all there. |
| An error (the test package, [below](#an-interrupted-save-and-quit)): "The server did not receive the data." with the arrow and without numbers. Press A. | The line "Press HOME to return to the HOME Menu", under it the version with "-test" behind it ("dev-test" for a package built without a version), and the two numbers on the third line. A, B and START do nothing. HOME opens the HOME Menu, the lid puts the console to sleep, and Bank can be closed from the HOME Menu. |
| The same with Bank started from a running game (run 4): press HOME on the error's first screen, before A. | The HOME Menu appears. |
| A console on which Bank's extdata cannot be created for lack of space on the SD card: press HOME on the original's message about it. | The HOME Menu appears. |

### Poké Miles

A new Bank starts counting with its first Save and Quit. `N` is the number of Pokémon in the boxes when the game was loaded; 720 Pokémon-hours are one Mile.

| Do this | Expected |
| --- | --- |
| Save and Quit, set the console clock `H` full hours ahead (or wait), open and select a game. | `N × H ÷ 720` Miles more than before, rounded down. |
| With 720 or more Pokémon: Save and Quit at some minute 50, open ten minutes later, after the full hour. | The Miles of one hour. |
| 15 Pokémon, open and Save and Quit once a day. | 1 Mile every second day. |
| Open, deposit Pokémon, Save and Quit some hours later. Open the next day. | The hours before the save are paid for the old number, those after it for the new one. |
| Open a different game in the same hour. | Same balance. No second credit. |
| Fewer than 10 Miles, not redeemed today, the last Save and Quit on an earlier day. | Original message, no redemption choices. Again at each opening until a Save and Quit on that day. |
| Fewer than 10 Miles, reopened the same day after Save and Quit. | No reward dialog; the boxes open directly. |
| The same balance once it has reached 10 on that day (many Pokémon, some hours later). | The message and the redemption choices. |
| 10 or more Miles, decline, Save and Quit, reopen the same day. | The choices again, with the Miles of the hours since. |
| Redeem Miles or Battle Points, Save and Quit, reopen the same day, also with 10 or more by then. | No reward dialog. On the next day the dialog comes again. |
| A Gen 6 game with a gift still waiting in it (or a Gen 7 game with a full gift list), with fewer than 10 Miles or after a redemption today. | No notice about it. |
| The same with 10 or more Miles and no redemption today. | The original's notice about it. |
| 10 or more, Gen 6: Miles one day, Battle Points another. | Miles: balance 0. BP: `Miles ÷ 10` to the game, remainder kept. The gift waits in the game after Save and Quit. |
| 10 or more, Gen 7. | Miles refused by the original message; BP arrives in the gift list. |
| Redeem, then leave without saving. | Balance as before the claim; no gift in the game; the dialog again at the next opening. |
| A Bank last used with v0.3.1, with Miles in it: convert it with the migration, install this version, open, pick a game. | The balance it had, plus the Miles of the hours since its last Save and Quit. After a Save and Quit and a restart the same balance, not twice. |
| Set the date back a day, open, Save and Quit. | Save works. No Miles, no dialog. After returning to the real date, none earned twice. |

### Pokédex

| Do this | Expected |
| --- | --- |
| Open Y, open the records screen, Save and Quit. Do the same with Ultra Sun. | A species known only to Y and one known only to Ultra Sun both show in the National Pokédex. |
| Open a game whose Pokédex the Bank does not have, do not open the records screen, Save and Quit. | That game's Pokédex and records are not taken over. |
| A game the Bank has taken over before: see a new species in the game, open Bank with it, do not open the records screen, Save and Quit, start Bank again. | The new species shows in the National Pokédex, and the game's records are the current ones. |
| The same, but leave without saving. | Not taken over. |
| Another save of the same version with another trainer, without opening the records screen: Save and Quit. | That version's entries and records stay the first trainer's. |
| Open such a game, open the records screen, leave without saving. | Not taken over. |
| Same version, different trainer (for example two Omega Ruby saves): open the records screen and choose that game's entry. | The original replacement prompt. Accept: that version's entries and records become the new save's. Decline: unchanged. Either way stored only by Save and Quit. |

## Transporter, feature by feature

With the normal Bank and Transporter packages. What each feature is meant to do is in [transporter.md](transporter.md).

### Transfers

| Do this | Expected |
| --- | --- |
| Gen 5 cartridge, Box 1 with gaps, Bank's transport box empty: transport. | After START the game list appears without a message. The spinner keeps animating while the box is read and while the Pokémon are written. The original "transported" message appears. The Pokémon are gone from the source game. Bank shows them in the transport box, in the same positions. |
| In Bank, move them into a box, Save and Quit, restart Bank. | They are in the box. A Gen 6 or Gen 7 game can withdraw them. |
| Gen 1 or Gen 2 game. | The Pokémon arrive as Gen 7 Pokémon and cannot be withdrawn into X, Y, Omega Ruby, or Alpha Sapphire. |
| A Box 1 that was never used, or Pokémon with never-used slots between them. | No message about Pokémon that cannot be transported. An empty box gives only the "nothing to transport" message. |
| A Pokémon the original's own checks refuse (for example an Egg). | Three original dialogs in a row: one that a Pokémon cannot be sent, one with the reason, one that it was removed from the Transport Box. The others are offered. |
| An egg in Box 1 beside other Pokémon. | The original dialog about Pokémon that cannot be transported appears for the egg; the others are offered. |
| Any transfer. | The notice about nicknames and OT names does not appear; the notices about not being able to return Pokémon and about held items do. |
| Press HOME and close the lid right after START, while the game list is being built, while the chosen game is read, and during the transfer. | Nothing happens. |
| Press HOME and close the lid on the title screen, on the game list, and on the notices and the "move this Box?" question. | The HOME Menu opens; the lid puts the console to sleep. |
| Press HOME and close the lid on each message that waits for A: "Could not find a game" (no game in the listed language), the closing message of a transfer, a damaged save's message, "No compatible Pokémon Game Card detected" (take the cartridge out while it is read, or after confirming the question), and the two messages of a refusal. | The HOME Menu opens; the lid puts the console to sleep. |
| Press HOME together with A when confirming a game, and when confirming the question. | Nothing happens. |
| After Back on the game list, after "Could not find a game", and after a transfer: press HOME on the title screen. | The HOME Menu opens. |

### Refusals

| Do this | Expected |
| --- | --- |
| After a transfer, press START again without opening Bank in between. | The "Pokémon remains in the Transport Box" message right after START: its first page with the arrow, after A its second without one, after A the title screen. No game list appears. |
| Leave some Pokémon in the transport box, Save and Quit, start Transporter and press START. | The same two screens, until the transport box is empty and saved. |
| Empty Box 1, Bank's transport box empty. | The original message that there is nothing to transport, then the title screen. |
| A console on which the offline Bank was never started: press START. | The "did not complete correctly… open Pokémon Bank" message right after START, both pages as the original has them, then the title screen. After Bank was started once, START leads to the game list. |
| No game in the listed language, Bank ready: press START. | "Could not find a game", then the title screen, as before; START can be pressed again. |

### Language of the games

| Do this | Expected |
| --- | --- |
| Start Transporter and open the game list without visiting the language screen. | Only games in the console's language are listed: cartridge, saves on the SD card, Virtual Console titles. |
| Open the language screen. | Six languages: not the one whose games are listed, no Chinese ones. Below them a Back button that looks like the six entries above it, with the game-selection screen's "Back" text in the middle and a teal return icon at the right. The seven entries are centred on the screen with a small gap between them. Up and down wrap around the seven; touching an entry works. |
| Choose another language and confirm. | The confirm screen appears, for Japanese too (no kana/kanji choice). Back on the title screen the screens are in the same language as before. The game list now shows the games of the chosen language only. |
| Choose another language and look at the title screen. | The logo and the start prompt are in the chosen language; the texts of the other screens are not. Without a choice, and after a restart, both are in the console's language. Back on the language screen leaves them as they were. |
| Open the language screen again. | The language chosen before is now missing from the list, and the one listed before is back. |
| Back on the language list: touch it, press A on it, or press B. | The cancel sound, then the title screen. The game list shows the same language as before. |
| Back or B on the confirm screen. | The language list again; nothing changes. |
| Choose a language no game is present in. | The original behaviour for an empty game list. |
| Close and restart Transporter. | The games in the console's language are listed again. |
| A console set to Chinese. | The screens are Chinese. English games are listed at first; the language list offers the six other languages and Back, with Back's text in Chinese. |

### Saves on the SD card

The saves are in the chosen language.

| Do this | Expected |
| --- | --- |
| No cartridge, one accepted save. | The game appears in the list with its trainer name. A transfer works as from a cartridge; afterwards `<name>.sav.bak` holds the save as it was, and the game started in TWiLight Menu shows the Pokémon gone from Box 1. |
| No cartridge, saves of several games. | One entry per save; the one you pick is the one read and changed. |
| No cartridge, saves of the same game in two languages. | Only the one in the chosen language is listed; after choosing the other language, only the other. |
| A Gen 5 cartridge in the chosen language, saves on the SD card. | The games are listed in the order Black, White, Black 2, White 2; the cartridge stands at its game's place, the saves at theirs. A save of the cartridge's own game is not offered. Picking the cartridge reads and changes the cartridge; picking a save reads and changes that file. |
| A Gen 5 cartridge in another language than the chosen one. | The cartridge is not listed; the saves in the chosen language are. After choosing the cartridge's language, the cartridge is listed. |
| A save with another name, or a file of another size, in the folder. | It is not offered. |
| An accepted White save copied to the Black name of its language, and a German save copied to the English name of its game (English chosen). | Neither copy is offered. The saves under their own names still are. |
| An accepted save renamed to the other revision (`_01` instead of `_00`, or for Japanese Black 2 and White 2 `_00` instead of `_01`). | It is not offered; under its name from the table in the guide it is. |

## An interrupted Save and Quit

All runs here use the test package. After START it shows the original's main menu, and the button chosen there says what the Save and Quit of that session does:

| Button on the main menu | What Save and Quit does | The card afterwards |
| --- | --- | --- |
| Use Pokémon Bank | Saves normally. | |
| Use Pokémon Bank (before) | Stops after the Bank has marked its save as in progress, before the game is written. Error `00007E57 00000001`. | As after a power cut at that point: mark set, the game has its old save. |
| Use Pokémon Bank (after) | Stops after the game is written, before the mark is cleared. Error `00007E57 00000002`. | Mark set, the game has its new save. |
| Use Pokémon Bank (record) | First writes the journal record 150 times, then saves normally. | A valid Bank at every instant. |
| Use Pokémon Bank (boxes) | First writes the file of the boxes that is not in use 30 times, then saves normally. | A valid Bank at every instant. |
| Back | The original's: asks, then returns to the start screen. | |

The two stops end Save and Quit at a fixed point with an error on purpose: "The server did not receive the data." Press A; the line about the HOME Menu follows with the numbers. Close Bank from the HOME Menu, do the "Then" step, and start Bank again. Where a run says "start Bank", choose the plain **Use Pokémon Bank** on the menu. Back on the game selection leads to the menu again, so another entry can be chosen without closing Bank. What Bank decides is explained in [bank.md](bank.md#if-save-and-quit-is-interrupted).

The menu also shows what the original showed there about the pass; that is not part of any run. No reward dialog comes in this package.

### The game keeps its old save

Menu entry **Use Pokémon Bank (before)**. Save and Quit ends on `00007E57 00000001`.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 5 | deposit one, withdraw one | start Bank, the game still there | boxes as before the session; the game unchanged |
| 6 | deposit one | play the game, save, start Bank | the deposited Pokémon in Bank; it is also still in the game |
| 7 | withdraw one | play the game, save, start Bank | the Pokémon still in Bank; not in the game |
| 8 | deposit one, withdraw one | play the game, save, start Bank | the deposited one in Bank and in the game; the withdrawn one is gone |

Run 8 is the one case in which a Pokémon is lost by design; use one you do not need.

### The game is not inserted

Still **Use Pokémon Bank (before)**, with a cartridge game that is not also installed.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 9 | deposit one | take the cartridge out, start Bank | Bank opens; the deposited Pokémon is in Bank, and still on the cartridge |
| 10 | deposit one, withdraw one | take the cartridge out, start Bank | the original's message "You previously used a different copy of this game…" with the name and trainer ID of the cartridge's game; A leads to the start screen. With the cartridge back in, START without closing Bank: boxes as before the session |

The trainer ID in that message has five digits for X, Y, Omega Ruby and Alpha Sapphire and six for the later games, as the game itself shows it.

### The same game as cartridge and installed

Still **Use Pokémon Bank (before)**. This needs one game both as a cartridge and installed, each with a save of its own. An installed copy that was never played has no save, and Bank does not find it.

| Run | Session on | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- | --- |
| 11 | cartridge | deposit one | take the cartridge out, start Bank | Bank opens; the deposited Pokémon is in Bank, and still on the cartridge |
| 12 | cartridge | deposit one, withdraw one | take the cartridge out, start Bank | the message of run 10 with the name and trainer ID of the cartridge's game; with the cartridge back in, START: boxes as before the session |
| 13 | installed copy, cartridge out | deposit one, withdraw one | put the cartridge in, start Bank | the message of run 10 with the name and trainer ID of the installed copy's game; with the cartridge out again, START: boxes as before the session |

If run 12 or 13 opens Bank with the new boxes and no message, Bank did not tell the cartridge from the installed copy.

### The game has its new save

Menu entry **Use Pokémon Bank (after)**. Save and Quit ends on `00007E57 00000002`.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 14 | deposit one, withdraw one | start Bank, the game still there | both moves done, in Bank and in the game |
| 15 | withdraw one | play the game, save, start Bank | the Pokémon back in Bank; it is also in the game |

## A power cut during a save

Menu entries **Use Pokémon Bank (boxes)** and **Use Pokémon Bank (record)** of the test package. They save normally, but first write for a while, so that a real cut lands inside a write. The saving screen stays clearly longer than usual.

For each of the two entries, three to five times:

| Step | Do | Must show |
| --- | --- | --- |
| 1 | Start Bank, choose the entry, pick a game, move one Pokémon, Save and Quit | the saving screen |
| 2 | About five seconds in, cut the power for real | |
| 3 | Start Bank, Use Pokémon Bank | Bank opens, with the boxes as before step 1 |
| 4 | Move one Pokémon, Save and Quit, and let it finish. Close Bank and start it again. | the save completes, and the change is there |

How to cut the power, best first: pull the SD card, if the model lets you reach it while it runs; take out the battery; hold POWER. Holding POWER is the weakest, because the console may shut down in order and nothing is interrupted.

Step 3 shows that a cut damaged nothing that was still needed. Step 4 shows that the file the cut left behind is replaced by the next save.

Pulling a card that is being written is at your own risk. Only Bank's own files are being written at that moment; the backup is what protects everything else.

## A power cut during a transfer

With the Transporter package and the normal Bank package, a Gen 5 game with Pokémon in Box 1, and Bank's transport box empty. Three to five times:

| Step | Do | Must show |
| --- | --- | --- |
| 1 | Start Transporter, press START, choose the game, confirm the question | the screen that says the Pokémon are being moved |
| 2 | Cut the power for real as soon as that screen is up | |
| 3 | Start Bank and pick a game | Bank opens with its boxes as they were. The transport box is empty, or it holds every Pokémon of Box 1 in its position; never some of them |
| 4 | Look at Box 1 in the game | With an empty transport box: every Pokémon is still there. With a full one: they are still there too, or gone |
| 5 | In Bank, move what is in the transport box into a box, Save and Quit. Start Transporter and press START | the game list |

Step 3 shows that Bank never sees a delivery that was not written whole. Step 4 is the one case in which Pokémon can be doubled ([transporter.md](transporter.md#limits)): the delivery was complete and the game was not yet saved without them. Step 5 shows that the file the cut left behind is replaced by Bank's save. Before step 5, START in Transporter may show one of its two refusals ([transporter.md](transporter.md#transfers-and-refusals)); both are right at that point.

## Updating from the version before

A release that changes how the Bank is stored ships one migration patch, for the Bank of the one version before it ([building.md](building.md#release)). The normal patches never read the earlier files; they have to refuse them, and the migration has to convert them. These runs check both. *Old* is the version before, *new* the release.

Start from a console whose Bank was last used with the old version, and from a backup of it: after run 18 the old version cannot open the Bank any more.

| Run | Installed | Do | Must show |
| --- | --- | --- | --- |
| 16 | new Bank | start Bank, before converting | the error message, then the line about the HOME Menu with `00000003` and the size of the file it refused as second number; nothing changed |
| 17 | new Transporter | start Transporter and press START, before converting | the "did not complete correctly… open Pokémon Bank" message right after START, then the title screen; no game list appears |
| 18 | migration | start Bank, press START | a loading screen, then the success sound and "Success! Changes saved."; A leads to the start screen |
| 19 | migration | press START again | the line about the HOME Menu with the version and "-migration" on its second line and `0000600D 00000002` on its third, and nothing before it. Close Bank from the HOME Menu |
| 20 | new Bank | start Bank | boxes, Pokédex, Poké Miles and transport box as they were under the old version |
| 21 | new Bank and new Transporter | run 3 of [Normal use](#normal-use) | as there |

Runs 16 and 17 are what keeps the normal patches free of code for earlier versions: an old Bank must be refused by both, with nothing written, and Transporter must stop before it reads any game.

A migration patch never opens the Bank. A conversion ends with the success message and the start screen; every other result is two numbers, under the line about the HOME Menu:

| Numbers | Meaning |
| --- | --- |
| `0000600D 00000002` | already converted; nothing changed |
| `0000600D 00000000` | no Bank found |
| `00000BAD 00000007` | the old Bank has a Save and Quit in progress; the old version has to finish it |
| `00000BAD 00000008` | the old Bank has no Pokédex or no transport box for its current save; the old version writes them at its next Save and Quit |
| `00000BA1` to `00000BA4` | a step failed; the second number says why. Starting it again must be safe. |

That a migration refuses a Bank with a Save and Quit in progress, and that a cut at any of its steps is picked up by the next start, is covered by its PC tests; released versions have no package that stops a save on purpose.

**A Save and Quit left in progress by another version** is that version's to finish, with or without a migration between the two ([bank.md](bank.md#if-save-and-quit-is-interrupted)). This needs a test package built from the other version's sources:

| Installed | Do | Must show |
| --- | --- | --- |
| test package of the other version | a Save and Quit that stops, A, close Bank | its numbers |
| new Bank | start Bank, with its game inserted and without it | `0000000B`, then the line about the HOME Menu; nothing changed |
| test package of the other version | start Bank | the save is settled as that version settles it |
| new Bank | start Bank | Bank opens |

## The download from the official server

Package `build/download/`. None of these runs has been made yet. It needs the live service, an internet connection and the console's Nintendo Network ID. No game is needed, and none is read or saved. Back up Bank's extdata first, and the games on the console, so that run D8 can compare them.

| Run | Do | Must show |
| --- | --- | --- |
| D1 | Start screen | "Ver. 1.5-download" |
| D2 | START, with an offline Bank on the card | after a loading screen the line about the HOME Menu, the version with "-download" on its second line and `0000D0AD 00000002` on its third; nothing is contacted |
| D3 | Move the offline Bank's files off the card (keep them). With a game on the console: START | the original connects and loads; no main menu, no game selection, no reward dialog, no boxes; then the success sound and "Success! Changes saved.", and after A the start screen |
| D4 | START again | the line about the HOME Menu with `0000D0AD 00000002` |
| D5 | Install the offline Bank and start it | the boxes, the transport box and the Poké Miles as they are on the server |
| D6 | Install the original Bank without a patch and use it online | it opens as always: nothing is locked, and the boxes on the server are unchanged |
| D7 | The download again, wireless off, no Bank on the card: START | the original's message about the connection, then its start screen; nothing written |
| D8 | After D3, compare the games' saves with the backup | unchanged, byte for byte |
| D9 | On a console without any game the original can use (no cartridge, none installed), no Bank on the card: START | no question about Pokémon HOME; the original connects and loads as in D3; then the success message as there |

D6 shows that the session was closed on the server; it is the run that matters most. The load used here does not write Bank's own save, so D6 also shows that the original copes with that. If the original says the server is locked, wait as it asks and note it.

## If something else shows

Stop there. Photograph the screen with both numbers, note the run and the package, and restore the backup before trying anything else. The first number says what failed and the second why ([bank.md](bank.md#troubleshooting)).

## Afterwards

Restore the extdata and the game saves from the backup, and put the normal packages back.
