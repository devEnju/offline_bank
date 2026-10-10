<#
Builds the console test packages of the Bank patch into build/bank-test/<name>
and checks each with Verify-BankPatch.ps1. They are for rehearsing an
interrupted Save and Quit on a console (docs/building.md) and are never part
of a release: New-Release.ps1 does not know them, and each folder holds a
TEST-BUILD.txt that says what it does. Existing test packages are replaced.
The normal payload in build/intermediate/bank is not touched.
#>
[CmdletBinding()]
param(
    [string]$CiaPath = 'input/Bank-v1.5-update.cia',
    [string]$CtrtoolPath = 'tools/ctrtool.exe',
    [ValidateSet('stop-before-game', 'stop-after-game', 'tear-record', 'tear-boxes')]
    [string[]]$Name = @('stop-before-game', 'stop-after-game', 'tear-record', 'tear-boxes')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$notes = @{
    'stop-before-game' = 'Save and Quit stops after the Bank has marked its save as in progress and before the game is written. Shows error 00007E57 00000001.'
    'stop-after-game'  = 'Save and Quit stops after the game is written and before the Bank clears its mark. Shows error 00007E57 00000002.'
    'tear-record'      = 'Save and Quit first writes the journal record 150 times, then saves normally. For cutting the power during those writes.'
    'tear-boxes'       = 'Save and Quit first writes the spare snapshot slot 30 times, then saves normally. For cutting the power during those writes.'
}
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
    foreach ($variant in $Name) {
        $intermediate = "build/intermediate/bank-test/$variant"
        $relative = "build/bank-test/$variant"
        $output = Join-Path $projectRoot $relative
        & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') -Features "test-$variant" -IntermediateDirectory $intermediate | Out-Host
        $elf = Join-Path $projectRoot "$intermediate/bank-payload.elf"
        if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
        if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
        New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
        & cargo +stable run --locked --offline -p patch-builder -- build-development offline $inputRecord.code $inputRecord.exheader $elf $output
        if ($LASTEXITCODE -ne 0) { throw "Test patch build failed: $LASTEXITCODE" }
        & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') -TestPackagePath $relative -ElfPath "$intermediate/bank-payload.elf" | Out-Null
        $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
        if ($report.status -ne 'passed') { throw "Verification failed for $variant" }
        Set-Content -LiteralPath (Join-Path $output 'TEST-BUILD.txt') -Encoding utf8 -Value @(
            "Pokemon Bank offline patch, TEST BUILD '$variant'. Not for normal use."
            $notes[$variant]
            'Back up Bank''s extdata and the game saves before using it, and put the normal package back afterwards.'
        )
        Write-Host "Test package: $output (verified)"
    }
} finally { Pop-Location }
