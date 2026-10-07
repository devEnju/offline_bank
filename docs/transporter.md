# Poké Transporter Offline Patch

A Luma3DS patch for **Poké Transporter 1.5** (title `00040000000C9C00`, TMD version 5200) that works without Nintendo's servers and delivers into the transport box of the [offline Bank](bank.md).

How it is built is in [building.md](building.md); how it works inside is in [internals.md](internals.md#transporter).

## What changes

| Step | Original Transporter | Patched |
| --- | --- | --- |
| Connect, disconnect | Nintendo's servers | Skipped; wireless can stay off |
| Read the source game and convert the Pokémon | Transporter itself | Unchanged |
| Legality check | Server | None. Whatever Transporter itself accepts is transferred (eggs, for example, are still refused). |
| "Is Bank's transport box empty?" | Server | Reads `/transport.bin` in Bank's extdata |
| Store the Pokémon in Bank | Server | Writes one delivery into `/transport.bin` |
| Remove the Pokémon from the source game | Transporter itself | Unchanged |
| Source of a Gen 5 game | Cartridge | Cartridge and saves on the SD card ([below](#blackwhite-saves-on-the-sd-card)) |
| Notice that nicknames and OT names with prohibited words will be erased | Shown; the server erased them | Skipped. No name is ever changed offline. The other notices are unchanged. |
| HOME and sleep | Refused from connecting until disconnecting | Refused from choosing a game until the session ends |
| Screens and messages | | The originals; no new text |

Gen 5 games produce Gen 6 Pokémon; Gen 1 and Gen 2 Virtual Console games produce Gen 7 Pokémon, as with the original service.

## Transfers and refusals

**Nothing is converted by the patch.** The original Transporter builds a complete Bank transport box in memory and used to upload it. The patch writes that box, unchanged, into Bank's file. The next time you open Bank, the Pokémon are in the transport box, in the positions they had in Box 1 of the source game.

**Order of the two writes.** The delivery is written, flushed, and read back first. Only then does the original remove the Pokémon from the source game. A power cut in between leaves the Pokémon in both places, never in neither. If the delivery cannot be written, the original failure message is shown and the game is not touched.

**When Transporter refuses.** It shows its original "Bank's transport box is not empty" message and removes nothing when:

- the transport box in Bank still holds Pokémon,
- an earlier delivery has not been picked up,
- Bank has an unfinished save (open Bank and Save and Quit once),
- the offline Bank has never been started on this console, or
- Transporter cannot open Bank's data.

Transporter never creates files and touches no Bank file other than `/transport.bin`.

## Black/White saves on the SD card

Transporter looks for saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card and offers each one as its own entry in the game list, beside the cartridge and the Virtual Console titles. **The cartridge is preferred for its own game and language:** with, say, a German White cartridge inserted, the German White entry is the cartridge and a German White save on the SD card is not offered. Saves of White in other languages and saves of the other three games still are.

Only the names GodMode9 gives a cartridge dump are accepted, in TWiLight Menu's default folder:

```text
SD:/roms/nds/saves/POKEMON_B_IRB?01_??.sav     Black
SD:/roms/nds/saves/POKEMON_W_IRA?01_??.sav     White
SD:/roms/nds/saves/POKEMON_B2_IRE?01_??.sav    Black 2
SD:/roms/nds/saves/POKEMON_W2_IRD?01_??.sav    White 2
```

- The first `?` is the language letter of the cartridge (`J`, `O`, `F`, `I`, `D`, `S` or `K`); for example `POKEMON_W2_IRDD01_00.sav` is a German White 2. Different languages of the same game are separate entries.
- `??` is the cartridge revision, `00` or `01`. If both exist for the same game and language, `01` is used.
- Any other name, folder or spelling is ignored. A file that is not exactly 524,288 bytes is ignored.
- At most eight saves are offered, and the whole list holds 40 games, as in the original.

What happens to the file:

- It is read and checked by the original code exactly as a cartridge's save would be; a damaged save is refused by the original.
- Before the first change in a session, the untouched save is copied to `<name>.sav.bak` beside it (one copy, replaced the next time). If that copy cannot be written, nothing is changed.
- The Pokémon are removed from the file only after the delivery to Bank has been written and verified, as with a cartridge.
- The file is never created, resized or renamed.

Do not insert or remove a cartridge while a session is running; what is offered is decided when you leave the title screen.

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

## Checking that it works

### Transfers

| Do this | Expected |
| --- | --- |
| Gen 5 cartridge, Box 1 with gaps, Bank's transport box empty: transport. | The spinner keeps animating while the box is read, while Bank is checked, and while the Pokémon are written. The original "transported" message appears. The Pokémon are gone from the source game. Bank shows them in the transport box, in the same positions. |
| In Bank, move them into a box, Save and Quit, restart Bank. | They are in the box. A Gen 6 or Gen 7 game can withdraw them. |
| Gen 1 or Gen 2 game. | The Pokémon arrive as Gen 7 Pokémon and cannot be withdrawn into X, Y, Omega Ruby, or Alpha Sapphire. |
| A Box 1 that was never used, or Pokémon with never-used slots between them. | No message about Pokémon that cannot be transported. An empty box gives only the "nothing to transport" message. |
| A Pokémon the original's own checks refuse (for example an Egg). | Three original dialogs in a row: one that a Pokémon cannot be sent, one with the reason, one that it was removed from the Transport Box. The others are offered. |
| An egg in Box 1 beside other Pokémon. | The original dialog about Pokémon that cannot be transported appears for the egg; the others are offered. |
| Any transfer. | The notice about nicknames and OT names does not appear; the notices about not being able to return Pokémon and about held items do. |
| Press HOME and close the lid after choosing a game, at every screen until the title screen is back. | Nothing happens. Both work on the title screen. |

### Refusals

| Do this | Expected |
| --- | --- |
| Transport again without opening Bank in between. | Refused with the "not empty" message. Nothing is removed from the source game. |
| Leave some Pokémon in the transport box, Save and Quit, transport again. | Refused until the transport box is empty and saved. |
| Empty Box 1, Bank's transport box empty. | The original message that there is nothing to transport, then the title screen. |
| Empty Box 1, Bank's transport box not empty. | The "not empty" message. |

### Saves on the SD card

| Do this | Expected |
| --- | --- |
| No cartridge, one accepted save. | The game appears in the list with its trainer name. A transfer works as from a cartridge; afterwards `<name>.sav.bak` holds the save as it was, and the game started in TWiLight Menu shows the Pokémon gone from Box 1. |
| No cartridge, several accepted saves (different games or languages). | One entry per save; the one you pick is the one read and changed. |
| A Gen 5 cartridge inserted, saves on the SD card. | The games are listed in the order Black, White, Black 2, White 2; the cartridge stands at its game's place, the saves at theirs. A save of the cartridge's own game and language is not offered; the same game in another language is. Picking the cartridge reads and changes the cartridge; picking a save reads and changes that file. |
| A save with another name, or a file of another size, in the folder. | It is not offered. |

## Troubleshooting

| What you see | Likely cause |
| --- | --- |
| Transporter does not start, or stops while loading. | `code.ips` and `exheader.bin` are not from the same release or build, or Transporter is not version 1.5. |
| Every transfer is refused with "not empty". | Open Bank, empty the transport box, and Save and Quit. If Bank's transport box is empty and saved and the refusal stays, Transporter cannot open Bank's data on this setup; please report it. |
| The original failure message after confirming a transfer. | The delivery could not be written. The source game was not changed. |
| A save on the SD card is not listed. | A cartridge of the same game and language is inserted; the name or folder differs from the pattern above; the file is not 524,288 bytes; or more than eight saves are present. |
| A save on the SD card is reported as damaged. | The original's own check failed on the file's contents, as it would on a cartridge. |

**Reporting a problem.** Open an issue with the console model, Luma version, the release or package name, the source game, which screen was showing when it stopped, and whether the source game still holds the Pokémon.

## Limits

- **Wording of refusals.** The original app has no text for "open Bank first" or "Bank not set up", so every refusal uses the "not empty" message.
- **Access to Bank's data.** Transporter's own header grants no access to Bank's extdata. Opening it worked under Luma on one console; other setups are untested.
- **Saves on the SD card.** Only revisions `00` and `01` and at most eight saves are looked for. The search adds a short moment to the loading screen. The list holds 40 games, the original's limit; if cartridge, saves and installed Virtual Console titles together are more, the last Virtual Console titles are left out.
