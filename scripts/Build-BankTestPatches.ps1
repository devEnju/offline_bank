<#
Builds the console test package of the Bank patch into build/bank-test and
checks it with Verify-BankPatch.ps1. It is for rehearsing an interrupted Save
and Quit on a console (docs/building.md) and is never part of a release:
New-Release.ps1 does not know it, and its folder holds a TEST-BUILD.txt that
says what it does. An existing test package is replaced. The normal payload in
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
    $relative = 'build/bank-test'
    $output = Join-Path $projectRoot $relative
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Patch test | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/bank-test/bank-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
    & cargo +stable run --locked --offline -p patch-builder -- build-development test $inputRecord.code $inputRecord.exheader $elf $output
    if ($LASTEXITCODE -ne 0) { throw "Test patch build failed: $LASTEXITCODE" }
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -Patch test -TestPackagePath $relative | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification failed for the test package' }
    Set-Content -LiteralPath (Join-Path $output 'TEST-BUILD.txt') -Encoding utf8 -Value @(
        'Pokemon Bank offline patch, TEST BUILD. Not for normal use. Its start screen says "Ver. 1.5-test".'
        ''
        'After START it shows the main menu of the original. The entry chosen there opens the Bank and says'
        'what the next Save and Quit of that session does. The buttons keep their original labels:'
        ''
        '  Use Pokemon Bank              saves normally. Also settles a save that another entry stopped.'
        '  Learn about Pokemon Bank      stops after the Bank has marked its save as in progress and before'
        '                                the game is written. Shows error 00007E57 00000001.'
        '  View Your Support ID Number   stops after the game is written and before the Bank clears its'
        '                                mark. Shows error 00007E57 00000002.'
        '  Download Poke Transporter     first writes the journal record 150 times, then saves normally.'
        '  (or: Poke Transporter Downloaded)  For cutting the power during those writes.'
        '  Move Pokemon to Pokemon HOME  first writes the spare snapshot slot 30 times, then saves'
        '                                normally. For cutting the power during those writes.'
        '  Back                          the original: asks, then returns to the start screen.'
        ''
        'The reward dialogs are never shown, and nothing is claimed.'
        'Back up Bank''s extdata and the game saves before using it, and put the normal package back afterwards.'
    )
    Write-Host "Test package: $output (verified)"
} finally { Pop-Location }
