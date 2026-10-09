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
| Language screen | Switches the language of the screens | Chooses which language's games are listed ([below](#choosing-the-language-of-the-games)); the screens stay in the console's language |
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

## Choosing the language of the games

The game list shows the games of **one language** only: the cartridge, the saves on the SD card and the Virtual Console titles alike. The original language screen, reached from the title screen, chooses that language. It no longer changes the language of the screens, which stay in the console's language.

- Until you choose, the games in the console's language are listed. A console set to a language no source game exists in (Chinese, Dutch, Portuguese, Russian) starts with English.
- The choice is not saved, as the original saved nothing either: it lasts until Transporter is closed.
- After a choice, the title screen shows it: its logo and its start prompt appear in the chosen language. Japanese, English and Korean have the same prompt and differ in the logo only. Everything else stays in the console's language.
- The list offers the six languages that are not chosen already, and a **Back** button below them that leaves the screen without changing anything, as B does. The two Chinese entries are gone, because no Gen 1, 2 or 5 game exists in Chinese.
- Japanese is confirmed like every other language; the kana/kanji step is gone, since the screens do not change.
- The Back button is built like the Back buttons of Pokémon Bank's menus, to which this screen belongs: the screen's own list button with the text of the game-selection screen's Back, in the language of the screens, and the return icon in its own colour. It is put together when the screen is opened from parts the original already has; no file of the application is replaced.
- The seven entries are spaced like the buttons of Bank's main menu, with a small gap between them, and centred on the screen.
- The language is the game's, not the save's: for a Gen 5 game the language letter of its game code, for a Virtual Console title the language of the installed title.

## Black/White saves on the SD card

Transporter looks for saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card and offers each one as its own entry in the game list, beside the cartridge and the Virtual Console titles. Only saves in the [chosen language](#choosing-the-language-of-the-games) are looked for. **The cartridge is preferred for its own game:** with, say, German chosen and a German White cartridge inserted, the White entry is the cartridge and a German White save on the SD card is not offered; saves of the other three games still are. A cartridge in another language than the chosen one is not listed.

Only the names GodMode9 gives a cartridge dump are accepted, in TWiLight Menu's default folder:

```text
SD:/roms/nds/saves/POKEMON_B_IRB?01_??.sav     Black
SD:/roms/nds/saves/POKEMON_W_IRA?01_??.sav     White
SD:/roms/nds/saves/POKEMON_B2_IRE?01_??.sav    Black 2
SD:/roms/nds/saves/POKEMON_W2_IRD?01_??.sav    White 2
```

- The first `?` is the language letter of the cartridge: `J` Japanese, `O` English, `F` French, `I` Italian, `D` German, `S` Spanish, `K` Korean. For example `POKEMON_W2_IRDD01_00.sav` is a German White 2 and is listed while German is chosen.
- `??` is the cartridge revision, `00` or `01`. If both exist for the same game and language, only `01` is used.
- Any other name, folder or spelling is ignored. A file that is not exactly 524,288 bytes is ignored.
- At most four saves are offered, one per game, and the whole list holds 40 games, as in the original.

What happens to the file:

- It is read and checked by the original code exactly as a cartridge's save would be; a damaged save is refused by the original.
- Before the first change in a session, the untouched save is copied to `<name>.sav.bak` beside it (one copy, replaced the next time). If that copy cannot be written, nothing is changed.
- The Pokémon are removed from the file only after the delivery to Bank has been written and verified, as with a cartridge.
- The file is never created, resized or renamed.

Do not insert or remove a cartridge while a session is running; what is offered is decided when you leave the title screen, with the language chosen at that moment.

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

## Troubleshooting

| What you see | Likely cause |
| --- | --- |
| Transporter does not start, or stops while loading. | `code.ips` and `exheader.bin` are not from the same release or build, or Transporter is not version 1.5. |
| Every transfer is refused with "not empty". | Open Bank, empty the transport box, and Save and Quit. If Bank's transport box is empty and saved and the refusal stays, Transporter cannot open Bank's data on this setup: check that Luma3DS is v10.0 or later, and otherwise please report it. |
| The original failure message after confirming a transfer. | The delivery could not be written. The source game was not changed. |
| A game is not listed: cartridge, save on the SD card or Virtual Console title. | It is in another language than the one whose games are listed. Choose its language on the language screen. |
| A save on the SD card is not listed. | Its language is not the chosen one; a cartridge of the same game in that language is inserted; the name or folder differs from the pattern above; or the file is not 524,288 bytes. |
| The language screen does not change the language of the screens. | Intended: it chooses the games. The screens follow the console's language. |
| The last entry of the language list shows Chinese text instead of Back. | The screen's layout is not the one the patch knows, so it was left as it is. Transporter is not version 1.5, or its files are modified. The entry still works as Back. |
| A save on the SD card is reported as damaged. | The original's own check failed on the file's contents, as it would on a cartridge. |

**Reporting a problem.** Open an issue with the console model, Luma version, the release or package name, the source game, which screen was showing when it stopped, and whether the source game still holds the Pokémon.

## Limits

- **Wording of refusals.** The original app has no text for "open Bank first" or "Bank not set up", so every refusal uses the "not empty" message.
- **Access to Bank's data and to the SD card.** Transporter's own header grants neither. Luma3DS gives every application full filesystem access, which is what makes both work; this needs Luma3DS v10.0 or later.
- **Saves on the SD card.** Only revisions `00` and `01` are looked for, one save per game in the chosen language. The search adds a short moment to the loading screen.
- **Language of the screens.** It can no longer be changed inside Transporter; it is the console's.
