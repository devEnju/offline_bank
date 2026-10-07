[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CiaPath,
    [string]$CtrtoolPath = 'tools/ctrtool.exe'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
function Resolve-ProjectFile([string]$value) {
    $path = if ([IO.Path]::IsPathRooted($value)) { $value } else { Join-Path $projectRoot $value }
    return (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
}
$cia = Resolve-ProjectFile $CiaPath
$ctrtool = Resolve-ProjectFile $CtrtoolPath
$ciaHash = (Get-FileHash -LiteralPath $cia -Algorithm SHA256).Hash.ToLowerInvariant()
# Inputs are never modified. Every extraction has its own content-addressed directory.
$output = Join-Path $projectRoot ('build/extracted/' + $ciaHash.Substring(0,16))
New-Item -ItemType Directory -Path $output -Force | Out-Null
& $ctrtool -q "--contents=$(Join-Path $output 'content')" $cia
if ($LASTEXITCODE -ne 0) { throw "CIA extraction failed with exit code $LASTEXITCODE" }
$main = @(Get-ChildItem -LiteralPath $output -File -Filter 'content.0000.*')
if ($main.Count -ne 1) { throw 'Expected exactly one main NCCH content at index 0.' }
$exheader = Join-Path $output 'exheader.bin'
$exefs = Join-Path $output 'exefs'
& $ctrtool -q "--exheader=$exheader" "--exefsdir=$exefs" "--romfs=$(Join-Path $output 'romfs.bin')" $main[0].FullName
if ($LASTEXITCODE -ne 0) { throw "NCCH extraction failed with exit code $LASTEXITCODE" }
$code = Join-Path $exefs 'code.bin'
$codeHash = (Get-FileHash -LiteralPath $code -Algorithm SHA256).Hash.ToLowerInvariant()
$record = [ordered]@{
    schema_version = 1
    source = $cia
    cia_sha256 = $ciaHash
    code = $code
    code_sha256 = $codeHash
    exheader = $exheader
    exheader_sha256 = (Get-FileHash -LiteralPath $exheader -Algorithm SHA256).Hash.ToLowerInvariant()
    main_content = $main[0].FullName
    note = 'Extraction metadata only; not proof of patch compatibility.'
}
$record | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'source.json') -Encoding utf8
[pscustomobject]$record
