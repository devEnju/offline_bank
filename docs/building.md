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
./scripts/Build-Patches.ps1
```

This builds the two patches of a release, Bank's and Transporter's, one after the other with the same steps: it unpacks the CIA, compiles the injected code, writes a package, and verifies it:

```text
build/binary/bank/           code.ips, exheader.bin, manifest.json, report.md, verification.json, PACKAGE.txt
build/binary/transporter/    the same
```

**The version on Bank's screens.** Bank's error and result screens say which version and package they come from, on the line above the two numbers ([bank.md](bank.md#troubleshooting)). The version is given to the build: `Build-Patches.ps1` takes `-Version v1.2.3` (or `v1.2.3-draft`) and compiles it into every package for Bank, and `New-Release.ps1` passes its own, with `-draft` for the offline package of a draft; the migration gets the version it converts from (`-MigrateFrom`), as its zip is named: `v1.0.0-migration`. A package built without it says `dev`. The version is part of the compiled code, so it is part of the package's name too.

A folder holds one package, the latest: the next build replaces it. `PACKAGE.txt` holds the package's name, the first 16 hex digits of a hash over the compiled code and the builder's profile of that patch (`crates/patch-builder/src/bank15.rs` with `bank15/offline.rs`, or `transporter15.rs`, which list the edits of the original), so the same source built in the same folder gives the same name, and any change to the patch gives a new one. The folder's path goes into the compiled code: a build in another folder has another name. `./scripts/Get-PackageId.ps1 -Kind bank` prints it. The other packages for Bank are built the same way with `-Kind`, each into a folder beside `binary`: `-Kind test` into `build/test/`, `-Kind migration` into `build/migration/`, `-Kind download` into `build/download/` (all below).

## Verify

```powershell
./scripts/Verify-BankPatch.ps1 [-Kind bank|test|migration|download]
./scripts/Verify-TransporterPatch.ps1
```

The build script runs these itself. The verify scripts share no code with the builder. They apply `code.ips` to the original themselves and check that only the listed locations and the added memory changed, that the start-up hook and the added memory hold exactly the bytes of the linked payload, and that `exheader.bin` differs from the original only in its three size fields. They also check that the package is the one of the current sources, by its name. On success they write `verification.json` into the package. This checks the files, not the behaviour on a console.

## Tests

```powershell
cargo +stable test --workspace --locked --offline
cargo +stable clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +stable fmt --all -- --check
```

The tests run on the PC and cover the storage format, a power cut injected at every write of a save, Miles rules, routing, deliveries into the transport box, the SD-save rules, and the builder. What only a console can show is in [testing.md](testing.md).

One test needs a file from your own copy of Transporter and is ignored in a normal run: the language screen's layout, `Turtle_lang_select_lower.bclyt`, which the patch changes into a Back button ([internals.md](internals.md)). Name it and run the test on its own:

```powershell
$env:TRANSPORTER_LANGUAGE_LAYOUT = 'path/to/Turtle_lang_select_lower.bclyt'
cargo +stable test -p transporter-payload --locked --offline the_real_layout -- --ignored
```

### CRC throughput on the host

The checksum integration tests compare the table implementation with the original bitwise calculation, including exhaustive one-byte and two-byte inputs and streamed native bodies. The throughput benchmark is ignored by normal test runs; invoke it explicitly, twice with the host otherwise idle:

```powershell
cargo +stable test -p offline-core --test checksum --release --locked --offline crc32_throughput -- --ignored --exact --nocapture
```

It uses the workspace's release settings, warms up both implementations, and reports median throughput and their ratio across seven samples, alternating their order. Inputs are a journal's 136 checksummed bytes, a 512-byte read chunk, the Bank file, and the native body, including body updates in 512-byte chunks. Data generation and allocation are outside timing; input, function, and output pass through `black_box`. These are PC measurements of CRC work, not ARM11 timings or total loading speed.

### Test builds for an interrupted save

Recovery after a power cut cannot be triggered on demand on a console, so one extra Bank package exists for rehearsing it. It is built behind a cargo feature that is off by default and is never part of a release.

```powershell
./scripts/Build-Patches.ps1 -Kind test
```

This builds and verifies it into `build/test/` (`Verify-BankPatch.ps1 -Kind test`). Its start screen says "Ver. 1.5-test". It is the offline patch with three differences:

- After START it shows the original's main menu, which the offline patch skips. The button chosen there opens the Bank and says what the next Save and Quit of that session does. All five buttons say "Use Pokémon Bank" in the console's language, and the four rehearsals have a word in brackets behind it, the same in every language. Back on the game selection leads to the menu again.
- The reward dialogs never come, and nothing is claimed, so that a run is short.
- While the menu is on screen Bank's files are open, and HOME works there; closing Bank from the HOME Menu at that point writes nothing.

The buttons, and the runs to make with them, are in [testing.md](testing.md#an-interrupted-save-and-quit). "(before)" and "(after)" stop Save and Quit at a fixed point and test what the next start decides ([bank.md](bank.md#if-save-and-quit-is-interrupted)); the plain "Use Pokémon Bank" of the same package settles what they left. "(record)" and "(boxes)" write for a while before they save, which gives a real power cut a long time to land inside a write; a stop cannot produce a half-written file.

The PC tests run the same loops under a simulated cut at every write:

```powershell
cargo +stable test -p bank-common --features test-build --release --locked --offline
```

## Other patches for Bank

Two patches do one thing each and are not the offline patch. Each has its own crate and builder profile, and nothing depends on either.

### The migration from the previous release

The release before this one stored the Bank differently ([internals.md](internals.md#files)). The step from the one to the other has a patch of its own, [patches/bank-migrate](../patches/bank-migrate/src/), which converts the Bank and does nothing else ([internals.md](internals.md#patches-for-bank)). It holds the only code that reads the earlier files: that release's storage code, frozen in [previous/](../patches/bank-migrate/src/previous/). The offline patch has no code for the earlier files.

```powershell
./scripts/Build-Patches.ps1 -Kind migration
```

This builds it into `build/migration/` and verifies it (`Verify-BankPatch.ps1 -Kind migration`). Its PC tests run with the others; among them a cut at every step of the conversion.

After START, migration checks existing local extdata before any cartridge or installed-game detection; neither is run. A missing archive gives `0000600D 00000000` without creating one. The presence or absence of a game never decides whether conversion runs.

It belongs to that one step and ships with the releases that follow it while users of the previous release are expected ([Release](#release)). Nothing depends on it: when the step is behind, the crate with its entry in the workspace's `Cargo.toml`, `bank15/migrate.rs` with its `mod` line in `bank15.rs` and its line in `profile.rs`, and the migration lines of `Verify-BankPatch.ps1`, `Get-PackageId.ps1`, `Build-Patches.ps1` and `New-Release.ps1` can be deleted; the next cargo command without `--locked` then updates `Cargo.lock`.

This is the form for every change of how the Bank is stored: the patches read only the current files and refuse earlier ones, and one migration patch of this shape converts the Bank of the version before. During conversion it shows the original preparation message and power-off warning. Every result stays under the original's line about the HOME Menu, with the version/package label and two numbers. `0000600D 00000001` means converted; no success message, sound or A press comes before it. Close Bank from HOME before switching packages. Its PC tests cover interrupted conversions; the console checks are in [testing.md](testing.md#a-release-with-a-migration), the user's steps in the package's README.

### The download from the official server

While the official service runs (it ends on February 25, 2027), a patch of its own, [patches/bank-download](../patches/bank-download/src/), fetches the Bank from the official server into a new offline Bank and does nothing else ([internals.md](internals.md#the-download-patch)). It needs no game: it takes the load the original made for Pokémon HOME, which reads and saves none.

```powershell
./scripts/Build-Patches.ps1 -Kind download
```

This builds it into `build/download/` and verifies it (`Verify-BankPatch.ps1 -Kind download`). Its start screen says "Ver. 1.5-download". Everything that talks to the server is the original's own code, so none of that runs on the PC; the PC tests cover its routing against the original's router, and the writing of the files with a cut at every write.

After START, download checks local extdata before contacting anything. If no offline Bank is present, it keeps the original archive and first-start preparation, then connects without detecting a cartridge or installed game. A missing archive is absence; unreadable or previous-format data stops the download instead of allowing an overwrite.

What it checks in the original's code is listed in [internals.md](internals.md#the-download-patch); the console checks are in [testing.md](testing.md#the-download).

Local inspection and file writing use the original preparation message and power-off warning. Native online phases keep their own messages. Like migration, every completed result stays under the original's line about the HOME Menu, with the version/package label and two numbers. Success appears after native session release and cleanup, with no success message, sound or A press. Close Bank from HOME before switching packages or reopening to check it again:

| Numbers | Meaning |
| --- | --- |
| `0000D0AD 00000001` | the Bank was downloaded and written successfully |
| `0000D0AD 00000002` | an offline Bank was on the card already; nothing was contacted or changed |
| `00000BAD 00000001` | the server holds no Bank for this console; nothing was created or changed |
| any other pair | the files could not be read or written; the numbers are the offline Bank's ([bank.md](bank.md#troubleshooting)) |

A message of the original on the way (no connection, maintenance, the server locked for a while) leads back to its start screen as it always did; nothing is written then.

Nothing depends on it: when the service has ended, the crate with its entry in the workspace's `Cargo.toml`, `bank15/download.rs` with its `mod` line in `bank15.rs` and its line in `profile.rs`, and the download lines of `Verify-BankPatch.ps1`, `Get-PackageId.ps1`, `Build-Patches.ps1` and `New-Release.ps1` can be deleted; the next cargo command without `--locked` then updates `Cargo.lock`.

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
4. writes `build/release/v1.0.0/offline_bank-v1.0.0.zip`: every release has a folder of its own, named for its version, which also takes its migration and download zips;
5. writes the release notes `build/release/v1.0.0/offline_bank-v1.0.0.md`;
6. creates the annotated tag `v1.0.0` on the commit it built from, locally.

**A release that ships the migration** names the one version it converts from, the version before the Bank was last stored differently:

```powershell
./scripts/New-Release.ps1 -Version v1.1.0 -MigrateFrom v1.0.0
```

It then writes a second zip, `offline_bank-v1.0.0-migration.zip`: the migration patch for Bank's folder and a `README.txt` that says whose Bank it is for and what to do. The zip is named after the version it converts from, and it is for that version only. The `README.txt` of every release names the oldest Bank the version continues: the release that ships the migration is that version itself, and every later release names it in `$bankFilesSince` in the script. Guides for a release, with pictures, belong in the release on GitHub, not in this repository.

**A release made while the official service runs** can ship the download:

```powershell
./scripts/New-Release.ps1 -Version v1.1.0 -Download
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
patches/bank-migrate/   the migration patch for Bank, from the previous release
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
