[CmdletBinding()]
param(
    # Extra cargo features: the console test builds (docs/building.md).
    [string[]]$Features = @(),
    # Where the linked ELF goes, relative to the repository. A test build
    # names its own folder so that the normal payload stays as it is.
    [string]$IntermediateDirectory = 'build/intermediate/bank'
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$linkerScript = Join-Path $projectRoot 'patches/bank/link/bank15.ld'
$outputDir = Join-Path $projectRoot $IntermediateDirectory
$featureList = (@('linked-image') + $Features) -join ','
$elfPath = Join-Path $outputDir 'bank-payload.elf'
$mapPath = Join-Path $outputDir 'bank-payload.map'
$toolchain = 'nightly-2026-10-03'
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    $linkerArgument = '-T' + $linkerScript.Replace('\', '/')
    $mapArgument = '-Map=' + $mapPath.Replace('\', '/')
    & cargo "+$toolchain" rustc --locked --offline -p bank-payload --bin bank-payload-link --features $featureList --release --target targets/armv6k-3ds.json -Z json-target-spec -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -- -C "link-arg=$linkerArgument" -C "link-arg=$mapArgument" -C link-arg=--build-id=none -C link-arg=--fatal-warnings -C link-arg=-zmax-page-size=4096 --remap-path-prefix "$projectRoot=/offline_bank"
    if ($LASTEXITCODE -ne 0) { throw "Payload link failed: $LASTEXITCODE" }
    $linkedPath = Join-Path $projectRoot 'target/armv6k-3ds/release/bank-payload-link'
    if (-not (Test-Path -LiteralPath $linkedPath -PathType Leaf)) { throw "Missing linked ELF: $linkedPath" }
    Copy-Item -LiteralPath $linkedPath -Destination $elfPath -Force
    & cargo +stable run --locked --offline -p patch-builder -- inspect-payload $elfPath
    if ($LASTEXITCODE -ne 0) { throw "Linked ELF validation failed: $LASTEXITCODE" }
    Get-FileHash -Algorithm SHA256 -LiteralPath $elfPath
    Write-Host 'Linked native-adapter ELF validated. Runtime hook profile and console validation remain required.'
} finally { Pop-Location }
