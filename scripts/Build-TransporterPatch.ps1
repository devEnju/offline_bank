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
    $output = Join-Path $projectRoot ('build/transporter/' + (& (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind transporter))
    if (Test-Path -LiteralPath $output) { throw "Output already exists and will not be replaced: $output" }
    New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
    & cargo +stable run --locked --offline -p patch-builder -- build-transporter $code $exheader $elf $output
    if ($LASTEXITCODE -ne 0) { throw "Transporter patch build failed: $LASTEXITCODE" }
    Write-Host "Transporter patch: $output"
    Write-Host 'No SD deployment performed. Console validation remains required.'
} finally { Pop-Location }
