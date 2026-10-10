[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Package,
    [string]$InputPath = ''
)
# Independent check of a Transporter patch package. Shares no code with the
# builder: it applies code.ips itself and compares the result with the
# original executable and exheader. Writes verification.json into the package.
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
$entryCount = 17

function Get-Sha256([byte[]]$bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return -join ($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) } finally { $sha.Dispose() }
}
function Get-Word([byte[]]$bytes, [int]$address) { return [BitConverter]::ToUInt32($bytes, $address - $base) }
function Get-BranchTarget([int]$address, [uint32]$word) {
    $offset = [int]($word -band 0x00ffffff)
    if ($offset -band 0x00800000) { $offset -= 0x01000000 }
    return $address + 8 + $offset * 4
}
function Assert-That([bool]$condition, [string]$message) { if (-not $condition) { throw "Verification failed: $message" } }

if (-not $InputPath) {
    $candidates = @(Get-ChildItem -LiteralPath (Join-Path $projectRoot 'build/extracted') -Directory |
        Where-Object {
            $code = Join-Path $_.FullName 'exefs/code.bin'
            (Test-Path -LiteralPath $code) -and ((Get-FileHash -LiteralPath $code -Algorithm SHA256).Hash.ToLowerInvariant() -eq $expectedCode)
        })
    Assert-That ($candidates.Count -ge 1) 'no extracted Transporter input with the reviewed code hash was found'
    $InputPath = $candidates[0].FullName
}
$original = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exefs/code.bin'))
$originalHeader = [IO.File]::ReadAllBytes((Join-Path $InputPath 'exheader.bin'))
Assert-That ((Get-Sha256 $original) -eq $expectedCode) 'original code.bin hash'
Assert-That ((Get-Sha256 $originalHeader) -eq $expectedExheader) 'original exheader.bin hash'
$directory = Join-Path $projectRoot ('build/transporter/' + $Package)

# Whole-word edits: address -> expected new word, or a branch form checked
# separately. Plain hashtable: an ordered dictionary would treat these integer
# keys as positions. Entry words are the first seventeen words of the payload.
$allowed = @{
    0x00103D9C = 'bl:0028D1AC'
    0x00242EF8 = 'bl:00364008'; 0x002445EC = 'bl:00364040'
    0x00246F48 = 'b:00364000'; 0x00248CDC = 'b:00248E44'
    0x00245728 = 'b:00245800'; 0x002460B8 = 'b:002461D0'
    0x002488A0 = 0xE3A00001; 0x002483CC = 0xE3A00001
    0x002458DC = 'bl:0036400C'
    0x00247478 = 'b:002474E0'
    0x00243528 = 'bl:00364010'; 0x002439A8 = 'bl:00364014'
    0x002437F4 = 'bl:00364018'; 0x0024397C = 'bl:00364018'
    0x00244F10 = 'bl:0036401C'; 0x00244908 = 'bl:00364020'
    0x0025C5FC = 'bl:00364028'; 0x0025C618 = 0xE320F000
    0x0022BEE0 = 'bl:0036402C'; 0x0022B5B0 = 'bl:00364030'
    0x0022B6E0 = 0xE3500007; 0x0022B6E4 = 0x03A01003
    0x002412D4 = 'bl:00364024'
    0x0022B3B8 = 'b:00364034'; 0x0013AEE4 = 'bl:0028D1DC'
    0x0024BA44 = 'bl:00364038'; 0x0024BA74 = 'bl:0036403C'
    0x0024A150 = 'b:00364004'
    0x0024A3C0 = 0xE3A0000E
}

$ips = [IO.File]::ReadAllBytes((Join-Path $directory 'code.ips'))
$header = [IO.File]::ReadAllBytes((Join-Path $directory 'exheader.bin'))
$manifest = Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw | ConvertFrom-Json

# Geometry, derived here from the original sizes only.
$bssStart = $dataAddress + $dataSize
$bssEnd = $bssStart + $bssSize
$payloadAddress = ($bssEnd + 0xFFF) -band (-bnot 0xFFF)
Assert-That ($bssEnd -eq 0x003638A4 -and $payloadAddress -eq 0x00364000) 'memory geometry'
Assert-That ([Convert]::ToInt32($manifest.payload_address, 16) -eq $payloadAddress) 'payload address'
$payloadCode = [int]$manifest.payload_executable_bytes
$payloadMemory = [int]$manifest.payload_memory_bytes
Assert-That ($payloadCode -gt 0 -and $payloadCode % 0x1000 -eq 0 -and $payloadMemory % 0x1000 -eq 0 -and $payloadCode -lt $payloadMemory) 'payload sizes are whole pages'
$expandedEnd = $payloadAddress + $payloadMemory
$expandedLength = $expandedEnd - $base
Assert-That ($expandedLength -eq [int]$manifest.expanded_code_bytes) 'expanded image size'
Assert-That ($expandedEnd -lt 0x08000000) 'expanded image stays below the application heap'

# Apply the IPS to the original image extended with zero bytes.
Assert-That ([Text.Encoding]::ASCII.GetString($ips, 0, 5) -eq 'PATCH') 'IPS header'
$patched = New-Object byte[] $expandedLength
[Array]::Copy($original, $patched, $original.Length)
$written = New-Object bool[] $expandedLength
$at = 5; $records = 0
while ([Text.Encoding]::ASCII.GetString($ips, $at, 3) -ne 'EOF') {
    $offset = ([int]$ips[$at] -shl 16) -bor ([int]$ips[$at + 1] -shl 8) -bor [int]$ips[$at + 2]
    $length = ([int]$ips[$at + 3] -shl 8) -bor [int]$ips[$at + 4]
    Assert-That ($length -gt 0 -and $offset + $length -le $expandedLength) 'IPS record inside the expanded image'
    for ($index = $offset; $index -lt $offset + $length; $index++) {
        Assert-That (-not $written[$index]) 'IPS records do not overlap'
        $written[$index] = $true
    }
    [Array]::Copy($ips, $at + 5, $patched, $offset, $length)
    $at += 5 + $length; $records++
}
Assert-That ($at + 3 -eq $ips.Length) 'IPS has trailing bytes'
Assert-That ((Get-Sha256 $patched) -eq $manifest.expanded_code_sha256) 'expanded image hash equals manifest'
Assert-That ((Get-Sha256 $ips) -eq $manifest.artifacts.'code.ips'.sha256) 'IPS hash equals manifest'
Assert-That ((Get-Sha256 $header) -eq $manifest.artifacts.'exheader.bin'.sha256) 'exheader hash equals manifest'

# Original text: every changed word is a listed edit or inside the start-up
# hook, which may only replace zero bytes after the original text.
$bootstrapStart = [Convert]::ToInt32($manifest.bootstrap_address, 16)
$bootstrapEnd = $bootstrapStart + [int]$manifest.bootstrap_bytes
Assert-That ($bootstrapStart -eq $textEnd -and $bootstrapEnd -le $textPageEnd) 'start-up hook lies after the original text'
for ($address = $textEnd; $address -lt $textPageEnd; $address++) {
    Assert-That ($original[$address - $base] -eq 0) 'bytes after the original text are zero'
}
$changed = 0
for ($address = $base; $address -lt $textPageEnd; $address += 4) {
    $old = Get-Word $original $address; $new = Get-Word $patched $address
    if ($old -eq $new) { continue }
    $changed++
    if ($address -ge $bootstrapStart -and $address -lt $bootstrapEnd) { continue }
    Assert-That $allowed.ContainsKey($address) ("unlisted word changed at {0:X8}" -f $address)
}
foreach ($address in $allowed.Keys) {
    $new = Get-Word $patched $address
    $want = $allowed[$address]
    if ($want -is [string]) {
        $kind, $target = $want.Split(':')
        $top = @{ 'b' = 0xEA; 'bl' = 0xEB }[$kind]
        Assert-That (($new -shr 24) -eq $top) ("{0:X8} is not '$kind'" -f $address)
        Assert-That ((Get-BranchTarget $address $new) -eq [Convert]::ToInt32($target, 16)) ("{0:X8} branch target" -f $address)
    } else {
        Assert-That (([int64]$new) -eq (([int64]$want) -band 0xFFFFFFFFL)) ("{0:X8} word" -f $address)
    }
}
# The original call that the hook replaces, and the hook's first two words.
$originalCall = Get-Word $original $startupCall
Assert-That ((($originalCall -shr 24) -eq 0xEB) -and ((Get-BranchTarget $startupCall $originalCall) -eq $originalAppInit)) 'original start-up call'
Assert-That (([int64](Get-Word $patched $bootstrapStart)) -eq 0xE92D4010L) 'start-up hook begins with push {r4, lr}'
$second = Get-Word $patched ($bootstrapStart + 4)
Assert-That ((($second -shr 24) -eq 0xEB) -and ((Get-BranchTarget ($bootstrapStart + 4) $second) -eq $originalAppInit)) 'start-up hook calls the original application init first'
# The layout check, called from 0013AEE4 before the payload can run, lies in
# the same executable place and begins with the instruction it stands in for.
Assert-That ($bootstrapStart + 0x30 + 4 -le $bootstrapEnd -and ([int64](Get-Word $patched ($bootstrapStart + 0x30))) -eq 0xE1A01000L) 'layout check begins with mov r1, r0'

# Original read-only data and data: unchanged.
for ($index = $textPageEnd - $base; $index -lt $original.Length; $index++) {
    if ($original[$index] -ne $patched[$index]) { throw ("Verification failed: original data changed at {0:X8}" -f ($index + $base)) }
}
# Former zero-initialised data: written explicitly and zero. The bytes of the
# original file after its data section are part of it and must be zero too.
for ($address = $bssStart; $address -lt $payloadAddress; $address++) {
    Assert-That ($patched[$address - $base] -eq 0) 'former zero-initialised data is zero'
}
for ($index = $original.Length; $index -lt $expandedLength; $index++) {
    Assert-That $written[$index] 'every added byte is written by the IPS'
}
# Payload: distinct entry branches into its own code pages.
$targets = @()
for ($index = 0; $index -lt $entryCount; $index++) {
    $entry = $payloadAddress + 4 * $index
    $word = Get-Word $patched $entry
    $target = Get-BranchTarget $entry $word
    Assert-That (($word -shr 24) -eq 0xEA -and $target -ge $payloadAddress + 4 * $entryCount -and $target -lt $payloadAddress + $payloadCode) ("entry word at {0:X8} branches into the payload code" -f $entry)
    Assert-That ($targets -notcontains $target) 'entry targets are distinct'
    $targets += $target
}
$text = [Text.Encoding]::ASCII.GetString($patched, $payloadAddress - $base, $payloadCode)
Assert-That ($text.Contains('/mover.bin') -and $text.Contains('/mover.alt.bin')) 'payload names /mover.bin and /mover.alt.bin'
Assert-That ($text.Contains('/roms/nds/saves/POKEMON_')) 'payload names the SD save folder'

# Exheader: only the data page count, data size and BSS size may differ.
Assert-That ($header.Length -eq $originalHeader.Length) 'exheader length'
for ($index = 0; $index -lt $header.Length; $index++) {
    if ($index -ge 0x34 -and $index -lt 0x40) { continue }
    Assert-That ($header[$index] -eq $originalHeader[$index]) ("exheader byte {0:X} changed" -f $index)
}
$newDataSize = $expandedEnd - $dataAddress
Assert-That ([BitConverter]::ToUInt32($header, 0x34) -eq $newDataSize / 0x1000) 'exheader data pages cover the expanded image'
Assert-That ([BitConverter]::ToUInt32($header, 0x38) -eq $newDataSize) 'exheader data size covers the expanded image'
Assert-That ([BitConverter]::ToUInt32($header, 0x3C) -eq 0) 'exheader BSS size is zero'
Assert-That ([BitConverter]::ToUInt32($originalHeader, 0x38) -eq $dataSize -and [BitConverter]::ToUInt32($originalHeader, 0x3C) -eq $bssSize) 'original data and BSS sizes'

$report = [ordered]@{
    status = 'passed'
    package = $Package
    source_code_sha256 = $expectedCode
    source_exheader_sha256 = $expectedExheader
    ips_sha256 = (Get-Sha256 $ips)
    paired_exheader_sha256 = (Get-Sha256 $header)
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
        'every listed edit holds the expected instruction or branch target',
        'start-up hook replaces only zero bytes and calls the original application init first',
        'original read-only data and data unchanged',
        'former zero-initialised data written as zero; every added byte written',
        'payload starts with seventeen distinct branches into its own code pages',
        'only exheader fields 0x34/0x38/0x3c changed, and they cover the expanded image',
        'manifest hashes match'
    )
    limitations = @('static artifact verification; hardware behavior not established')
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $directory 'verification.json') -Encoding utf8
$report | ConvertTo-Json -Depth 5
