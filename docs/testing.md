# Testing on a console

**Every release is tested manually against this baseline.** It covers the current release's normal use and data safety, including the complete interrupted Save and Quit checks below.

Note the release and package name, the console model and its language with the results. The [PC tests](building.md#tests) cover what can be checked without a console; they do not show how the patches behave on one.

## Before you start

- Back up Bank's **extdata** ([Checkpoint](https://github.com/BernardoGiordano/Checkpoint), extdata mode, X button) together with the folder `/3ds/Pokémon Bank/`, and the game saves. Restore the extdata and that folder only together, from the same moment, with Bank closed: the offline boxes are no part of the extdata. Use disposable Pokémon for interrupted saves and transfers.
- Install `code.ips` and `exheader.bin` from the same package ([where the builds are](building.md#build)).
- Finish any interrupted Save and Quit with the package that began it, or restore the backup, before switching packages.

## Normal use

Use the normal Bank and Transporter packages. Cover cartridge and installed games, and Gen 6 and Gen 7, where available.

| Check | Expected |
| --- | --- |
| Start with empty Bank extdata and a usable game. Reopen after creation. | Welcome, preparation, then an empty Bank with correct box and group names. Reopening does not repeat creation or the welcome. No online connection. |
| Deposit and withdraw Pokémon, rename a box and a group, Save and Quit, then reopen. Repeat a save in the same application session. | Pokémon, names, and other saved changes persist in Bank and the game; nothing is lost or duplicated. |
| Back on game selection, then Cancel; leave the boxes without saving. | Back opens the transfer screen's question ([below](#the-transfer-screen)), and Cancel returns to the start screen. Leaving the boxes returns to the start screen; unsaved changes are discarded. |
| START without a usable game; then insert one and START again. | The original's two pages of explanation, then the start screen after the second A press. No Pokémon Home question or Bank-file creation. A usable game proceeds normally. |
| Select a save without the required Pokédex progress. | The original's notice, then game selection; other eligible saves remain usable. |
| Press START on the start screen, in Bank and in Transporter. | The "Checking the Game Card… Don't turn off the power or remove the Game Card or SD card." screen with its spinner and sound appears with the press, not after a pause; In Bank the same panel runs on into game selection. In Transporter the same one spinner and sound stay, standing still while the game list loads, until the list is on screen; no second spinner, no restart of the sound, and nothing behind the list's "choose a game" text. The list comes as soon as it is loaded, at least 3 s after START, not 3 s after the search. |
| Watch each loading screen as it comes in: in Bank START, creating a Bank, loading the chosen game, Save and Quit, and the way back to the start screen; in Transporter START, reading the chosen game, the transfer, and the way back to the start screen. Time a short one, such as Save and Quit with nothing changed, and an error, such as choosing a cartridge game in Transporter and taking the card out before A. | The screen slides in without a stop in its first half second. One that ends in a result or the next screen stays at least 1.5 s (Transporter's game list at least 3 s after START); an error message may come earlier, but not within the first half second. |
| Start a game, press HOME and start Bank without closing the game first. Press START, choose the game, and press HOME on game selection, on the boxes and on every other screen that waits; then Save and Quit and press HOME on the start screen. | The HOME Menu appears each time, and Bank goes on where it was. |
| Observe loading and try HOME and sleep during work and on idle screens. Watch the spinner at the press of Save and Quit and just before the menu or the boxes appear after loading. | Background work keeps updating the spinner and sound, also at those moments; a stop there is a fault, a short pause of the original elsewhere is not. HOME and sleep are refused during loading and saving, and work on idle screens and on finished result and error screens. Pressing HOME repeatedly just before the boxes appear (Bank) and just before Transporter's question appears does nothing while the spinner is still on screen. |

## Bank, feature by feature

| Check | Expected |
| --- | --- |
| On a new Bank, receive the 100-Mile welcome gift, Save and Quit, then reopen. | Gift is saved once. Leaving without saving does not make it permanent. |
| Check Miles accrual, redeem a reward, Save and Quit, then reopen. | Balance and game gift persist correctly. Gen 6 takes Miles and Battle Points, Gen 7 Battle Points only. [Rules](bank.md#poké-miles). |
| Import records from two game versions, save and reopen; later save with one known game after its progress changes. | Both versions remain in the National Pokédex. The known game's records update without replacing the other version. Unsaved imports are discarded. |
| With a cartridge game loaded, move a Pokémon, take the cartridge out on the box screen and Save and Quit. Press A. Repeat in Chinese. | Only "Please confirm that your Game Card is properly inserted, and then try again." (the original's second part of its failure message), without sound, numbers or "Press HOME"; HOME works while it waits. A returns to the start screen. |
| Then put the same cartridge back, START, load it, and save normally; or put in another compatible game and START. | Bank and the game are as before the session: the moved Pokémon is where it was. The new session opens, loads and saves as usual. |
| Choose a cartridge game and pull the cartridge while its Bank loads. Press A, put it back, START, choose it again. | "Please confirm that your Game Card is properly inserted, and then try again." at once, without numbers; HOME works on it. A returns to the start screen; Bank does not have to be closed, and the game loads. |
| Move a Pokémon and pull the cartridge two to five seconds into Save and Quit's loading screen. Press A, put it back, START, choose the game. Repeat a few times at other moments. | Each time "Please confirm… try again." or "Success! Changes saved.", never the numbers. After START the game loads, and Bank and the game agree: the moved Pokémon is in one place, or in both. Or, if the pull damaged the game's save, "The saved data in your Pokémon game is corrupted." when it is chosen, and another game still loads. |
| Choose a game whose save fails its checksums (a cartridge or installed save with one byte of a box changed in a hex editor, the game's own check fails too). Then choose another game. | "The saved data in your Pokémon game is corrupted." at once, without numbers; HOME works on it. A returns to the start screen. The other game loads as usual. With the save restored, the game loads again. |
| Choose an intact game of every generation, cartridge and installed (X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, Ultra Moon). | Each loads as usual: the check passes for every intact save. |
| Test package, **(before)**, then reopen with the cartridge, START, and pull it on the checking screen. Press A, put it back, START. | "Please confirm… try again." without numbers, then the start screen. The second START settles the save as in [case 1](#before-the-game-is-written). |
| Fill the SD card until less than 1 MB is free, delete Bank's extdata, start Bank. | After the welcome, or at once if even Bank's storage cannot be made: "Pokémon Bank could not be started due to a lack of space on your SD card." without numbers, then the start screen. Free space, START: the welcome again, and an empty Bank. |
| Delete `/mover.alt.bin` from Bank's extdata (GodMode9) while the transport box is empty, fill the SD card completely (until copying any further file fails), then move a Pokémon and Save and Quit. | "The SD card is full." on one line, without numbers or "You can't download data.", then the start screen. Free space, START: the Pokémon is where it was before the session. |
| An error screen: test package, **(before)** ([below](#an-interrupted-save-and-quit)). Press A; repeat in Japanese, Korean or Chinese. | The original's error text, then "Press HOME to return to the HOME Menu", the version with "-test", and the two numbers, all characters readable. A, B and START do nothing; HOME and sleep work, also with Bank started from a running game. |

## The transfer screen

| Steps | Expected |
| --- | --- |
| With the test package: START. | Bank opens as usual; `/3ds/Pokémon Bank/` exists on the SD card, shows under that name on a PC ("é" included), and holds no folder `probe` or `probe2` (the test build makes, lists and renames such a folder at every opening and stops with `00000015` if it cannot). It makes the same check of a folder with a 16-character name and a 29,192-byte file in Bank's extdata, and leaves nothing there. An error `00000015` here means Bank cannot use the SD card's files or that folder. |
| START, choose nothing, press Back on game selection (also in the test package), once in each language. | "Do you want to move the Pokémon in Pokémon Bank?" (German "Möchtest du die Pokémon, die sich derzeit in Pokémon Bank befinden, transferieren?"), nothing about Pokémon HOME, with the buttons "Begin the move" and "Cancel", as when Bank opens without a game; no screen "You're about to send some of your Pokémon to Pokémon HOME." with "Begin". Cancel: the start screen. Begin the move: "Communicating with the Pokémon Bank server..." for at least 1.5 s, then the 100 boxes and the four cells behind them, with no text frame left over them. On the four cells the upper screen names "Transport Box", or an offline box's own name. |
| The same, first button. | The 100 boxes, and in the last row four cells more behind box 100. |
| Move the cursor right from box 100, down from boxes 88..91, up from the R button. | The cursor reaches the four cells (transport box, offline 1..3) and leaves them the same ways. |
| Put the cursor on the first cell with Pokémon in the transport box, then on each offline cell, with no offline files on the SD card. | The upper screen shows the transport box's Pokémon and "Transport Box"; the offline cells read "Box 1", "Box 2", "Box 3", without Pokémon. |
| The same in German, in Spanish and in Japanese. | The offline cells read "Box: 1" / "Caja 1" / "ボックス1" and so on; the transport box "Transferbox" / "Caja de Traslado" / "てんそうボックス". |
| Press START three times; then SELECT three times. | Each press turns the hand blue, green, red, with the box screen's sound for it; SELECT does what it does on the box screen. The hand bobs in every colour. |
| Blue hand: A on a box with Pokémon, move, A on another box. | Lifting and putting down sound as on the box screen; the lifted box has a shadow 5 right and 5 down, as a held Pokémon on the box screen, and its place shows nothing until it is put down. Put down, its shadow is gone one frame after A, together with the put-down sound, and it glides from under the hand onto the box in the next 8 frames, and the box that was there glides at the same time onto the lifted box's place, over the other boxes and under the held box and the hand; B glides it back onto its own place, with the put-down sound one frame after B and no sound on B itself, and the cursor is then on that place. No key does anything while it glides; then the hand bobs again. While it is held, the upper screen keeps showing it, its Pokémon, name and check, wherever the cursor goes; once it is put down, it shows the box now under the cursor. |
| Blue: lift a box still named "Bank 12", move over other boxes, put it onto box 40 named "Fire". | The upper screen reads "Bank 12" with box 12's Pokémon all the way; after putting down it reads "Bank 40" with them. |
| Green hand: A on a box with Pokémon, move over other boxes and the four cells, back over the box, A on an empty box. | The upper screen shows the box under the cursor, with the held Pokémon over it, as high over their slots as the lower box over its cell, without shadows; over their own box that box is empty, its box icon too, and they are not shown twice. The lower screen shows the lifted box empty. Put down (or back with B), the held box glides onto its place in 8 frames and the Pokémon on the upper screen sink into their slots with it. Once put down, the held Pokémon are gone from over the box, and the target shows them in their slots. |
| Red: check a box with Pokémon; blue: lift it, put it onto another box. | The check moves with the box; the lifted checked box (smaller) has the smaller shadow. The counter reads "(1/104)" throughout; checking one of the four cells counts too. |
| Blue: move box 37 onto offline cell 1 (without Pokémon); then green: move Pokémon from a box into offline cell 2 (without Pokémon). | Box 37's name goes with it to the cell; box 37 now reads "Bank 37" (in the console's language, e.g. "Online-Box: 37"). Cell 2 still reads "Box 2", with the Pokémon. |
| Blue: move a box still named "Bank 12" onto offline cell 3 (without Pokémon), then that cell onto box 40 named "Fire". | Cell 3 reads "Box 3"; box 12 reads "Bank 12". Then box 40 reads "Bank 40" and cell 3 reads "Fire". |
| Blue: move a box named "Fire" from a cell onto box 40 named "Water", which has no Pokémon; Y, Yes; open the screen again. | Box 40 reads "Fire"; the cell reads "Water", without Pokémon, also after saving. |
| Copy a session folder's `mover3.bin` of a box named "Box 3" over `/3ds/Pokémon Bank/mover1.bin` of a cell without Pokémon; open the screen; blue: that cell onto box 40, still named "Bank 40" and without Pokémon. | Cell 1 reads "Box 3"; afterwards box 40 reads "Box 3", and cell 1 "Box 1". |
| With each hand, wait without pressing anything; then lift a box and wait; then put it down. | Without anything held the hand bobs, in every colour. When a box is lifted the hand stops bobbing and dips 10 px onto the box in the next frame; in the frame after, the hand is back up and the box lies on its cell with its shadow, its place empty with the green hand and blank with the blue one; one frame later the box is 10 px over its cell, its shadow with it, and stays 10 px over whatever place the cursor goes to, as a Pokémon held on the box screen; the green hand's Pokémon on the upper screen rise with it and stay up while the cursor moves over other boxes; no key does anything during the lift; A on a place without Pokémon plays no sound; once it is put down (or back with B), the hand bobs again. |
| Red, in German, English and Japanese: look at the top line, check a box; START to blue and back. | The title starts 30 px before where the group's name starts with the blue hand, at the same place in every language, and ends before the counter; the counter "(0/104)" at its old place counts the check. |
| Blue and green: move the cursor over boxes of different groups, the transport box, offline box 2 and the button row. | "Gruppe: " ends where the group's name starts, and "index/amount" without brackets follows at the counter's place; the transport box "Transferbox" "1/1", offline box 2 its name "2/3"; on the button row the title and the counter "(0/104)" with brackets again. |
| Blue: lift a box, move it over a box of its group, of another group and an offline box; B. | Over its group: the other box's index; over another group: its own; over the offline box: that box; after B its own place again. |
| Blue: hold L on a box shown 13/13 until it has moved several places, then let go; press L once, briefly. On the box screen, hold L for comparison. | It moves one place before each time, with the sound each time, at the same speed as the box screen turns its pages while L is held; it stops when L is let go. A normal press moves it once. |
| Blue: hold L on a box until it moves; while still holding it, press A, then put the box down; press B, then No; press START twice and back to blue; move the cursor; touch DONE. | A lifts the box under the cursor, and L moves nothing while it is held; put down, the box under the cursor goes on moving. B asks "Would you like to quit without saving?", and after No it goes on moving. START changes the hand, and with blue again it goes on moving. The cursor moves, and L then moves the box it is on. DONE asks its question. |
| Red and green: hold L, and press A or move the cursor meanwhile. | L does nothing, and A and the cursor work as usual once L has been held for more than three frames. |
| Blue, a box with Pokémon in a group of two or more: hold L until the box moves one place, let go, wait a second, press A once. Then tap L four times, briefly, with pauses. | The first A lifts the box. The short taps move nothing: each starts L's count anew. |
| Blue: L on a box shown 13/13, then on 1/13; L in a group of one box. | 12/13 with the box screen's sound for L, and the box that was 12th shows 13/13; 1/13 becomes 13/13, and the boxes that were 2nd to 13th show 1/13 to 12/13, in their old order; in the group of one nothing and no sound. Y after L back round the group: "Quit moving Pokémon?"; after one L, Y, Yes: the box screen shows the new order. |
| Check a cell, move the cursor away and back. | The upper screen shows the cell's check each time. |
| Red: move the cursor from box 100 onto the transport box and on to offline box 1, with the transport box empty, then with Pokémon in it, then checked; then check box 100 and move between it and the transport box. | On the transport box the upper screen's name label is blue, as on the box screen, in the same frame as "Transport Box" appears; on box 100 and offline box 1 it is beige again, also when the box shown is checked. The check icon left of the label shows each box's own state throughout. |
| Green: lift the transport box's Pokémon, move over other boxes and back, then put them back with B from another box. | The label is beige over every other box and blue over the transport box, also while the upper screen turns back to it for the put-back. |
| Blue: swap two boxes of the same category, two of different categories, and one with an empty offline cell; save; open the box screen. | The first two change order within their category; the other two each stay in their own category; the box from the cell's spot keeps the place's category and position. Every category lists all its boxes once. |
| Blue hand: lift a box, A on the transport box. | No sound on A; one frame later the put-down sound, the shadow gone, and the box glides back from the transport box to its place in 8 frames; the upper screen shows the held box throughout; then the refusal sound and "Cannot be transported there." in the bar, without Yes and No, the cursor hidden. No key but A or B does anything, and no touch; A or B closes it with the box screen's sound; the cursor is on the box's place, and nothing has moved. |
| Green hand: lift a box, A on the transport box. | No sound on A, and the cursor stays on the transport box. One frame later the upper screen shows the box the Pokémon came from, with them over it; 10 frames after that the put-down sound, and they sink into their slots in 8 frames while the lower box glides back to its place; then the refusal sound and the message as above, the cursor hidden. After A or B the cursor is on that box, and nothing has moved. |
| Blue: A on the transport box (with Pokémon). Then green: A on it, A on a box without Pokémon. | Blue: nothing happens and nothing sounds, as on a place without Pokémon. Green: its Pokémon are lifted and put down there; the transport box is empty. |
| Blue hand: lift a box, move the cursor onto another box, B. | No sound on B; one frame later the put-down sound, and the box glides back where it was; the upper screen shows it throughout, and the cursor is then on its place. |
| Green hand: lift a box, move the cursor onto another box, B. | No sound on B, and the cursor stays where it is. One frame later the upper screen shows the box the Pokémon came from, with them over it; 10 frames after that the put-down sound, and they sink into their slots in 8 frames while the lower box glides back; the cursor is then on that box. With the cursor on their own box, B puts them down from the frame after it, as A does. |
| Blue hand, then green: lift a box; press START, X, Y, L, R; touch DONE, RESET and ALL; move the cursor onto the button row and press A. | Nothing happens, no sound, no question, no button pressed in; the hand keeps its colour and the box stays in it. The cursor still moves, with its sound, and A on a box and B work. |
| Red: check a box with Pokémon and a cell with Pokémon. Green: A on each. Then blue: A on each, A on another box. | Green: nothing happens and nothing sounds, as on a place without Pokémon. Blue: each is lifted, and put down its check is on the place it went to. Red: uncheck one; then green lifts its Pokémon. |
| Empty every box of Bank (move them offline), Y, Yes; START, Back, first button. | The screen opens, with the offline boxes in their cells. |
| With no offline files on the SD card: blue, A on box 1 (with Pokémon), A on the fourth cell (offline 3). Y, Yes. | "Success! Changes saved.", the start screen. On the SD card `/3ds/Pokémon Bank/mover1.bin`, `mover2.bin` and `mover3.bin` (7,298 bytes each); the first two hold no Pokémon. START, Back, first button: box 1 empty with its name, the fourth cell shows a box with Pokémon. |
| Blue: A on the fourth cell, A on box 50. Y, Yes. | `mover3.bin` is still there, without Pokémon; box 50 holds the Pokémon and the name of the old box 1. |
| Red: check box 2 and the second cell (with a box moved there first). Y, Yes. | A folder `/3ds/Pokémon Bank/YYYYMMDD-HHMMSS/` with `bank02.bin` and `mover1.bin`, its Pokémon in it; `mover1.bin` in the folder above holds no Pokémon; box 2 is unchanged in Bank. |
| Red: check the transport box (with Pokémon). Y, Yes. | The session's folder holds `mover0.bin`; from offset `1B30` it holds "Transport Box" in the console's language, as UTF-16, then zeros. |
| Green: A on a box, A on a box with Pokémon in the same places; then A on a box with Pokémon only in other places. | The refusal sound, no message; nothing moved, the Pokémon stay in the hand, and can be put down elsewhere or back with B. |
| With nothing held: B, with nothing changed, after a move, and with a box checked. | Each time the original's sound and "Would you like to quit without saving?". B or No: the boxes again, as they were, the check and the move still there. Yes after the move: the start screen; START, Back, first button: nothing moved, and no session folder on the SD card. |
| Blue: swap box 1 and box 2, then swap them back; Y. Then swap box 1 to box 2, box 2 to offline box 1 and back round to the start; Y. | "Quit moving Pokémon?" both times. |
| Blue: swap box 1 and box 2 and leave them; Y. | "Would you like to save and quit?". |
| Blue, with boxes A and B of one category and an empty box E of another: A onto E, B onto E, A onto E, then B onto A, so that both are at their places again; Y. | "Would you like to save and quit?": A and B have changed their order in the category. Swapped back the same way, Y: "Would you like to quit without saving?". |
| Blue: name box 13 "Bank 41" (on the box screen) and leave box 41 named "Bank 41"; swap box 13 with box 41 and back; Y. | Box 13 reads "Bank 13" now; "Would you like to save and quit?". |
| Blue: swap a box named by you with box 1, which bears its default "Bank 1", and back, also with an offline box; Y. | "Quit moving Pokémon?". |
| Red: check box 1; blue: swap box 1 with box 5; Y, Yes. | The session's folder holds `bank05.bin` with box 1's Pokémon and name, and no `bank01.bin`. |
| Put boxes down with the blue and the green hand, also onto offline boxes. | No pause at the put-down: the glide starts in the frame after A, as before. |
| With at least 720 Pokémon in the Bank, note the Miles; a few hours later open the transfer screen, move a box of Pokémon into an offline box, Y, Yes; at once open a game. | The Miles of those hours, counted with the Pokémon before the move (720 Pokémon earn one Mile an hour); at the next game's opening nothing more for the hours before the save. |
| With the red hand, then the blue and the green hand holding nothing: put the cursor on box 92 and press Down once, onto the buttons; there press R (ALL), then X (RESET), with a Pokémon in box 92 and box 100 empty. Then lift box 37's Pokémon with the green hand, move onto the buttons and press B. | The upper screen keeps showing box 92 on the buttons, not box 100, checked after ALL and unchecked after RESET. With box 37's Pokémon held it shows box 37 with them over it, and B puts them back at once, with the put-down sound and no wait. |
| Green: move a box's Pokémon into an empty box and back again; Y. | "Would you like to save and quit?". Yes: "Success! Changes saved."; the Pokémon are in their box again. |
| Green: lift a box's Pokémon and put them down with A on their own box; Y. Again from the start with B instead of A; Y. Then with the Transport Box's Pokémon and A; Y. | "Would you like to save and quit?" the first time; "Quit moving Pokémon?" after B and after the Transport Box. |
| Blue, with a box with Pokémon and two empty boxes of their defaults: the box onto the first empty box, from there onto the second, from there back to its place; Y. Then the same with two empty boxes of another category than the box, their order put back with L; Y. | "Quit moving Pokémon?" both times. |
| Change nothing, Y. | "Quit moving Pokémon?". No: the boxes again. Y, Yes: the start screen. |
| Move a box, Y. | "Would you like to save and quit?". No: the boxes again, the move still made. Y, Yes: "Success! Changes saved.". |
| Check box 1 only, with Pokémon in offline box 1; Y; while "Move these Pokémon?" is up, touch where ALL lies, then Yes. | ALL does nothing under the question: the session's folder holds only `bank01.bin`, and offline box 1 keeps its Pokémon. |
| Check a box, Y, once in each language. | "Move these Pokémon?" (German "Soll der Transfer beginnen?"), nothing about Pokémon HOME. No: the boxes again, the box still checked. Y, Yes: "Success! Changes saved." |
| With no question up, touch the middle of the lower screen where Yes and No stand in a question. | Nothing happens and nothing sounds. With a question up, Yes and No work by touch and by A. |
| Copy a `moverN.bin` (1 to 3) from a session folder to `/3ds/Pokémon Bank/` over the file of a cell without Pokémon, open the transfer screen. | The cell shows the box; moving it into Bank works. |
| Copy a session folder's `mover0.bin` to `/3ds/Pokémon Bank/mover1.bin` and a `bankNN.bin` to `mover2.bin` there, over cells without Pokémon. Open the screen; blue: swap a box of Bank with each cell; Y, Yes; open the screen again. | Cell 1 reads "Transport Box" and cell 2 the box's name before the swaps. After saving, the screen opens again without an error and the cells hold the boxes swapped in. |
| Put a file of another size at `/3ds/Pokémon Bank/mover2.bin`, open the transfer screen. | Error `00000015 7B000066`; the file is unchanged. |
| Move a box onto an offline cell, Y, Yes, and switch the console off while the loading screen is up. START. | Box and offline box as before the session, or as after it; never neither. No `interrupted-` folder is left after START; a `YYYYMMDD-HHMMSS-backup/` folder may hold a `.bak` whose Pokémon are also in Bank or a cell. Repeat at different moments. |
| Green: move the transport box's Pokémon (from Transporter) into an empty offline cell, nothing else, Y, Yes, and switch the console off while the loading screen is up. START, open the transfer screen. | The Pokémon are in the transport box or in the cell, never in both and never in neither. No `interrupted-` folder is left after START; a `YYYYMMDD-HHMMSS-backup/` folder may hold a `.bak` whose Pokémon are also in the transport box or the cell. Repeat at different moments. |
| Swap two offline cells only, Y, Yes, and switch the console off while the loading screen is up. START. | Both cells as before the session, or both as after it. Repeat at different moments. |
| A save that moves a box onto an offline cell, without a cut. | No `interrupted-` folder and no `.bak` is left on the SD card afterwards. |
| Create `/3ds/Pokémon Bank/interrupted-00000001-00000002/` on a PC and copy a cell's `mover1.bin` into it as `mover1.bak`. Create an empty `/3ds/Pokémon Bank/interrupted-00000003-00000004/` too. START. | The journal names neither state of the first folder: it is renamed to `YYYYMMDD-HHMMSS-backup/`, with the console's date and time, and holds `mover1.bak`; the empty one is gone and no second dated folder is made; the cells are unchanged. |

## Transporter, feature by feature

| Check | Expected |
| --- | --- |
| Transfer a Gen 5 Box 1 with gaps, then try another transfer before emptying Bank's transport box. Move the delivery into Bank, save, and reopen. | Accepted Pokémon arrive once in their original positions and leave the source. Another delivery is refused while Pokémon remain; it is allowed after Bank saves an empty transport box. |
| Transfer from a supported Gen 5 SD card save. | Only saves of the matching game and language are listed. The correct file changes; its backup is `/3ds/Pokémon Bank/<name>.sav.bak`, and nothing new lies beside the save; unrelated saves stay unchanged. |
| With a White cartridge and a Black 2 save on the SD card, START; take the cartridge out on the list, choose Black 2 and transfer. START again, take the cartridge out on the list and choose White. | The Black 2 transfer runs as usual. White: the original's message that no compatible game is detected. |
| With a White cartridge and a White save on the SD card, START; take the cartridge out on the list and choose White. | The original's message that no compatible game is detected; the save on the SD card is not changed. |
| With a White cartridge and Black and Black 2 saves, take the cartridge out on the checking screen after START. | The list shows both saves and no White. |
| Choose a cartridge game with Pokémon in Box 1, take the cartridge out on the question and answer "Yes". Open Bank. Repeat in Chinese. | Only "Please confirm that your Game Card is properly inserted, and then try again." (in Chinese, the line after the first), then the start screen. Bank's transport box is empty, and the game still holds its Pokémon. |
| The same with a save on the SD card chosen, and with a Virtual Console title. | The transfer runs as usual. |
| Fill the SD card to less than 512 KiB free, delete the `.sav.bak` of a Black/White save on it, choose that save and transfer. | After the game is read: "The SD card is full." on one line, without numbers or "You can't download data.", then the start screen. The save is unchanged. A `.sav.bak` may be there, but incomplete. Free space and transfer: the backup is made, the transfer runs. |
| Transfer from a Gen 1 or Gen 2 Virtual Console game. | The Pokémon arrive and can be withdrawn into a Gen 7 game only. |
| A Box 1 with an Egg or another Pokémon the original refuses. | The original's refusal dialogs for it; the others are offered. No notice about nicknames or OT names. |
| START on a console where the offline Bank was never started. | The checking screen, then the original's "did not complete correctly… open Pokémon Bank" message without the spinner behind it, then the start screen. No game list. |
| START with no game at all, then START again. | The checking screen, then "Could not find a game…" without the spinner behind it, then the start screen; the second START shows the checking screen again. |
| Choose a game whose first box is empty; then choose one with Pokémon and answer "No" at the question. | Both times the original's message, then directly "The program is now closing. Please wait..." (23:0x21); no "Communicating with the Pokémon Bank server..." (23:0x0F) in between. |
| START without a cartridge or SD save: once with only Virtual Console games of the listed language, once with only ones of another language, once with none. | The spinner stops only briefly. The first lists those games as before; the other two end with "Could not find a game that is compatible with Poké Transporter." (23:0x00). Transporter starts normally. |
| Choose another language on the language screen; then Back, and B. | Only that language's games are listed; the logo and start prompt follow it, the other screens stay in the console's language. Back and B leave the choice unchanged. After a restart the console's language applies again. |
| Try HOME and sleep while the list is built, a game is read and the transfer runs, then on the start screen, the list, the question and every message that waits for A. | Refused while Transporter works; they work everywhere else. |

## An interrupted Save and Quit

Use the [test package](building.md#test-builds-for-an-interrupted-save) and restore the starting backup before each case. Its main-menu entry selects the next Save and Quit:

| Entry | What it does |
| --- | --- |
| **Use Pokémon Bank** | Normal save; also settles an interrupted save on reopening. |
| **(before)** | Stops after marking the Bank save in progress, before writing the game. Error `00007E57 00000001`. |
| **(after)** | Stops after writing the game, before clearing the mark. Error `00007E57 00000002`. |
| **(record)** | Writes the journal 150 times before saving normally, for a real power-cut test. |
| **(boxes)** | Writes the inactive boxes file 30 times before saving normally, for a real power-cut test. |

For each case, perform the moves, Save and Quit with the named entry, acknowledge the intentional error, and close Bank. Follow the [recovery steps](bank.md#if-save-and-quit-is-interrupted), then reopen with plain **Use Pokémon Bank**. After recovery, complete a normal save and reopen again to confirm that saving still works.

### The backup of the changed boxes

| Steps | Expected |
| --- | --- |
| With the **(before)** entry, withdraw one Pokémon to the game and deposit another, from two different boxes. Save on in the game with the same trainer, then START Bank. | The two-page message to open Transporter. On the SD card `/3ds/Pokémon Bank/interrupted-XXXXXXXX-XXXXXXXX/` holds `bankNN.old` and `bankNN.new` for the two changed boxes (NN from 01; box 100 is 00), and nothing else; each file is 7,298 bytes. |
| As above, but also take one Pokémon out of the transport box into a box (it needs a Pokémon there from Transporter). | Also `mover0.old` and `mover0.new`; from offset `1B30` each holds zeros, no name. |
| START Bank again. | The same message; the files' dates are unchanged. |
| Open Transporter, START, answer **Yes**, then **Yes** to the final warning. Look at the SD card, then START Bank. | After Transporter the `interrupted-` folder is unchanged. Bank opens with the boxes from before; the `interrupted-` folder is renamed to `YYYYMMDD-HHMMSS-backup/`, with the console's date and time of that START, and holds only the `.new` files, as before. |
| Repeat the first step, open Transporter, START, answer **No**, then **Yes** to the final warning, then START Bank. | Bank opens with the boxes from after; the dated folder holds only the `.old` files. |
| Repeat the first step, then START Bank with the game holding the save before the session (restore it with Checkpoint first). | Bank opens with the boxes from before; as after Transporter's Yes, the dated folder holds only the `.new` files. |
| Repeat the first step, fill the SD card until less than 15 KB is free, then START Bank. | "The SD card is full." on one line, then the start screen; Transporter still refuses with its message to open Bank. Free space and START: the two-page message, and the folder. |

### Before the game is written

Choose **(before)**. “Play and save” below means making a new save in the game before reopening Bank.

| Case | Moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 1 | Deposit one, withdraw one | Reopen with the same game unchanged. | Bank and game are as before the session. |
| 2 | Deposit one | Play and save, then reopen. | Deposited Pokémon is in Bank and still in the game. |
| 3 | Withdraw one | Play and save, then reopen. | Pokémon remains in Bank and is absent from the game. |
| 4 | Deposit one, withdraw one | Play and save, then reopen. | "Your communication with Poké Transporter did not complete correctly during your last session." → A → "Please open Poké Transporter and perform the cleanup process before using Pokémon Bank.", then the start screen; START shows it again. With a cartridge game and another readable compatible game present ([why](#the-cartridge-is-absent)), remove the cartridge and START: the different-copy notice instead. Put it back and START: the message again. |

Then, from the state of case 4:

| Case | Step | Expected |
| --- | --- | --- |
| 4a | Open Transporter and START. On the question press HOME and close Transporter; open it and START again. | The checking screen for about 1.5 s, then "You can try restoring your data, but you may lose the Pokémon currently being transported. Would you still like to try restoring your data?" with Yes and No. HOME works on it; the second START asks again. |
| 4f | From case 4: on the first question look at the selection, press B, then press right. | Yes is selected; B does nothing; right selects No. On the second question No is selected, and B chooses No: "The program is now closing. Please wait..." as soon as the buttons are gone, without the cleanup text before it; the music stops at once, and the start screen follows after about 1.5 s. |
| 4b | Answer Yes, then No on the next question; START again, answer No, then No. | Each time "This is your final warning. The Pokémon you are trying to send could be lost forever. Are you sure you want to try to restore your data?" with Yes and No, then, as soon as the buttons are gone, "The program is now closing. Please wait..." without the cleanup text before it; the music stops at once, and the start screen follows after about 1.5 s. Bank still shows the message to open Transporter. HOME works on the second question too. |
| 4c | Answer Yes, then Yes. Open Bank and the game. | "Communication in your last session did not complete correctly. Performing cleanup process now..." with its spinner for about 1.5 s, then at once "The program is now closing. Please wait..." in the same panel, the music stopping, and after about 1.5 s the start screen. Bank opens: the withdrawn Pokémon is in Bank, the deposited one in the game; nothing lost. |
| 4d | Repeat case 4 from the backup, answer No, then Yes. Open Bank. | The same screens. Bank opens with the deposited Pokémon, which is also in the game; the withdrawn Pokémon is lost. |
| 4e | From case 4: answer Yes, then on the second question press HOME and close Transporter; open it and START. | The first question again; nothing was written. |

**Case 4 leaves a save that only an answer can settle. Use disposable Pokémon.**

### The cartridge is absent

Choose **(before)** with a cartridge game that is not also installed. Keep another readable compatible game available so the scan can reach recovery; with no usable game at all, Bank stops at its no-game notice first.

| Case | Moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 5 | Deposit one | Remove the cartridge and reopen. | Bank opens with the deposited Pokémon; it also remains on the cartridge. |
| 6 | Deposit one, withdraw one | Remove the cartridge and reopen. | The original's different-copy notice names the cartridge's trainer and ID; A returns to the start screen. Reinsert it and START again: Bank and game are as before the session. |

Then, from the state of case 6 before the cartridge goes back:

| Case | Step | Expected |
| --- | --- | --- |
| 6a | START, holding L, A and START from the moment the loading screen is up until it ends. | "Please enter the unlock key:" with eight digits, and the number keyboard. Holding them only in the loading screen's first half second, or not at all, gives the different-copy notice. |
| 6b | Type another number. | "The unlock key is incorrect.", then the code again. HOME does nothing in the keyboard. |
| 6c | Cancel. | The different-copy notice at once, with no loading screen, then the start screen. START without the keys: the notice again. |
| 6d | Repeat 6a and type the code shown. | "Communication in your last session did not complete correctly. Performing cleanup process now..." with its spinner for at least 1.5 s, then "The program is now closing. Please wait..." in the same panel, which does not slide out and in again, the start screen. START: "Your communication with Poké Transporter did not complete correctly during your last session." → A → "Please open Poké Transporter and perform the cleanup process before using Pokémon Bank." START with the keys held: the same message, no unlock screen. |
| 6e | Insert another trainer's cartridge of the game and START. | The same message to open Transporter. Transporter then asks its questions (case 4a). |

From a fresh state of case 6, and of case 4 for 6h and 6i:

| Case | Step | Expected |
| --- | --- | --- |
| 6f | START; hold L, A and START briefly while the game is still being checked, and let go before it ends. | The unlock screen. |
| 6g | START; hold them briefly only just before the loading screen ends. | The unlock screen replaces the loading screen at once, with no pause after the game check. |
| 6h | From case 4 (the game played on): START holding the keys; Cancel. | The unlock screen; after Cancel "Your communication with Poké Transporter did not complete correctly…" at once, then the start screen. |
| 6i | From case 4: START holding the keys; type the code shown; then remove the cartridge and START. | The cleanup screen, the start screen; then the message to open Transporter, not the different-copy notice. |
| 6j | Unlock as in 6d with the cartridge out; put it back, not played on since case 6, and START. | Bank opens without a message, as before the session: the unlock does not keep Bank from settling the save itself. |

### The same game is a cartridge and installed

Choose **(before)**. Both copies must have readable saves of their own.

| Case | Session and moves before stopping | Recovery step | Expected |
| --- | --- | --- | --- |
| 7 | Cartridge: deposit one | Remove it and reopen with the installed copy present. | Bank opens with the deposited Pokémon; it also remains on the cartridge. |
| 8 | Cartridge: deposit one, withdraw one | Remove it and reopen with the installed copy present. | Different-copy notice names the cartridge's trainer and ID. Reinsert it and START: state from before the session. |
| 9 | Installed copy, cartridge out: deposit one, withdraw one | Insert the cartridge and reopen. | Different-copy notice names the installed copy's trainer and ID. Remove the cartridge and START: state from before the session. |
| 9a | Cartridge: deposit one, withdraw one | Reopen with another trainer's cartridge of the same game, then with the session's cartridge played and saved. | First the different-copy notice with the session's trainer and ID, then "Your communication with Poké Transporter did not complete correctly during your last session." → A → "Please open Poké Transporter and perform the cleanup process before using Pokémon Bank." |
| 9b | Cartridge: deposit one, withdraw one | As 9a, once with a Gen 6 game and once with a Gen 7 game; then with a copy of the session's save edited to another secret ID (same name and Trainer ID, for example with Checkpoint and PKHeX), played and saved. | Each as 9a; the edited save gets the different-copy notice, not the message to open Transporter. |

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
| New Transporter, before converting: START. | The "did not complete correctly… open Pokémon Bank" message, then the start screen. No game list. |
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
| Wireless off, no offline Bank: START. | The original's connection message, then its start screen; nothing written. |
| No offline Bank: START, once with a game and once without any; once on a console where Bank was never started. | The original connects and loads, without language screen, the "Precaution for Use" notice, menu, game selection or Pokémon Home question, then `0000D0AD 00000001`. |
| Install the normal Bank and open it. | The boxes, transport box and Miles from the server. |
| Install the unpatched Bank and use it online. | It opens as always: the server is not locked and its boxes are unchanged. This is the check that matters most. |
| Compare the game saves with the backup. | Unchanged, byte for byte. |
