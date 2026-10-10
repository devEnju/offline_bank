# Internals

For people changing the patches. Three parts: the [Bank](#bank) patch, the [storage](#storage) format both patches share, and the [Transporter](#transporter) patch. Addresses are ARM virtual addresses of the exact program versions named under each Scope; they come from static disassembly of the two programs and host tests.

## Bank

The patch for Pokémon Bank. What it does for a user is in [bank.md](bank.md).

### Scope

Pokémon Bank 1.5, title `00040000000C9B00`, TMD 6272, remaster 6:

```text
code.bin     2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf
exheader.bin 39d93584b07901dfa7ea2fc8ce1236cdd92476584ee2dbb60c11a4ae9402abf7
```

Addresses are ARM virtual addresses of that image. They come from static analysis and host tests.

### Code

| Where | Role |
| --- | --- |
| [patches/bank/](../patches/bank/src/) | The offline patch: ARMv6K code injected into Bank. Hooks in [runtime.rs](../patches/bank/src/runtime.rs) and [dex.rs](../patches/bank/src/dex.rs), routing in [navigation.rs](../patches/bank/src/navigation.rs), the storage jobs in [storage_worker.rs](../patches/bank/src/storage_worker.rs), game adapters. |
| [crates/bank-common/](../crates/bank-common/src/) | What every patch for Bank shares: the start-up hook, file access ([fs.rs](../crates/bank-common/src/fs.rs)), file coordination ([bank_files.rs](../crates/bank-common/src/bank_files.rs)), the worker thread, the original's task objects and dialogs. See [Patches for Bank](#patches-for-bank). |
| [patches/bank-migrate/](../patches/bank-migrate/src/) | The migration patch from v0.2.1: the conversion in [migrate.rs](../patches/bank-migrate/src/migrate.rs) and two hooks. |
| [crates/offline-core/](../crates/offline-core/src/) | `no_std`, no `unsafe`. Bank file and journal, side files, transport rules, Miles rules, game-image hashing. |
| [crates/patch-builder/](../crates/patch-builder/src/) | Host tool. Verifies the inputs, parses the linked ELF, emits the paired `code.ips` and `exheader.bin`. [bank15.rs](../crates/patch-builder/src/bank15.rs) holds what belongs to the program and the edits more than one patch makes; each patch has a profile: [bank15/offline.rs](../crates/patch-builder/src/bank15/offline.rs), [bank15/migrate.rs](../crates/patch-builder/src/bank15/migrate.rs). |

### Patches for Bank

A patch for Bank is a crate of its own with its own payload, its own profile in the builder, and its own package. It takes from `bank-common` what does not change what Bank does, and adds its hooks.

| | From `bank-common` | The patch's own |
| --- | --- | --- |
| Loaded and executable | `bootstrap`, and the builder's start-up edits | |
| Bank's files | `fs` (access), `bank_files` and `session` (names, sizes, order of writes) | which of them it reads or writes, and when |
| Off the main thread | `worker`: thread, mailbox, staging buffer | its jobs and what runs them (`worker::Service`) |
| On screen | `task` (task object, HOME and sleep mask, the screen with two numbers), `ui` | its hooks, and which tasks they replace |
| In the builder | `bank15.rs`: identity, placement, the shared edits | a profile: its entry functions and its list of edits |

**Rule: a patch writes Bank's files only through `bank-common`.** The byte formats exist once, in `offline-core`; the file names, their sizes and the order in which they are written exist once, in `bank-common`. A patch that writes a Bank has a PC test that loads the result with `BankFiles` and compares every byte. So what one patch writes, every other reads.

The **offline patch** is the rest of this part. The **migration patch** converts the Bank of v0.2.1 for v0.3.0 ([Files](#files)) and has none of the offline behaviour. Its router hook leads from the game scan to task 9 and from there to cleanup; its task 9 hook shows the loading panel, runs the conversion on the worker, and shows the result as the screen with two numbers. It makes these edits, each one the offline patch makes at the same place:

| Edit of the original | Offline patch | Migration patch |
| --- | --- | --- |
| Start-up hook and its call (`001040a4`, `00313910`) | yes | yes |
| A corrupt extdata archive is kept (`0029f338`) | yes | yes |
| First start without prompts (8 words) | yes | yes |
| Task router (`002a5a2c`) | `bank_offline_next` | `bank_migrate_next` |
| Task 9, update and busy poll (`00361cfc`, `00361d0c`) | `bank_offline_load` | `bank_migrate_open` |
| Tasks `0x10`, 7, `0xC`, `0xD`, `0xB`, the Pokédex callbacks, the timestamp helper (11 regions) | yes | no |
| Regions / entry functions | 25 / 8 | 14 / 2 |

### Edits to the original

The offline patch: 25 regions (24 words and the start-up hook) and eight entry functions. The builder checks every original word before patching and refuses to build if the five main-menu locations (`001d6554`, `002b33a4`, `002b33d4`, `003617bc`, `003617dc`) are not original.

| Address | Original | Replacement |
| --- | --- | --- |
| `001040a4` | `BL 0010494c`, application init | `bank_bootstrap_startup`: runs init, makes payload pages executable, exits on failure |
| `00313910` | Zero text padding | Start-up hook (192 bytes) |
| `0029f338` | Enters delete/recreate of a corrupt extdata archive | Branch to the existing error cleanup `0029f4fc`; the archive is kept |
| `001d3bf4` | Timestamp helper with an online-client check | Tail branch to `bank_offline_timestamp` (local clock `0023a754`) |
| `002a5a2c` | `BL 002a5580`, task router | `bank_offline_next` |
| `00361e84` | Task `0xB` update `002af034`, a server check | The original "finished" stub `002af124`; the task's start-up routine stays |
| `00361cfc`, `00361d0c` | Task 9 update `002ae568` and busy poll `002b4a20` | `bank_offline_load` |
| `00361ed4`, `00361ee4` | Task `0x10` update `002af460` and busy poll | `bank_offline_load` |
| `00362034`, `00362044` | Task 7 update `002b1cf8` and busy poll | `bank_offline_save` |
| `00361be4` | Task `0xC` update `002ad1bc`, reward guard | `bank_offline_rewards` |
| `00361a08` | Task `0xD` update `002a9750`, reward claim | `bank_offline_rewards` |
| `0033d30c` | Box screen child callback `002a7578` | `bank_offline_dex_save_request` |
| `003600cc` | Records task update `0026d8ec` | `bank_offline_dex_records_update` |
| `0033d34c` | Records completion callback `002a71a8` | `bank_offline_dex_records_finish` |
| `002a4898` | `BL 0025c08c`: apply the language of Bank's save | Nothing; the console's language stays |
| `002a4940` | "Language known" flag is 0 without a saved language | 1: the language screen is never entered |
| `002ac5b0` | Task 3 without a valid save: show the "Precaution for Use" notice | Branch to `002ac8a8`, where accepting it continues |
| `002ac61c`, `002ac628`, `002ac62c`, `002ac640`, `002ac648` | Task 3, valid save: store a picked language and write the save | The same store and write for a save that holds a language, with language 0 and kanji 0 |

The last rows, from `002a4898` on, are plain instruction words; see [First start](#first-start).

Five internal `bank_svc_*` wrappers give SVC `23/24/38/39/3a` ordinary AAPCS call boundaries, because the kernel overwrites registers that inlined code would still use. The builder checks their exact instruction bodies.

### Placement

The payload links at `003fb000`, after the original BSS. The paired exheader enlarges the data segment and sets BSS to zero (fields `0x34`, `0x38`, `0x3c` only); the IPS writes the former BSS, the payload, and its zero tail explicitly. Luma bounds IPS records by the allocated image, so `code.ips` and `exheader.bin` only work together. The start-up hook duplicates the process handle (SVC `27`), sets the payload's code pages RX (SVC `70`), and flushes the caches whole (Luma SVC `92`/`94`). The range calls (`91`/`93`) are not used: the kernel carries a small range out by virtual address on every core, which faults where another process runs, and flushes everything only for a large one.

### Tasks and routing

Task object: vtable `+8` init, `+c` update, `+14` end, `+1c` busy poll; state at `+10`, outcome byte at `+30`. Factory `002a5a7c`, router `002a5580`.

| ID | Task | ID | Task |
| --- | --- | --- | --- |
| 2 | Start screen | `0xC` / `0xD` | Reward guard / claim |
| 3 | Game scan | `0x19` | Box screen |
| 4 | Main menu (never created) | 7 | Save |
| 9 | Open Bank | `0x14` | Cleanup, then task 2 |
| `0xA` | Game selection | `0x10` | Load Bank |
| `0xB` | Scene set-up | | |

`navigation::destination` rewrites the original router's answer: `3 → 9`, `9 → 0xA` (original: 4), `0xA` outcome `0x13` → `0x14` (original: 4), `0xA` outcome 5 → `0xB` (as the original), `0xB → 0x10`, `0x10 → 0xC`, box-screen cancel → `0x14`, and anything outside a small allow-list → `0x14`. A host test covers every input.

**What the skipped menu did.** Its initializer `002a6e38` also builds menu controls and account texts, so it is not called. Only its loading-transition calls are reproduced when task 9 succeeds: `001d6200(0x50011, 0, -1)` stops the loading sound, `001e7084(ctx, 0, 0x97, 0)` hides the loading pane, `001e69ac(ctx, 0, 0, 0)` unbinds animation 0. Game selection (`002ae310`, `002adc50`) creates or reuses the shared UI and registers its own controls.

**Scene and text window.** Bank draws its screen objects in the order in which they were registered with the manager (`001df0e4` appends to the list at `manager + 0x44`). The reward dialogs need the character scene (`manager + 0x8c`, created by `002a5228`) registered before the text window (the shared UI at `manager + 0x80`), or the scene covers the text. The start-up routine of task `0xB` (`002af12c`) establishes that order: `001d6a34` closes and destroys the shared UI, the scene is created and registered, `001d62a8` creates a new shared UI, and the loading panel is shown on it. The patch therefore runs task `0xB` between game selection and loading and replaces only its update. The reward claim's end (`002ab934`) and the manager's cleanup (`002a497c`) destroy the scene.

**Welcome.** The original's task 9 update (`002ae568`) plays the welcome when the server's reply says that no Bank exists (`[[task + 0x28] + 0x20] == 0x35`), before it creates one; nothing records that it was shown. Entry (`002ae634`): end loading, `001d6194(ui)` shows the dialog scene (pane `0xa2`), `002b6114(0x10004, 0x3c, 0)`. State 2 shows message `[00337e14 + 4 * [task + 0x54]]` (0, 1, 2 of text file 37) through `001d6650(ui, 1, message, 1)`, state 3 waits for `001d6600(ctx) == 4` and counts up; after the third it hides the scene (`002ae6fc`) and sets state 4, the request for a new Bank. The patch has the same case when its Bank file is missing: it makes the entry calls (`ui::welcome`), sets state 2, and calls the original update while the state is 2 or 3 (`Step::Introduce`). At state 4 it shows the loading panel again and creates the file. HOME is not refused during the welcome; a task that ends there has created nothing.

**No usable game.** Selection state 0 would repeat notice `0x1A` forever. Task 9 checks the availability bytes (`manager + kind*12 + 1`), shows the notice once via `001d6650(ui, 0, 0x1A, 1)`, and routes to cleanup.

**UI helpers.** Shared UI at `manager(+0x2c) + 0x80`; task UI pointer at `+0x3c` (tasks 9, 7), `+0x38` (`0x10`, `0xC`), `+0x40` (`0xD`); context at `ui + 0x5c`. Loading: `001d5b44(ui)` then `0025da24(ui, message)` with message `0xE` ("Communicating with the Pokémon Bank server…") for opening and loading, as the original has on screen in both, and message 2 ("Preparing Pokémon Bank for your use…") only while a new Bank is created (original: `002ae764`). Dialog poll `001d6600(ctx) == 4`. The error dialog is the original's message `0xB` (shown by its task `0x16` after a failed save) through `001d6650(ui, 0, 0xB, 1)`; the patch then ends the message at its first page break (control code `0x10, 1, 0xBE01`; in Simplified Chinese, which has one page, at the first line feed), which leaves its first sentence, and puts two eight-digit numbers on the third line (one or two line feeds first) in the UI's own string object (`ui + 0x94`: data pointer `+4`, capacity `+8`, length `+0xa`) and hands it to the dialog again (`001d643c`). That sentence has one or two lines in all ten languages and the dialog holds three. No other text is written by the patch. A fault stays for the session: the router then sends everything to cleanup except the game scan, which still leads to task 9, where the hook shows the error again.

### First start

Both first-start prompts depend on Bank's own save data (`data:`, 128 KiB), not on the extdata.

- **Save object** `[app + 0x74]`, with `app = [[003ab90c] + 0x1c]`: language at `+0x30` (u16, 0 = none), kanji at `+0x32`. `002cb914` and `002cb8f4` read them, `002baf00(save, language, kanji)` writes them. `0015dbf0(app, heap)` loads and checks the file (1 = valid); `0015dc50` clears a new one, language 0.
- **Language.** At every start `00105c2c` takes the console's language and `00105c44`..`00105cc0` the text set for it; for Japanese that is text set 0, kana. The manager set-up `002a47f4` then loads the save: if it is valid and holds a language, it applies it (`0025c08c(language, heap, kanji)`, the only way to text set 1, kanji) and sets the manager's flag `+0x1b`; otherwise the flag is 0. The router sends task 0 to task 2, the start screen, with the flag set, and to task 1, the language screen (vtable `00361954`, screen `0025ce8c`), without it. The patch leaves the saved language unapplied and always sets the flag.
- **Notice.** Task 3 (`002abe08`), states `0x12` and `0x13`: without a valid save it shows the notice at `002ac5b0` (`002b2dbc(ui, 0)`; messages `0x4f` and `0x50` of text file 39) and waits. Accepting is state `0x1d`: from `002ac8a8` it clears the listener, shows the loading panel, and state `0x14` creates the save (`002bb380`), state `0x16` clears it, stores the language picked on the language screen if there is one (`002ac6ec`) and writes it. The pick (`[[manager + 0xf0] + 4]`, kanji at `+8`) is written by `0025d1d0` alone, whose one caller is the language task's listener `002d0b88`; without that screen it stays 0 and nothing is stored, so this code is not edited. Declining exits. With a valid save, `002ac618` stores a newly picked language (`002ac630`) and writes, or goes on. The patch branches from `002ac5b0` to `002ac8a8` and turns the store for a valid save into a reset: `002ac618` now takes the save (`[app + 0x74]`) and its language (`[[save + 4] + 0x30]`), goes on if that is 0, and otherwise runs the original's `002baf00(save, 0, 0)` and save write.

A save written by the patched Bank is therefore valid with language 0, which to the original means: notice accepted, language not chosen. It then asks for the language only. A save that holds a language loses it at the first patched start, in one write.

### Bank object

`0xBB530` bytes with vtable `003626FC`. `002CB870` saves the body, `0023650C` restores it. The transport-box model check is `0022bd54`; the patch counts occupied slots itself (see [Hand-over from Transporter](#hand-over-from-transporter)). The worker reads each file once into its final place in a staging buffer; the main thread only ever sees the whole body.

### Filesystem calls

The first argument is a pointer to the session or file handle; a negative `i32` is failure.

| Address | Call | Arguments after the handle pointer |
| --- | --- | --- |
| `0020a468` / `0020a438` | OpenArchive / CloseArchive | `out: *mut u64, id: u32, path: Path` / `archive: u64` |
| `001654f8` | CreateFile | `transaction: u32, archive: u64, path: Path, attributes: u32, length: u64` |
| `00165554` | DeleteFile | `transaction: u32, archive: u64, path: Path` |
| `001657ec` | OpenFile | `out: *mut u32, transaction: u32, archive: u64, path: Path, flags: u32, attributes: u32` |
| `001658c8` / `0016594c` | Read / Write | `out_count, offset: u64, buffer, length: u32` (+ `flags: u32` for Write) |
| `001659ac` / `00165920` | GetSize / CloseFile | `out: *mut u64` / none |
| `0016578c` / `0012146c` | Get / Set secure value | `first*, second*, value*, archive: u64, slot` / `archive: u64, slot, value: u64, option: u8` |

`Path` is `{ kind: u32, data: *const u8, byte_len: u32 }`, not libctru's order. The extdata archive is ID 6 with binary path `[1, 0xc9b, 0]`. Sync is a zero-length Write with flags `0x10001`. CloseFile does not close the kernel handle; SVC `23` must follow. The filesystem session lives at `00390100`.

### HOME button and sleep

Bank keeps an activity mask in the byte at `00372988 + 2`; `001d4d90(bit)` sets a bit and `00229eb4(bit)` clears it. The main loop (`0010455c`) calls `0010ba04` every frame, which reports a change to the system, and the suspend callback `001050e0` refuses while the mask is not zero. A HOME press is accepted only while the mask is zero: `0010ba04` then sets the flag at `+1`, and the main loop calls the HOME handler `0010784c` once the current scene is ready (`00108634`), which can be several frames later. The handler does not look at the mask again.

- **Bit 1**: the original sets it for the whole online session and clears it in cleanup (`002abc7c`). The patch sets it in the routing hook whenever the next task is a loading task (`navigation::loads`: game scan, opening, scene set-up, loading, saving) and in `Runtime::submit`. It clears it in `Runtime::complete`, before the error dialog, before the "no game" notice, and in the routing hook for every other destination. Setting it per task, not per job, leaves no open frame between the scan and the opening.
- **Accepted presses**: because the handler ignores the mask, a press accepted just before the mask was set would still be carried out. Accepting does nothing but set the flag at `+1` (`0010ba7c`; the handlers clear it afterwards at `00107924` and `00107a50`, and no other store to it was found), so the patch takes the press back: whenever it sets bit 1 it also clears that flag (`storage_activity`). From that frame on the original refuses every press itself. The HOME Menu, and closing Bank from it, therefore never meet a loading screen or a running job, and nothing waits for a press to be carried out. A press on the very frame a loading screen begins is ignored like any press during loading. This is the one byte of the original's HOME state the patch writes.
- **Bit 4**: set by the game-save writer launch (also in the patch's launcher in [native_game.rs](../patches/bank/src/native_game.rs)) and cleared by the original when the writer finishes.

The busy flag from `0025c420` is unrelated: it protects a task from Bank's own cancellation, not from HOME.

### Worker thread

One native SDK thread (`002320d4`, trampoline `001211e4`, event `00235c6c`/`00234a00`/`00231e70`) owns all storage handles. The thread and its mailbox are `bank-common`'s; what it runs is the patch's own (`worker::Service`, in the offline patch the jobs of [storage_worker.rs](../patches/bank/src/storage_worker.rs)). The main thread owns every native UI, task, and game object. A single mailbox with atomic states passes fixed-size jobs and the staging buffer; neither thread ever waits on the other. Task hooks submit a job and poll once per frame, with native busy protection (`0025c420`/`0025c3e8`) keeping the task alive. Stack 32 KiB with a guard pattern.

### Game saves

Save manager (0x100 bytes): selected kind at `+c8`; per kind `k` (1..8 = X, Y, OR, AS, S, M, US, UM), byte `k*12` is the kind, byte `+1` is "loaded", word `+4` the game object, word `+8` its writer. Game object: 0xb49d0 bytes, vtable `00362f88`, metadata pointer `+4`, kind byte `+8`.

- **Prepare** `002bc4bc` regenerates checksums (and Gen 7 signatures) after rotating the secure-value pair. Hash only after it.
- **Blocks** `002bc460(game, i)`: 55/58/37/39 data blocks (XY/ORAS/SM/USUM) plus a 0x1e8-byte metadata block, each written at a 512-aligned offset. Gaps and the file tail are not rewritten, so the expected after-image keeps their old bytes.
- **Write** uses the original writer thread (`001d37ec`, mode 4) and completion `0015dc74`, launched only after the journal is durable.
- **Secure value** slot `0x1000`. The platform value is advanced only by the one step a completed save takes, from the game file's own previous value to its current one: by a recovery whose journal matches the complete after-image, and by a load, for a save in progress that was settled without its game ([below](#an-interrupted-save)). Any other difference is fault `E`.

### Rewards

- Count: `001d5ec0(bank + 0xbb520)`, 100 boxes × 30 slots. Balance: `001d588c` / `001d59fc` on `bank + 0xbb528`.
- Native accrual in guard state 0 and claim state `0x1b` is `count × (1/30) × hours × (1/24)`. The hook sets the stored reward date to the session date first, so native accrual is zero; local earnings are added to the balance when task `0x10` finishes.
- The claim hook starts task `0xD` at state `0x1b` with `+4c = 1`, `+50 = 0`, and session `+38 = -1`, which skips service queries and distributions. Allowed states: `0`, `7..0xd`, `0x16`, `0x1b..0x22`.
- The allowed states are a guard, not a path a user can reach. From `0x1b` with `+4c = 1` and session `+38 = -1`, the claim task goes to `0x22` (total 0) or `0x16`, then through `7`..`0xd`, `0x1c`, `0x1e`..`0x22`; state `0xa` becomes `0xb` in the choice listener `002d144c`. States outside the list are set only by the server-reply listener `002ab17c`, by the dialog listener `002d0c4c` from state `0x1a`, and by the branches of states `0` and `0x1b` that those two fields rule out. The guard task has eight states, all allowed.
- Nothing to get (`rewards::nothing_to_get`: today is not later than the record's accounted-through date, balance below 10, no gift): the hook sets the guard to state 4 (`002ad658`, outcome 5, no notice 7 / `0x51` about a present in the game) and the claim to state `0x22` (`002ab124`, the original's end for a total of 0), after ending the loading panel as the original's state `0x1b` does before it looks at the total (`002aac80`). Otherwise claim state `0x1b` shows message `0xE` for any total above 0 and state `0x16` message `0xF` below 10 or the choice `0x10` from 10 on.
- Redemption runs in state `0xb` (choice at `+60`, quote at `+50`) and ends in `0xc`. Gen 6 writes the gift buffer from `(*(game + 0x1c384))->vtable[2]`: flag `+1ff |= 0x80`, Miles at `+6a2`, BP at `+6a0`. Gen 7 inserts a 0x108-byte record (type 3 at `+51`, amount at `+68`) through `002b6d68` into the 48-slot store at `game + 0xad43c`; the original ignores that call's result, so the hook compares counts and the new record. `002b6d68` counts the used slots (a nonzero title halfword, as `001d58b0` does) and copies the record to the slot of that number (`002b7130`..`002b7140`), so the hook reads the new record at the count it took before the claim, the same slot the original writes.

### Pokédex

Records task init `0026de9c` loads the eight version caches (`002b9a3c`). Import: `001e1ba8` copies nine record words, `002b8e00` stores trainer identity, `001e1b94` copies the game's Pokédex into its version region and marks it valid. `002cb7b8` compares identity for the original replacement prompt. The hooks run this import at Save and Quit instead of on a records visit, check that no other version changed, and after every body restore reload all caches (`002b9a3c`, `002b9b90`).

## Storage

The files of the offline Bank and the rules by which Transporter delivers into them. Both patches use the same code for this: [`offline-core`](../crates/offline-core/src/). The user-level description is in [bank.md](bank.md#where-the-bank-is-stored).

### Native body

The Bank object of the original is `0xBB530` bytes; its serialized body is `object + 8`, length `0xBB518`. Only that range is stored; helper pointers behind it are never persisted. Offsets are hexadecimal.

| Body offset | Content |
| --- | --- |
| `15C`, `15E` | Format `2`, box count `100` |
| `160`, `170` | Reward date (year u16, month, day, hour, minute); whole Miles (u32, capped at 65,535) |
| `17C` | 100 boxes, stride `1B56`: 30 records of `E8` bytes, then a `24`-byte name and a u16 index |
| `AAF14`, `AD5FC` | 30 transport-box records and their 30 format tags |
| `ACA44`, `B4AA0`, `B5658` | Per-slot format tag, flag byte, and 8-byte timestamp; they move with the Pokémon |
| `AD61C` | Eight `44`-byte trainer and adventure records, one per game version |
| `AD83C` | `7260` bytes of Pokédex data: eight version regions plus derived state |

Pokémon records are encrypted with the LCG `seed * 0x41C64E6D + 0x6073`; the checksum at `+6` is the 16-bit sum of the 112 decrypted words.

### Files

All files are in SD extdata archive `0x00000C9B` (created by the original with room for 10 directories and 100 files). The body is cut at fixed offsets and each piece is stored unchanged, in body order ([sections.rs](../crates/offline-core/src/sections.rs)). There are four containers; each is a range of fixed layout:

| Container | Length | Body ranges of one payload |
| --- | --- | --- |
| Bank | 1,461,332 | `0..AAF14`, `ACA44..AD5FC`, `AD61A..AD61C`, `B4A9C..BB518` (730,442 bytes). The Miles field `170` is stored as zero. |
| Pokédex | 59,776 | `AD61C..B4A9C` (29,824 bytes) |
| Transport box | 14,112 | `AAF14..ACA44` then `AD5FC..AD61A` (6,990 bytes) |
| Rewards | 160 | none; a 16-byte record |

**Every unit of a container is a file of its own** ([bank_files.rs](../crates/bank-common/src/bank_files.rs), `FileName::units`), in the order of the container's layout:

| Container | Files | Unit |
| --- | --- | --- |
| Bank | `/journal.bin`, `/journal.alt.bin` (192 each) | journal record A, B |
| | `/bank.bin`, `/bank.alt.bin` (730,474 each) | snapshot slot A, B |
| Pokédex | `/dex.bin`, `/dex.alt.bin` (29,888 each) | slot 0, 1 |
| Transport box | `/mover.bin`, `/mover.alt.bin` (7,056 each) | slot 0, 1 |
| Rewards | `/rewards.bin`, `/rewards.alt.bin` (80 each) | slot 0, 1 |

The reason is how the console writes extdata. Each file is its own container with check values over blocks of its contents; new data is written in place before the check values are updated. A write that the power interrupts leaves the blocks it touched failing their check: reads return a result of the corrupted-data class (`D900458B` was seen), not bad bytes. Units that share a file share blocks at their edges, so in one file a write to the spare slot could take the slot in use with it, and did. With one file per unit, only the unit being written can be lost, and by the order of a save that unit never holds anything still needed.

`Split` in [fs.rs](../crates/bank-common/src/fs.rs) presents the unit files of a container as one range to the unchanged store and side-file code:

| A unit file that is | Reads | Writes |
| --- | --- | --- |
| there | as stored; an error if the console refuses | as written |
| absent | as zeros | are preceded by creating it, zero-filled |
| there but not to be opened | an error | an error |
| there with another size | the container does not open: a hard error, and it is never replaced | |

- A container counts as missing only when none of its files exists.
- `Storage::recreate` deletes one unit file, which leaves it absent. The store and the side files ask for it when writing, syncing or reading back a unit fails, and then write once more; they only ever write units that are not in use.
- An unreadable journal record counts as a damaged one. An unreadable side slot is void where that is certain: for the Pokédex and rewards always (they never block the Bank), for the transport box only beside a slot that is seen to belong to the current snapshot (`Sidecar::slots_beside`), and never for Transporter, which uses the strict `Sidecar::slots`.

File names in 3DS extdata are limited to 16 characters without the leading slash; a longer name fails at creation with `E0E046C7`. `UnitFile::new` refuses longer names at compile time.

**Rule for new features:** add a new container with tagged slots, each slot a file of its own. Never change the size or layout of an existing file. A build that does not know a file ignores it; its saves advance the Bank snapshot, so the unknown file's slots stop matching and a later build treats them as "no data yet".

**The Bank of v0.2.1** kept each container in one file (`/bank.bin`, `/dex.bin`, `/transport.bin`, `/rewards.bin`) with exactly the container layout above. The offline patch does not read them: their `/bank.bin`, `/dex.bin` and `/rewards.bin` have another size than the files of those names now, which is a hard error. They are converted by the migration patch ([migrate.rs](../patches/bank-migrate/src/migrate.rs), [building.md](building.md#the-migration-from-v021)), which cuts the four files at the unit boundaries: it copies them into temporary files (`/m0.tmp` to `/m9.tmp`) and compares, creates the marker `/migrate.ok`, removes the old files, writes the final files from the temporary ones (journal records last) and compares, removes the marker, then the temporary files. Without the marker a start begins again from the untouched old files; with it, from the temporary ones. It refuses a Bank that is damaged or has a save in progress and changes nothing. Its result is two numbers: `0000600D` with 1 (converted), 2 (already in the new files) or 0 (no Bank); `00000BAD 00000007` for a save in progress; `00000BA1` to `00000BA4` for a failed step (reading the old files, writing the temporary ones, removing the old ones, writing the new ones) with the console's code, or 1 for a copy that does not compare, 2 for a missing file, 5 for a Bank that does not open.

### Bank file

[store.rs](../crates/offline-core/src/store.rs), [format.rs](../crates/offline-core/src/format.rs). The container is two 192-byte metadata records (`BKOFMETA`), then two slots of a 32-byte snapshot header (`BKOFSNAP`) plus the payload; each of the four is a file (above). Field offsets are in the [crate docs](../crates/offline-core/src/lib.rs).

- A new snapshot goes to the inactive slot and is synced and read back; metadata is written to one replica, synced, then the other.
- A save: verify the game's before-image, `prepare_transfer`, write the game, `reconcile`, read the game back. A before-image keeps the old snapshot, an after-image commits the new one. Right after its own write (`Job::Finalize`) the worker does not wait for the complete read to commit: the 16-byte secure pair in the game's file must be the pair of the image it prepared, which is new with every prepared image, and the game's commit replaces the file whole. The complete fingerprint is checked after the commit; a difference is fault `11` with the new snapshot current. The store blocks on anything else; the worker then chooses the snapshot itself ([below](#an-interrupted-save)).
- A load checks the journal and the 32-byte header, then reads the payload once and checks its CRC on that pass.
- A new Bank writes the side files first, then creates the four Bank files zero-filled, writes the first snapshot, and publishes the first journal record last. A start that finds Bank files without any valid journal record, with at least one record still empty (zero or absent) and with no snapshot other than a first one, treats it as "no Bank yet" and finishes the creation in place (`BankStore::reinitialize`, which discards the four units and begins again). No Bank that was ever current can be in that state: it has a journal record, and after its first save a second snapshot. Two unreadable records are never taken for it; that is fault `5`. A delivery Transporter made in between is kept and shown.

### Side files

[sidecar.rs](../crates/offline-core/src/sidecar.rs). The Pokédex, transport and rewards containers each hold two slots at `index * slot_len`, where `slot_len = 64 + capacity rounded up to 4`; each slot is a file (above). Slot header, little endian:

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | Magic: `BKOFDEX1`, `BKOFTRN1`, `BKOFRWD1` |
| 8 | 2+2 | Version 1, header size 64 |
| 12 | 4 | Payload length |
| 16 | 8 | Tag: generation of the Bank snapshot (0 = no tag) |
| 24 | 4 | Tag: payload CRC of that Bank snapshot |
| 28 | 4 | `aux` (transport: delivery id) |
| 32 | 4 | `count` (transport: number of Pokémon) |
| 36 | 4 | Payload CRC-32 |
| 40 | 20 | Zero |
| 60 | 4 | CRC-32 of bytes 0..60 |

The payload follows at +64. An all-zero or damaged header is a void slot. Writing voids the header first, then writes payload and header, each synced.

**Keeping the files in step** ([bank_files.rs](../crates/bank-common/src/bank_files.rs)). A save writes every side file's spare slot, tagged with the snapshot about to be written (generation + 1, CRC of the new Bank payload), and only then prepares the Bank journal. A load uses the slot whose tag equals the current snapshot. A rolled-back save leaves the old slots matching; a committed one makes the new slots match. Missing side files load as defaults, and so do a Pokédex file and a rewards file without a readable matching slot; only the transport box is an error then.

**Rewards record** (16 bytes, [rewards.rs](../crates/offline-core/src/rewards.rs)): u32 balance; u8 state (0 none, 1 record); u8 fraction 0..29; u16 saved count; accounted-through date (u16 year, month, day); 4 zero bytes.

### An interrupted save

A save in progress (`Phase::Prepared`) is settled by the next start, in `Job::Recover` ([storage_worker.rs](../patches/bank/src/storage_worker.rs)). The journal holds the fingerprint of the game's complete save before and after, and a 32-byte binding, a SHA-256 over the title, both fingerprints and the kind of copy the save was made with, cartridge or installed ([transaction.rs](../patches/bank/src/transaction.rs)). The worker compares the game's file with the two images:

| The game of the journal's title | Decision | Code |
| --- | --- | --- |
| Its file is the before-image | Keep the old snapshot | `reconcile`; the platform secure value must be the file's (`require_secure`) |
| Its file is the after-image | Commit the new snapshot | `reconcile`; the platform secure value is advanced if the cut came before that (`finish_secure`) |
| Its file is neither, on the kind of copy the save was made with (`Found::SameKind`) | By what the save moved | `settle_by_moves(.., true)`, `moved::choose` |
| Its file is neither, on the other kind of copy (`Found::StandIn`) | As for a game that is not there, next row | `settle_by_moves(.., false)`, `moved::choose_unseen` |
| Not among the games the scan loaded (`NativeGameError::NotLoaded`, `Job::Recover { game: None }`) | By what the save moved if that was one way; otherwise fault `7` and nothing is written | `settle_by_moves(.., false)`, `moved::choose_unseen` |

A file that is neither image, on the same kind of copy, was written since the cut: by the game played on, by a new game started on it, or it is another cartridge. Nothing in it tells for certain which image it was made from, and the three are not told apart.

**The kind of copy.** The original holds one copy per title: its mount (`00163F24`) opens archive `567890B4` for the title on medium 2, the game card, and only if that fails on medium 1, the SD card. When the copy of a save in progress is gone, the other kind stands in under the same title with an unrelated save, and the game must count as absent. The console says which kind a loaded copy is: the reply to the secure-value query (`0876`) carries a flag for a title on a game card, the one the original's check at `0016443C` uses to accept any value for cartridges (`PlatformSecureValue::gamecard`). At recovery the binding is computed for the kind found and for the other one; the one that fits tells which kind the save was made with. An exact image is accepted from either kind. A binding that fits neither is fault `B`: that is a damaged record, or a save in progress begun by a build with another form of binding, which only that build finishes. No other form is read.

**What a save moved** (`BankFiles::pending_moves`). Both snapshots are still in the Bank file. Each is read into the staging buffer in turn, and the identity of every occupied record is collected (`sections::stored_identity`: encryption constant, personality value, trainer ID and secret ID, all in the record's first block):

| | Snapshot before | Snapshot after |
| --- | --- | --- |
| The 100 boxes | Every occupied record | Every occupied record |
| The transport box | What a load showed: its own box, or a delivery it took (`transport::on_load`) | The slot the save wrote (tagged with the snapshot after) |

The two lists are compared as multisets ([moved.rs](../crates/offline-core/src/moved.rs)). What only the snapshot after holds was deposited; what only the one before holds was withdrawn or released; a Pokémon moved inside the Bank is in both.

| Deposited | Withdrawn | `choose` (a game is there) | `choose_unseen` (none is) |
| --- | --- | --- | --- |
| some | none | Commit | Commit |
| none | some | Keep | Keep |
| none | none | Keep | Keep |
| some | some | Commit | Wait |

The first two rows cannot lose a Pokémon whichever way the game's save went, and the third cannot lose the Miles of a claim. The last row has no such answer: after a commit, if the game's save had not been written, the withdrawn Pokémon are in neither place.

- The chosen snapshot is published through `reconcile` with that image's fingerprint (`BankFiles::reconcile_as`); `decide_recovery` itself is unchanged.
- No secure value is checked or changed in the last three rows of the first table. If a game that was not there holds the after-image and the cut came before the original writer advanced the platform value, that value is one step behind; the next load of that game advances it (`finish_secure` in `Job::InspectAndLoad`).
- Right after Bank's own write (`Job::Finalize`), a game file that does not carry the prepared secure pair is fault `11`; the journal stays in progress and the next start settles it by the table.

### Hand-over from Transporter

[transport.rs](../crates/offline-core/src/transport.rs) holds these rules as tested functions. The Transporter patch runs `transport::deliver` from that file; Bank's tests deliver with the same function.

- A slot's payload is the native transport box: 30 records of `E8` bytes, then 30 format-tag bytes.
- **Transporter delivers** by writing one slot with no tag that holds the complete box the original Transporter built, with `count` = 1..30 occupied positions anywhere in it and a nonzero `aux` id different from every `aux` it read. It may deliver only if at most one slot is valid and every valid slot has `count` 0 (`may_deliver`). Two valid slots mean a Bank save is unresolved: the user must open Bank first. The Transporter patch asks this before a session begins and picks the original's message by the slots (`layout::refusal_message`); `deliver` checks it again before it writes.
- **Bank loads** the slot tagged with its snapshot. If that box is empty or absent and a delivery waits, the delivery becomes the box, unchanged. A delivery whose id equals the current slot's `aux` was already taken and is ignored. A load never writes this file.
- **Bank saves** the box to the other slot, tagged, with `aux` = the id it took and `count` = occupied slots. After the save is published (`tidy_transport`) it voids every other slot. That is the only time slots are removed. If a save is cut before that step, two valid slots remain and Transporter must wait until Bank completes a Save and Quit.

Occupied slots are counted by decrypting each record's species word with the standard stored layout.

## Transporter

The patch for Poké Transporter. What it does for a user is in [transporter.md](transporter.md); the delivery file and its rules are under [Hand-over from Transporter](#hand-over-from-transporter).

### Scope

Poké Transporter 1.5, title `00040000000C9C00`, TMD 5200, remaster 5:

```text
code.bin     001c20ada74016507c969bb44a0a50f8edf803ec06a3fc46834263ba8df0fd2f
exheader.bin 4b6081a5d0242d43713fd0be3cec21ae73f075441a759af3ed03c5270d1baceb
```

Addresses are ARM virtual addresses of that image. They come from static analysis and host tests.

### Code

| Where | Role |
| --- | --- |
| [patches/transporter/](../patches/transporter/src/) | ARMv6K code injected into Transporter. Bank check and delivery in [hooks.rs](../patches/transporter/src/hooks.rs), SD saves in [cart.rs](../patches/transporter/src/cart.rs) with their rules in [sdsave.rs](../patches/transporter/src/sdsave.rs), the language of the listed games in [language.rs](../patches/transporter/src/language.rs), entry table and assembly stubs in [link.rs](../patches/transporter/src/link.rs), start-up hook in [bootstrap.rs](../patches/transporter/src/bootstrap.rs). |
| [crates/offline-core/](../crates/offline-core/src/) | `transport::deliver` and the side-file format, shared with Bank. |
| [crates/patch-builder/](../crates/patch-builder/src/) | Host tool. The profile with every edit and its original word is [transporter15.rs](../crates/patch-builder/src/transporter15.rs). |

### Edits to the original

30 instruction words, each checked against its original value by the builder, plus the start-up hook. The payload starts with seventeen entry branches, in this order: check, deliver, router, slot, cartridge id, cartridge read, cartridge write, list next, select, title scan, language chosen, language order, language buttons, language back, title begin, title end, list ready. Where a site leaves the original's flow for good, it is one branch to a stub in the payload, and the stub goes on inside the original itself; the original holds no decision logic of the patch. One edit, at `0013AEE4`, does not branch to an entry but to the layout check beside the start-up hook (see [Placement](#placement-1)).

| Address | Purpose | Replacement |
| --- | --- | --- |
| `00103D9C` | `BL 00104644`, application init | Start-up hook |
| `00242EF8` | The manager's one call of the router `get_next_state` (`00242BA0`) | Router entry: asks the original, changes its answer in two places, and refuses HOME and sleep for the steps that read or write |
| `002445EC` | Game list, first instruction of its update: its set-up is done | List-ready entry: lets HOME and sleep through again, then the instruction |
| `00246F48` | Game search, first step: start of the cartridge scan | Check entry: asks Bank first, then goes on with the search or ends it through the search's own steps |
| `00248CDC` | Bank step, sub-state 0: create the server request | Branch to `00248E44`, where the original continues after a "yes" |
| `00245728`, `002460B8`, `002488A0`, `002483CC` | Reading Gen 5 and Gen 1/2: remote validation | Skipped |
| `002458DC` | Reading Gen 5: per-slot result code | Slot entry: answers the "skip" code for empty slots |
| `00247478` | Question step: nickname notice (message `0x10`) | Branch to the original "go to sub-state 4" (`002474E0`) |
| `00243528` | Cartridge task: read the game code | Cartridge-id entry |
| `002439A8` | Cartridge task: read the save | Cartridge-read entry |
| `002437F4`, `0024397C` | Cartridge task: write part of the save | Cartridge-write entry |
| `00244F10` | Game list: go on to the Virtual Console titles | List-next entry |
| `00244908` | Game list: a DS entry was confirmed | Select entry |
| `002412D4` | Scanner of the Virtual Console titles: is this row's title installed? (`bl 002512A8`) | Title-scan entry: no for a row in another language than the listed one, otherwise the original's answer |
| `0025C5FC` | Language screen ended: switch the language of the screens | Language-chosen entry |
| `0025C618` | Language screen ended: record the language in the manager | Skipped |
| `0022BEE0` | Language screen: order and place the nine list entries | Language-order entry |
| `0022B5B0` | Language screen: cursor range and enabled buttons of the list | Language-buttons entry: the original, then the shown entries only |
| `0022B6E0`, `0022B6E4` | Language screen: list index 5, Japanese, goes to the kana/kanji part | List index 7, Back, goes to part 3, the end; every language goes to the confirm part |
| `0022B3B8` | Language screen: B on a part without a Back button does nothing | Language-back entry: on the list, presses the Back button |
| `0024BA44`, `0024BA74` | Title screen: before and after it loads its archives | Title-begin and title-end entries: the archive with logo and start prompt is read in the chosen language |
| `0013AEE4` | Building a layout: the layout binary on its way to `Layout::Build` | Layout check: the same, and the payload adjusts the language screen's layout first |
| `0024A150` | Transfer, sub-state 0: create the upload request | Deliver entry, which goes on by its result |
| `0024A3C0` | Transfer: `mov r0, #0xa`, the one place that selects the commit request (sub-state `0xA`) | `mov r0, #0xe`: the final save comes next |

The offline flow follows zaksabeast's Transporter-Offline-Patch. Unlike that patch, the transfer ends with the original final save and success message.

### Placement

Same method as the [Bank patch](#placement). Transporter's main function is the same engine code as Bank's: `00103D9C` is its one `BL` to application init. The start-up hook sits at `0028D1AC`, in the zero bytes after the original text. The layout check for `0013AEE4` lies there too, at `0028D1DC`: that site is on the way of every layout, and the first layouts are built during start-up, before the payload is executable (a branch into the payload from there was a prefetch abort at its entry word). The check hands only a layout binary of the language screen's size to the payload, and that layout is built when the screen is opened. No site that only the language screen runs can stand in for this one: its constructor (`0022BD28`) reaches `Layout::Build` through `001A1964` and the set-up function `0022C438`, which eight screens share (vtable slot `+0x2C`). That function sets up the screen's resources (`0022ECC8`, called with the screen's archive table; taken to be the archive load, not traced) and builds the layouts (`0022E8AC`, then `001DD234`, `0012ECC8` and `0013AE2C`) in one go, and the one call it makes back into the screen (vtable slot `+0x28`) comes before both. The layout binary is first looked up inside `0013AE2C` itself, at `0013AEE0`. Like Bank's hook it flushes the caches whole (Luma SVC `92`/`94`): its code range is only 12 KiB, and a range flush of a small range is carried out by virtual address on every core, which faulted in the kernel on core 1 under another process's address space. The payload links at `00364000`, the first page after the original zero-initialised data, which follows the data section directly (`003293FC..003638A4`) and is not page aligned. The paired exheader enlarges the data segment and sets BSS to zero (fields `0x34`, `0x38`, `0x3c` only); the IPS writes the former BSS as zero, then the payload. `code.ips` and `exheader.bin` only work together.

### Bank data object

Transporter contains Bank's data object (initialiser `00115410`, body `BB518` bytes).

- **Reaching it from a task:** `[task + 8]` manager, `[manager + CC]` data object, `[object + BB524]` transport accessor, `[accessor + 4]` body pointer.
- **Transport box:** record `i` at body pointer `+ AAF1C + i * E8`, tag `i` at `+ AD604 + i` (the body offsets under [Native body](#native-body) plus 8).
- Reading a game (`00245460` for Gen 5, `002463D8` for Gen 1/2) clears the 30 slots and stores each converted Pokémon at its source position through `0019A6F4`, which also writes the tag from the Pokémon object's `+0x10`.

The patch copies the 30 records and 30 tags as they are.

### Session flow

- **Routing.** The original's router `get_next_state` (`00242BA0`) is called from one place in the manager's step function (`00242EF8`), with the finished step and its outcome byte (`task + 0x30`). The router entry calls it and changes two answers (`navigation::destination`, tested on the PC against a transcription of the router's cases): after the game list with a chosen game comes the reading of the game, where the original answers "connect"; and after the reading comes the Bank step, whatever the outcome, where the original's server steps came in between. A cancelled step (the manager's flag `+0x1A`) is left to the original, which answers the disconnect step. Nothing inside the router is edited.
- **Check at START** (game search, `00246F04`, the task after the title screen). Its steps: 0 start the cartridge scan, 1 wait for it, 2 look for Virtual Console titles, 3 end with "found", 4 show message 0 ("Could not find a game…") through `0019B50C(ui, 0, 1)` with the ui at `task + 0x34`, 5 wait for the acknowledgement and end with "none", which the router answers with the disconnect step and then the title screen. The first instruction of step 0 (`00246F48`) branches to the check entry's stub, which calls the entry with the task (`r4`). The entry reads the two slots of Bank's transport box on the worker thread and answers: 0 go on (the stub runs the replaced instruction and continues at `00246F4C`), 1 come back next frame (`00247024`), 2 a message is on screen (`00247000`, which selects step 5), 3 end without a message (`0024702C`; only if the task had no dialog owner). The message is the original's 5 when a slot holds Pokémon, and its 3 when the slots cannot be read or two valid empty ones show an unfinished Bank save (`layout::refusal_message`); the entry shows it with the call step 4 makes. The original shows message 3 from a reply listener of its sign-in (`0025C964`) when the server asks for a cleanup. The check runs at every START, so a second transfer in one run meets it again.
- **Bank step** (`00248C68`, after the game is read). The original asked the server here whether Bank can take the Pokémon. Its other messages are `0x23` ("the server has been locked") and `0x22` (no Game Card). After a "yes" it continues at `00248E44`: unless the Game Card was removed (message `0x22`), it writes its note into the chosen game ([below](#the-originals-note-in-the-game)), starts saving that game (`0024B92C`, mode 2) and waits for the save (`0024B868`) before it ends with "allowed". The patch branches from the step's first instruction (`00248CDC`) to `00248E44`, so the request and its reply are never entered and the note and the save happen as before; none of the patch's code runs in this step.
- **Question step** (`00247304`). Counts the transport box (`0024D624`) and at zero shows its own message and ends the session.
- **Transfer** (`0024A0C4`). Original sub-states: 0 create request, 1 serialize the data object and upload it, 3 save the source game without the Pokémon, `0xA` commit request, `0xE` final save, `0x10` success message, `0x11` failure message. Patched: sub-state 0 (`0024A150`) branches to the deliver entry's stub; on 1 the stub continues at the original removal code `0024A274`, on 2 at the original return `0024A5DC` to come back next frame, and on anything else it selects `0x11` through the store at `0024A4C0`. Sub-state `0xA` is never selected: its one setter (`0024A3C0`) selects `0xE`.
- **Nickname notice.** Sub-state 2 of the question step shows message `0x10` unless a flag in the Bank data object says it was shown. Names were only ever erased on the server's per-slot codes (`0xFA`..`0xFC`, `0xFE`, `0xFF`), which never arrive offline.

### The original's note in the game

The original keeps 32 bytes in the chosen game that are its half of a transaction with the server. `0019BA68(manager, out)` reads them and `0019A13C(manager, in)` stores them: for a Gen 5 game inside the save image the cartridge task holds (`+0x1DA00` in the active copy), for a Virtual Console title through that title's save.

| Offset | Size | Content |
| --- | --- | --- |
| 0 | 8 | ID the server gave for this copy and session |
| 8 | 8 | Transfer number; zero means "nothing in progress" |
| `0x10` | 12 | Three words of the server's reply; the first is compared together with the ID |
| `0x1C` | 1 | State: 1 started, 2 sent |

| Moment in the original | Note | Server |
| --- | --- | --- |
| Bank step, after a "yes" (`00248ED0`) | ID, number, state 1; the game is saved | remembers an open transfer |
| Transfer, after the upload (`0024A300`) | state 2; saved with the box without the Pokémon | holds the Pokémon, not yet in the transport box |
| Transfer, after the commit request (`0024A484`) | number cleared; saved | puts them into the transport box |

At the next session three server steps used the note. Step `0xA` (`00248814`) asked the server whether a transfer was open. Step `0xD` (`00246B68`) read the note: with a number and state 1 it had the server discard its copy, with state 2 it had the server finish, then it cleared the number and saved. Step `0xE` (`00243AF4`) first compared the note's ID with the server's; another copy of the game got message 6 ("You previously used a different copy…"), with the choice of giving up the Pokémon in transit (messages `0x1A`, `0x1B`). The result was a transfer that a power failure could neither lose nor double, sorted out with the same copy of the game.

**Offline** the note is written and never read:

- The values come from the server's reply, which never arrives. The number is taken to be zero (the reply buffer was not traced to its source), so the note reads "nothing in progress"; the state byte becomes 1 and then 2.
- The three saves still happen, because they are the original's: once after the game is read and before the question, with the box in the transfer, and at the end. Steps `0xA`, `0xD` and `0xE` are never entered ([Session flow](#session-flow)).
- What takes the transaction's place is the order of the two writes: the delivery is written, flushed and read back before the game is saved without the Pokémon. A power cut in between leaves the Pokémon in Bank and in the game: never lost, but possibly doubled. That is the one guarantee of the original the patch does not give.
- Giving it would need Bank's transport file in the server's role: a delivery written as unconfirmed, which Bank ignores; the game's save; then the confirmation; and an unconfirmed delivery sorted out with the same copy of the game by the note's state. This is not built.
- A swapped Game Card is not what the note guarded against within a session. The original checks its "card removed" signal (`001DE43C`) before each of the three saves and stops with message `0x22`. A save on the SD card is addressed by its file name for the whole session, whatever cartridge is inserted ([Saves on the SD card](#saves-on-the-sd-card)).

### Empty Gen 5 slots

The Gen 5 reader (`002455D0`, sub-state 4) runs 14 local checks (table `002AFFE0`) and a conversion on every slot whose per-slot result code (`[info + 0x48 + 4 * slot]`) is zero. The server sent `0x14` for empty slots, which skips them. Without it, empty slots fail the checks, set the "removed" flags at `info + 0x1C0`, and raise a false dialog. The slot entry answers `0x14` for a slot that holds no species ([gen5.rs](../patches/transporter/src/gen5.rs)). The Gen 1/2 reader skips empty slots itself.

The species has to be decrypted; the stored header does not tell. A box record is 136 bytes: personality value, flags (u16 at 4), checksum (u16 at 6), then four 32-byte blocks, XORed word by word with the LCG stream seeded by the checksum and stored in the order `((personality >> 13) & 31) % 24`. The games store empty slots in two forms: a slot that was cleared has personality value 0 and checksum 0; a slot that was set up and never used has personality value 0 and checksum 4, because a blank record carries the genderless flag (byte `0x40`).

### Refusal dialogs

The question step shows one fixed sequence when the reader flagged anything (`002474E8`..`00247648`): message `0x11` (there is a Pokémon that cannot be sent) if `info + 0x1C0` is set; then one message per reason flag, `0x12` for an Egg (`+0x1C1`), `0x13` for a fused Kyurem (`+0x1C2`), `0x14` for a problem with a Pokémon (`+0x1C4`), `0x41` for too many held items (`+0x1C6`); then always `0x15` (the Pokémon were removed from the Transport Box). A Pokémon refused by the local checks therefore produces three dialogs, as it did with the server. The patch does not touch this sequence.

### HOME button and sleep

Same mechanism as [Bank](#home-button-and-sleep), in the same engine code: mask at `002F49E8 + 2`, set `0022AEEC`, clear `0011A5FC`. A HOME press is accepted only while the mask is zero (`0010AD78` then sets the flag at `+1`), and the main loop carries an accepted press out some frames later without looking at the mask again (`001044D0`). The original sets bit 1 in the connect step (`00248B58`) and clears it in the disconnect step (`002470F4`).

The original set bit 1 once for its whole online session. The patch follows the rule of the Bank patch instead: refused while files are read or written, let through on every screen that waits for the user.

- **By the kind of the next step.** The router entry (`hooks::transporter_next`) replaces the manager's call of the router. After the original has answered, `navigation::destination` gives the next step and `navigation::loads` says whether it reads or writes: the game search (with the Bank check in front and the cartridge scan), the game list, reading the chosen game, the Bank step (its record save), and the transfer. For these the entry sets bit 1; for every other next step it clears it. Which steps start a read or a save was read from their code: the search calls the scan (`00153E3C`), the list's set-up and the reader call the cartridge read (`0019ADEC`) and the list's set-up also starts the Virtual Console scan, the Bank step and the transfer store the record and begin a save; the list's update, the question step and the disconnect step call none of these.
- **The game list** both reads and waits. Its set-up (`00244D2C`, called until it reports "done") reads every listed save; its update then runs the list. The first instruction of the update's first step (`002445EC`) runs once, after the set-up and before any input, and the list-ready entry clears bit 1 there.
- **A message of the Bank check** is shown inside the search step, so the check entry clears bit 1 before it shows one. The original's own messages inside loading steps, "Could not find a game…" and the closing message of a transfer, keep it set until they are acknowledged; no code of the patch runs there.
- **Accepted presses.** As in Bank, setting the bit also clears the flag at `+1` (`hooks::refuse_home`), at every beginning of a refused step, so a press accepted in the frame before is not carried out while files are read or written.
- The original disconnect step still clears bit 1 as well. The original's own bit for a running Virtual Console save (value 4, `00251490` and `001DFAF8`) is not touched.

### Worker thread

The check and the delivery run on a thread created per job through the original `svcCreateThread` wrapper `0010E250` (priority `0x31`, default processor) and ended through `0011F140`; the handle is closed with an own `svc 0x23` wrapper. The entries answer "pending" until the thread has stored its result. If no thread can be created, the job runs on the calling thread. State and a 16 KiB stack are statics of the payload.

### File access

Transporter's SDK wrappers: open-directly `001DF448`, read `0015930C`, write `00159390`, size `001593F0`, close `00159364`; `fs:USER` handle at `00311F80`.

- Bank's extdata: archive 6, binary path `{1, C9B, 0}`, ASCII file paths `/mover.bin` and `/mover.alt.bin`, one per slot of the transport box. A file that is missing or does not open makes its slot unreadable, and the strict slot read then refuses; Transporter never creates, deletes or replaces a file. Transporter's exheader grants neither: its storage info lists only its own extdata id (`C9C`), and its filesystem access mask (`0x10`) has no SD card bit. Both work because Luma3DS's own process manager registers every process with a filesystem access mask of all bits (`sysmodules/pm/source/launch.c`, `loadWithoutDependencies`: "Not in official PM: patch local caps to give access to everything"), whatever the exheader says and for every title. Luma3DS has had its own process manager since v10.0. The paired `exheader.bin` therefore leaves the access fields as they are.
- SD card: archive 9 with an empty path.

### Cartridge and game list

- **Cartridge task** (`00243510`; mode at `+0x30`: 2 scan before the game list, 1 read, 0 write). It runs on a thread of the original and reaches the cartridge through three functions: `0021AA0C(out)` reads the four-letter game code, `0021A7E0(9, offset, buffer, length)` reads the save, `0021AB50(9, offset, buffer, length)` writes part of it. It accepts `IR` + `A`/`B`/`D`/`E` + a language letter.
- **Game list** (`00244D2C` builds it, `002445A4` runs it). An array of kind bytes at `task + 0x3C`, count at `+0x64`, cursor at `+0x66`, heap at `+0x68`. Kinds 1..4 are Black, White, Black 2, White 2; higher kinds are the 39 Virtual Console titles (table `002C2A80`). State 1 notes the cartridge's kind and starts a read (`0019ADEC(manager, heap)`), state 2 adds its entry and goes to state 3, where the Virtual Console titles follow. Confirming an entry of kind 1..4 stores the game version in the manager; the cartridge is read again afterwards. `001DE43C(manager)` reports a removed card.

### Saves on the SD card

The three cartridge calls are redirected at their call sites. `Saves` ([sdsave.rs](../patches/transporter/src/sdsave.rs)) holds what is offered and which item is presented to the cartridge code right now: the cartridge slot itself, or one save.

- **Scan.** In scan mode the cartridge-id entry calls the original first, then probes the accepted file names in the [listed language](#language-screen) (8 open attempts at most). The items are kept in the order of the original's list kinds (Black, White, Black 2, White 2). A Gen 5 cartridge in that language stands at the place of its game, and the save of that game is left out. The first item, cartridge or save, is presented in the cartridge's place. A Gen 5 cartridge in another language is no item; if no save stands in its place, the entry answers a game code of zero, which the cartridge task takes as "no Pokémon cartridge" (`00243554`).
- **Calls.** While the cartridge slot is presented, all three entries pass through to the original functions. While a save is presented, the game code comes from its file name, reads and writes go to the file, and the save is copied to `<name>.sav.bak` before the first write of a session.
- **List.** The list-next entry runs after each read for a DS entry: it notes the next item's kind and starts its read, as state 1 does for a cartridge, so the cartridge and every save get an entry. The select entry notes which entry was picked. List positions are mapped back to the cartridge or a file, so an item the original rejects does not shift the others.

The list holds 40 entries: 40 kind bytes at `task + 0x3C` up to the count at `+0x64`, and 40 info entries of 0x24 bytes at `[task + 0x38] + 0x90`, ending before the fields at `+0x630`. The original does not check that bound; it cannot exceed it with one cartridge and 39 Virtual Console titles. The patch does not check it either and builds the list with the original's loop unchanged: with one language listed there are at most 11 entries (one per DS game, where the cartridge stands in for the save of its own game, and seven titles). A host test holds that for the payload's copy of the titles' languages (`one_language_never_fills_the_game_list`), and the builder compares that copy with the table in the executable (`002AFE0C`) and refuses to build if one language could fill the list. The redirect is based on zaksabeast's DreamRadarCartRedirect, which replaces the same three functions for one hard-coded file.

### Language screen

**Nothing is saved by the original.** Its exheader declares no save data and no extdata. At start, `001058E8` reads the console's language (`001E49BC`, config block `000A0002`), maps it with `00106EAC` and stores it in the byte at `[0032938C]`, which `001E356C` returns. The only other caller of the setter `0022AF28` (`00240D9C`) passes a start-up parameter. Language ids: 1 Japanese, 2 English, 3 French, 4 Italian, 5 German, 7 Spanish, 8 Korean, 9 and 10 Chinese; consoles in Dutch, Portuguese or Russian get 2.

**States.** `get_next_state` (`00242BA0`) and the task factory (`0019CEE4`): 2 is the title screen, 1 the language screen (task vtable `002E6680`), `0x11` a reload that returns to the title screen, 8 the game list. The title screen's outcome `0x12` leads to state 1.

**The screen** (constructor `0022BD28`, vtable `002E543C`, `0xC0` bytes):

| Offset | Content |
| --- | --- |
| `+0x10`, `+0x5C` | Button manager, layout |
| `+0x7C`, `+0x7D` | Part (0 list, 1 kana/kanji, 2 confirm, 3 end) and its step |
| `+0x80` | List index of the picked entry |
| `+0x84` | Kana (0) or kanji (1) |
| `+0x88` | Nine list indices in display order |
| `+0xAC`, `+0xB0`, `+0xB4` | First and one-past-last position of the cursor's range, cursor |

- List index to language id, table `002CA380`: `2, 7, 3, 5, 4, 1, 8, 9, 10`. Buttons 0..8 are the languages in that order, 9 and `0xA` kana and kanji, `0xC` confirm, `0xB` and `0xD` back (button handler `0025BBD8`).
- `0022BC0C` builds the display order, the current language first and the others in table order, and places the nine panes 26 units apart, from 104 down to -104, through `001A1B3C(layout, 1, pane, &xyz)`; the panes of the list indices are in table `002CA3A4`.
- `001A1FE0` sets the cursor's range for the part about to be shown and enables exactly the buttons whose ids are in it (`0022CA5C(buttons, id, 0)`, the others `0022CB98(buttons, id)`). `001A18CC(buttons, id, sound)` gives a button its sound; `0x5000A` is the cancel sound.
- The key handler `0022B330` moves the cursor, presses the button under it on A (`001A2304(buttons, id)`), and on B presses the Back button of part 1 or 2.
- A pressed list button stores its list index in `+0x80` (button listener `0025BBD8`). After the list has faded out, `0022B6DC`..`0022B6F8` send list index 5 (Japanese) to part 1 and every other to part 2. The text frame of the picked language comes from table `002CA70C`; index 5 has frame 0.
- Part 3 calls the task's listener `0025C5BC(listener, language id of +0x80, kanji)`, which switches the language (`0022AF28`), records it in `[manager + 0x168]` (`0022C06C`) and ends the task.

**The patch** ([language.rs](../patches/transporter/src/language.rs)) keeps one byte: the chosen language id, 0 until a choice is made. The listed language is that byte or, while it is 0, the language of the screens; ids without source games become 2.

- The language-chosen entry stores the id instead of switching, if it is a language games exist in. The record in the manager is left alone, so everything else keeps seeing the language of the screens. The reload state still runs.
- **The list.** The language-order entry stands in for `0022BC0C`: the six languages other than the listed one in table order, then list index 7 as the Back button, centred and 32 units apart (96 down to -96), the distance of the long buttons in Bank's main menu (`Turtle_lower5.bclyt` in Bank: 58 down to -102), which the original's nine entries, 26 apart and overlapping, have no room for; the listed language and list index 8 follow and are placed far off the screen. The language-buttons entry runs the original `001A1FE0` and then ends the cursor's range after the shown entries and disables the buttons of the others.
- **Back.** List index 7 is sent to part 3 instead of a sub-screen, so the screen ends as after a confirm; the listener then reports language id 9, which the language-chosen entry ignores. Button 7 gets the cancel sound, and B on the list presses it.
- **Back's text.** The language screen's own message file (36 of the text archive; 24 is the game-selection screen's) has no "Back": its two Back buttons say "Select language" (message 6). The game-selection screen's Back is message `0x1F` of file `0x18` (`002427BC`). The language-order entry opens that file the way `0022EF30` opens a screen's own (`001E6360(0x28, heap)`, `001DFB00(memory, 001E2540(10), file, heap, 1, 5)`, heap from the constructor's parameter `+4`), sets the pane's text with the original's `0019F7B0(strings, layout, pane, messages, message)`, which copies the string, and deletes the object through its second virtual function, as `0022F004` does.
- **The Back button's panes** ([lytpatch.rs](../patches/transporter/src/lytpatch.rs)). The language screen is one of Bank's screens, and the Back buttons of Bank's menus (`Turtle_lower5.bclyt` in Bank, panes `5`, `l`, `2h`) are a 304x30 long button holding a text pane centred at (0, 0) and a 30x30 `return_icon.bclim` at x 137, drawn as the texture is: teal, `008899` (material black colour 0, white corner colours). The game-selection screen's Back (`Salmon_lower4.bclyt`, pane `3`) is the same in Transporter's brown: its material's black colour `FFFFFF` turns the icon white and the pane's corner colours tint it `533324`, a brown near the dark end of the button's bar (`482321` to `7C3C2F`) but none of its pixels. The language screen's layout (`Turtle_lang_select_lower.bclyt`, 12,680 bytes; pane names are the pane ids in base 36) has the same long button in blue for each list entry, but with the language's name as a picture. The layout check sees every layout binary before `Layout::Build` (`0013AEF8`); the payload gets the ones of this size and changes exactly this one, identified by its CRC-32: the picture `1a` of the Simplified Chinese button `m` becomes the icon (the texture name it refers to is replaced by `return_icon.bclim`, which the common archive `a/0/4/5` holds and this screen loads; size and position as in Bank; the material's black colour, `323232` for the name pictures, becomes 0 as in Bank's icon material, so the icon has the texture's teal), and the text pane `2e` of the kana/kanji part is moved into `m` and centred. The button itself, `m` with `long_button.bclim`, is not changed. A layout is a flat list of sections in which `pas1`/`pae1` bracket a pane's children and nothing refers to a section by offset, so the pane is moved by rotating bytes; no animation of the screen names the moved or changed panes. If the layout is not the known one, it is left alone: the entry keeps its Chinese picture and still works as Back.
- **Virtual Console titles.** The scanner (`0024120C`) walks the table `002AFE0C`, 39 rows of `{ version, language id, title index }`, and gives row `i` the list kind `i + 5`. For each installed title, number `j` in the order found, it writes the kind to `scanner + 0x30 + j` and the trainer's name and ID to entry `base + j` of the list's info table (`[task + 0x38] + 0x90`, 0x24 bytes each: name 0x1A, ID at `+0x1C`), where `base` (`scanner + 0x5A`) is the number of entries before the titles. The game list appends the kinds in the loop `00245064`..`00245094`. Kinds and info only stay in step if every found title is listed, so the titles of other languages are left out where they are found: the title-scan entry replaces the scanner's question `002512A8(version, language)` (`002412D4`, its only caller) and answers no for them. Leaving them out in the list loop instead, as an earlier version did, showed each later title with the name and ID of its predecessor; the game opened was the right one, since confirming reads version and language from the table by the kind (`002448C0`, `0024492C`).
- **Gen 5.** The scan for SD saves and the cartridge use the letter of the listed language ([Saves on the SD card](#saves-on-the-sd-card)).

**The title screen.** The start prompt and the logo are pictures (`push_start_button.bclim` in the lower layout `Salmon_lower3`, `Salmon_logo.bclim` in the upper one), both in archive `a/0/4/6`, whose one entry exists in a variant per language id (0 the default, used for Japanese; 2, 3, 4, 5, 7, 8, 9, 10). English and Korean have the same prompt; the logo differs in all. An archive entry is read in the variant of the global at `002F4D7C` (`001DDC0C`), which the original sets through `001071B0` to the language of the screens whenever that changes (`00104EBC`, its only caller). The title screen's constructor (`0024B9E4`) loads its three archives (`0x2C`, `0x2D`, `0x2E`) and builds its layouts in one call (`0024BA70`); the title is built anew each time the language screen ends. The two entries call `001071B0` around that call: with the chosen language before it, if one was chosen, and with the language of the screens after it. The other two archives have one variant only, and the screen's texts come from the text archive of the screens' language, which that global does not touch.

**A console set to Chinese.** The screens are in Chinese (ids 9 and 10), the listed language is English until one is chosen, so English is the hidden entry. The Chinese entries are never offered as languages: list index 7 is the Back button, index 8 is hidden. Back reports id 9, which is ignored whatever the screens' language is. All ten text archives, both Chinese ones included, have message `0x1F` in file `0x18`.

### Messages

Texts are in the GARC `a/0/0/6` of the RomFS, file 23 for English (Game Freak text format). Used here: `0x03` the last session did not complete, open Pokémon Bank; `0x05` transport box not empty, `0x08` no Pokémon to move, `0x0A` moved, `0x0B` failure, `0x10` nickname notice, `0x11`..`0x15` and `0x41` Pokémon that cannot be sent, `0x16` held items returned.
