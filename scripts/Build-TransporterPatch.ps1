<#
Builds the Transporter patch into build/binary/transporter and checks it with
Verify-TransporterPatch.ps1. The folder holds one package, the latest: an
existing one is replaced. Its name (Get-PackageId.ps1) is in PACKAGE.txt.
#>
[CmdletBinding()]
param(
    [string]$CiaPath = 'input/Transporter-v1.5-update.cia',
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
    $code = $extractions[0].code
    $exheader = $extractions[0].exheader
    & (Join-Path $PSScriptRoot 'Build-TransporterPayload.ps1') | Out-Host
    $elf = Join-Path $projectRoot 'build/intermediate/transporter/transporter-payload.elf'
    if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) { throw "Missing hook image: $elf" }
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind transporter
    $output = Join-Path $projectRoot 'build/binary/transporter'
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    & cargo +stable run --locked --offline -p patch-builder -- build transporter $code $exheader $elf $output | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Transporter patch build failed: $LASTEXITCODE" }
    Set-Content -LiteralPath (Join-Path $output 'PACKAGE.txt') -Encoding utf8 -Value $id
    & (Join-Path $PSScriptRoot 'Verify-TransporterPatch.ps1') | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Verification of the Transporter package failed' }
    Write-Host "Transporter package $id`: $output (verified)"
    Write-Host 'No SD deployment performed. Console validation remains required.'
} finally { Pop-Location }
