# Testing on a console

**Every release is tested manually against this baseline.** It covers the current release's normal use and data safety, including the complete interrupted Save and Quit checks below. Add focused checks for the features changed in that release; the detailed behavior belongs in the [Bank](bank.md) and [Transporter](transporter.md) guides.

Record the release/package IDs, console model, language, and results. Mark unavailable or unfinished checks as not run. The [PC tests](building.md#tests) complement this checklist; they do not establish console behavior.

## Before you start

- Back up Bank's **extdata** and the game saves ([how](bank.md#backing-up-the-bank)). Use disposable Pokémon for interrupted saves and transfers.
- Install matching `code.ips` and `exheader.bin` files from the same package. [Build/package locations](building.md#build).
- Finish any interrupted Save and Quit with the package that began it, or restore the backup, before switching packages.

## Normal use

Use the normal Bank and Transporter packages. Cover cartridge and installed games, and Gen 6 and Gen 7, where available.

| Check | Expected |
| --- | --- |
| Start with empty Bank extdata and a usable game. Reopen after creation. | Welcome, preparation, then an empty Bank with correct box/group names. Reopening does not repeat creation or the welcome. No online connection. |
| Deposit and withdraw Pokémon, rename a box/group, Save and Quit, then reopen. Repeat a save in the same application session. | Pokémon, names, and other saved changes persist in Bank and the game; nothing is lost or duplicated. |
| Back out of game selection; leave the boxes without saving. | Return to the title. Unsaved changes are discarded. |
| START without a usable game; then insert one and START again. | Two native explanatory pages, then the title after the second A press. No HOME-transfer question or Bank-file creation. A usable game proceeds normally. |
| Select a save without the required Pokédex progress. | Native notice, then game selection; other eligible saves remain usable. |
| Press START on the start screen, in Bank and in Transporter. | The "Checking the Game Card… Don't turn off the power or remove the Game Card or SD card." screen with its spinner and sound appears with the press, not after a pause; In Bank the same panel runs on into game selection. In Transporter the same one spinner and sound stay, standing still while the game list loads, until the list is on screen; no second spinner, no restart of the sound, and nothing behind the list's "choose a game" text. The list comes as soon as it is loaded, at least 3 s after START, not 3 s after the search. |
| Watch each loading screen as it comes in: in Bank START, creating a Bank, loading the chosen game, Save and Quit, and the way back to the start screen; in Transporter START, reading the chosen game, the transfer, and the way back to the start screen. Time a short one, such as Save and Quit with nothing changed, and an error, such as choosing a cartridge game in Transporter and taking the card out before A. | The screen slides in without a stop in its first half second. One that ends in a result or the next screen stays at least 1.5 s (Transporter's game list at least 3 s after START); an error message may come earlier, but not within the first half second. |
| Start a game, press HOME and start Bank without closing the game first. Press START, choose the game, and press HOME on game selection, on the boxes and on every other screen that waits; then Save and Quit and press HOME on the start screen. | The HOME Menu appears each time, and Bank goes on where it was. |
| Observe loading and try HOME/sleep during work and on idle screens. Watch the spinner at the press of Save and Quit and just before the menu or the boxes appear after loading. | Background work keeps updating the spinner and sound, also at those moments; a stop there is a fault, a short native pause elsewhere is not. HOME/sleep are refused during loading/saving and work on idle screens and completed result/error screens. Pressing HOME repeatedly just before the boxes appear (Bank) and just before Transporter's question appears does nothing while the spinner is still on screen. |

## Bank, feature by feature

| Check | Expected |
| --- | --- |
| On a new Bank, receive the 100-Mile welcome gift, Save and Quit, then reopen. | Gift is saved once. Leaving without saving does not make it permanent. |
| Check Miles accrual, redeem a reward, Save and Quit, then reopen. | Balance and game gift persist correctly. Gen 6 supports Miles/BP; Gen 7 supports BP. [Rules](bank.md#poké-miles). |
| Import records from two game versions, save and reopen; later save with one known game after its progress changes. | Both versions remain in the National Pokédex. The known game's records update without replacing the other version. Unsaved imports are discarded. |
| With a cartridge game loaded, move a Pokémon, take the cartridge out on the box screen and Save and Quit. Press A. Repeat in Chinese. | Only "Please confirm that your Game Card is properly inserted, and then try again." (the original's second part of its failure message), without sound, numbers or "Press HOME"; HOME works while it waits. A returns to the title. |
| Then put the same cartridge back, START, load it, and save normally; or put in another compatible game and START. | Bank and the game are as before the session: the moved Pokémon is where it was. The new session opens, loads and saves as usual. |
| Choose a cartridge game and pull the cartridge while its Bank loads. Press A, put it back, START, choose it again. | "Please confirm that your Game Card is properly inserted, and then try again." at once, without numbers; HOME works on it. A returns to the title; Bank does not have to be closed, and the game loads. |
| Move a Pokémon and pull the cartridge two to five seconds into Save and Quit's loading screen. Press A, put it back, START, choose the game. Repeat a few times at other moments. | Each time "Please confirm… try again." or "Success! Changes saved.", never the numbers. After START the game loads, and Bank and the game agree: the moved Pokémon is in one place, or in both. Or, if the pull damaged the game's save, "The saved data in your Pokémon game is corrupted." when it is chosen, and another game still loads. |
| Choose a game whose save fails its checksums (a cartridge or installed save with one byte of a box changed in a hex editor, the game's own check fails too). Then choose another game. | "The saved data in your Pokémon game is corrupted." at once, without numbers; HOME works on it. A returns to the title. The other game loads as usual. With the save restored, the game loads again. |
| Choose an intact game of every generation, cartridge and installed (X/Y, Omega Ruby/Alpha Sapphire, Sun/Moon, Ultra Sun/Ultra Moon). | Each loads as usual: the check passes for every intact save. |
| Test package, **(before)**, then reopen with the cartridge, START, and pull it on the checking screen. Press A, put it back, START. | "Please confirm… try again." without numbers, then the title. The second START settles the save as in [case 1](#before-the-game-is-written). |
| Fill the SD card until less than 1 MB is free, delete Bank's extdata, start Bank. | After the welcome, or at once if even Bank's storage cannot be made: "Pokémon Bank could not be started due to a lack of space on your SD card." without numbers, then the title. Free space, START: the welcome again, and an empty Bank. |
| Delete `/mover.alt.bin` from Bank's extdata (GodMode9) while the transport box is empty, fill the SD card completely (until copying any further file fails), then move a Pokémon and Save and Quit. | "The SD card is full." on one line, without numbers or "You can't download data.", then the title. Free space, START: the Pokémon is where it was before the session. |
| An error screen: test package, **(before)** ([below](#an-interrupted-save-and-quit)). Press A; repeat in Japanese, Korean or Chinese. | The original's error text, then "Press HOME to return to the HOME Menu", the version with "-test", and the two numbers, all characters readable. A, B and START do nothing; HOME and sleep work, also with Bank started from a running game. |

## Transporter, feature by feature

| Check | Expected |
| --- | --- |
| Transfer a Gen 5 Box 1 with gaps, then try another transfer before emptying Bank's transport box. Move the delivery into Bank, save, and reopen. | Accepted Pokémon arrive once in their original positions and leave the source. Another delivery is refused while Pokémon remain; it is allowed after Bank saves an empty transport box. |
| Transfer from a supported Gen 5 SD card save. | Only matching game/language saves are listed. The correct file changes, with a `.sav.bak` backup; unrelated saves stay unchanged. |
| With a White cartridge and a Black 2 save on the SD card, START; take the cartridge out on the list, choose Black 2 and transfer. START again, take the cartridge out on the list and choose White. | The Black 2 transfer runs as usual. White: the original's message that no compatible game is detected. |
| With a White cartridge and a White save on the SD card, START; take the cartridge out on the list and choose White. | The original's message that no compatible game is detected; the save on the SD card is not changed. |
| With a White cartridge and Black and Black 2 saves, take the cartridge out on the checking screen after START. | The list shows both saves and no White. |
| Choose a cartridge game with Pokémon in Box 1, take the cartridge out on the question and answer "Yes". Open Bank. Repeat in Chinese. | Only "Please confirm that your Game Card is properly inserted, and then try again." (in Chinese, the line after the first), then the title. Bank's transport box is empty, and the game still holds its Pokémon. |
| The same with a save on the SD card chosen, and with a Virtual Console title. | The transfer runs as usual. |
| Fill the SD card to less than 512 KiB free, delete the `.sav.bak` of a Black/White save on it, choose that save and transfer. | After the game is read: "The SD card is full." on one line, without numbers or "You can't download data.", then the title. The save is unchanged. A `.sav.bak` may be there, but incomplete. Free space and transfer: the backup is made, the transfer runs. |
| Transfer from a Gen 1 or Gen 2 Virtual Console game. | The Pokémon arrive and can be withdrawn into a Gen 7 game only. |
| A Box 1 with an Egg or another Pokémon the original refuses. | The original's refusal dialogs for it; the others are offered. No notice about nicknames or OT names. |
| START on a console where the offline Bank was never started. | The checking screen, then the original's "did not complete correctly… open Pokémon Bank" message without the spinner behind it, then the title. No game list. |
| START with no game at all, then START again. | The checking screen, then "Could not find a game…" without the spinner behind it, then the title; the second START shows the checking screen again. |
| Choose a game whose first box is empty; then choose one with Pokémon and answer "No" at the question. | Both times the original's message, then directly "Software wird beendet…"; no "Datenübertragung über den Server…" in between. |
| START without a cartridge or SD save: once with only Virtual Console games of the listed language, once with only ones of another language, once with none. | The spinner stops only briefly. The first lists those games as before; the other two end with "Es wurde kein Spiel gefunden…". Transporter starts normally. |
| Choose another language on the language screen; then Back, and B. | Only that language's games are listed; the logo and start prompt follow it, the other screens stay in the console's language. Back and B leave the choice unchanged. After a restart the console's language applies again. |
| Try HOME and sleep while the list is built, a game is read and the transfer runs, then on the title, the list, the question and every message that waits for A. | Refused while Transporter works; they work everywhere else. |

## An interrupted Save and Quit

Use the [test package](building.md#test-builds-for-an-interrupted-save) and restore the starting backup before each case. Its main-menu entry selects the next Save and Quit:

| Entry | Behavior |
| --- | --- |
| **Use Pokémon Bank** | Normal save; also settles an interrupted save on reopening. |
| **(before)** | Stops after marking the Bank save in progress, before writing the game. Error `00007E57 00000001`. |
| **(after)** | Stops after writing the game, before clearing the mark. Error `00007E57 00000002`. |
| **(record)** | Writes the journal 150 times before saving normally, for a real power-cut test. |
| **(boxes)** | Writes the inactive boxes file 30 times before saving normally, for a real power-cut test. |

For each case, perform the moves, Save and Quit with the named entry, acknowledge the intentional error, and close Bank. Follow the recovery step, then reopen with plain **Use Pokémon Bank**. After recovery, complete a normal save and reopen again to confirm that saving still works. [Recovery rules](bank.md#if-save-and-quit-is-interrupted).

### Before the game is written

Choose **(before)**. “Play and save” below means making a new save in the game before reopening Bank.

| Case | Moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 1 | Deposit one, withdraw one | Reopen with the same game unchanged. | Bank and game are as before the session. |
| 2 | Deposit one | Play and save, then reopen. | Deposited Pokémon is in Bank and still in the game. |
| 3 | Withdraw one | Play and save, then reopen. | Pokémon remains in Bank and is absent from the game. |
| 4 | Deposit one, withdraw one | Play and save, then reopen. | "Your communication with Poké Transporter did not complete correctly during your last session." → A → "Please open Poké Transporter and perform the cleanup process before using Pokémon Bank.", then the title; START shows it again. Remove the game and START: the different-copy notice instead. Put it back and START: the message again. |

Then, from the state of case 4:

| Case | Step | Expected |
| --- | --- | --- |
| 4a | Open Transporter and START. On the question press HOME and close Transporter; open it and START again. | The checking screen, then "You can try restoring your data, but you may lose the Pokémon currently being transported. Would you still like to try restoring your data?" with Yes and No, without "This is your final warning…". HOME works on it; the second START asks again. |
| 4b | Answer Yes. Open Bank and the game. | "Communication in your last session did not complete correctly. Performing cleanup process now..." with its spinner for about 1.5 s, the closing screen, the title. Bank opens: the withdrawn Pokémon is in Bank, the deposited one in the game; nothing lost. |
| 4c | Repeat case 4 from the backup, answer No. Open Bank. | The same screens. Bank opens with the deposited Pokémon, which is also in the game; the withdrawn Pokémon is lost. |

**Case 4 leaves a save that only an answer can settle. Use disposable Pokémon.**

### The cartridge is absent

Choose **(before)** with a cartridge game that is not also installed. Keep another readable compatible game available so the scan can reach recovery; with no usable game at all, Bank stops at its no-game notice first.

| Case | Moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 5 | Deposit one | Remove the cartridge and reopen. | Bank opens with the deposited Pokémon; it also remains on the cartridge. |
| 6 | Deposit one, withdraw one | Remove the cartridge and reopen. | Native different-copy notice names the cartridge's trainer and ID; A returns to the title. Reinsert it and START again: Bank and game are as before the session. |

### The same game is cartridge and installed

Choose **(before)**. Both copies must have readable saves of their own.

| Case | Session and moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 7 | Cartridge: deposit one | Remove it and reopen with the installed copy present. | Bank opens with the deposited Pokémon; it also remains on the cartridge. |
| 8 | Cartridge: deposit one, withdraw one | Remove it and reopen with the installed copy present. | Different-copy notice names the cartridge's trainer and ID. Reinsert it and START: state from before the session. |
| 9 | Installed copy, cartridge out: deposit one, withdraw one | Insert the cartridge and reopen. | Different-copy notice names the installed copy's trainer and ID. Remove the cartridge and START: state from before the session. |

The trainer ID has five digits for Gen 6 and six for Gen 7. The other copy must not satisfy recovery for the original.

### After the game is written

Choose **(after)**.

| Case | Moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 10 | Deposit one, withdraw one | Reopen with the same game unchanged. | Both moves are committed in Bank and the game. |
| 11 | Withdraw one | Play and save, then reopen. | Pokémon is back in Bank and also in the game. |

## A real power cut

Cut the power three to five times for each row: pull the SD card if the model allows it while running, else take out the battery. Holding POWER may shut down in order and interrupt nothing.

| Check | Expected |
| --- | --- |
| Test package, **(record)** and then **(boxes)**: move one Pokémon, Save and Quit, cut the power about five seconds in. Reopen; then save again and reopen. | Bank opens with the boxes from before the save. The next save completes and persists. |
| Normal packages: confirm a Gen 5 transfer, cut the power as soon as the moving screen appears. Open Bank, then look at Box 1 in the game. | The transport box is empty or holds every Pokémon of Box 1, never some. With an empty transport box all are still in the game; with a full one they may be in both places. After Bank saves the transport box empty, Transporter lists games again. |

## The migration

Once with a Bank last used with v0.2.1 and once with one last used with v0.4.0, each from a backup: after the conversion those releases cannot open it. Note the Miles and the date of the last Save and Quit before.

| Check | Expected |
| --- | --- |
| New Bank, before converting: START. | The error screen with `00000003` and `00164C54` (v0.2.1) or `000000C0` (v0.4.0); nothing changed. |
| New Transporter, before converting: START. | The "did not complete correctly… open Pokémon Bank" message, then the title. No game list. |
| Migration package: START, also with no game present. | Preparation text, then the HOME line with "v0.2.1-" or "v0.4.0-" and the release's version, and `0000600D 00000001`. No game scan or game selection. |
| Close Bank from HOME, reopen with the migration, START. | The HOME line with "-migration" behind the release's version, and `0000600D 00000002`. |
| New Bank and Transporter. | Boxes, Pokédex and transport box as before; a transfer works. |
| Miles at the first opening with a game. | The balance from before, and what the days since the last Save and Quit earn on top: Pokémon in the Bank ÷ 30 per day. From v0.2.1, also the day of that save counts from its start. |
| Save and Quit, reopen. | The same boxes and Miles. |
| A Bank of v0.3.0 with a save in progress (v0.3.0, a cut during Save and Quit), then the migration. | `00000BAD 00000007`. |
| v0.4.0 with the game, START. | v0.4.0 finishes the save as it finishes its own; without the game its message names no trainer. Then the migration converts (`0000600D 00000001`). |

## The download

Only while the official service runs. Needs the console's Nintendo Network ID; back up the game saves to compare them afterwards.

| Check | Expected |
| --- | --- |
| START with an offline Bank on the card. | `0000D0AD 00000002`; nothing is contacted. |
| Wireless off, no offline Bank: START. | The original's connection message, then its title; nothing written. |
| No offline Bank: START, once with a game and once without any. | The original connects and loads, without menu, game selection or HOME question, then `0000D0AD 00000001`. |
| Install the normal Bank and open it. | The boxes, transport box and Miles from the server. |
| Install the unpatched Bank and use it online. | It opens as always: the server is not locked and its boxes are unchanged. This is the check that matters most. |
| Compare the game saves with the backup. | Unchanged, byte for byte. |

## If a check fails

Stop and record the package ID, console/language, steps, and a photo of the complete error/result screen. Preserve the failing data before restoring the backup. After testing, restore the normal packages and any backups needed.
