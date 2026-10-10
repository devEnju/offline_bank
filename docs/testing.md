# Testing on a console

The PC tests ([building.md](building.md#tests)) cover the storage format and every rule of a save, including a power cut at each write on a simulated card. What they cannot show is the console itself: its file system, the original programs, and a real power cut. This page lists the checks that need a console, as steps and what must be seen.

It is a list of procedures only. Nothing here records which of them were run, on what, or with which result.

## Before you start

1. **Back up** Bank's extdata and the saves of the games you will use, with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint) ([how](bank.md#backing-up-the-bank)). Some runs leave the Bank and a game in a state that only the backup undoes.
2. **Use Pokémon you can lose** or have twice. A few runs lose or duplicate one on purpose.
3. **Switching packages.** Each package is a folder with `code.ips` and `exheader.bin`. Copy both to `SD:/luma/titles/00040000000C9B00/` for Bank, or `SD:/luma/titles/00040000000C9C00/` for Transporter, replacing the two files there.
4. **Finish what a package began.** A Save and Quit that a test package left in progress must be settled with that same package, or the backup restored, before another package is tried.

| Package | Built by | Folder |
| --- | --- | --- |
| Bank | `Build-BankPatch.ps1` | `build/bank/<package>/` |
| Transporter | `Build-TransporterPatch.ps1` | `build/transporter/<package>/` |
| `stop-before-game`, `stop-after-game`, `tear-record`, `tear-boxes` | `Build-BankTestPatches.ps1` | `build/bank-test/<name>/` |
| Migration, when a release has one | `Build-BankMigrationPatch.ps1` | `build/bank-migrate/<package>/` |

What the four test packages do is in [building.md](building.md#test-builds-for-an-interrupted-save).

## Which tests after which change

| What changed | Run |
| --- | --- |
| Anything, before a release | [Normal use](#normal-use) |
| How or where files are stored, or the file access | [Normal use](#normal-use), [A power cut during a save](#a-power-cut-during-a-save) |
| The rules for an interrupted save | [An interrupted Save and Quit](#an-interrupted-save-and-quit) |
| The Transporter patch, or the transport box files | Run 3 of [Normal use](#normal-use) |
| How or where files are stored, with a migration for it | [Updating from the version before](#updating-from-the-version-before) as well |
| A migration patch | [Updating from the version before](#updating-from-the-version-before) |

## Normal use

With the normal Bank and Transporter packages.

| Run | Do | Must show |
| --- | --- | --- |
| 1 | Start Bank on a console whose Bank extdata is empty | The welcome, then an empty Bank |
| 2 | Pick a game, move Pokémon both ways, Save and Quit. Open the same game again in the same session, move again, Save and Quit a second time. Close Bank and start it again. Once with an installed game and once with a cartridge, if you have both. | Everything where it was put, in Bank and in the game; nothing twice; Pokédex and Poké Miles as expected |
| 3 | Transfer from a Generation 5 game with Transporter. Back on its title screen, press START again. Then open Bank, take the Pokémon out of the transport box, Save and Quit. Start Transporter again and press START. | The transfer completes. The second START answers with the "Pokémon remains in the Transport Box" message and returns to the title screen, without a game list. In Bank the Pokémon are there once. Afterwards START leads to the game list again. |

## An interrupted Save and Quit

The two stop packages end Save and Quit at a fixed point with an error on purpose. Press A, close Bank, do the "Then" step, and start Bank again. What Bank decides is explained in [bank.md](bank.md#if-save-and-quit-is-interrupted).

### The game keeps its old save

Package `stop-before-game`. Save and Quit shows `00007E57 00000001`.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 4 | deposit one, withdraw one | start Bank, the game still there | boxes as before the session; the game unchanged |
| 5 | deposit one | play the game, save, start Bank | the deposited Pokémon in Bank; it is also still in the game |
| 6 | withdraw one | play the game, save, start Bank | the Pokémon still in Bank; not in the game |
| 7 | deposit one, withdraw one | play the game, save, start Bank | the deposited one in Bank and in the game; the withdrawn one is gone |

Run 7 is the one case in which a Pokémon is lost by design; use one you do not need.

### The game is not inserted

Still `stop-before-game`, with a cartridge game that is not also installed.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 8 | deposit one | take the cartridge out, start Bank | Bank opens; the deposited Pokémon is in Bank, and still on the cartridge |
| 9 | deposit one, withdraw one | take the cartridge out, start Bank | error `00000007`; with the cartridge back in: boxes as before the session |

### The same game as cartridge and installed

Still `stop-before-game`. This needs one game both as a cartridge and installed, each with a save of its own. An installed copy that was never played has no save, and Bank does not find it.

| Run | Session on | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- | --- |
| 10 | cartridge | deposit one | take the cartridge out, start Bank | Bank opens; the deposited Pokémon is in Bank, and still on the cartridge |
| 11 | cartridge | deposit one, withdraw one | take the cartridge out, start Bank | error `00000007`; with the cartridge back in: boxes as before the session |
| 12 | installed copy, cartridge out | deposit one, withdraw one | put the cartridge in, start Bank | error `00000007`; with the cartridge out again: boxes as before the session |

If run 11 or 12 opens Bank with the new boxes and no error, Bank did not tell the cartridge from the installed copy.

### The game has its new save

Package `stop-after-game`. Save and Quit shows `00007E57 00000002`.

| Run | In Bank, then Save and Quit | Then | Bank must show |
| --- | --- | --- | --- |
| 13 | deposit one, withdraw one | start Bank, the game still there | both moves done, in Bank and in the game |
| 14 | withdraw one | play the game, save, start Bank | the Pokémon back in Bank; it is also in the game |

## A power cut during a save

Packages `tear-boxes` and `tear-record`. They save normally, but first write for a while, so that a real cut lands inside a write. The saving screen stays clearly longer than usual.

For each of the two packages, three to five times:

| Step | Do | Must show |
| --- | --- | --- |
| 1 | Open Bank, pick a game, move one Pokémon, Save and Quit | the saving screen |
| 2 | About five seconds in, cut the power for real | |
| 3 | Start Bank | Bank opens, with the boxes as before step 1 |
| 4 | Move one Pokémon, Save and Quit, and let it finish. Close Bank and start it again. | the save completes, and the change is there |

How to cut the power, best first: pull the SD card, if the model lets you reach it while it runs; take out the battery; hold POWER. Holding POWER is the weakest, because the console may shut down in order and nothing is interrupted.

Step 3 shows that a cut damaged nothing that was still needed. Step 4 shows that the file the cut left behind is replaced by the next save.

Pulling a card that is being written is at your own risk. Only Bank's own files are being written at that moment; the backup is what protects everything else.

## Updating from the version before

A release that changes how the Bank is stored ships one migration patch, for the Bank of the one version before it ([building.md](building.md#release)). The normal patches never read the earlier files; they have to refuse them, and the migration has to convert them. These runs check both. *Old* is the version before, *new* the release.

Start from a console whose Bank was last used with the old version, and from a backup of it: after run 17 the old version cannot open the Bank any more.

| Run | Installed | Do | Must show |
| --- | --- | --- | --- |
| 15 | new Bank | start Bank, before converting | error `00000003` with the size of the file it refused as second number; nothing changed |
| 16 | new Transporter | start Transporter and press START, before converting | the "did not complete correctly… open Pokémon Bank" message right after START, then the title screen; no game list appears |
| 17 | migration | start Bank, press START | a loading screen, then `0000600D 00000001` |
| 18 | migration | the same again | `0000600D 00000002` |
| 19 | new Bank | start Bank | boxes, Pokédex, Poké Miles and transport box as they were under the old version |
| 20 | new Bank and new Transporter | run 3 of [Normal use](#normal-use) | as there |

Runs 15 and 16 are what keeps the normal patches free of code for earlier versions: an old Bank must be refused by both, with nothing written, and Transporter must stop before it reads any game.

A migration patch never opens the Bank. Its result is always two numbers:

| Numbers | Meaning |
| --- | --- |
| `0000600D 00000001` | converted |
| `0000600D 00000002` | already converted; nothing changed |
| `0000600D 00000000` | no Bank found |
| `00000BAD 00000007` | the old Bank has a Save and Quit in progress; the old version has to finish it |
| `00000BA1` to `00000BA4` | a step failed; the second number says why. Starting it again must be safe. |

That a migration refuses a Bank with a Save and Quit in progress, and that a cut at any of its steps is picked up by the next start, is covered by its PC tests; released versions have no package that stops a save on purpose.

## If something else shows

Stop there. Photograph the screen with both numbers, note the run and the package, and restore the backup before trying anything else. The first number says what failed and the second why ([bank.md](bank.md#troubleshooting)).

## Afterwards

Restore the extdata and the game saves from the backup, and put the normal packages back.
