# Building and releasing

How to build the two patches from source, check them, and make a release. Everything runs on a PC; no script touches a console, an SD card, or the CIA files you supply.

## Requirements

- Windows with PowerShell 7.
- Rust: the stable toolchain and `nightly-2026-10-03` with `rust-src` (the nightly builds the ARM code).
- Your own decrypted update CIAs in `input/`, and `ctrtool.exe` in `tools/`. Both folders are git-ignored.

  ```text
  input/Bank-v1.5-update.cia
  input/Transporter-v1.5-update.cia
  tools/ctrtool.exe
  ```

The builder identifies the inputs by hash and refuses any other version. The expected hashes are listed in [internals.md](internals.md) under the Scope of each patch.

## Build

```powershell
./scripts/Build-BankPatch.ps1
./scripts/Build-TransporterPatch.ps1
```

Each script unpacks its CIA, compiles the injected code, writes a package, and verifies it:

```text
build/bank/           code.ips, exheader.bin, manifest.json, report.md, verification.json, PACKAGE.txt
build/transporter/    the same
```

A folder holds one package, the latest: the next build replaces it. `PACKAGE.txt` holds the package's name, the first 16 hex digits of a hash over the compiled code and the builder's profile of that patch (`crates/patch-builder/src/bank15.rs` with `bank15/offline.rs`, or `transporter15.rs`, which list the edits of the original), so the same source built in the same folder gives the same name, and any change to the patch gives a new one. The folder's path goes into the compiled code: a build in another folder has another name. `./scripts/Get-PackageId.ps1 -Kind bank` prints it. The other packages for Bank are built the same way: the test package into `build/bank-test/`, the migration into `build/bank-migrate/`, the download into `build/bank-download/` (all below).

## Verify

```powershell
./scripts/Verify-BankPatch.ps1
./scripts/Verify-TransporterPatch.ps1
```

The build scripts run these themselves. The verify scripts share no code with the builder. They apply `code.ips` to the original themselves and check that only the listed locations and the added memory changed and that `exheader.bin` differs from the original only in its three size fields. They also check that the package is the one of the current sources, by its name. On success they write `verification.json` into the package. This checks the files, not the behaviour on a console.

## Tests

```powershell
cargo +stable test --workspace --locked --offline
cargo +stable clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +stable fmt --all -- --check
```

The tests run on the PC and cover the storage format, a power cut injected at every write of a save, Miles rules, routing, deliveries into the transport box, the SD-save rules, and the builder. What only a console can show is in [testing.md](testing.md).

### Test builds for an interrupted save

Recovery after a power cut cannot be triggered on demand on a console, so one extra Bank package exists for rehearsing it. It is built behind a cargo feature that is off by default and is never part of a release.

```powershell
./scripts/Build-BankTestPatches.ps1
```

This builds and verifies it into `build/bank-test/`. Its start screen says "Ver. 1.5-test". It is the offline patch with three differences:

- After START it shows the original's main menu, which the offline patch skips. The button chosen there opens the Bank and says what the next Save and Quit of that session does. All five buttons say "Use Pokémon Bank" in the console's language, and the four rehearsals have a word in brackets behind it, the same in every language. Back on the game selection leads to the menu again.
- The reward dialogs never come, and nothing is claimed, so that a run is short.
- While the menu is on screen Bank's files are open, and HOME works there; closing Bank from the HOME Menu at that point writes nothing.

The buttons, and the runs to make with them, are in [testing.md](testing.md#an-interrupted-save-and-quit). "(before)" and "(after)" stop Save and Quit at a fixed point and test what the next start decides ([bank.md](bank.md#if-save-and-quit-is-interrupted)); the plain "Use Pokémon Bank" of the same package settles what they left. "(record)" and "(boxes)" write for a while before they save, which gives a real power cut a long time to land inside a write; a stop cannot produce a half-written file.

The PC tests run the same loops under a simulated cut at every write:

```powershell
cargo +stable test -p bank-common --features test-build --release --locked --offline
```

## Other patches for Bank

Two patches do one thing each and are not the offline patch. Each has its own crate, builder profile and script, and nothing depends on either.

### The migration from v0.2.1

Version 0.2.1 stored the Bank in four files; since v0.3.0 it has ten ([internals.md](internals.md#files)). The step from the one to the other has a patch of its own, [patches/bank-migrate](../patches/bank-migrate/src/), which converts the Bank and does nothing else ([internals.md](internals.md#patches-for-bank)). The offline patch has no code for the earlier files.

```powershell
./scripts/Build-BankMigrationPatch.ps1
```

This builds it into `build/bank-migrate/` and verifies it (`Verify-BankPatch.ps1 -Patch migrate`). Its PC tests run with the others; among them a cut at every step of the conversion.

It belongs to that one step and ships with the releases that follow it while users of v0.2.1 are expected ([Release](#release)). Nothing depends on it: when the step is behind, the crate, `bank15/migrate.rs`, the script, and the migration lines of `Verify-BankPatch.ps1`, `Get-PackageId.ps1`, `Build-BankPayload.ps1` and `New-Release.ps1` can be deleted.

This is the form for every change of how the Bank is stored: the patches read only the current files and refuse earlier ones, and one migration patch of this shape converts the Bank of the version before. Its result is always two numbers under the original's line about the HOME Menu, and the runs that check a migration are the same each time ([testing.md](testing.md#updating-from-the-version-before)).

### The download from the official server

While the official service runs (it ends on February 25, 2027), a patch of its own, [patches/bank-download](../patches/bank-download/src/), fetches the Bank from the official server into a new offline Bank and does nothing else ([internals.md](internals.md#the-download-patch)).

```powershell
./scripts/Build-BankDownloadPatch.ps1
```

This builds it into `build/bank-download/` and verifies it (`Verify-BankPatch.ps1 -Patch download`). Its start screen says "Ver. 1.5-download". Everything that talks to the server is the original's own code, so none of that runs on the PC; the PC tests cover its routing against the original's router, and the writing of the files with a cut at every write.

**It has not been run against the official server.** It was written from the original's code alone; the checks made there are listed in [internals.md](internals.md#the-download-patch). Its guide in the release says so.

It ends, like the migration, on two numbers under the original's line about the HOME Menu:

| Numbers | Meaning |
| --- | --- |
| `0000D0AD 00000001` | downloaded and written |
| `0000D0AD 00000002` | an offline Bank was on the card already; nothing was contacted or changed |
| `00000BAD 00000001` | the server holds no Bank for this console; nothing was created or changed |
| any other pair | the files could not be read or written; the numbers are the offline Bank's ([bank.md](bank.md#troubleshooting)) |

A message of the original on the way (no connection, maintenance, the server locked for a while) leads back to its start screen as it always did; nothing is written then.

Nothing depends on it: when the service has ended, the crate, `bank15/download.rs`, the script, and the download lines of `Verify-BankPatch.ps1`, `Get-PackageId.ps1`, `Build-BankPayload.ps1` and `New-Release.ps1` can be deleted.

## Release

A release is one zip file that holds both patches in the folder layout of the SD card, so that it can be extracted onto the card as it is:

```text
offline_bank-v1.0.0.zip
├── README.txt
└── luma/
    └── titles/
        ├── 00040000000C9B00/     Pokémon Bank
        │   ├── code.ips
        │   └── exheader.bin
        └── 00040000000C9C00/     Poké Transporter
            ├── code.ips
            └── exheader.bin
```

One command makes it. You choose the version:

```powershell
./scripts/New-Release.ps1 -Version v1.0.0
```

In this order, stopping at the first problem, it:

1. checks that you are on `main`, that no tracked file has uncommitted changes, and that the tag does not exist yet (files git does not track may lie in the folder);
2. runs the tests, Clippy and the formatting check;
3. builds both patches from the committed sources and verifies them;
4. writes `build/release/offline_bank-v1.0.0.zip`;
5. writes the release notes `build/release/offline_bank-v1.0.0.md`;
6. creates the annotated tag `v1.0.0` on the commit it built from, locally.

**A release that ships the migration** names the one version it converts from, the version before the Bank was last stored differently:

```powershell
./scripts/New-Release.ps1 -Version v0.3.1 -MigrateFrom v0.2.1
```

It then writes a second zip, `offline_bank-v0.2.1-migration.zip`: the migration patch for Bank's folder and a `README.txt` that says whose Bank it is for and what to do. The zip is named after the version it converts from, and it is for that version only. The `README.txt` of every release names the oldest Bank the version continues (`$bankFilesSince` in the script). Guides for a release, with pictures, belong in the release on GitHub, not in this repository.

**A release made while the official service runs** can ship the download:

```powershell
./scripts/New-Release.ps1 -Version v0.3.1 -Download
```

It then writes one more zip, `offline_bank-download.zip`, without a version in its name: the download patch for Bank's folder and a `README.txt` with what it needs, its steps and its numbers. The guide belongs to the release, like the migration's.

Nothing is pushed or uploaded. Try the zip on a console first, then publish:

```powershell
./scripts/New-Release.ps1 -Version v1.0.0 -Publish
```

This pushes the tag and creates the GitHub release with the zip, a migration zip or a download zip the notes name, and the notes through the `gh` tool. It refuses if a zip changed since its notes were written. By hand instead: `git push origin v1.0.0`, then on GitHub draft a new release for that tag, paste the notes file and attach the zip.

To withdraw a version before it is published, delete the local tag (`git tag -d v1.0.0`) and the two files. `-Draft` packs the working tree as it is for your own testing: no tag, and the zip is marked as a draft.

**Tracing a version.** The zip's `README.txt`, the release notes and the tag message all carry the same facts: the version, the date, the full commit hash, the two package names, and the SHA-256 of each patch file (the notes and the tag also that of the zip).

| From | To | How |
| --- | --- | --- |
| A release on GitHub | Its commit | The release belongs to the tag, the tag to the commit; the notes name the commit as well. |
| A zip | Version and commit | `README.txt` inside it. |
| A patch file on an SD card | Its release | Its SHA-256 appears in that release's notes. |
| A commit | The releases containing it | `git tag --contains <commit>` |

Patch files are never committed to the repository; they exist only as release downloads.

## Repository layout

```text
patches/bank/           the offline patch for Bank
patches/bank-migrate/   the migration patch for Bank, from v0.2.1
patches/bank-download/  the download patch for Bank, from the official server
patches/transporter/    code injected into Transporter
crates/bank-common/     what every patch for Bank shares: files, worker thread, dialogs
crates/offline-core/    storage format and hand-over rules, shared by all patches
crates/patch-builder/   host tool that checks the inputs and emits the patches
targets/                ARM target description
scripts/                build, verify, and release scripts
docs/                   user guides, this page, internals, and the console tests
pictures/               pictures for the README and for release guides
input/, tools/          yours: the CIAs and ctrtool (git-ignored)
build/                  generated: unpacked CIAs, intermediate files, packages, releases (git-ignored)
```
