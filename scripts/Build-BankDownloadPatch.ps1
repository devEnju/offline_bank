<#
Builds the download patch for Bank into build/bank-download and checks it
with Verify-BankPatch.ps1. The download patch has the original sign in to
its server and load the Bank, writes what was loaded as the files of the
offline patch, and has the original leave without saving; it never shows the
boxes (docs/building.md). It is a crate of its own, patches/bank-download,
with its own short list of edits, and of use only while the service runs.
The folder holds one package, the latest: an existing one is replaced. Its
name (Get-PackageId.ps1) is in PACKAGE.txt.
#>
[CmdletBinding()]
param(
    [string]$CiaPath = 'input/Bank-v1.5-update.cia',
    [string]$CtrtoolPath = 'tools/ctrtool.exe'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $projectRoot
try {
    $records = @(& (Join-Path $PSScriptRoot 'Extract-Cia.ps1') -CiaPath $CiaPath -CtrtoolPath $CtrtoolPath)
    $extractions = @($records | Where-Object {
        $_.PSObject.Properties.Name -contains 'code' -and
        $_.PSObject.Properties.Name -contains 'exheader'
    })
    if ($extractions.Count -ne 1) { throw 'Extraction did not return one input record.' }
    $inputRecord = $extractions[0]
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Patch download | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/bank-download/bank-download-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind bank-download
    $output = Join-Path $projectRoot 'build/bank-download'
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    & cargo +stable run --locked --offline -p patch-builder -- build-development download $inputRecord.code $inputRecord.exheader $elf $output | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Download patch build failed: $LASTEXITCODE" }
    Set-Content -LiteralPath (Join-Path $output 'PACKAGE.txt') -Encoding utf8 -Value $id
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -Patch download | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the download package failed' }
    Write-Host "Download package $id`: $output (verified)"
} finally { Pop-Location }
