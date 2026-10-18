<#
Tests the two verify scripts: each built package passes, and a copy with one
thing changed is refused for that thing. Where a change touches bytes, the
copy's manifest gets the new hashes, so that the refusal comes from the
independent comparison and not from a hash. The changes are made to Bank's
and Transporter's packages, which must be built (Build-Patches.ps1); of the
others, each one built is checked unchanged. Works on copies in a temporary
folder, which it deletes.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$packages = Import-PowerShellDataFile (Join-Path $PSScriptRoot 'packages.psd1')
# The unpacked originals (Extract-Cia.ps1), by program.
$originals = @{
    bank        = 'build/extracted/559b2b6a6fd3b001/exefs/code.bin'
    transporter = 'build/extracted/09f0a4ba5969f715/exefs/code.bin'
}
$work = Join-Path ([IO.Path]::GetTempPath()) ('offline_bank-verify-' + [Guid]::NewGuid().ToString('N'))

function Get-Sha([byte[]]$bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { [Convert]::ToHexString($sha.ComputeHash($bytes)).ToLowerInvariant() } finally { $sha.Dispose() }
}

# The IPS records: offset, length and where the data begins in the file.
function Get-IpsRecords([byte[]]$ips) {
    $records = @()
    $at = 5
    while ([Text.Encoding]::ASCII.GetString($ips, $at, 3) -ne 'EOF') {
        $offset = ([int]$ips[$at] -shl 16) -bor ([int]$ips[$at + 1] -shl 8) -bor [int]$ips[$at + 2]
        $length = ([int]$ips[$at + 3] -shl 8) -bor [int]$ips[$at + 4]
        $records += [pscustomobject]@{ Offset = $offset; Length = $length; Data = $at + 5 }
        $at += 5 + $length
    }
    return $records
}

function Read-Manifest([string]$dir) { Get-Content -LiteralPath (Join-Path $dir 'manifest.json') -Raw | ConvertFrom-Json }
function Write-Manifest([string]$dir, $manifest) {
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $dir 'manifest.json') -Encoding utf8
}

# Gives the copy's manifest the hashes of its changed files: the ELF, both
# package files, and the image the IPS makes of the original.
function Update-Hashes([string]$dir, [string]$kind) {
    $manifest = Read-Manifest $dir
    [byte[]]$ips = [IO.File]::ReadAllBytes((Join-Path $dir 'code.ips'))
    $manifest.source_elf_sha256 = Get-Sha ([IO.File]::ReadAllBytes((Join-Path $dir 'payload.elf')))
    $manifest.artifacts.'code.ips'.sha256 = Get-Sha $ips
    $manifest.artifacts.'exheader.bin'.sha256 = Get-Sha ([IO.File]::ReadAllBytes((Join-Path $dir 'exheader.bin')))
    [byte[]]$original = [IO.File]::ReadAllBytes((Join-Path $projectRoot $originals[$packages[$kind].Program]))
    $image = [byte[]]::new([int]$manifest.expanded_code_bytes)
    [Array]::Copy($original, $image, $original.Length)
    foreach ($record in Get-IpsRecords $ips) { [Array]::Copy($ips, $record.Data, $image, $record.Offset, $record.Length) }
    $manifest.expanded_code_sha256 = Get-Sha $image
    Write-Manifest $dir $manifest
}

# A copy of the package of `kind` and its ELF, as `payload.elf`.
function Copy-Package([string]$kind, [string]$name) {
    $dir = Join-Path $work $name
    New-Item -ItemType Directory -Path $dir | Out-Null
    foreach ($file in 'code.ips', 'exheader.bin', 'manifest.json', 'PACKAGE.txt') {
        Copy-Item -LiteralPath (Join-Path $projectRoot $packages[$kind].Folder $file) -Destination $dir
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot "build/intermediate/$kind/$($packages[$kind].Crate).elf") -Destination (Join-Path $dir 'payload.elf')
    return $dir
}

# Flips one byte of `file` in the copy at `at`.
function Edit-Byte([string]$dir, [string]$file, [int]$at) {
    $path = Join-Path $dir $file
    [byte[]]$bytes = [IO.File]::ReadAllBytes($path)
    $bytes[$at] = $bytes[$at] -bxor 0x01
    [IO.File]::WriteAllBytes($path, $bytes)
}

# Where in code.ips the byte of the image at `address` lies.
function Get-IpsByte([string]$dir, [int]$address) {
    [byte[]]$ips = [IO.File]::ReadAllBytes((Join-Path $dir 'code.ips'))
    $offset = $address - 0x00100000
    foreach ($record in Get-IpsRecords $ips) {
        if ($offset -ge $record.Offset -and $offset -lt $record.Offset + $record.Length) { return $record.Data + $offset - $record.Offset }
    }
    throw ("no IPS record holds {0:X8}" -f $address)
}

# The message the verify script refuses with, or $null if it accepts.
function Invoke-Verify([string]$kind, [string]$dir, [string]$as) {
    $script = Join-Path $PSScriptRoot $packages[$kind].Verify
    try {
        & $script -Kind $as -PackagePath $dir -ElfPath (Join-Path $dir 'payload.elf') | Out-Null
        return $null
    } catch { return $_.Exception.Message }
}

$failures = [Collections.Generic.List[string]]::new()
function Test-Case([string]$kind, [string]$name, [scriptblock]$change, [string]$expect, [string]$as = $kind) {
    $dir = Copy-Package $kind "$kind-$($name -replace '\W', '-')"
    if ($change) { & $change $dir }
    $message = Invoke-Verify $kind $dir $as
    $result = if ($expect -eq '') {
        if ($null -eq $message) { 'passed' } else { "FAILED: refused ($message)" }
    } elseif ($null -eq $message) {
        'FAILED: accepted'
    } elseif ($message -notlike "*$expect*") {
        "FAILED: refused for another reason ($message)"
    } else { 'refused' }
    Write-Host ('{0,-12} {1,-28} {2}' -f $kind, $name, $result)
    if ($result -like 'FAILED*') { $failures.Add("$kind $name") }
}

$location = Get-Location
New-Item -ItemType Directory -Path $work | Out-Null
try {
    foreach ($kind in 'bank', 'transporter', 'test', 'migration', 'download') {
        if ($kind -in 'bank', 'transporter' -or (Test-Path -LiteralPath (Join-Path $projectRoot $packages[$kind].Folder 'manifest.json'))) {
            Test-Case $kind 'unchanged' $null ''
        } else {
            Write-Host ('{0,-12} {1,-28} {2}' -f $kind, 'unchanged', 'not built')
        }
    }
    # Where each script refuses each change.
    $expected = @{
        bank        = @{
            'ELF header'          = 'ELF32 little-endian header'
            'start-up hook byte'  = 'Expanded image differs from independent expected image'
            'payload byte'        = 'Expanded image differs from independent expected image'
            'exheader size field' = 'Unexpected paired-exheader edit'
            'manifest export'     = 'ELF export bank_offline_next'
            'manifest placement'  = 'Manifest placement'
            'manifest kind'       = 'Manifest names another patch'
            'package name'        = 'Package is not the one of the current payload and profile'
        }
        transporter = @{
            'ELF header'          = 'ELF32 little-endian header'
            'start-up hook byte'  = 'ELF LOAD bytes at 0028D1AC'
            'payload byte'        = 'ELF LOAD bytes at 00364000'
            'exheader size field' = 'exheader BSS size'
            'manifest export'     = 'manifest export transporter_next'
            'manifest placement'  = 'manifest placement'
            'manifest kind'       = 'manifest names this patch and version'
            'package name'        = 'package is not the one of the current payload and profile'
        }
    }
    foreach ($kind in 'bank', 'transporter') {
        $kindNow = $kind
        Test-Case $kind 'ELF header' { param($dir) Edit-Byte $dir 'payload.elf' 4; Update-Hashes $dir $kindNow } $expected[$kind]['ELF header']
        Test-Case $kind 'start-up hook byte' {
            param($dir)
            Edit-Byte $dir 'code.ips' (Get-IpsByte $dir ([int](Read-Manifest $dir).placement.bootstrap_address + 8))
            Update-Hashes $dir $kindNow
        } $expected[$kind]['start-up hook byte']
        Test-Case $kind 'payload byte' {
            param($dir)
            Edit-Byte $dir 'code.ips' (Get-IpsByte $dir ([int](Read-Manifest $dir).placement.payload_address + 0x40))
            Update-Hashes $dir $kindNow
        } $expected[$kind]['payload byte']
        Test-Case $kind 'exheader size field' { param($dir) Edit-Byte $dir 'exheader.bin' 0x3d; Update-Hashes $dir $kindNow } $expected[$kind]['exheader size field']
        Test-Case $kind 'manifest export' {
            param($dir)
            $manifest = Read-Manifest $dir
            $first = @($manifest.exports.psobject.Properties)[0].Name
            $manifest.exports.$first = 0
            Write-Manifest $dir $manifest
        } $expected[$kind]['manifest export']
        Test-Case $kind 'manifest placement' {
            param($dir)
            $manifest = Read-Manifest $dir
            $manifest.placement.payload_rw_bytes += 0x1000
            Write-Manifest $dir $manifest
        } $expected[$kind]['manifest placement']
        Test-Case $kind 'manifest kind' {
            param($dir)
            $manifest = Read-Manifest $dir
            $manifest.patch = 'download'
            Write-Manifest $dir $manifest
        } $expected[$kind]['manifest kind']
        Test-Case $kind 'package name' {
            param($dir) Set-Content -LiteralPath (Join-Path $dir 'PACKAGE.txt') -Encoding utf8 -Value '0000000000000000'
        } $expected[$kind]['package name']
    }
    # The package of one kind is not that of another.
    Test-Case 'bank' 'checked as the test build' $null 'Package is not the one of the current payload and profile' 'test'
} finally {
    Set-Location -LiteralPath $location
    Remove-Item -LiteralPath $work -Recurse -Force
}
if ($failures.Count) { throw "Verify script tests failed: $($failures -join ', ')" }
Write-Host 'Verify script tests passed.'
