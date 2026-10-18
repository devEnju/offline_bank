<#
Independent check of the Transporter patch package in build/binary/transporter.
This script shares no code with patch-builder: it applies code.ips itself,
reads the entry functions from the linked ELF's symbols, and compares the
result with the original executable and exheader. Writes verification.json
into the package.

-Kind is as for Verify-BankPatch.ps1; Transporter has one package.
#>
[CmdletBinding()]
param(
    [ValidateSet('transporter')][string]$Kind = 'transporter',
    [string]$InputPath = '',
    # Another copy of the package or its payload (Test-Verify.ps1).
    [string]$PackagePath = '',
    [string]$ElfPath = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$base = 0x00100000
$expectedCode = '001c20ada74016507c969bb44a0a50f8edf803ec06a3fc46834263ba8df0fd2f'
$expectedExheader = '4b6081a5d0242d43713fd0be3cec21ae73f075441a759af3ed03c5270d1baceb'
$textEnd = 0x0028D1AC          # end of the original text; zero up to the page end
$textPageEnd = 0x0028E000
$dataAddress = 0x002EC000
$dataSize = 0x3D3FC
$bssSize = 0x3A4A8
$originalAppInit = 0x00104644
$startupCall = 0x00103D9C
# Where the package and its payload lie (packages.psd1); what they must
# contain is written out below.
$package = (Import-PowerShellDataFile (Join-Path $PSScriptRoot 'packages.psd1'))[$Kind]
if (-not $ElfPath) { $ElfPath = Join-Path $projectRoot "build/intermediate/$Kind/$($package.Crate).elf" }

function Assert-Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw "Verification failed: $message" }
}
function U16([byte[]]$bytes,[int]$offset) { [BitConverter]::ToUInt16($bytes,$offset) }
function U32([byte[]]$bytes,[int]$offset) { [BitConverter]::ToUInt32($bytes,$offset) }
function Hash([byte[]]$bytes,[int]$offset=0,[int]$length=-1) {
    if($length -lt 0){$length=$bytes.Length-$offset}
    $algorithm=[Security.Cryptography.SHA256]::Create()
    try { [Convert]::ToHexString($algorithm.ComputeHash($bytes,$offset,$length)).ToLowerInvariant() }
    finally {$algorithm.Dispose()}
}
function CString([byte[]]$bytes,[int]$offset) {
    $end=[Array]::IndexOf[byte]($bytes,0,$offset)
    Assert-Check ($end -ge $offset) 'Unterminated ELF string'
    [Text.Encoding]::ASCII.GetString($bytes,$offset,$end-$offset)
}
# The word at a virtual address of the code image, and the target of a branch.
function Get-Word([byte[]]$bytes, [int]$address) { return [BitConverter]::ToUInt32($bytes, $address - $base) }
function Get-BranchTarget([int]$address, [uint32]$word) {
    $offset = [int]($word -band 0x00ffffff)
    if ($offset -band 0x00800000) { $offset -= 0x01000000 }
    return $address + 8 + $offset * 4
}

if (-not $InputPath) {
    $candidates = @(Get-ChildItem -LiteralPath (Join-Path $projectRoot 'build/extracted') -Directory |
        Where-Object {
            $code = Join-Path $_.FullName 'exefs/code.bin'
            (Test-Path -LiteralPath $code) -and ((Get-FileHash -LiteralPath $code -Algorithm SHA256).Hash.ToLowerInvariant() -eq $expectedCode)
        })
    Assert-Check ($candidates.Count -ge 1) 'no extracted Transporter input with the reviewed code hash was found'
    $InputPath = $candidates[0].FullName
}
$original = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exefs/code.bin'))
$originalHeader = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exheader.bin'))
Assert-Check ((Hash $original) -eq $expectedCode) 'original code.bin hash'
# SDK CreateThread (001E3F58) adds 0x20 to the worker's SDK argument before SVC 08.
foreach ($check in @(@(0x1E3FC8, 0xE3550020L), @(0x1E3FD0, 0x92850020L), @(0x1E3FD8, 0x9A000008L), @(0x1E3FE4, 0xE3500027L), @(0x1E3FE8, 0x92800018L), @(0x1E3FF8, 0xE3500040L), @(0x1E3FFC, 0x83E00000L), @(0x1E403C, 0xAEF62B00L), @(0x1E4040, 0x93725B00L))) {
    Assert-Check ([BitConverter]::ToUInt32($original, $check[0] - 0x100000) -eq $check[1]) 'Native SDK thread-priority conversion'
}
Assert-Check ((Hash $originalHeader) -eq $expectedExheader) 'original exheader.bin hash'
$directory = if ($PackagePath) { $PackagePath } else { Join-Path $projectRoot $package.Folder }
$packageName = (Get-Content -LiteralPath (Join-Path $directory 'PACKAGE.txt') -Raw).Trim()
Assert-Check ($packageName -eq (& (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind $Kind)) 'package is not the one of the current payload and profile'

# Whole-word edits: address -> expected new word, or a branch to an address
# or to an entry function by name, checked separately. Plain hashtable: an
# ordered dictionary would treat these integer keys as positions.
$allowed = @{
    0x00103D9C = 'bl:0028D1AC'
    0x00242EF8 = 'bl:transporter_next'; 0x002445EC = 'bl:transporter_list_ready_stub'
    0x00244EAC = 'bl:transporter_list_indicator_stub'; 0x00244D40 = 'b:transporter_list_frame_stub'
    0x001E41C0 = 'bl:transporter_vc_mount_stub'; 0x0010621C = 0xE320F000
    0x0019B50C = 'b:transporter_message_stub'; 0x0019B578 = 'bl:transporter_message_text'
    0x0024BAE8 = 'bl:transporter_version'
    0x00246F48 = 'b:transporter_check_stub'; 0x00248CDC = 'b:00248E44'
    0x00248EBC = 0xE3A0100B; 0x00248F88 = 0x0AFFFFBA
    0x00245728 = 'b:00245800'; 0x002460B8 = 'b:002461D0'
    0x002488A0 = 0xE3A00001
    0x002458DC = 'bl:transporter_slot'
    0x00247478 = 'b:002474E0'
    0x00243528 = 'bl:transporter_cart_id_stub'; 0x002439A8 = 'bl:transporter_cart_read'
    0x002437F4 = 'bl:transporter_cart_write'; 0x0024397C = 'bl:transporter_cart_write'
    0x00244F10 = 'bl:transporter_list_next_stub'; 0x00244908 = 'bl:transporter_select_stub'
    0x00244F00 = 'bl:transporter_card_removed'; 0x00245658 = 'bl:transporter_card_removed'
    0x00248E68 = 'bl:transporter_card_removed'; 0x0024A298 = 'bl:transporter_card_removed'
    0x0024A474 = 'bl:transporter_card_removed'
    0x0025C5A4 = 0xE1A00004; 0x0025C5A8 = 'bl:transporter_entry_card_removed'
    0x0025C5FC = 'bl:transporter_language_chosen'; 0x0025C618 = 0xE320F000
    0x0022BEE0 = 'bl:transporter_language_order_stub'; 0x0022B5B0 = 'bl:transporter_language_buttons'
    0x0022B6E0 = 0xE3500007; 0x0022B6E4 = 0x03A01003
    0x002412D4 = 'bl:transporter_vc_scan_stub'
    0x0022B3B8 = 'b:transporter_language_back_stub'; 0x0013AEE4 = 'bl:transporter_layout_check'
    0x0024BA44 = 'bl:transporter_title_begin_stub'; 0x0024BA74 = 'bl:transporter_title_end_stub'
    0x0024A150 = 'b:transporter_deliver_stub'
    0x0024A3C0 = 0xE3A0000E
    0x0024A3D0 = 'b:0024A2A8'; 0x0024A4E8 = 0x0AFFFF6E
    0x0024F6B0 = 'bl:transporter_shiny_build_stub'; 0x0024F8E0 = 'bl:transporter_shiny_gender_stub'
}

$ips = [IO.File]::ReadAllBytes((Join-Path $directory 'code.ips'))
$header = [IO.File]::ReadAllBytes((Join-Path $directory 'exheader.bin'))
$manifest = Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw | ConvertFrom-Json

# The linked ELF the package was built from, and its function symbols.
$elf = [IO.File]::ReadAllBytes($elfPath)
Assert-Check ((Hash $elf) -eq $manifest.source_elf_sha256) 'ELF hash equals manifest'
Assert-Check ($manifest.patch -eq 'transporter' -and $manifest.title_id -eq '00040000000c9c00' -and $manifest.tmd_version -eq 5200 -and $manifest.remaster_version -eq 5) 'manifest names this patch and version'
Assert-Check ($manifest.native_edits -eq $allowed.Count + 1 -and $manifest.main_menu_edits -eq 0 -and $manifest.paired_files_required -eq $true -and $manifest.hardware_tested_by_builder -eq $false) 'manifest edit count and status'
Assert-Check ($elf[0] -eq 0x7F -and [Text.Encoding]::ASCII.GetString($elf, 1, 3) -eq 'ELF' -and $elf[4] -eq 1 -and $elf[5] -eq 1) 'ELF32 little-endian header'
$sectionTable = U32 $elf 32
$sectionCount = U16 $elf 48
Assert-Check ((U16 $elf 46) -eq 40) 'ELF section header size'
$symbols = @{}
$tables = 0
for ($index = 0; $index -lt $sectionCount; $index++) {
    $at = $sectionTable + 40 * $index
    if ((U32 $elf ($at + 4)) -ne 2) { continue }
    $tables++
    $first = U32 $elf ($at + 16); $length = U32 $elf ($at + 20)
    $strings = U32 $elf ($sectionTable + 40 * (U32 $elf ($at + 24)) + 16)
    Assert-Check ((U32 $elf ($at + 36)) -eq 16 -and $length % 16 -eq 0) 'ELF symbol entries'
    for ($entry = $first; $entry -lt $first + $length; $entry += 16) {
        $name = CString $elf ($strings + (U32 $elf $entry))
        if ($name) { $symbols[$name] = [pscustomobject]@{ value = (U32 $elf ($entry + 4)); size = (U32 $elf ($entry + 8)); info = $elf[$entry + 12] } }
    }
}
Assert-Check ($tables -eq 1) 'one ELF symbol table'

# Geometry, derived here from the original sizes only.
$bssStart = $dataAddress + $dataSize
$bssEnd = $bssStart + $bssSize
$payloadAddress = ($bssEnd + 0xFFF) -band (-bnot 0xFFF)
Assert-Check ($bssEnd -eq 0x003638A4 -and $payloadAddress -eq 0x00364000) 'memory geometry'
Assert-Check ([int]$manifest.placement.payload_address -eq $payloadAddress) 'payload address'
$payloadCode = [int]$manifest.placement.payload_rx_bytes
$payloadMemory = [int]$manifest.placement.payload_memory_bytes
Assert-Check ($payloadCode -gt 0 -and $payloadCode % 0x1000 -eq 0 -and $payloadMemory % 0x1000 -eq 0 -and $payloadCode -lt $payloadMemory) 'payload sizes are whole pages'
$expandedEnd = $payloadAddress + $payloadMemory
$expandedLength = $expandedEnd - $base
Assert-Check ($expandedLength -eq [int]$manifest.expanded_code_bytes) 'expanded image size'
Assert-Check ($expandedEnd -lt 0x08000000) 'expanded image stays below the application heap'

# Apply the IPS to the original image extended with zero bytes.
Assert-Check ([Text.Encoding]::ASCII.GetString($ips, 0, 5) -eq 'PATCH') 'IPS header'
$patched = New-Object byte[] $expandedLength
[Array]::Copy($original, $patched, $original.Length)
$written = New-Object bool[] $expandedLength
$at = 5; $records = 0
while ([Text.Encoding]::ASCII.GetString($ips, $at, 3) -ne 'EOF') {
    $offset = ([int]$ips[$at] -shl 16) -bor ([int]$ips[$at + 1] -shl 8) -bor [int]$ips[$at + 2]
    $length = ([int]$ips[$at + 3] -shl 8) -bor [int]$ips[$at + 4]
    Assert-Check ($length -gt 0 -and $offset + $length -le $expandedLength) 'IPS record inside the expanded image'
    for ($index = $offset; $index -lt $offset + $length; $index++) {
        Assert-Check (-not $written[$index]) 'IPS records do not overlap'
        $written[$index] = $true
    }
    [Array]::Copy($ips, $at + 5, $patched, $offset, $length)
    $at += 5 + $length; $records++
}
Assert-Check ($at + 3 -eq $ips.Length) 'IPS has trailing bytes'
Assert-Check ((Hash $patched) -eq $manifest.expanded_code_sha256) 'expanded image hash equals manifest'
Assert-Check ((Hash $ips) -eq $manifest.artifacts.'code.ips'.sha256) 'IPS hash equals manifest'
Assert-Check ((Hash $header) -eq $manifest.artifacts.'exheader.bin'.sha256) 'exheader hash equals manifest'

# Original text: every changed word is a listed edit or inside the start-up
# hook, which may only replace zero bytes after the original text.
$bootstrapStart = [int]$manifest.placement.bootstrap_address
$bootstrapEnd = $bootstrapStart + [int]$manifest.placement.bootstrap_bytes
Assert-Check ($bootstrapStart -eq $textEnd -and $bootstrapEnd -le $textPageEnd) 'start-up hook lies after the original text'
for ($address = $textEnd; $address -lt $textPageEnd; $address++) {
    Assert-Check ($original[$address - $base] -eq 0) 'bytes after the original text are zero'
}
$changed = 0
for ($address = $base; $address -lt $textPageEnd; $address += 4) {
    $old = Get-Word $original $address; $new = Get-Word $patched $address
    if ($old -eq $new) { continue }
    $changed++
    if ($address -ge $bootstrapStart -and $address -lt $bootstrapEnd) { continue }
    Assert-Check $allowed.ContainsKey($address) ("unlisted word changed at {0:X8}" -f $address)
}
foreach ($address in $allowed.Keys) {
    $new = Get-Word $patched $address
    $want = $allowed[$address]
    if ($want -is [string]) {
        $branch, $target = $want.Split(':')
        $top = @{ 'b' = 0xEA; 'bl' = 0xEB }[$branch]
        Assert-Check (($new -shr 24) -eq $top) ("{0:X8} is not '$branch'" -f $address)
        if ($target -like 'transporter_*') {
            # An entry function: a global ARM function of the start-up hook
            # or of the payload's code pages.
            Assert-Check $symbols.ContainsKey($target) "missing ELF symbol $target"
            $symbol = $symbols[$target]
            $inBootstrap = $symbol.value -ge $bootstrapStart -and $symbol.value + $symbol.size -le $bootstrapEnd
            $inPayload = $symbol.value -ge $payloadAddress -and $symbol.value + $symbol.size -le $payloadAddress + $payloadCode
            Assert-Check ($symbol.info -eq 0x12 -and $symbol.value % 4 -eq 0 -and $symbol.size -ge 4 -and ($inBootstrap -or $inPayload)) "$target is a global ARM function of the hook or the payload code"
            $target = $symbol.value
        } else {
            $target = [Convert]::ToInt32($target, 16)
        }
        Assert-Check ((Get-BranchTarget $address $new) -eq $target) ("{0:X8} branch target" -f $address)
    } else {
        Assert-Check (([int64]$new) -eq (([int64]$want) -band 0xFFFFFFFFL)) ("{0:X8} word" -f $address)
    }
}
# The original call that the hook replaces, and the hook's first two words.
$originalCall = Get-Word $original $startupCall
Assert-Check ((($originalCall -shr 24) -eq 0xEB) -and ((Get-BranchTarget $startupCall $originalCall) -eq $originalAppInit)) 'original start-up call'
Assert-Check (([int64](Get-Word $patched $bootstrapStart)) -eq 0xE92D4010L) 'start-up hook begins with push {r4, lr}'
$second = Get-Word $patched ($bootstrapStart + 4)
Assert-Check ((($second -shr 24) -eq 0xEB) -and ((Get-BranchTarget ($bootstrapStart + 4) $second) -eq $originalAppInit)) 'start-up hook calls the original application init first'
# The layout check, called from 0013AEE4 before the payload can run, lies in
# the same executable place and begins with the instruction it stands in for.
$layoutCheck = $symbols['transporter_layout_check'].value
Assert-Check ($layoutCheck -ge $bootstrapStart -and $layoutCheck + 4 -le $bootstrapEnd -and ([int64](Get-Word $patched $layoutCheck)) -eq 0xE1A01000L) 'layout check lies in the start-up hook and begins with mov r1, r0'
# The ELF's entry is the router entry; the start-up hook is the linked one.
Assert-Check ((U32 $elf 24) -eq $symbols['transporter_next'].value) 'ELF entry is transporter_next'
Assert-Check ($symbols['patch_bootstrap_startup'].value -eq $bootstrapStart) 'start-up hook symbol'

# Original read-only data and data: unchanged.
for ($index = $textPageEnd - $base; $index -lt $original.Length; $index++) {
    if ($original[$index] -ne $patched[$index]) { throw ("Verification failed: original data changed at {0:X8}" -f ($index + $base)) }
}
# Former zero-initialised data: written explicitly and zero. The bytes of the
# original file after its data section are part of it and must be zero too.
for ($address = $bssStart; $address -lt $payloadAddress; $address++) {
    Assert-Check ($patched[$address - $base] -eq 0) 'former zero-initialised data is zero'
}
for ($index = $original.Length; $index -lt $expandedLength; $index++) {
    Assert-Check $written[$index] 'every added byte is written by the IPS'
}
# The ELF's loaded segments, as the IPS installs them: the start-up hook,
# then the payload's code and its data, each byte for byte, and the rest of
# each segment zero. Nothing of the expanded image lies outside them but the
# former zero-initialised data checked above.
$programTable = U32 $elf 28
Assert-Check ((U16 $elf 42) -eq 32) 'ELF program header size'
$loads = @()
for ($index = 0; $index -lt (U16 $elf 44); $index++) {
    $at = $programTable + 32 * $index
    if ((U32 $elf $at) -ne 1) { continue }
    $load = [pscustomobject]@{ offset = (U32 $elf ($at + 4)); address = (U32 $elf ($at + 8)); file_bytes = (U32 $elf ($at + 16)); memory_bytes = (U32 $elf ($at + 20)); flags = (U32 $elf ($at + 24)) }
    Assert-Check ($load.address -eq (U32 $elf ($at + 12)) -and $load.file_bytes -le $load.memory_bytes -and $load.offset + $load.file_bytes -le $elf.Length) 'ELF LOAD mapping'
    $loads += $load
}
$loads = @($loads | Sort-Object address)
Assert-Check ($loads.Count -eq 3) 'ELF LOADs are the start-up hook, the payload code and its data'
$boot, $rx, $rw = $loads
Assert-Check ($boot.address -eq $bootstrapStart -and $boot.memory_bytes -eq $boot.file_bytes -and $boot.address + $boot.memory_bytes -eq $bootstrapEnd -and $boot.flags -eq 5) 'start-up hook LOAD'
Assert-Check ($rx.address -eq $payloadAddress -and $rx.memory_bytes -eq $rx.file_bytes -and $rx.memory_bytes -eq $payloadCode -and $rx.flags -eq 5) 'payload code LOAD'
Assert-Check ($rw.address -eq $payloadAddress + $payloadCode -and $rw.address + $rw.memory_bytes -eq $expandedEnd -and $rw.flags -eq 6) 'payload data LOAD'
foreach ($load in $loads) {
    Assert-Check ((Hash $patched ($load.address - $base) $load.file_bytes) -eq (Hash $elf $load.offset $load.file_bytes)) ("ELF LOAD bytes at {0:X8}" -f $load.address)
    $zero = $load.memory_bytes - $load.file_bytes
    if ($zero -gt 0) {
        Assert-Check ((Hash $patched ($load.address - $base + $load.file_bytes) $zero) -eq (Hash ([byte[]]::new($zero)))) ("ELF zero-initialised bytes at {0:X8}" -f $load.address)
    }
}
# The manifest's placement, payload hash and entry functions, against the
# ELF and the patched image.
$payloadImageLength = $rw.address + $rw.file_bytes - $rx.address
Assert-Check ((Hash $patched ($rx.address - $base) $payloadImageLength) -eq $manifest.payload_image_sha256) 'payload image hash equals manifest'
$placement = $manifest.placement
Assert-Check ($placement.payload_file_offset -eq $rx.address - $base -and $placement.payload_rw_address -eq $rw.address -and $placement.payload_rw_bytes -eq $rw.memory_bytes -and $placement.bootstrap_address -eq $boot.address -and $placement.bootstrap_bytes -eq $boot.file_bytes) 'manifest placement'
$entryNames = @($allowed.Values | Where-Object { $_ -is [string] } | ForEach-Object { $_.Split(':')[1] } | Where-Object { $_ -like 'transporter_*' } | Sort-Object -Unique)
Assert-Check ($entryNames.Count -eq 28 -and $manifest.runtime_exports -eq $entryNames.Count -and @($manifest.exports.psobject.Properties).Count -eq $entryNames.Count) 'manifest entry count'
foreach ($name in $entryNames) {
    Assert-Check ($manifest.exports.$name -eq $symbols[$name].value) "manifest export $name"
}
$text = [Text.Encoding]::ASCII.GetString($patched, $payloadAddress - $base, $payloadCode)
Assert-Check ($text.Contains('/mover.bin') -and $text.Contains('/mover.alt.bin')) 'payload names /mover.bin and /mover.alt.bin'
Assert-Check ($text.Contains('/roms/nds/saves/POKEMON_')) 'payload names the SD save folder'

# Exheader: only the data page count, data size and BSS size may differ.
Assert-Check ($header.Length -eq $originalHeader.Length) 'exheader length'
for ($index = 0; $index -lt $header.Length; $index++) {
    if ($index -ge 0x34 -and $index -lt 0x40) { continue }
    Assert-Check ($header[$index] -eq $originalHeader[$index]) ("exheader byte {0:X} changed" -f $index)
}
$newDataSize = $expandedEnd - $dataAddress
Assert-Check ([BitConverter]::ToUInt32($header, 0x34) -eq $newDataSize / 0x1000) 'exheader data pages cover the expanded image'
Assert-Check ([BitConverter]::ToUInt32($header, 0x38) -eq $newDataSize) 'exheader data size covers the expanded image'
Assert-Check ([BitConverter]::ToUInt32($header, 0x3C) -eq 0) 'exheader BSS size is zero'
Assert-Check ([BitConverter]::ToUInt32($originalHeader, 0x38) -eq $dataSize -and [BitConverter]::ToUInt32($originalHeader, 0x3C) -eq $bssSize) 'original data and BSS sizes'

$report = [ordered]@{
    status = 'passed'
    package = $packageName
    source_code_sha256 = $expectedCode
    source_exheader_sha256 = $expectedExheader
    ips_sha256 = (Hash $ips)
    paired_exheader_sha256 = (Hash $header)
    ips_records = $records
    changed_text_words = $changed
    listed_edits = $allowed.Count
    bootstrap_bytes = $bootstrapEnd - $bootstrapStart
    payload_code_bytes = $payloadCode
    payload_memory_bytes = $payloadMemory
    expanded_code_bytes = $expandedLength
    checks = @(
        'original inputs identified by hash',
        'all IPS records inside the expanded image and disjoint',
        'every changed word of the original text is a listed edit or inside the start-up hook',
        'every listed edit holds the expected instruction or branch target; entry functions by their ELF symbols',
        'start-up hook replaces only zero bytes and calls the original application init first',
        'original read-only data and data unchanged',
        'former zero-initialised data written as zero; every added byte written',
        'every entry function is a global ARM function of the start-up hook or the payload code',
        'the start-up hook, payload code and payload data are the ELF LOAD bytes, the rest of each segment zero',
        'manifest placement, payload hash and entry functions match the ELF and the patched image',
        'only exheader fields 0x34/0x38/0x3c changed, and they cover the expanded image',
        'manifest hashes match'
    )
    limitations = @('static artifact verification; hardware behavior not established')
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $directory 'verification.json') -Encoding utf8
$report | ConvertTo-Json -Depth 5
