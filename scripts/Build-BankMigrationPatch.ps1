<#
Builds the migration patch for Bank into build/migration and checks it
with Verify-BankPatch.ps1. The migration belongs to one step: it converts the
Bank that the previous release stored into the current files and does nothing else; it
never opens the Bank (docs/building.md). It is a crate of its own,
patches/bank-migrate, with its own short list of edits. The folder holds one
package, the latest: an existing one is replaced. Its name
(Get-PackageId.ps1) is in PACKAGE.txt.
#>
[CmdletBinding()]
param(
    [string]$CiaPath = 'input/Bank-v1.5-update.cia',
    [string]$CtrtoolPath = 'tools/ctrtool.exe',
    # The version the package says it is; without one, "dev" (docs/building.md).
    [string]$Version = ''
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
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Patch migrate -Version $Version | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/migration/bank-migrate-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind migration
    $output = Join-Path $projectRoot 'build/migration'
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    & cargo +stable run --locked --offline -p patch-builder -- build-development migrate $inputRecord.code $inputRecord.exheader $elf $output | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Migration patch build failed: $LASTEXITCODE" }
    Set-Content -LiteralPath (Join-Path $output 'PACKAGE.txt') -Encoding utf8 -Value $id
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -Patch migrate | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the migration package failed' }
    Write-Host "Migration package $id`: $output (verified)"
} finally { Pop-Location }
