# Pokémon Bank and Transporter Offline Patch

> **This project was developed entirely with AI assistance:** analysis, code, tests, and documentation. It's tried on a real console, but it has not had the review or testing that a conventionally developed tool would have. Use it at your own risk and keep backups.

Two Luma3DS patches that make **Pokémon Bank 1.5** and **Poké Transporter 1.5** work without Nintendo's servers. Bank keeps its boxes on the SD card; Transporter moves Pokémon from Gen 1, 2 and 5 games into Bank's transport box to prevent Gen 6 and 7 games to be cut off after the official servers go offline.

**Status: development build.** It has been tried on one console only. Back up your game saves and Bank's extra data before trying it.

## Pokémon Bank

- All 100 boxes, with the original box, search, and group screens.
- Transfers to and from X, Y, Omega Ruby, Alpha Sapphire, Sun, Moon, Ultra Sun, and Ultra Moon.
- Starts straight at game selection; no main menu, no connection.
- National Pokédex and adventure records, updated at Save and Quit.
- Poké Miles earned locally at the original rate and redeemed through the original screens.
- Saves are journaled: an interrupted save leaves either the old or the new state.

Details: [docs/bank.md](docs/bank.md).

## Poké Transporter

- Works with wireless off: connection and server checks are removed.
- Reads Gen 5 cartridges and Gen 1/2 Virtual Console games and converts the Pokémon with Transporter's own code.
- Also lists Black, White, Black 2 and White 2 saves that TWiLight Menu++ or an nds-bootstrap forwarder keeps on the SD card, beside a cartridge and the Virtual Console titles ([details](docs/transporter.md#blackwhite-saves-on-the-sd-card)).
- The language screen chooses which language's games are listed, so the list stays short ([details](docs/transporter.md#choosing-the-language-of-the-games)).
- Writes the Pokémon into Bank's transport box first, then removes them from the source game. A power cut in between never loses a Pokémon.

Details: [docs/transporter.md](docs/transporter.md).

## Download

Releases are published on the [Releases page](../../releases) as one file, `offline_bank-<version>.zip`, laid out like the SD card:

```text
luma/titles/00040000000C9B00/code.ips        Pokémon Bank
luma/titles/00040000000C9B00/exheader.bin
luma/titles/00040000000C9C00/code.ips        Poké Transporter
luma/titles/00040000000C9C00/exheader.bin
README.txt
```

The repository itself holds no patch files. Without a release, [build them yourself](docs/building.md).

## Install

You need a 3DS with [Luma3DS](https://github.com/LumaTeam/Luma3DS) v10.0 or later, and Pokémon Bank and Poké Transporter installed in version 1.5, their final updates.

1. Back up your game saves and Bank's extra data. The boxes are in Bank's *extdata*, not in its save ([how to back them up](docs/bank.md#backing-up-the-bank)).
2. Extract the zip and copy its `luma` folder to the root of the SD card, merging it with the one that is there.
3. Hold SELECT while powering on and switch on *Enable game patching*.
4. Start Bank once before using Transporter.

The two files of a title only work as a pair; never mix files from different releases. The guides for [Bank](docs/bank.md#install) and [Transporter](docs/transporter.md#install) have the details, a checklist, and troubleshooting.

## Documentation

- [docs/bank.md](docs/bank.md) and [docs/transporter.md](docs/transporter.md): using each patch. What changes, install, checklist, troubleshooting, limits.
- [docs/internals.md](docs/internals.md): how they work, in three parts: Bank, storage, Transporter.
- [docs/building.md](docs/building.md): building from source, verifying, and packing a release.
- [docs/testing.md](docs/testing.md): the checks that need a console, as steps and what is expected to be seen.

## FAQ

### Why can't I see some of my Virtual Console titles?

Transporter lists the games of one language at a time, and starts with the console's language. As an example, Pokémon Green exists in Japanese only. Press L on Transporter's title screen to open the language screen and choose Japanese; the game list then shows the Japanese titles. The same goes for any other game in a language that is not the console's: cartridge, save on the SD card, or Virtual Console title.

The choice lasts until Transporter is closed. More in [docs/transporter.md](docs/transporter.md#choosing-the-language-of-the-games).
### Can I get my Pokémon from the official Pokémon Bank?

While the official service runs, yes. It ends on February 25, 2027. A release made before then ships a separate download patch: you start Bank with it once, it signs in as the original does, fetches your boxes into a new offline Bank and shows two numbers. Then you put the normal patch in its place. It only works when no offline Bank is on the SD card yet, it does not fetch Poké Miles, and your Pokémon stay on the server as well. The steps are in the `README.txt` of its zip.

### Why do I need a migration between two versions?

The normal patches read only the files of their own version, so that they stay free of code for earlier formats. Such code would never go away, and it would sit on the path that loads and saves your Pokémon.

When a release changes how the Bank is stored, as v0.3.0 did, a separate migration patch converts the Bank once. You start Bank with it one time and then put the normal patch in its place. The release that needs one says how ([background](docs/building.md#the-migration-from-v021)).

### Why has the save format been changed?

So that a power cut cannot cost you the Bank. Up to v0.2.1 a cut during Save and Quit could leave the one Bank file unreadable, and with it every box. Since v0.3.0 everything is stored in pairs of files, and a save only writes into the file of a pair that is not in use. A cut can then only damage a file Bank does not need, and the next start goes on with the other one ([details](docs/bank.md#where-the-bank-is-stored)).

## Problems and feature requests

**Something does not work or behaves oddly?** Please [open an issue](../../issues) in this repository. Include:

- what you did, step by step, and what happened instead of what you expected,
- the two numbers of any error shown in Bank, or a photo of that screen,
- which games were involved, and whether cartridge, digital, or a save on the SD card,
- the release you used, your console model, and your Luma3DS version.

With enough information I will probably look into it, but this is a hobby project and I cannot promise a fix or a date.

**Feature requests** will most likely not be taken up. The project is open source under the GPL: if you want it to do more, you are welcome to fork it.

### Not included

Pokémon HOME, purchases, importing a Bank from Nintendo's servers, legality checks (they existed only on the server), event gifts, and import or export tools for save editors.

## Credits

The Transporter patch would not exist without [zaksabeast](https://github.com/zaksabeast). He worked out how Poké Transporter works inside, showed that it can run offline, and published all of it openly. The patches in the following repositories are not compatible with the ones released here and cannot be combined with them, but this project builds on what they worked out, so they are listed here with many thanks to him:

- [Transporter-Offline-Patch](https://github.com/zaksabeast/Transporter-Offline-Patch): the edits that let Transporter run without servers. The offline flow here follows them.
- [Transporter-PKSM-Bank-Patch](https://github.com/zaksabeast/Transporter-PKSM-Bank-Patch): his notes on Transporter's state machine, and the points where a patch can take over the Bank check and the transfer.
- [DreamRadarCartRedirect](https://github.com/zaksabeast/DreamRadarCartRedirect): the functions through which Transporter reads and writes a DS cartridge, and the idea of pointing them at a file. Reading saves from the SD card is built on it.

## Licence and disclaimer

The source code of this project is licensed under [GPL-3.0-or-later](LICENSE).

**This is an unofficial, non-commercial fan project.**

- It is **not affiliated with, endorsed, sponsored, or approved by** Nintendo Co., Ltd., The Pokémon Company, GAME FREAK inc., Creatures Inc., or any of their affiliates. It is not an official product or service of any of them and does not restore or replace one.
- Pokémon, Pokémon Bank, Poké Transporter, Nintendo 3DS, and all related names are trademarks of their respective owners. They are used here only to say what the patches are for.
- The repository contains no game files, no Nintendo code, and no keys. To build the patches you must supply your own copies of software you legally own.
- The patches only work on a console that already runs custom firmware, with your own installed copies of the two applications. They do not provide, unlock, or pay for any software or service.
- The software is provided as is, without warranty of any kind, as set out in the licence. You use it at your own risk; the author is not responsible for lost Pokémon, damaged saves, or any other damage. Whether using it is permitted where you live is your own responsibility.
