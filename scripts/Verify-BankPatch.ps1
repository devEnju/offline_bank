<#
Independent check of the built package of one patch for Bank, in
build/binary/bank, build/migration, build/download or build/test
(Verify-Package.ps1, which shares no code with patch-builder). Writes
verification.json into the package folder.

-Kind says which package for Bank it is: the offline patch (bank), the
migration patch (migration), the download patch (download), or the console
test build of the offline patch (test). Each has its own list of edits,
written out below with their original words. The test build is the offline
patch with two more entry functions and two more edits, in the main menu,
which only it shows.
#>
[CmdletBinding()]
param(
    [ValidateSet('bank', 'migration', 'test', 'download')][string]$Kind = 'bank',
    # Another copy of the package or its payload (Test-Verify.ps1).
    [string]$PackagePath = '',
    [string]$ElfPath = '',
    # The extracted original (Extract-Cia.ps1); by default the folder under
    # build/extracted whose code.bin has the reviewed hash, whatever its name.
    [string]$InputPath = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'Verify-Package.ps1')

$program = @{
    Name = 'Bank'; CodeLength = 0x2ac000; TitleId = '00040000000c9b00'; TmdVersion = 6272; Remaster = 6
    CodeHash = '2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf'
    ExheaderHash = '39d93584b07901dfa7ea2fc8ce1236cdd92476584ee2dbb60c11a4ae9402abf7'
    Bootstrap = 0x00313910; BootstrapEnd = 0x00314000; Payload = 0x003fb000
    AppInit = 0x0010494c; StartupCall = 0x001040a4
}
$offline = $Kind -in 'bank', 'test'
$router = @{ bank = 'bank_offline_next'; test = 'bank_offline_next'; migration = 'bank_migrate_next'; download = 'bank_download_next' }[$Kind]

# What every patch edits: the start-up hook is called, the archive is kept,
# the router is the patch's own, and the start screen's call that writes its
# version line goes to the entry that adds the patch's name to it.
$edits = @(
    @(0x001040a4, 0xeb000228L, 'bl:patch_bootstrap_startup'),
    @(0x0029f338, 0xe594002cL, 'b:0029f4fc'),
    @(0x002a5a2c, 0xebfffed3L, "bl:$router"),
    @(0x002b49c4, 0xebfcb5ffL, 'bl:bank_version_shown')
)
# Every patch but the download, which runs the original online (there the
# first start keeps its prompts, and the original refuses HOME and sleep for
# its whole session itself): the "show a message" function lets HOME and
# sleep through for a message that waits, and the manager's set-up does not
# ask for a language (nop; mov r5, #1).
if ($Kind -ne 'download') {
    $edits += @(
        @(0x001d6650, 0xe92d41f0L, 'b:bank_message_stub'),
        @(0x002a4898, 0xebfeddfbL, 0xe320f000L),
        @(0x002a4940, 0xe1a05007L, 0xe3a05001L)
    )
}
switch ($Kind) {
    'migration' {
        # Task 3's update and busy poll run the conversion.
        $edits += @(
            @(0x00361ac4, 0x002abe08L, 'ptr:bank_migrate_scan'),
            @(0x00361ad4, 0x002b4a20L, 'ptr:bank_migrate_scan')
        )
    }
    'download' {
        # Task 9 and task 3 each have an entry for their update and their
        # busy poll; the reward step's update writes the files and ends as
        # cancelled.
        $edits += @(
            @(0x00361cfc, 0x002ae568L, 'ptr:bank_download_open'),
            @(0x00361d0c, 0x002b4a20L, 'ptr:bank_download_open_busy'),
            @(0x00361ac4, 0x002abe08L, 'ptr:bank_download_scan'),
            @(0x00361ad4, 0x002b4a20L, 'ptr:bank_download_scan_busy'),
            @(0x00361be4, 0x002ad1bcL, 'ptr:bank_download_save')
        )
    }
    default {
        # The offline patch: the first start's notice is skipped to where
        # accepting it continues, and a saved language is cleared through
        # the original's store and write in task 3 (ldr r6, [r0, #0x74];
        # ldrh r5, [r0, #0x30]; cmp r5, #0; mov r2, #0; mov r1, #0). The
        # scan's no-game notice without its HOME question, its first state
        # without the loading panel (bl 001d5b44, bl 0025da24 -> nop), the
        # clock, and the updates of opening, loading, saving, rewards and
        # the scene set-up's server check (the original's "finished" stub).
        $edits += @(
            @(0x002ac5b0, 0xe5945040L, 'b:002ac8c8'),
            @(0x002ac61c, 0xe59060f0L, 0xe5906074L),
            @(0x002ac628, 0xe1b05000L, 0xe1d053b0L),
            @(0x002ac62c, 0xe320f000L, 0xe3550000L),
            @(0x002ac640, 0xe1a02000L, 0xe3a02000L),
            @(0x002ac648, 0xe6ff1075L, 0xe3a01000L),
            @(0x002ac7a4, 0xebfca7a9L, 'bl:bank_offline_no_game_stub'),
            @(0x002ac7a8, 0xe3a00021L, 0xe3a00019L),
            @(0x001d3bf4, 0xe92d4ff3L, 'b:bank_offline_timestamp'),
            @(0x002ac00c, 0xebfca6ccL, 0xe320f000L),
            @(0x002ac018, 0xebfec681L, 0xe320f000L),
            @(0x00361ac4, 0x002abe08L, 'ptr:bank_offline_scan'),
            @(0x00361a08, 0x002a9750L, 'ptr:bank_offline_rewards'),
            @(0x00361be4, 0x002ad1bcL, 'ptr:bank_offline_rewards'),
            @(0x00361cfc, 0x002ae568L, 'ptr:bank_offline_load'),
            @(0x00361d0c, 0x002b4a20L, 'ptr:bank_offline_load'),
            @(0x00361ed4, 0x002af460L, 'ptr:bank_offline_load'),
            @(0x00361ee4, 0x002b4a20L, 'ptr:bank_offline_load'),
            @(0x00362034, 0x002b1cf8L, 'ptr:bank_offline_save'),
            @(0x00362044, 0x002b4a20L, 'ptr:bank_offline_save'),
            @(0x00361e84, 0x002af034L, 0x002af124L)
        )
    }
}
# Only the test build: the main menu's update (its table slot) and its
# listener "an entry was chosen" are the build's own.
if ($Kind -eq 'test') {
    $edits += @(
        @(0x003617bc, 0x002a6750L, 'ptr:bank_test_menu_update'),
        @(0x003617dc, 0x002a6c84L, 'ptr:bank_test_menu_chosen')
    )
}

# What every patch relies on: SDK CreateThread adds 0x20 to the worker's SDK
# argument before SVC 08, and the main menu's locations, which only the test
# build may edit, hold their original words. The offline patch also relies
# on the original's "finished" stub (002af124), on claim state 0x1b reaching
# the Bank field at 174 only behind its test of the session's count
# (cmn r0, #1; beq 002aac54), where the patch keeps the day of the last
# redemption, and on the scan's no-game route and acknowledgment.
$menuLocations = @(@(0x001d6554, 0xeb0000efL), @(0x002b33a4, 0xebfc853eL), @(0x002b33d4, 0xebfc8532L), @(0x003617bc, 0x002a6750L), @(0x003617dc, 0x002a6c84L))
$relies = @(@(0x00232144, 0xe3550020L), @(0x0023214c, 0x92850020L), @(0x00232154, 0x9a000008L), @(0x00232160, 0xe3500027L), @(0x00232164, 0x92800018L), @(0x00232174, 0xe3500040L), @(0x00232178, 0x83e00000L), @(0x002321b8, 0xaef62b00L), @(0x002321bc, 0x93725b00L)) + $menuLocations
# Checked native objects: the factory's bound and the cases of the tasks the
# hooks check, their vtable literals, and the reward claim's constructor.
$relies += @(
    @(0x002a5a80, 0xe351001eL),
    @(0x002a5aa4, 0x002a5bdcL), @(0x002a5ab4, 0x002a5da4L), @(0x002a5abc, 0x002a5e50L), @(0x002a5ac8, 0x002a5ebcL),
    @(0x002a5acc, 0x002a5f08L), @(0x002a5ad8, 0x002a6130L), @(0x002a5b08, 0x002a63a8L),
    @(0x002a6534, 0x00361ab8L), @(0x002a6548, 0x00362028L), @(0x002a6550, 0x00361cf0L), @(0x002a6554, 0x00361bd8L), @(0x002a6570, 0x00361ec8L),
    @(0x002a5f1c, 0x1b0016fbL), @(0x002abbf0, 0x003619fcL), @(0x002abb40, 0xe5806000L), @(0x002a5c1c, 0xe5803000L)
)
if ($offline) {
    $relies += @(@(0x002af124, 0xe3a00001L), @(0x002af128, 0xe12fff1eL), @(0x002aac04, 0xe3700001L), @(0x002aac08, 0x0a000011L),
        @(0x002abe0c, 0xe1a04000L), @(0x002ac560, 0x03a00018L), @(0x002ac7b4, 0xe5940040L), @(0x002ac7c0, 0xe3500004L), @(0x002ac7c8, 0x0a0000b0L), @(0x002ac924, 0xe3a00003L), @(0x002aca90, 0xe3a00020L))
}

# What must stay the original's: the main menu's locations but the test
# build's own two, and for the offline patch the scan's no-game route and
# acknowledgment and the code of game selection and its progress check.
$protected = @($menuLocations | Where-Object { -not ($Kind -eq 'test' -and $_[0] -in 0x003617bc, 0x003617dc) } | ForEach-Object { , @($_[0], 4) })
# The factory, the reward claim's constructor and the vtables' first three
# words stay native too; their update slots are edits.
$protected += @(@(0x002a5a7c, 0xb14), @(0x002abb10, 0xe4),
    @(0x00361ab8, 0xc), @(0x00361cf0, 0xc), @(0x00361ec8, 0xc), @(0x00362028, 0xc), @(0x003619fc, 0xc), @(0x00361bd8, 0xc))
if ($offline) {
    $protected += @(@(0x002abe0c, 4), @(0x002ac560, 4), @(0x002ac7b4, 4), @(0x002ac7c0, 4), @(0x002ac7c8, 4), @(0x002ac924, 4), @(0x002aca90, 4),
        @(0x002adc50, 0x388), @(0x002d0fc8, 0xbc), @(0x002b0098, 0x1d8), @(0x002ae310, 0x230))
}

$extra = {
    param([byte[]]$original, [byte[]]$patched, $elf)
    $symbols = $elf.symbols
    # No entry function of another patch is linked into this one, nor the
    # test build's menu entries into a package that is released.
    $foreign = @{ bank = @('bank_migrate_*', 'bank_download_*'); migration = @('bank_offline_*', 'bank_download_*'); test = @('bank_migrate_*', 'bank_download_*'); download = @('bank_offline_*', 'bank_migrate_*') }[$Kind]
    foreach ($pattern in $foreign + @('bank_offline_menu*')) {
        Assert-Check (@($symbols.Keys | Where-Object { $_ -like $pattern }).Count -eq 0) ('Entry function of another patch is linked: ' + $pattern)
    }
    foreach ($name in 'bank_test_menu_chosen', 'bank_test_menu_update') {
        Assert-Check (($Kind -eq 'test') -eq $symbols.ContainsKey($name)) ('Menu entry of the test build ' + $name)
    }
    if ($offline) {
        # The no-game stub hands the task in r4 on and keeps lr and sp.
        $stub = $symbols.bank_offline_no_game_stub
        $at = $stub.value - 0x00100000
        Assert-Check ($stub.size -eq 8 -and (U32 $patched $at) -eq 0xe1a00004L -and ((U32 $patched ($at + 4)) -band 0xff000000L) -eq 0xea000000L) 'No-game stub must pass r4 and tail-branch without changing LR/SP'
    }
}

Test-Package -Kind $Kind -Program $program -Edits $edits -Relies $relies -Protected $protected -Router $router `
    -Entries (@{ bank = 9; migration = 4; test = 11; download = 7 }[$Kind]) `
    -MenuEdits ($(if ($Kind -eq 'test') { 2 } else { 0 })) -Extra $extra `
    -PackagePath $PackagePath -ElfPath $ElfPath -InputPath $InputPath `
    -Checks @('no entry function of another patch is linked; the test build''s menu entries only in the test build', 'the main-menu locations hold their original words, and only the test build edits two of them')
