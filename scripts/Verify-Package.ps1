<#
The independent check of one package, which Verify-BankPatch.ps1 and
Verify-TransporterPatch.ps1 dot-source: it defines Test-Package and runs
nothing itself. It shares no code with patch-builder: it parses the ELF and
the IPS itself, applies code.ips to the original over added memory that is
not zero, rebuilds the expected image from the caller's hard-coded list of
edits and the ELF's loaded segments, and compares every byte. Each verify
script brings what only it knows: its program, its list of edits with their
original words, its entry functions, and the checks of its own.
#>

function Assert-Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw "Verification failed: $message" }
}
function U16([byte[]]$bytes, [int]$offset) { [BitConverter]::ToUInt16($bytes, $offset) }
function U32([byte[]]$bytes, [int]$offset) { [BitConverter]::ToUInt32($bytes, $offset) }
function Hash([byte[]]$bytes, [int]$offset = 0, [int]$length = -1) {
    if ($length -lt 0) { $length = $bytes.Length - $offset }
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { [Convert]::ToHexString($algorithm.ComputeHash($bytes, $offset, $length)).ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}
function CString([byte[]]$bytes, [int]$offset) {
    $end = [Array]::IndexOf[byte]($bytes, 0, $offset)
    Assert-Check ($end -ge $offset) 'Unterminated ELF string'
    [Text.Encoding]::ASCII.GetString($bytes, $offset, $end - $offset)
}
# The target of a B or BL at `address`.
function Get-BranchTarget([long]$address, [uint32]$word) {
    $offset = [long]($word -band 0x00ffffff)
    if ($offset -band 0x00800000) { $offset -= 0x01000000 }
    return $address + 8 + $offset * 4
}

# The ELF's three loaded segments (start-up hook, payload code, payload
# data) and its symbols.
function Read-Elf([byte[]]$elf) {
    Assert-Check ($elf[0] -eq 0x7f -and [Text.Encoding]::ASCII.GetString($elf, 1, 3) -eq 'ELF' -and $elf[4] -eq 1 -and $elf[5] -eq 1) 'ELF32 little-endian header'
    Assert-Check ((U16 $elf 16) -eq 2 -and (U16 $elf 18) -eq 40) 'ARM executable ELF'
    $phoff = U32 $elf 28; $phsize = U16 $elf 42; $phcount = U16 $elf 44
    $shoff = U32 $elf 32; $shsize = U16 $elf 46; $shcount = U16 $elf 48
    Assert-Check ($phsize -eq 32 -and $shsize -eq 40) 'ELF table sizes'
    $loads = @()
    for ($index = 0; $index -lt $phcount; $index++) {
        $at = $phoff + $index * $phsize
        if ((U32 $elf $at) -ne 1) { continue }
        $load = [pscustomobject]@{ offset = (U32 $elf ($at + 4)); address = (U32 $elf ($at + 8)); file_bytes = (U32 $elf ($at + 16)); memory_bytes = (U32 $elf ($at + 20)); flags = (U32 $elf ($at + 24)); alignment = (U32 $elf ($at + 28)) }
        Assert-Check ($load.address -eq (U32 $elf ($at + 12)) -and $load.file_bytes -le $load.memory_bytes) 'ELF LOAD mapping'
        Assert-Check ($load.offset + $load.file_bytes -le $elf.Length -and $load.alignment -eq 0x1000) 'ELF LOAD file range'
        Assert-Check (($load.address % 0x1000) -eq ($load.offset % 0x1000)) 'ELF LOAD congruence'
        $loads += $load
    }
    $loads = @($loads | Sort-Object address)
    Assert-Check ($loads.Count -eq 3) 'ELF LOADs are the start-up hook, the payload code and its data'
    $symbols = @{}
    $tables = 0
    for ($index = 0; $index -lt $shcount; $index++) {
        $at = $shoff + $index * $shsize
        if ((U32 $elf ($at + 4)) -ne 2) { continue }
        $tables++
        $first = U32 $elf ($at + 16); $length = U32 $elf ($at + 20)
        $strings = U32 $elf ($shoff + (U32 $elf ($at + 24)) * $shsize + 16)
        Assert-Check ((U32 $elf ($at + 36)) -eq 16 -and $length % 16 -eq 0) 'ELF symbol entries'
        for ($entry = $first; $entry -lt $first + $length; $entry += 16) {
            $name = CString $elf ($strings + (U32 $elf $entry))
            if ($name) { $symbols[$name] = [pscustomobject]@{ value = (U32 $elf ($entry + 4)); size = (U32 $elf ($entry + 8)); info = $elf[$entry + 12] } }
        }
    }
    Assert-Check ($tables -eq 1) 'One ELF symbol table'
    return [pscustomobject]@{ boot = $loads[0]; rx = $loads[1]; rw = $loads[2]; loads = $loads; symbols = $symbols; entry = (U32 $elf 24) }
}

# Applies the IPS to the original, extended to `size` bytes of memory that
# is not zero, as Luma3DS does: every record bounded and disjoint, no record
# into the original outside `allowed`, and every added byte written.
function Expand-Ips([byte[]]$ips, [byte[]]$original, [int]$size, [byte[]]$allowed) {
    [byte[]]$patched = [byte[]]::new($size)
    [Array]::Fill[byte]($patched, 0xa5)
    [Array]::Copy($original, 0, $patched, 0, $original.Length)
    [byte[]]$covered = [byte[]]::new($size)
    Assert-Check ([Text.Encoding]::ASCII.GetString($ips, 0, 5) -eq 'PATCH') 'IPS magic'
    $at = 5; $records = 0
    while ($true) {
        Assert-Check ($at + 3 -le $ips.Length) 'IPS truncated offset'
        if ([Text.Encoding]::ASCII.GetString($ips, $at, 3) -eq 'EOF') { $at += 3; break }
        $offset = ([int]$ips[$at] -shl 16) -bor ([int]$ips[$at + 1] -shl 8) -bor [int]$ips[$at + 2]; $at += 3
        Assert-Check ($at + 2 -le $ips.Length) 'IPS truncated length'
        $length = ([int]$ips[$at] -shl 8) -bor [int]$ips[$at + 1]; $at += 2
        $rle = $length -eq 0
        if ($rle) {
            Assert-Check ($at + 3 -le $ips.Length) 'IPS truncated RLE'
            $length = ([int]$ips[$at] -shl 8) -bor [int]$ips[$at + 1]; $value = $ips[$at + 2]; $at += 3
        }
        Assert-Check ($length -gt 0 -and $offset + $length -le $size) 'IPS bounds'
        Assert-Check ([Array]::IndexOf[byte]($covered, 1, $offset, $length) -eq -1) 'Overlapping IPS records'
        if ($offset -lt $original.Length) {
            Assert-Check ($offset + $length -le $original.Length) 'IPS native/extension boundary'
            Assert-Check ([Array]::IndexOf[byte]($allowed, 0, $offset, $length) -eq -1) 'IPS writes an unapproved native region'
        }
        [Array]::Fill[byte]($covered, 1, $offset, $length)
        if ($rle) { [Array]::Fill[byte]($patched, $value, $offset, $length) }
        else {
            Assert-Check ($at + $length -le $ips.Length) 'IPS truncated data'
            [Array]::Copy($ips, $at, $patched, $offset, $length); $at += $length
        }
        $records++
    }
    Assert-Check ($at -eq $ips.Length) 'Unexpected IPS EOF trailer'
    Assert-Check ([Array]::IndexOf[byte]($covered, 0, $original.Length, $size - $original.Length) -eq -1) 'Uninitialized extension byte'
    return [pscustomobject]@{ patched = $patched; records = $records }
}

<#
Checks the package of `Kind` and writes verification.json into it.
- Program: Name (the manifest's patch is checked against Kind), CodeHash,
  ExheaderHash, CodeLength, TitleId (hex), TmdVersion, Remaster, Bootstrap,
  BootstrapEnd, Payload, AppInit, StartupCall.
- Edits: @(address, original word, replacement), the replacement a word,
  'b:<target>', 'bl:<target>' or 'ptr:<target>', the target a symbol or a
  hex address. The start-up hook's bytes are one more region.
- Relies: @(address, word) of the original the patch relies on.
- Router: the ELF's entry; Entries: how many entry functions the edits name.
- MenuEdits: how many edits the manifest counts as main-menu edits.
- Extra: a script block with checks of the caller's own; it gets the
  original, the patched image and the parsed ELF.
#>
function Test-Package {
    param(
        [Parameter(Mandatory)][string]$Kind,
        [Parameter(Mandatory)][hashtable]$Program,
        [Parameter(Mandatory)][object[]]$Edits,
        [object[]]$Relies = @(),
        [Parameter(Mandatory)][string]$Router,
        [Parameter(Mandatory)][int]$Entries,
        [int]$MenuEdits = 0,
        [scriptblock]$Extra = $null,
        [string]$PackagePath = '',
        [string]$ElfPath = '',
        [string]$InputPath = '',
        [string[]]$Checks = @()
    )
    $projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $base = 0x00100000
    # Where the package and its payload lie (packages.psd1); what they must
    # contain is the caller's.
    $package = (Import-PowerShellDataFile (Join-Path $PSScriptRoot 'packages.psd1'))[$Kind]
    if (-not $ElfPath) { $ElfPath = Join-Path $projectRoot "build/intermediate/$Kind/$($package.Crate).elf" }
    if (-not $PackagePath) { $PackagePath = Join-Path $projectRoot $package.Folder }
    $reportPath = Join-Path $PackagePath 'verification.json'
    $packageName = 'unknown'
    try {
        $packageName = (Get-Content -LiteralPath (Join-Path $PackagePath 'PACKAGE.txt') -Raw).Trim()
        $manifest = Get-Content -LiteralPath (Join-Path $PackagePath 'manifest.json') -Raw | ConvertFrom-Json
        if (-not $InputPath) {
            $candidates = @(Get-ChildItem -LiteralPath (Join-Path $projectRoot 'build/extracted') -Directory | Where-Object {
                    $code = Join-Path $_.FullName 'exefs/code.bin'
                    (Test-Path -LiteralPath $code) -and ((Get-FileHash -LiteralPath $code -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Program.CodeHash)
                })
            Assert-Check ($candidates.Count -ge 1) "No extracted $($Program.Name) input with the reviewed code hash"
            $InputPath = $candidates[0].FullName
        }
        [byte[]]$original = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exefs/code.bin'))
        [byte[]]$originalHeader = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exheader.bin'))
        [byte[]]$elf = [IO.File]::ReadAllBytes($ElfPath)
        [byte[]]$ips = [IO.File]::ReadAllBytes((Join-Path $PackagePath 'code.ips'))
        [byte[]]$header = [IO.File]::ReadAllBytes((Join-Path $PackagePath 'exheader.bin'))

        # The inputs, the manifest and the package name.
        $codeHash = Hash $original; $headerHash = Hash $originalHeader; $elfHash = Hash $elf
        Assert-Check ($codeHash -eq $Program.CodeHash -and $original.Length -eq $Program.CodeLength) 'Original code'
        Assert-Check ($headerHash -eq $Program.ExheaderHash -and $originalHeader.Length -eq 0x800) 'Original exheader'
        Assert-Check ($manifest.source_code_sha256 -eq $codeHash -and $manifest.source_exheader_sha256 -eq $headerHash) 'Manifest input hashes'
        Assert-Check ($manifest.source_elf_sha256 -eq $elfHash) 'Manifest ELF hash'
        Assert-Check ($packageName -eq (& (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind $Kind)) 'Package is not the one of the current payload and profile'
        Assert-Check ($manifest.patch -eq $Kind) 'Manifest names another patch'
        Assert-Check ($manifest.title_id -eq $Program.TitleId -and $manifest.tmd_version -eq $Program.TmdVersion -and $manifest.remaster_version -eq $Program.Remaster) 'Manifest version'
        foreach ($name in 'code.ips', 'exheader.bin') {
            $bytes = if ($name -eq 'code.ips') { $ips } else { $header }
            $artifact = $manifest.artifacts.$name
            Assert-Check ($artifact.sha256 -eq (Hash $bytes) -and $artifact.bytes -eq $bytes.Length) ('Artifact ' + $name)
        }
        Assert-Check ($manifest.paired_files_required -eq $true -and $manifest.hardware_tested_by_builder -eq $false) 'Manifest validation status'
        foreach ($check in $Relies) {
            Assert-Check ((U32 $original ($check[0] - $base)) -eq $check[1]) ('Original word {0:x8}' -f $check[0])
        }

        # The linked payload: where its segments lie, its entry functions,
        # the start-up hook and the kernel-call wrappers.
        $parsed = Read-Elf $elf
        $boot = $parsed.boot; $rx = $parsed.rx; $rw = $parsed.rw; $symbols = $parsed.symbols
        Assert-Check ($boot.address -eq $Program.Bootstrap -and $boot.flags -eq 5 -and $boot.memory_bytes -eq $boot.file_bytes -and $boot.address + $boot.memory_bytes -le $Program.BootstrapEnd) 'Start-up hook LOAD'
        Assert-Check ($rx.address -eq $Program.Payload -and $rx.flags -eq 5 -and $rx.memory_bytes -eq $rx.file_bytes -and $rx.memory_bytes % 0x1000 -eq 0) 'Payload code LOAD'
        Assert-Check ($rw.address -eq $rx.address + $rx.memory_bytes -and $rw.flags -eq 6 -and $rw.memory_bytes % 0x1000 -eq 0) 'Payload data LOAD'
        $imageEnd = $rw.address + $rw.memory_bytes
        $expandedSize = $imageEnd - $base
        Assert-Check ($expandedSize -eq $manifest.expanded_code_bytes -and $imageEnd -lt 0x08000000) 'Expanded allocation'
        $function = {
            param([string]$name, [string]$what)
            Assert-Check $symbols.ContainsKey($name) ("Missing ELF symbol $name")
            $symbol = $symbols[$name]
            $inBoot = $symbol.value -ge $boot.address -and $symbol.value + $symbol.size -le $boot.address + $boot.file_bytes
            $inCode = $symbol.value -ge $rx.address -and $symbol.value + $symbol.size -le $rx.address + $rx.file_bytes
            Assert-Check ($symbol.info -eq 0x12 -and $symbol.value % 4 -eq 0 -and $symbol.size -gt 0 -and ($inBoot -or $inCode)) "$what $name is a global ARM function of the start-up hook or the payload code"
            return $symbol.value
        }
        $wrappers = @('patch_svc_close_handle', 'patch_svc_wait_thread', 'patch_svc_get_resource_limit', 'patch_svc_get_resource_limit_values', 'patch_svc_get_resource_current_values')
        foreach ($name in $wrappers) { & $function $name 'System-call wrapper' | Out-Null }
        $names = @($Edits | ForEach-Object { $_[2] } | Where-Object { $_ -is [string] } | ForEach-Object { $_.Split(':')[1] } |
                Where-Object { $_ -notmatch '^[0-9A-Fa-f]{8}$' -and $_ -ne 'patch_bootstrap_startup' } | Sort-Object -Unique)
        Assert-Check ($names.Count -eq $Entries -and $manifest.runtime_exports -eq $Entries -and @($manifest.exports.psobject.Properties).Count -eq $Entries) 'Entry function count'
        foreach ($name in $names) {
            Assert-Check ((& $function $name 'Entry') -eq $manifest.exports.$name) "Manifest export $name"
        }
        Assert-Check ($symbols.patch_bootstrap_startup.value -eq $boot.address -and $parsed.entry -eq $symbols[$Router].value) 'ELF entry points'
        foreach ($bound in @(@('__patch_payload_start', $rx.address), @('__patch_payload_rx_end', $rw.address), @('__patch_payload_rx_size', $rx.memory_bytes), @('__patch_payload_end', $imageEnd), @('__patch_original_app_init', $Program.AppInit))) {
            Assert-Check ($symbols.ContainsKey($bound[0]) -and $symbols[$bound[0]].value -eq $bound[1]) ('ELF bound ' + $bound[0])
        }
        # The start-up hook replaces the one call of application init and
        # makes it first: push {r4, lr}; bl <application init>.
        $originalCall = U32 $original ($Program.StartupCall - $base)
        Assert-Check ((($originalCall -shr 24) -eq 0xeb) -and (Get-BranchTarget $Program.StartupCall $originalCall) -eq $Program.AppInit) 'Original start-up call'
        Assert-Check ((U32 $elf $boot.offset) -eq 0xe92d4010L) 'Start-up hook begins with push {r4, lr}'
        $second = U32 $elf ($boot.offset + 4)
        Assert-Check ((($second -shr 24) -eq 0xeb) -and (Get-BranchTarget ($boot.address + 4) $second) -eq $Program.AppInit) 'Start-up hook calls the original application init first'

        # The image as it must be: the original with every listed edit, the
        # start-up hook in the zero bytes after the original text, the former
        # zero-initialised data zero, and the payload's segments.
        [byte[]]$expected = [byte[]]::new($expandedSize)
        [Array]::Copy($original, 0, $expected, 0, $original.Length)
        [byte[]]$allowed = [byte[]]::new($original.Length)
        foreach ($edit in $Edits) {
            $address = [long]$edit[0]; $offset = [int]($address - $base)
            Assert-Check ((U32 $original $offset) -eq $edit[1]) ('Original word at {0:x8}' -f $address)
            $want = $edit[2]
            if ($want -is [string]) {
                $kindOf, $target = $want.Split(':')
                $target = if ($target -match '^[0-9A-Fa-f]{8}$') { [Convert]::ToInt64($target, 16) } else { & $function $target 'Target' }
                if ($kindOf -eq 'ptr') { $word = [uint32]$target }
                else {
                    $delta = [long]$target - $address - 8
                    Assert-Check ($delta % 4 -eq 0 -and $delta -ge -0x2000000 -and $delta -lt 0x2000000) 'ARM branch reach'
                    $opcode = @{ 'b' = 0xea000000L; 'bl' = 0xeb000000L }[$kindOf]
                    $word = [uint32]($opcode -bor (($delta -shr 2) -band 0xffffff))
                }
            } else { $word = [uint32]([long]$want -band 0xffffffffL) }
            Assert-Check ([Array]::IndexOf[byte]($allowed, 1, $offset, 4) -eq -1) ('Edit listed twice at {0:x8}' -f $address)
            [Array]::Copy([BitConverter]::GetBytes($word), 0, $expected, $offset, 4)
            [Array]::Fill[byte]($allowed, 1, $offset, 4)
        }
        $bootOffset = $boot.address - $base
        Assert-Check ((Hash $original $bootOffset ($Program.BootstrapEnd - $boot.address)) -eq (Hash ([byte[]]::new($Program.BootstrapEnd - $boot.address)))) 'Bytes after the original text are zero'
        [Array]::Fill[byte]($allowed, 1, $bootOffset, $boot.file_bytes)
        foreach ($load in $parsed.loads) { [Array]::Copy($elf, $load.offset, $expected, $load.address - $base, $load.file_bytes) }
        $expansion = Expand-Ips $ips $original $expandedSize $allowed
        $patched = $expansion.patched
        $patchedHash = Hash $patched
        Assert-Check ($patchedHash -eq (Hash $expected) -and $patchedHash -eq $manifest.expanded_code_sha256) 'Expanded image differs from independent expected image'
        foreach ($load in $parsed.loads) {
            $zero = $load.memory_bytes - $load.file_bytes
            if ($zero -gt 0) { Assert-Check ((Hash $patched ($load.address - $base + $load.file_bytes) $zero) -eq (Hash ([byte[]]::new($zero)))) 'ELF zero-initialised bytes' }
        }
        Assert-Check ($Edits.Count + 1 -eq $manifest.native_edits -and $manifest.main_menu_edits -eq $MenuEdits) 'Native region count'

        # The manifest's placement and payload hash.
        $payloadImageLength = $rw.address + $rw.file_bytes - $rx.address
        Assert-Check ((Hash $patched ($rx.address - $base) $payloadImageLength) -eq $manifest.payload_image_sha256) 'Payload image hash'
        $placement = $manifest.placement
        Assert-Check ($placement.payload_address -eq $rx.address -and $placement.payload_file_offset -eq $rx.address - $base -and $placement.payload_memory_bytes -eq $imageEnd - $rx.address -and $placement.payload_rx_bytes -eq $rx.memory_bytes -and $placement.payload_rw_address -eq $rw.address -and $placement.payload_rw_bytes -eq $rw.memory_bytes -and $placement.bootstrap_address -eq $boot.address -and $placement.bootstrap_bytes -eq $boot.file_bytes) 'Manifest placement'

        # The paired exheader: only the data page count, its size and the BSS
        # size, so that the data segment covers the expanded image. The
        # payload follows the original zero-initialised data, which the IPS
        # writes as zero.
        $dataAddress = U32 $originalHeader 0x30
        $dataBytes = $imageEnd - $dataAddress
        [byte[]]$expectedHeader = $originalHeader.Clone()
        [Array]::Copy([BitConverter]::GetBytes([uint32]($dataBytes / 0x1000)), 0, $expectedHeader, 0x34, 4)
        [Array]::Copy([BitConverter]::GetBytes([uint32]$dataBytes), 0, $expectedHeader, 0x38, 4)
        [Array]::Clear($expectedHeader, 0x3c, 4)
        Assert-Check ((Hash $header) -eq (Hash $expectedHeader)) 'Unexpected paired-exheader edit'
        Assert-Check (((U32 $header 0x14) + (U32 $header 0x24) + (U32 $header 0x34)) * 0x1000 -eq $expandedSize) 'Paired allocation length'
        $bssStart = $dataAddress + (U32 $originalHeader 0x38)
        $bssEnd = $bssStart + (U32 $originalHeader 0x3c)
        Assert-Check ((($bssEnd + 0xfff) -band -4096) -eq $rx.address) 'Payload follows the original zero-initialised data'
        Assert-Check ((Hash $patched ($bssStart - $base) ($rx.address - $bssStart)) -eq (Hash ([byte[]]::new($rx.address - $bssStart)))) 'Former zero-initialised data is zero'

        if ($Extra) { & $Extra $original $patched $parsed }

        $report = [ordered]@{
            status = 'passed'; package = $packageName; patch = $Kind
            source_code_sha256 = $codeHash; source_exheader_sha256 = $headerHash; source_elf_sha256 = $elfHash
            ips_sha256 = (Hash $ips); paired_exheader_sha256 = (Hash $header)
            ips_records = $expansion.records; native_regions = $Edits.Count + 1; runtime_exports = $names.Count
            system_call_wrappers = $wrappers.Count
            expanded_code_bytes = $expandedSize; expanded_code_sha256 = $patchedHash
            initial_extension_fill = 'a5'; elf_loads = $parsed.loads
            checks = @('original inputs identified by hash', 'every listed edit holds its original word before and its replacement after, branch and pointer targets by their ELF symbols', 'all IPS records bounded and disjoint, none outside the listed edits and the start-up hook', 'added memory fully written over a nonzero fill', 'the expanded image equals the independently built one, byte for byte', 'the start-up hook replaces only zero bytes and calls the original application init first', 'every entry function and system-call wrapper is a global ARM function of the start-up hook or the payload code', 'former zero-initialised data and every zero-initialised ELF byte are zero', 'only paired exheader fields 0x34/0x38/0x3c changed, and they cover the expanded image', 'manifest hashes, exports, placement and status match') + $Checks
            limitations = @('static artifact verification; hardware behavior not established')
        }
        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
        $report | ConvertTo-Json -Depth 8
    } catch {
        [ordered]@{ status = 'failed'; package = $packageName; reason = $_.Exception.Message; line = $_.InvocationInfo.ScriptLineNumber } |
            ConvertTo-Json | Set-Content -LiteralPath $reportPath -Encoding utf8
        throw
    }
}
