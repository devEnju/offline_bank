[CmdletBinding()]
param(
    # Which patch for Bank: the offline patch, the migration patch, or the
    # console test build of the offline patch (docs/building.md).
    [ValidateSet('offline', 'migrate', 'test')][string]$Patch = 'offline'
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
# The crate, its linker script, its cargo features, and the name the package
# scripts know it by. The test build has a folder of its own, so that the
# normal payload stays as it is.
$source = @{
    'offline' = @{ Crate = 'bank-payload'; Linker = 'patches/bank/link/bank15.ld'; Kind = 'bank'; Features = @() }
    'migrate' = @{ Crate = 'bank-migrate-payload'; Linker = 'patches/bank-migrate/link/bank15-migrate.ld'; Kind = 'bank-migrate'; Features = @() }
    'test' = @{ Crate = 'bank-payload'; Linker = 'patches/bank/link/bank15.ld'; Kind = 'bank-test'; Features = @('test-build') }
}[$Patch]
$linkerScript = Join-Path $projectRoot $source.Linker
$outputDir = Join-Path $projectRoot "build/intermediate/$($source.Kind)"
$featureList = (@('linked-image') + $source.Features) -join ','
$elfPath = Join-Path $outputDir "$($source.Crate).elf"
$mapPath = Join-Path $outputDir "$($source.Crate).map"
$toolchain = 'nightly-2026-10-03'
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    $linkerArgument = '-T' + $linkerScript.Replace('\', '/')
    $mapArgument = '-Map=' + $mapPath.Replace('\', '/')
    & cargo "+$toolchain" rustc --locked --offline -p $source.Crate --bin "$($source.Crate)-link" --features $featureList --release --target targets/armv6k-3ds.json -Z json-target-spec -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -- -C "link-arg=$linkerArgument" -C "link-arg=$mapArgument" -C link-arg=--build-id=none -C link-arg=--fatal-warnings -C link-arg=-zmax-page-size=4096 --remap-path-prefix "$projectRoot=/offline_bank"
    if ($LASTEXITCODE -ne 0) { throw "Payload link failed: $LASTEXITCODE" }
    $linkedPath = Join-Path $projectRoot "target/armv6k-3ds/release/$($source.Crate)-link"
    if (-not (Test-Path -LiteralPath $linkedPath -PathType Leaf)) { throw "Missing linked ELF: $linkedPath" }
    Copy-Item -LiteralPath $linkedPath -Destination $elfPath -Force
    & cargo +stable run --locked --offline -p patch-builder -- inspect-payload $Patch $elfPath
    if ($LASTEXITCODE -ne 0) { throw "Linked ELF validation failed: $LASTEXITCODE" }
    Get-FileHash -Algorithm SHA256 -LiteralPath $elfPath
    Write-Host 'Linked native-adapter ELF validated. Runtime hook profile and console validation remain required.'
} finally { Pop-Location }
