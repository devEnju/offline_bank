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

Each script unpacks its CIA, compiles the injected code, and writes a package:

```text
build/bank/<package>/           code.ips, exheader.bin, manifest.json, report.md
build/transporter/<package>/    code.ips, exheader.bin, manifest.json, report.md
```

`<package>` is the first 16 hex digits of the hash of the compiled code, so the same source gives the same name. An existing package is never replaced.

## Verify

```powershell
./scripts/Verify-BankPatch.ps1 -Package <package>
./scripts/Verify-TransporterPatch.ps1 -Package <package>
```

The verify scripts share no code with the builder. They apply `code.ips` to the original themselves and check that only the listed locations and the added memory changed and that `exheader.bin` differs from the original only in its three size fields. On success they write `verification.json` into the package. This checks the files, not the behaviour on a console.

## Tests

```powershell
cargo +stable test --workspace --locked --offline
cargo +stable clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +stable fmt --all -- --check
```

The tests run on the PC and cover the storage format, a power cut injected at every write of a save, Miles rules, routing, deliveries into the transport box, the SD-save rules, and the builder.

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

1. checks that you are on `main`, that nothing is uncommitted, and that the tag does not exist yet;
2. runs the tests, Clippy and the formatting check;
3. builds both patches from the committed sources and verifies them (a package that already exists for the same code is reused);
4. writes `build/release/offline_bank-v1.0.0.zip`;
5. writes the release notes `build/release/offline_bank-v1.0.0.md`;
6. creates the annotated tag `v1.0.0` on the commit it built from, locally.

Nothing is pushed or uploaded. Try the zip on a console first, then publish:

```powershell
./scripts/New-Release.ps1 -Version v1.0.0 -Publish
```

This pushes the tag and creates the GitHub release with the zip and the notes through the `gh` tool. It refuses if the zip changed since its notes were written. By hand instead: `git push origin v1.0.0`, then on GitHub draft a new release for that tag, paste the notes file and attach the zip.

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
patches/bank/           code injected into Bank
patches/transporter/    code injected into Transporter
crates/offline-core/    storage format and hand-over rules, shared by both patches
crates/patch-builder/   host tool that checks the inputs and emits the patches
targets/                ARM target description
scripts/                build, verify, and release scripts
docs/                   user guides, this page, and internals
input/, tools/          yours: the CIAs and ctrtool (git-ignored)
build/                  generated: unpacked CIAs, intermediate files, packages, releases (git-ignored)
```
