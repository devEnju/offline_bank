# Building and releasing

How to build the two patches from source, check them, and make a release. Everything runs on a PC; nothing here touches a console, an SD card, or the CIA files you supply. The commands are one Rust program in the workspace, [xtask](../xtask/src/), which `.cargo/config.toml` makes a cargo command: `cargo +stable xtask help` lists them.

## Requirements

- Windows, with Git.
- Rust: the stable toolchain and the nightly that `rust-toolchain.toml` pins, with `rust-src` and `clippy` (the nightly builds and lints the ARM code; `cargo xtask build` reads its name from there).
- Your own decrypted update CIAs in `input/`, and `ctrtool.exe` in `tools/`. Both folders are git-ignored.

  ```text
  input/Bank-v1.5-update.cia
  input/Transporter-v1.5-update.cia
  tools/ctrtool.exe
  ```

The builder identifies the inputs by hash and refuses any other version. The expected hashes are listed in [internals.md](internals.md) under the Scope of each patch.

## Build

```powershell
cargo +stable xtask build
```

This builds the two patches of a release, Bank's and Transporter's, one after the other with the same steps: it unpacks the CIA, lints and compiles the injected code (Clippy with `-D warnings`; the host lint below does not compile the code that runs on the console), writes a package, and verifies it:

```text
build/binary/bank/           code.ips, exheader.bin, manifest.json, report.md, verification.json, PACKAGE.txt
build/binary/transporter/    the same
```

**The version.** The version of a release is the workspace's, `version` under `[workspace.package]` in `Cargo.toml`; every crate but `offline-core` takes it (`version.workspace = true`). It changes only with a release: the release commit sets the new number there and in `Cargo.lock` (`cargo +stable update --workspace --offline`). `offline-core` has a version of its own, that of the stored files.

**The version on the error screens.** Bank's error and result screens, and Transporter's error screen, say which version and package they come from, on the line above the two numbers ([bank.md](bank.md#troubleshooting)). Only a release has a version there: `cargo xtask release` builds the packages as a release, and the payload then compiles the version from `Cargo.toml` in (`numbers::VERSION`, through `OFFLINE_BANK_RELEASE`), as `v1.2.3`. Every other build is a draft: one built with `cargo xtask build`, and the console test build. It says `draft` and the short hash of the commit it is built from, as `draft-a782e1b` or `draft-a782e1b-test` (`OFFLINE_BANK_COMMIT`); a build that xtask did not make, as `cargo test`, says `draft` alone. A draft is built from committed sources only: built from uncommitted changes it names a commit it is not, and `cargo xtask build` warns. A draft is never released; `cargo xtask release` only makes releases. The migration puts the version of the Bank it converted in front, as `v0.2.1-1.2.3` or `v0.2.1-draft-a782e1b`. The version is part of the compiled code, so it is part of the package's name too.

A folder holds one package, the latest: the next build replaces what it holds, and the folder stays. `PACKAGE.txt` holds the package's name, which the builder writes with it: the first 16 hex digits of the SHA-256 over `code.ips` followed by `exheader.bin`, the two files that go onto the SD card. The same files have the same name, and any change to them gives a new one; a change that leaves them as they are, as a reworded comment, leaves the name. The folder's path goes into the compiled code: a build in another folder has another name. The other packages for Bank are built the same way with `--kind`, each into a folder beside `binary`: `--kind test` into `build/test/`, `--kind migration` into `build/migration/`, `--kind download` into `build/download/` (all below). Each kind's crate, features and folder are listed once, in [packages.rs](../xtask/src/packages.rs). Every payload is linked with the same layout, which the builder writes from its program's addresses and its router ([link.rs](../crates/patch-builder/src/link.rs)) into `build/intermediate/<kind>/`.

## Verify

```powershell
cargo +stable xtask verify bank
```

The build runs it itself, for every kind. The verifier, [patch-verify](../crates/patch-verify/src/), shares no code with the builder and depends on no payload crate, so Cargo holds it to its independence. It has one check ([lib.rs](../crates/patch-verify/src/lib.rs)), and each program its own hard-coded list of edits with their original words, the places of the original that must stay as they are, its entry functions and the checks only it needs ([bank.rs](../crates/patch-verify/src/bank.rs), [transporter.rs](../crates/patch-verify/src/transporter.rs)). It applies `code.ips` to the original itself, over added memory that is not zero, which it finds under `build/extracted/` by the hash of its code, whatever the CIA it came from, and checks that only the listed locations and the added memory changed, that the start-up hook and the added memory hold exactly the bytes of the linked payload, and that `exheader.bin` differs from the original only in its three size fields. It also checks that `PACKAGE.txt` names the package's two files. That the package is the one of the current sources only a new build shows, which is why `cargo xtask release` builds before it verifies. It writes `verification.json` into the package, `passed` or `failed` with the reason. This checks the files, not the behaviour on a console.

```powershell
cargo +stable xtask test-verify
```

This tests the verifier on the built packages: every one passes, and a copy of Bank's or Transporter's package with one thing changed is refused for that thing, with the message expected (a byte of the ELF header, the start-up hook, the payload or the exheader, an entry of the manifest, the package name), and a payload changed in its ELF and its `code.ips` alike, so that only a check of one program's own refuses it (Bank's no-game stub, Transporter's layout check and the name of its transport-box file). Where a change touches bytes, the copy's manifest gets the new hashes and its `PACKAGE.txt` the new name, so the refusal comes from the independent comparison. It works on copies in a temporary folder, the original's too (it is found there by its hash under another folder name), and changes nothing under `build/`. `cargo xtask release` runs it after its builds.

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
cargo +stable xtask build --kind test
```

This builds and verifies it into `build/test/`. Its start screen says "Ver. 1.5-test". It is the offline patch with three differences:

- After START it shows the original's main menu, which the offline patch skips. The button chosen there opens the Bank and says what the next Save and Quit of that session does. All five buttons say "Use Pokémon Bank" in the console's language, and the four rehearsals have a word in brackets behind it, the same in every language. Back on the game selection leads to the menu again.
- The reward dialogs never come, and nothing is claimed, so that a run is short.
- While the menu is on screen Bank's files are open, and HOME works there; closing Bank from the HOME Menu at that point writes nothing.

The buttons, and the runs to make with them, are in [testing.md](testing.md#an-interrupted-save-and-quit). "(before)" and "(after)" stop Save and Quit at a fixed point and test what the next start decides ([bank.md](bank.md#if-save-and-quit-is-interrupted)); the plain "Use Pokémon Bank" of the same package settles what they left. "(record)" and "(boxes)" write for a while before they save, which gives a real power cut a long time to land inside a write; a stop cannot produce a half-written file.

The PC tests run the same loops under a simulated cut at every write, and the test build's routing through the main menu:

```powershell
cargo +stable test -p bank-common -p bank-payload --features test-build --release --locked --offline
```

## Other patches for Bank

Two patches do one thing each and are not the offline patch. Each has its own crate and builder profile, and nothing depends on either.

### The migration from earlier releases

Earlier releases stored the Bank in two other ways, named for the last release of each: v0.2.1 and v0.4.0 ([internals.md](internals.md#files)). A patch of its own, [patches/bank-migrate](../patches/bank-migrate/src/), converts a Bank of either straight into the current files and does nothing else ([internals.md](internals.md#patches-for-bank)). It holds the only code that reads the earlier files, a folder per way: [v0_2_1/](../patches/bank-migrate/src/v0_2_1/) and [v0_4_0/](../patches/bank-migrate/src/v0_4_0/), the latter with v0.4.0's storage code, frozen. The offline patch has no code for the earlier files.

```powershell
cargo +stable xtask build --kind migration
```

This builds it into `build/migration/` and verifies it. Its PC tests run with the others; among them a cut at every step of the conversion.

After START, migration checks existing local extdata before any cartridge or installed-game detection; neither is run. A missing archive gives `0000600D 00000000` without creating one. The presence or absence of a game never decides whether conversion runs.

Every release below v1.0.0 ships it ([Release](#release)). Nothing depends on it: before v1.0.0, the crate with its entries in the workspace's `Cargo.toml` and in `xtask/Cargo.toml`, `bank15/migrate.rs` with its `mod` line in `bank15.rs` and its line in `profile.rs`, its entry in `xtask/src/packages.rs`, `xtask/src/release/migration.rs` with its `mod` line and its line in `release.rs`, and the migration lines of `crates/patch-verify/src/` are deleted; the next cargo command without `--locked` then updates `Cargo.lock`. `cargo xtask release` then points a Bank of an earlier way to the migration of the last release below v1.0.0.

The crate has three parts. [target.rs](../patches/bank-migrate/src/target.rs) is the only code that knows the current files: it writes a body as a Bank under temporary names, reads it back, and puts it in place. Each earlier way has a folder, named for the last release that stored the Bank so, which reads that way with frozen code and fulfils the `Way` contract of [source.rs](../patches/bank-migrate/src/source.rs). A folder meets the target only through the native body and `target::Miles`, what the current release keeps beside the native data. [migrate.rs](../patches/bank-migrate/src/migrate.rs) is the run between them: detection, the marker, the steps and the result. The tests that every way needs (conversion, a second run, a cut at every write, refusals, files of two ways) are written once in `migrate.rs` and run for each folder's `Fixtures`.

This is the form for every change of how the Bank is stored: the patches read only the current files and refuse earlier ones, and the migration gets one more folder.

1. A folder named for the last release that stores the Bank the current way (`v1_2_3/`), with a frozen copy of the storage code that changes, as `v0_4_0/` holds v0.4.0's. It implements `Way`: its tag, the second number of error 3 with which the new release refuses such a Bank (`REFUSED`), how its Bank is recognised, how it is read into a body (its Miles mapped into `target::Miles`), and which files it removes; and it gives `Fixtures`: a Bank of it saved once, one with a save in progress, and the ones its release still repairs. A save in progress is never settled by the migration: the last release of the way does that, and it stays published. If releases of the way recorded one in forms that last release does not read, `ready_for_its_release` writes it in the form it reads, as `v0_4_0` does for v0.3.0.
2. Its variant in `Source`, with an arm in each `match`; the compiler names every place.
3. `target.rs` follows the new files. It takes names and sizes from `bank-common`, so mostly only `ORDER` and the temporary names change.
4. If `target::Miles` or the unused places of a new Bank change, every folder's `read` is reviewed and its tests run again: freezing a reader fixes what it reads, not what it writes into.
5. Its picture in `pictures/`. The release notes take the way from the migration itself (below).

During conversion it shows the original preparation message and power-off warning. Every result stays under the original's line about the HOME Menu, with a label and two numbers; the label names the way converted and the release, as "v0.2.1-1.2.3". `0000600D 00000001` means converted; no success message, sound or A press comes before it. Close Bank from HOME before switching packages. Its PC tests cover interrupted conversions; the console checks are in [testing.md](testing.md#the-migration), the user's steps in the package's README.

### The download from the official server

While the official service runs (it ends on February 25, 2027), a patch of its own, [patches/bank-download](../patches/bank-download/src/), fetches the Bank from the official server into a new offline Bank and does nothing else ([internals.md](internals.md#the-download-patch)). It needs no game: it takes the load the original made for Pokémon HOME, which reads and saves none.

```powershell
cargo +stable xtask build --kind download
```

This builds it into `build/download/` and verifies it. Its start screen says "Ver. 1.5-download". Everything that talks to the server is the original's own code, so none of that runs on the PC; the PC tests cover its routing against the original's router, and the writing of the files with a cut at every write.

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

Every release ships it until the service ends ([Release](#release)). Nothing depends on it: when the service has ended, the crate with its entries in the workspace's `Cargo.toml` and in `xtask/Cargo.toml`, `bank15/download.rs` with its `mod` line in `bank15.rs` and its line in `profile.rs`, its entry in `xtask/src/packages.rs`, `xtask/src/release/download.rs` with its `mod` line and its line in `release.rs`, and the download lines of `crates/patch-verify/src/` are deleted; the next cargo command without `--locked` then updates `Cargo.lock`. The release notes then leave the download out.

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

One command makes it, for the version in `Cargo.toml`. A release commit sets that first, to the number of the release, in `Cargo.toml` and `Cargo.lock`:

```powershell
cargo +stable xtask release
```

In this order, stopping at the first problem, it:

1. checks that you are on `main`, that no tracked file has uncommitted changes, and that the version is newer than every tag (files git does not track may lie in the folder);
2. runs the tests, the test build's tests, Clippy and the formatting check;
3. builds both patches, and the migration and the download while they ship (below), from the committed sources, verifies them, and tests the verifier;
4. writes `build/release/v1.0.0/offline_bank-v1.0.0.zip`: every release has a folder of its own, named for its version, which also takes the migration and download zips. Each zip is extracted again with Windows' own `tar`, and every file must come out as it was written;
5. writes the release notes `build/release/v1.0.0/offline_bank-v1.0.0.md`: a table of which way each Bank goes, the guides of the zips with their numbers, how to install, and the files and hashes;
6. creates the annotated tag `v1.0.0` on the commit it built from, locally.

**After a release** its journal records join the tests of the stored files' version, so that every later release of that version has to read them: at rest, with a delivery waiting, and with a save in progress, as `offline-core/tests/journal.rs` describes. They are taken from the release's tag with a temporary test in a git worktree of it, and added to `RELEASES` there and, the save in progress, to the test of `transaction.rs` that binds them.

**What ships beside the patches** is decided by the release, as long as its module is in `xtask/src/release/`:

| Zip | Ships | Afterwards |
| --- | --- | --- |
| `offline_bank-migration.zip` | in every release below v1.0.0 | from v1.0.0 on the release refuses until the migration is removed ([The migration](#the-migration-from-earlier-releases)) |
| `offline_bank-download.zip` | in every release until the official service ends, February 25, 2027 | after that day the release refuses until the download is removed ([The download](#the-download-from-the-official-server)) |

Each holds its patch for Bank's folder and a `README.txt` with its guide. Neither name carries a version: the migration converts a Bank of whichever earlier way it finds, and the download belongs to the official service. Each guide is written once in its module and shown twice, in the zip's `README.txt` as plain text and in the notes, there with pictures from `main`: `pictures/<version>-migration-converted.png` for each earlier way and `pictures/offline-boxes.png` for the migration, `pictures/download-title.png` for the download. A release refuses to start without them.

Which Bank goes which way comes from the migration alone: [migration.rs](../xtask/src/release/migration.rs) takes each earlier way from the migration's code (`Source::ALL`), as the last release of it and the second number of error 3 with which the release refuses such a Bank, and the patch's name and every result number the guide quotes from `migrate::numbers`; [download.rs](../xtask/src/release/download.rs) does the same for the download. So the numbers in the notes are the code's own, and the package titles come from each package's `manifest.json`, as the builder wrote them. The notes have a row for each: "An offline Bank from v0.2.1 or before", "… from v0.4.0 or before", then "… from every other version", and an error row for each number. Nothing else in the project knows the earlier ways; without the migration, one row and one error line point to the migration of the last release below v1.0.0.

The last release of each earlier way, v0.2.1 and v0.4.0, stays on GitHub for good: the migration's guide links it for whoever has to settle a Bank with it before converting (`00000BAD 00000007` and `00000008`). Guides for a release belong in the release on GitHub, not in `docs/`.

Nothing is pushed or uploaded. Try the zip on a console first, then publish:

```powershell
cargo +stable xtask release --publish
```

This pushes the tag and creates the GitHub release with the zip, the migration and download zips the notes name, and the notes through the `gh` tool. It refuses if a zip changed since its notes were written. Without `gh` it pushes the tag and lists what to upload: on GitHub, draft a new release for that tag, paste the notes file and attach the zips. By hand entirely: `git push origin v1.0.0`, then the same.

A release is tested as it is: made, tried on a console, then published. To withdraw one that fails before it is published, delete the local tag (`git tag -d v1.0.0`) and its folder in `build/release/`; after the fix it is made again under the same version.

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
patches/bank-migrate/   the migration patch for Bank, from earlier releases
patches/bank-download/  the download patch for Bank, from the official server
patches/transporter/    code injected into Transporter
crates/bank-common/     what every patch for Bank shares: files, Bank's side of the worker, dialogs
crates/patch-common/    what the patches of Bank and Transporter both use: the worker thread
crates/offline-core/    storage format and hand-over rules, shared by all patches
crates/patch-builder/   host tool that checks the inputs and emits the patches
crates/patch-verify/    host check of a built package, independent of the builder
xtask/                  the project's commands: build, verify, release (cargo +stable xtask <command>)
targets/                ARM target description
docs/                   user guides, this page, internals, and the console tests
pictures/               pictures for the README and for release guides
input/, tools/          yours: the CIAs and ctrtool (git-ignored)
build/                  generated: unpacked CIAs, intermediate files, packages, releases (git-ignored)
```
