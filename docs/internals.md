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
| [patches/bank/](../patches/bank/src/) | ARMv6K code injected into Bank. Hooks in [runtime.rs](../patches/bank/src/runtime.rs) and [dex.rs](../patches/bank/src/dex.rs), routing in [navigation.rs](../patches/bank/src/navigation.rs), file coordination in [bank_files.rs](../patches/bank/src/bank_files.rs), worker thread, filesystem and game adapters. |
| [crates/offline-core/](../crates/offline-core/src/) | `no_std`, no `unsafe`. Bank file and journal, side files, transport rules, Miles rules, game-image hashing. |
| [crates/patch-builder/](../crates/patch-builder/src/) | Host tool. Verifies the inputs, parses the linked ELF, emits the paired `code.ips` and `exheader.bin`. The profile is [bank15.rs](../crates/patch-builder/src/bank15.rs). |

### Edits to the original

18 regions and nine entry functions. The builder checks every original word before patching and refuses to build if the five main-menu locations (`001d6554`, `002b33a4`, `002b33d4`, `003617bc`, `003617dc`) are not original.

| Address | Original | Replacement |
| --- | --- | --- |
| `001040a4` | `BL 0010494c`, application init | `bank_bootstrap_startup`: runs init, makes payload pages executable, exits on failure |
| `00313910` | Zero text padding | Start-up hook (192 bytes) |
| `0029f338` | Enters delete/recreate of a corrupt extdata archive | Branch to the existing error cleanup `0029f4fc`; the archive is kept |
| `001d3bf4` | Timestamp helper with an online-client check | Tail branch to `bank_offline_timestamp` (local clock `0023a754`) |
| `002a5a2c` | `BL 002a5580`, task router | `bank_offline_next` |
| `00361e84` | Task `0xB` update `002af034`, a server check | The original "finished" stub `002af124`; the task's start-up routine stays |
| `002d1034` | `BL 00292d64`, selected-game secure-value check | `bank_offline_validate_game` (forwards the original) |
| `00361cfc`, `00361d0c` | Task 9 update `002ae568` and busy poll `002b4a20` | `bank_offline_load` |
| `00361ed4`, `00361ee4` | Task `0x10` update `002af460` and busy poll | `bank_offline_load` |
| `00362034`, `00362044` | Task 7 update `002b1cf8` and busy poll | `bank_offline_save` |
| `00361be4` | Task `0xC` update `002ad1bc`, reward guard | `bank_offline_rewards` |
| `00361a08` | Task `0xD` update `002a9750`, reward claim | `bank_offline_rewards` |
| `0033d30c` | Box screen child callback `002a7578` | `bank_offline_dex_save_request` |
| `003600cc` | Records task update `0026d8ec` | `bank_offline_dex_records_update` |
| `0033d34c` | Records completion callback `002a71a8` | `bank_offline_dex_records_finish` |

Five internal `bank_svc_*` wrappers give SVC `23/24/38/39/3a` ordinary AAPCS call boundaries, because the kernel overwrites registers that inlined code would still use. The builder checks their exact instruction bodies.

### Placement

The payload links at `003fb000`, after the original BSS. The paired exheader enlarges the data segment and sets BSS to zero (fields `0x34`, `0x38`, `0x3c` only); the IPS writes the former BSS, the payload, and its zero tail explicitly. Luma bounds IPS records by the allocated image, so `code.ips` and `exheader.bin` only work together. The start-up hook duplicates the process handle (SVC `27`), sets the payload's code pages RX (SVC `70`), and flushes caches (Luma SVC `91`/`93`, by range). The range calls are safe only because Bank's range is large (76 KiB): the kernel then flushes the whole cache. A small range is flushed by virtual address on every core and faults where another process runs; the linker script asserts the size.

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

**No usable game.** Selection state 0 would repeat notice `0x1A` forever. Task 9 checks the availability bytes (`manager + kind*12 + 1`), shows the notice once via `001d6650(ui, 0, 0x1A, 1)`, and routes to cleanup.

**UI helpers.** Shared UI at `manager(+0x2c) + 0x80`; task UI pointer at `+0x3c` (tasks 9, 7), `+0x38` (`0x10`, `0xC`), `+0x40` (`0xD`); context at `ui + 0x5c`. Loading: `001d5b44(ui)` then `0025da24(ui, message)` with message 2 (open) or `0xE` (load). Dialog poll `001d6600(ctx) == 4`. The error dialog writes its text into the UI's own string object (`ui + 0x94`).

### Bank object

`0xBB530` bytes with vtable `003626FC`. `002CB870` saves the body, `0023650C` restores it. The transport-box model check is `0022bd54`; the patch counts occupied slots itself (see [Hand-over from Transporter](#hand-over-from-transporter)). The worker reads each file once into its final place in a staging buffer; the main thread only ever sees the whole body.

### Filesystem calls

The first argument is a pointer to the session or file handle; a negative `i32` is failure.

| Address | Call | Arguments after the handle pointer |
| --- | --- | --- |
| `0020a468` / `0020a438` | OpenArchive / CloseArchive | `out: *mut u64, id: u32, path: Path` / `archive: u64` |
| `001654f8` | CreateFile | `transaction: u32, archive: u64, path: Path, attributes: u32, length: u64` |
| `001657ec` | OpenFile | `out: *mut u32, transaction: u32, archive: u64, path: Path, flags: u32, attributes: u32` |
| `001658c8` / `0016594c` | Read / Write | `out_count, offset: u64, buffer, length: u32` (+ `flags: u32` for Write) |
| `001659ac` / `00165920` | GetSize / CloseFile | `out: *mut u64` / none |
| `0016578c` / `0012146c` | Get / Set secure value | `first*, second*, value*, archive: u64, slot` / `archive: u64, slot, value: u64, option: u8` |

`Path` is `{ kind: u32, data: *const u8, byte_len: u32 }`, not libctru's order. The extdata archive is ID 6 with binary path `[1, 0xc9b, 0]`. Sync is a zero-length Write with flags `0x10001`. CloseFile does not close the kernel handle; SVC `23` must follow. The filesystem session lives at `00390100`.

### HOME button and sleep

Bank keeps an activity mask in the byte at `00372988 + 2`; `001d4d90(bit)` sets a bit and `00229eb4(bit)` clears it. The main loop (`0010455c`) calls `0010ba04` every frame, which reports a change to the system, and the suspend callback `001050e0` refuses while the mask is not zero. A HOME press is accepted only while the mask is zero: `0010ba04` then sets the flag at `+1`, and the main loop calls the HOME handler `0010784c` once the current scene is ready (`00108634`), which can be several frames later. The handler does not look at the mask again.

- **Bit 1**: the original sets it for the whole online session and clears it in cleanup (`002abc7c`). The patch sets it in the routing hook whenever the next task is a loading task (`navigation::loads`: game scan, opening, scene set-up, loading, saving) and in `Runtime::submit`. It clears it in `Runtime::complete`, before the error dialog, before the "no game" notice, and in the routing hook for every other destination. Setting it per task, not per job, leaves no open frame between the scan and the opening.
- **Accepted presses**: because the handler ignores the mask, `Runtime::home_pending` holds back the first job of a load or save while the flag at `+1` is set (at most 600 frames). The HOME Menu, and closing Bank from it, therefore never meet a running job.
- **Bit 4**: set by the game-save writer launch (also in the patch's launcher in [native_game.rs](../patches/bank/src/native_game.rs)) and cleared by the original when the writer finishes.

The busy flag from `0025c420` is unrelated: it protects a task from Bank's own cancellation, not from HOME.

### Worker thread

One native SDK thread (`002320d4`, trampoline `001211e4`, event `00235c6c`/`00234a00`/`00231e70`) owns all storage handles. The main thread owns every native UI, task, and game object. A single mailbox with atomic states passes fixed-size jobs and the staging buffer; neither thread ever waits on the other. Task hooks submit a job and poll once per frame, with native busy protection (`0025c420`/`0025c3e8`) keeping the task alive. Stack 32 KiB with a guard pattern.

### Game saves

Save manager (0x100 bytes): selected kind at `+c8`; per kind `k` (1..8 = X, Y, OR, AS, S, M, US, UM), byte `k*12` is the kind, byte `+1` is "loaded", word `+4` the game object, word `+8` its writer. Game object: 0xb49d0 bytes, vtable `00362f88`, metadata pointer `+4`, kind byte `+8`.

- **Prepare** `002bc4bc` regenerates checksums (and Gen 7 signatures) after rotating the secure-value pair. Hash only after it.
- **Blocks** `002bc460(game, i)`: 55/58/37/39 data blocks (XY/ORAS/SM/USUM) plus a 0x1e8-byte metadata block, each written at a 512-aligned offset. Gaps and the file tail are not rewritten, so the expected after-image keeps their old bytes.
- **Write** uses the original writer thread (`001d37ec`, mode 4) and completion `0015dc74`, launched only after the journal is durable.
- **Secure value** slot `0x1000`. After an interruption, only a journal whose title and complete after-image match may advance the platform value from the file's previous to its current value.

### Rewards

- Count: `001d5ec0(bank + 0xbb520)`, 100 boxes × 30 slots. Balance: `001d588c` / `001d59fc` on `bank + 0xbb528`.
- Native accrual in guard state 0 and claim state `0x1b` is `count × (1/30) × hours × (1/24)`. The hook sets the stored reward date to the session date first, so native accrual is zero; local earnings are added to the balance when task `0x10` finishes.
- The claim hook starts task `0xD` at state `0x1b` with `+4c = 1`, `+50 = 0`, and session `+38 = -1`, which skips service queries and distributions. Allowed states: `0`, `7..0xd`, `0x16`, `0x1b..0x22`.
- Redemption runs in state `0xb` (choice at `+60`, quote at `+50`) and ends in `0xc`. Gen 6 writes the gift buffer from `(*(game + 0x1c384))->vtable[2]`: flag `+1ff |= 0x80`, Miles at `+6a2`, BP at `+6a0`. Gen 7 inserts a 0x108-byte record (type 3 at `+51`, amount at `+68`) through `002b6d68` into the 48-slot store at `game + 0xad43c`; the original ignores that call's result, so the hook compares counts and the new record.

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

All files are in SD extdata archive `0x00000C9B`. The body is cut at fixed offsets and each piece is stored unchanged, in body order ([sections.rs](../crates/offline-core/src/sections.rs)).

| File | Size | Body ranges |
| --- | --- | --- |
| `/bank.bin` | 1,461,332 | `0..AAF14`, `ACA44..AD5FC`, `AD61A..AD61C`, `B4A9C..BB518` (730,442 bytes). The Miles field `170` is stored as zero. |
| `/dex.bin` | 59,776 | `AD61C..B4A9C` (29,824 bytes) |
| `/transport.bin` | 14,112 | `AAF14..ACA44` then `AD5FC..AD61A` (6,990 bytes) |
| `/rewards.bin` | 160 | none; a 16-byte record |

File names in 3DS extdata are limited to 16 characters including the leading slash; a longer name fails at creation with `E0E046C7`. `FileName::path` refuses longer names at compile time.

**Rule for new features:** add a new file with tagged slots. Never change the size or layout of an existing file. A build that does not know a file ignores it; its saves advance the Bank snapshot, so the unknown file's slots stop matching and a later build treats them as "no data yet".

### Bank file

[store.rs](../crates/offline-core/src/store.rs), [format.rs](../crates/offline-core/src/format.rs). Two 192-byte metadata records (`BKOFMETA`), then two slots of a 32-byte snapshot header (`BKOFSNAP`) plus the payload. Field offsets are in the [crate docs](../crates/offline-core/src/lib.rs).

- A new snapshot goes to the inactive slot and is synced and read back; metadata is written to one replica, synced, then the other.
- A save: verify the game's before-image, `prepare_transfer`, write the game, read it back, `reconcile`. A before-image keeps the old snapshot, an after-image commits the new one, anything else blocks.
- A load checks the journal and the 32-byte header, then reads the payload once and checks its CRC on that pass.

### Side files

[sidecar.rs](../crates/offline-core/src/sidecar.rs). `/dex.bin`, `/transport.bin` and `/rewards.bin` each hold two slots at `index * slot_len`, where `slot_len = 64 + capacity rounded up to 4`. Slot header, little endian:

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

**Keeping the files in step** ([bank_files.rs](../patches/bank/src/bank_files.rs)). A save writes every side file's spare slot, tagged with the snapshot about to be written (generation + 1, CRC of the new Bank payload), and only then prepares the Bank journal. A load uses the slot whose tag equals the current snapshot. A rolled-back save leaves the old slots matching; a committed one makes the new slots match. Missing side files load as defaults; a Pokédex file without a matching slot is an error.

**Rewards record** (16 bytes, [rewards.rs](../crates/offline-core/src/rewards.rs)): u32 balance; u8 state (0 none, 1 record); u8 fraction 0..29; u16 saved count; accounted-through date (u16 year, month, day); 4 zero bytes.

### Hand-over from Transporter

[transport.rs](../crates/offline-core/src/transport.rs) holds these rules as tested functions. The Transporter patch runs `transport::deliver` from that file; Bank's tests deliver with the same function.

- A slot's payload is the native transport box: 30 records of `E8` bytes, then 30 format-tag bytes.
- **Transporter delivers** by writing one slot with no tag that holds the complete box the original Transporter built, with `count` = 1..30 occupied positions anywhere in it and a nonzero `aux` id different from every `aux` it read. It may deliver only if at most one slot is valid and every valid slot has `count` 0 (`may_deliver`). Two valid slots mean a Bank save is unresolved: the user must open Bank first.
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
| [patches/transporter/](../patches/transporter/src/) | ARMv6K code injected into Transporter. Bank check and delivery in [hooks.rs](../patches/transporter/src/hooks.rs), SD saves in [cart.rs](../patches/transporter/src/cart.rs) with their rules in [sdsave.rs](../patches/transporter/src/sdsave.rs), entry table and assembly stubs in [link.rs](../patches/transporter/src/link.rs), start-up hook in [bootstrap.rs](../patches/transporter/src/bootstrap.rs). |
| [crates/offline-core/](../crates/offline-core/src/) | `transport::deliver` and the side-file format, shared with Bank. |
| [crates/patch-builder/](../crates/patch-builder/src/) | Host tool. The profile with every edit and its original word is [transporter15.rs](../crates/patch-builder/src/transporter15.rs). |

### Edits to the original

32 instruction words, each checked against its original value by the builder, plus the start-up hook. The payload starts with ten entry branches, in this order: check, deliver, session, slot, cartridge id, cartridge read, cartridge write, list next, select, list limit.

| Address | Purpose | Replacement |
| --- | --- | --- |
| `00103D9C` | `BL 00104644`, application init | Start-up hook |
| `00242D10` | `get_next_state`: after the game list comes "connect" | Session entry: refuses HOME and sleep as the connect step did, answers GET_POKEMON |
| `00242D28`, `00242D2C` | `get_next_state`: after GET_POKEMON | Next is the Bank check |
| `00248CDC`, `00248D58` | Bank check: create the request, parse the reply | Skipped |
| `00245728`, `002460B8`, `002488A0`, `002483CC` | Reading Gen 5 and Gen 1/2: remote validation | Skipped |
| `002458DC` | Reading Gen 5: per-slot result code | Slot entry: answers the "skip" code for empty slots |
| `00247478` | Question step: nickname notice (message `0x10`) | Branch to the original "go to sub-state 4" (`002474E0`) |
| `00243528` | Cartridge task: read the game code | Cartridge-id entry |
| `002439A8` | Cartridge task: read the save | Cartridge-read entry |
| `002437F4`, `0024397C` | Cartridge task: write part of the save | Cartridge-write entry |
| `00244F10` | Game list: go on to the Virtual Console titles | List-next entry |
| `00244908` | Game list: a DS entry was confirmed | Select entry |
| `0024508C` | Game list: bound of the loop that appends the Virtual Console titles | List-limit entry |
| `00248D3C`..`00248D44` | Bank check: the answer | Check entry; its result is the next sub-state |
| `0024A150`..`0024A16C` | Transfer, sub-state 0: create the upload request | Deliver entry, then branch on its result |
| `0024A3D8`, `0024A3DC` | Transfer, sub-state `0xA`: commit request | Continue with sub-state `0xE` |

The offline flow follows zaksabeast's Transporter-Offline-Patch. Unlike that patch, the transfer ends with the original final save and success message.

### Placement

Same method as the [Bank patch](#placement). Transporter's main function is the same engine code as Bank's: `00103D9C` is its one `BL` to application init. The start-up hook sits at `0028D1AC`, in the zero bytes after the original text. Unlike Bank's it flushes the caches whole (Luma SVC `92`/`94`): its code range is only 8 KiB, and a range flush of that size is carried out by virtual address on every core, which faulted in the kernel on core 1 under another process's address space. The payload links at `00364000`, the first page after the original zero-initialised data, which follows the data section directly (`003293FC..003638A4`) and is not page aligned. The paired exheader enlarges the data segment and sets BSS to zero (fields `0x34`, `0x38`, `0x3c` only); the IPS writes the former BSS as zero, then the payload. `code.ips` and `exheader.bin` only work together.

### Bank data object

Transporter contains Bank's data object (initialiser `00115410`, body `BB518` bytes).

- **Reaching it from a task:** `[task + 8]` manager, `[manager + CC]` data object, `[object + BB524]` transport accessor, `[accessor + 4]` body pointer.
- **Transport box:** record `i` at body pointer `+ AAF1C + i * E8`, tag `i` at `+ AD604 + i` (the body offsets under [Native body](#native-body) plus 8).
- Reading a game (`00245460` for Gen 5, `002463D8` for Gen 1/2) clears the 30 slots and stores each converted Pokémon at its source position through `0019A6F4`, which also writes the tag from the Pokémon object's `+0x10`.

The patch copies the 30 records and 30 tags as they are.

### Session flow

- **Bank check** (`00248C68`). Sub-state 0 jumps to `00248D3C`, which calls the check entry and stores its answer as the next sub-state: 0 come back next frame, 3 continue, 7 the original "not empty" message. The check consults Bank whatever Box 1 held, as the original asked the server.
- **Question step** (`00247304`). Counts the transport box (`0024D624`) and at zero shows its own message and ends the session. It runs after the Bank check, so a full transport box is refused first.
- **Transfer** (`0024A0C4`). Original sub-states: 0 create request, 1 serialize the data object and upload it, 3 save the source game without the Pokémon, `0xA` commit request, `0xE` final save, `0x10` success message, `0x11` failure message. Patched: sub-state 0 calls the deliver entry; 1 continues at the original removal code `0024A274`, 2 returns and comes back next frame, anything else selects `0x11`; sub-state `0xA` selects `0xE`.
- **Nickname notice.** Sub-state 2 of the question step shows message `0x10` unless a flag in the Bank data object says it was shown. Names were only ever erased on the server's per-slot codes (`0xFA`..`0xFC`, `0xFE`, `0xFF`), which never arrive offline.

### Empty Gen 5 slots

The Gen 5 reader (`002455D0`, sub-state 4) runs 14 local checks (table `002AFFE0`) and a conversion on every slot whose per-slot result code (`[info + 0x48 + 4 * slot]`) is zero. The server sent `0x14` for empty slots, which skips them. Without it, empty slots fail the checks, set the "removed" flags at `info + 0x1C0`, and raise a false dialog. The slot entry answers `0x14` for a slot that holds no species ([gen5.rs](../patches/transporter/src/gen5.rs)). The Gen 1/2 reader skips empty slots itself.

The species has to be decrypted; the stored header does not tell. A box record is 136 bytes: personality value, flags (u16 at 4), checksum (u16 at 6), then four 32-byte blocks, XORed word by word with the LCG stream seeded by the checksum and stored in the order `((personality >> 13) & 31) % 24`. The games store empty slots in two forms: a slot that was cleared has personality value 0 and checksum 0; a slot that was set up and never used has personality value 0 and checksum 4, because a blank record carries the genderless flag (byte `0x40`).

### Refusal dialogs

The question step shows one fixed sequence when the reader flagged anything (`002474E8`..`00247648`): message `0x11` (there is a Pokémon that cannot be sent) if `info + 0x1C0` is set; then one message per reason flag, `0x12` for an Egg (`+0x1C1`), `0x13` for a fused Kyurem (`+0x1C2`), `0x14` for a problem with a Pokémon (`+0x1C4`), `0x41` for too many held items (`+0x1C6`); then always `0x15` (the Pokémon were removed from the Transport Box). A Pokémon refused by the local checks therefore produces three dialogs, as it did with the server. The patch does not touch this sequence.

### HOME button and sleep

Same mechanism as [Bank](#home-button-and-sleep): mask at `002F49E8 + 2`, set `0022AEEC`, clear `0011A5FC`. The original sets bit 1 in the connect step (`00248B58`) and clears it in the disconnect step (`002470F4`). The session entry sets bit 1 in place of the connect step; the original disconnect step still clears it.

### Worker thread

The check and the delivery run on a thread created per job through the original `svcCreateThread` wrapper `0010E250` (priority `0x31`, default processor) and ended through `0011F140`; the handle is closed with an own `svc 0x23` wrapper. The entries answer "pending" until the thread has stored its result. If no thread can be created, the job runs on the calling thread. State and a 16 KiB stack are statics of the payload.

### File access

Transporter's SDK wrappers: open-directly `001DF448`, read `0015930C`, write `00159390`, size `001593F0`, close `00159364`; `fs:USER` handle at `00311F80`.

- Bank's extdata: archive 6, binary path `{1, C9B, 0}`, ASCII file path `/transport.bin`. Transporter's exheader grants no extdata access; opening it worked under Luma on one console.
- SD card: archive 9 with an empty path.

### Cartridge and game list

- **Cartridge task** (`00243510`; mode at `+0x30`: 2 scan before the game list, 1 read, 0 write). It runs on a thread of the original and reaches the cartridge through three functions: `0021AA0C(out)` reads the four-letter game code, `0021A7E0(9, offset, buffer, length)` reads the save, `0021AB50(9, offset, buffer, length)` writes part of it. It accepts `IR` + `A`/`B`/`D`/`E` + a language letter.
- **Game list** (`00244D2C` builds it, `002445A4` runs it). An array of kind bytes at `task + 0x3C`, count at `+0x64`, cursor at `+0x66`, heap at `+0x68`. Kinds 1..4 are Black, White, Black 2, White 2; higher kinds are the 39 Virtual Console titles (table `002C2A80`). State 1 notes the cartridge's kind and starts a read (`0019ADEC(manager, heap)`), state 2 adds its entry and goes to state 3, where the Virtual Console titles follow. Confirming an entry of kind 1..4 stores the game version in the manager; the cartridge is read again afterwards. `001DE43C(manager)` reports a removed card.

### Saves on the SD card

The three cartridge calls are redirected at their call sites. `Saves` ([sdsave.rs](../patches/transporter/src/sdsave.rs)) holds what is offered and which item is presented to the cartridge code right now: the cartridge slot itself, or one save.

- **Scan.** In scan mode the cartridge-id entry calls the original first, then probes the accepted file names (56 open attempts at most). The items are kept in the order of the original's list kinds (Black, White, Black 2, White 2; languages in the order `JOFIDSK`). A Gen 5 cartridge stands at the place of its game and language, and the save of exactly that game and language is left out. The first item, cartridge or save, is presented in the cartridge's place.
- **Calls.** While the cartridge slot is presented, all three entries pass through to the original functions. While a save is presented, the game code comes from its file name, reads and writes go to the file, and the save is copied to `<name>.sav.bak` before the first write of a session.
- **List.** The list-next entry runs after each read for a DS entry: it notes the next item's kind and starts its read, as state 1 does for a cartridge, so the cartridge and every save get an entry. The select entry notes which entry was picked. List positions are mapped back to the cartridge or a file, so an item the original rejects does not shift the others.

The list holds 40 entries (kind bytes at `task + 0x3C` up to the count at `+0x64`), which the original could not exceed with one cartridge and 39 Virtual Console titles. With saves it could, so the list-limit entry replaces the bound of the loop that appends the titles (state 4, `00245064`..`00245094`): once the list holds 40, the bound reads as zero and the loop ends. The redirect is based on zaksabeast's DreamRadarCartRedirect, which replaces the same three functions for one hard-coded file.

### Messages

Texts are in the GARC `a/0/0/6` of the RomFS, file 23 for English (Game Freak text format). Used here: `0x05` transport box not empty, `0x08` no Pokémon to move, `0x0A` moved, `0x0B` failure, `0x10` nickname notice, `0x11`..`0x15` and `0x41` Pokémon that cannot be sent, `0x16` held items returned.
