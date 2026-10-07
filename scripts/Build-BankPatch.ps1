[CmdletBinding()]
param(
    [string]$CiaPath = 'input/Bank-v1.5-update.cia',
    [string]$CtrtoolPath = 'tools/ctrtool.exe',
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $projectRoot
try {
    if ($OutputDirectory) {
        $output = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
            [IO.Path]::GetFullPath($OutputDirectory)
        } else {
            [IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
        }
        if (Test-Path -LiteralPath $output) {
            throw "Output already exists and will not be replaced: $output"
        }
    }
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
    if (-not $OutputDirectory) {
        $elfHash = (Get-FileHash -LiteralPath $elf -Algorithm SHA256).Hash.ToLowerInvariant()
        $output = Join-Path $projectRoot ('build/bank/' + $elfHash.Substring(0,16))
    }
    if (Test-Path -LiteralPath $output) {
        throw "Output already exists and will not be replaced: $output"
    }
    $parent = Split-Path -Parent $output
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    & cargo +stable run --locked --offline -p patch-builder -- build-development $inputRecord.code $inputRecord.exheader $elf $output
    if ($LASTEXITCODE -ne 0) { throw "Development patch build failed: $LASTEXITCODE" }
    Write-Host "Reviewable development patch: $output"
    Write-Host 'No SD deployment performed. Console validation remains required.'
} finally { Pop-Location }
