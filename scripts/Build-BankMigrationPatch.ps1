<#
Builds the migration package of the Bank patch into build/bank-migrate and
checks it with Verify-BankPatch.ps1. It converts a Bank stored by versions up
to 0.2.1 into the current files and does nothing else: it never opens the
Bank (docs/building.md). The folder holds a MIGRATION.txt that says so.
An existing migration package is replaced. The normal payload in
build/intermediate/bank is not touched.
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
    $intermediate = 'build/intermediate/bank-migrate'
    $relative = 'build/bank-migrate'
    $output = Join-Path $projectRoot $relative
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Features 'migrate' -IntermediateDirectory $intermediate | Out-Host
    $elf = Join-Path $projectRoot "$intermediate/bank-payload.elf"
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
    & cargo +stable run --locked --offline -p patch-builder -- build-development offline $inputRecord.code $inputRecord.exheader $elf $output
    if ($LASTEXITCODE -ne 0) { throw "Migration patch build failed: $LASTEXITCODE" }
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -TestPackagePath $relative -ElfPath "$intermediate/bank-payload.elf" | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the migration package failed' }
    Set-Content -LiteralPath (Join-Path $output 'MIGRATION.txt') -Encoding utf8 -Value @(
        'Pokemon Bank offline patch, MIGRATION PACKAGE. Not for normal use.'
        'It converts a Bank stored by versions up to 0.2.1 into the current files and never opens the Bank.'
        'Start Pokemon Bank once with it. The screen with two numbers is its result:'
        '  0000600D 00000001  converted'
        '  0000600D 00000002  already converted'
        '  0000600D 00000000  no Bank found'
        '  00000BAD 00000007  a Save and Quit is unfinished: start Bank once with the version you had, then run this again'
        '  00000BA1..00000BA4 a step failed; starting it again is safe'
        'Back up the extdata of Pokemon Bank and your game saves first. Then install the current Bank and Transporter packages.'
    )
    Write-Host "Migration package: $output (verified)"
} finally { Pop-Location }
