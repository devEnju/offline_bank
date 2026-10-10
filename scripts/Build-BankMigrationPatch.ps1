<#
Builds the migration patch for Bank into build/bank-migrate/<package> and
checks it with Verify-BankPatch.ps1. The migration belongs to one step: it
converts the Bank that v0.2.1 stored into the files of v0.3.0 and does
nothing else; it never opens the Bank (docs/building.md). It is a crate of
its own, patches/bank-migrate, with its own short list of edits.
An existing package of the same sources is kept. Prints the package name.
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
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Patch migrate | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/bank-migrate/bank-migrate-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind bank-migrate
    $output = Join-Path $projectRoot "build/bank-migrate/$id"
    if (-not (Test-Path -LiteralPath $output)) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
        & cargo +stable run --locked --offline -p patch-builder -- build-development migrate $inputRecord.code $inputRecord.exheader $elf $output | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Migration patch build failed: $LASTEXITCODE" }
    }
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -Patch migrate -Package $id | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the migration package failed' }
    Write-Host "Migration package: $output (verified)"
    $id
} finally { Pop-Location }
