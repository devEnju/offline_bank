<#
Builds the offline patch for Bank into build/bank and checks it with
Verify-BankPatch.ps1. The folder holds one package, the latest: an existing
one is replaced. Its name (Get-PackageId.ps1) is in PACKAGE.txt.
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
    & cargo +stable run --locked --offline -p patch-builder -- inspect $CiaPath
    if ($LASTEXITCODE -ne 0) { throw "CIA inspection failed: $LASTEXITCODE" }
    $records = @(& (Join-Path $PSScriptRoot 'Extract-Cia.ps1') -CiaPath $CiaPath -CtrtoolPath $CtrtoolPath)
    $extractions = @($records | Where-Object {
        $_.PSObject.Properties.Name -contains 'code' -and
        $_.PSObject.Properties.Name -contains 'exheader'
    })
    if ($extractions.Count -ne 1) { throw 'Extraction did not return one input record.' }
    $inputRecord = $extractions[0]
    & (Join-Path $PSScriptRoot 'Build-BankPayload.ps1') | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/bank/bank-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing payload ELF: $elf" }
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind bank
    $output = Join-Path $projectRoot 'build/bank'
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    & cargo +stable run --locked --offline -p patch-builder -- build-development offline $inputRecord.code $inputRecord.exheader $elf $output | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Development patch build failed: $LASTEXITCODE" }
    Set-Content -LiteralPath (Join-Path $output 'PACKAGE.txt') -Encoding utf8 -Value $id
    & (Join-Path $PSScriptRoot 'Verify-BankPatch.ps1') | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the Bank package failed' }
    Write-Host "Bank package $id`: $output (verified)"
    Write-Host 'No SD deployment performed. Console validation remains required.'
} finally { Pop-Location }
