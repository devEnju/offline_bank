<#
Builds packages and checks each with its verify script (docs/building.md).

Without -Kind it builds the two patches of a release, Bank's into
build/binary/bank and Transporter's into build/binary/transporter. -Kind
builds one of the other packages for Bank instead: the console test build
(test, build/test), the migration from earlier releases (migration,
build/migration), or the download from the official server (download,
build/download).

Every package is made with the same steps: unpack its CIA, link its payload,
inspect the payload, build the package, name it (PACKAGE.txt), verify it. A
folder holds one package, the latest: an existing one is replaced.
#>
[CmdletBinding()]
param(
    [ValidateSet('', 'test', 'migration', 'download')][string]$Kind = '',
    # The version a package for Bank says it is on its error and result
    # screens; without one, "dev". Transporter shows none.
    [ValidatePattern('^(v\d+\.\d+\.\d+(-draft)?)?$')][string]$Version = '',
    [string]$BankCiaPath = 'input/Bank-v1.5-update.cia',
    [string]$TransporterCiaPath = 'input/Transporter-v1.5-update.cia',
    [string]$CtrtoolPath = 'tools/ctrtool.exe'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$toolchain = 'nightly-2026-10-03'

$packages = Import-PowerShellDataFile (Join-Path $PSScriptRoot 'packages.psd1')
$cias = @{ 'bank' = $BankCiaPath; 'transporter' = $TransporterCiaPath }

function Invoke-Native([string]$what, [scriptblock]$command) {
    & $command | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$what failed: $LASTEXITCODE" }
}

# The original code.bin and exheader.bin of a program, unpacked from its CIA.
$inputs = @{}
function Get-Input([string]$program) {
    if (-not $inputs.ContainsKey($program)) {
        $cia = $cias[$program]
        Invoke-Native 'CIA inspection' { cargo +stable run --locked --offline --quiet -p patch-builder -- inspect $cia }
        $records = @(& (Join-Path $PSScriptRoot 'Extract-Cia.ps1') -CiaPath $cia -CtrtoolPath $CtrtoolPath | Where-Object {
            $_.PSObject.Properties.Name -contains 'code' -and $_.PSObject.Properties.Name -contains 'exheader'
        })
        if ($records.Count -ne 1) { throw "Extraction of $cia did not return one input record." }
        $inputs[$program] = $records[0]
    }
    return $inputs[$program]
}

# Links the payload of a kind into build/intermediate/<kind>.
function Build-Payload([string]$kind, $package) {
    $outputDir = Join-Path $projectRoot "build/intermediate/$kind"
    $elfPath = Join-Path $outputDir "$($package.Crate).elf"
    $mapPath = Join-Path $outputDir "$($package.Crate).map"
    $linkerArgument = '-T' + (Join-Path $projectRoot $package.Linker).Replace('\', '/')
    $mapArgument = '-Map=' + $mapPath.Replace('\', '/')
    $features = (@('linked-image') + $package.Features) -join ','
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    # The payload reads the version from this variable when it is compiled.
    $versionBefore = $env:OFFLINE_BANK_VERSION
    try {
        $env:OFFLINE_BANK_VERSION = if ($Version -and $package.Program -eq 'bank') { $Version } else { $null }
        Invoke-Native 'Payload link' { cargo "+$toolchain" rustc --locked --offline -p $package.Crate --bin "$($package.Crate)-link" --features $features --release --target targets/armv6k-3ds.json -Z json-target-spec -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -- -C "link-arg=$linkerArgument" -C "link-arg=$mapArgument" -C link-arg=--build-id=none -C link-arg=--fatal-warnings -C link-arg=-zmax-page-size=4096 --remap-path-prefix "$projectRoot=/offline_bank" }
    } finally { $env:OFFLINE_BANK_VERSION = $versionBefore }
    $linked = Join-Path $projectRoot "target/armv6k-3ds/release/$($package.Crate)-link"
    if (-not (Test-Path -LiteralPath $linked -PathType Leaf)) { throw "Missing linked ELF: $linked" }
    Copy-Item -LiteralPath $linked -Destination $elfPath -Force
    Invoke-Native 'Payload inspection' { cargo +stable run --locked --offline --quiet -p patch-builder -- inspect-payload $kind $elfPath }
    return $elfPath
}

function Build-Package([string]$kind) {
    $package = $packages[$kind]
    $original = Get-Input $package.Program
    $elf = Build-Payload $kind $package
    $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind $kind
    $output = Join-Path $projectRoot $package.Folder
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
    Invoke-Native 'Package build' { cargo +stable run --locked --offline --quiet -p patch-builder -- build $kind $original.code $original.exheader $elf $output }
    Set-Content -LiteralPath (Join-Path $output 'PACKAGE.txt') -Encoding utf8 -Value $id
    & (Join-Path $PSScriptRoot $package.Verify) -Kind $kind | Out-Null
    $report = Get-Content -LiteralPath (Join-Path $output 'verification.json') -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw "Verification of the $kind package failed" }
    if ($kind -eq 'test') { Write-TestNote $output }
    Write-Host "Package $kind $id`: $output (verified)"
}

# The test build is never released: its folder says what it does.
function Write-TestNote([string]$output) {
    Set-Content -LiteralPath (Join-Path $output 'TEST-BUILD.txt') -Encoding utf8 -Value @(
        'Pokemon Bank offline patch, TEST BUILD. Not for normal use. Its start screen says "Ver. 1.5-test".'
        ''
        'After START it shows the main menu of the original. The entry chosen there opens the Bank and says'
        'what the next Save and Quit of that session does. The buttons are in the console''s language:'
        ''
        '  Use Pokemon Bank           saves normally. Also settles a save that another entry stopped.'
        '  Use Pokemon Bank (before)  stops after the Bank has marked its save as in progress and before'
        '                             the game is written. Shows error 00007E57 00000001.'
        '  Use Pokemon Bank (after)   stops after the game is written and before the Bank clears its mark.'
        '                             Shows error 00007E57 00000002.'
        '  Use Pokemon Bank (record)  first writes the journal record 150 times, then saves normally.'
        '                             For cutting the power during those writes.'
        '  Use Pokemon Bank (boxes)   first writes the file of the boxes that is not in use 30 times, then'
        '                             saves normally.'
        '                             For cutting the power during those writes.'
        '  Back                       the original: asks, then returns to the start screen.'
        ''
        'Back on the game selection leads to this menu again.'
        'The reward dialogs are never shown, and nothing is claimed.'
        'Back up Bank''s extdata and the game saves before using it, and put the normal package back afterwards.'
    )
}

Push-Location -LiteralPath $projectRoot
try {
    $kinds = if ($Kind) { @($Kind) } else { @('bank', 'transporter') }
    foreach ($each in $kinds) { Build-Package $each }
} finally { Pop-Location }
