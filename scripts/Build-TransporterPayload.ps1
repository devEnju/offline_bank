[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$linkerScript = Join-Path $projectRoot 'patches/transporter/link/transporter15.ld'
$outputDir = Join-Path $projectRoot 'build/intermediate/transporter'
$elfPath = Join-Path $outputDir 'transporter-payload.elf'
$mapPath = Join-Path $outputDir 'transporter-payload.map'
$toolchain = 'nightly-2026-10-03'
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    $linkerArgument = '-T' + $linkerScript.Replace('\', '/')
    $mapArgument = '-Map=' + $mapPath.Replace('\', '/')
    & cargo "+$toolchain" rustc --locked --offline -p transporter-payload --bin transporter-payload-link --features linked-image --profile transporter --target targets/armv6k-3ds.json -Z json-target-spec -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -- -C "link-arg=$linkerArgument" -C "link-arg=$mapArgument" -C link-arg=--build-id=none -C link-arg=--fatal-warnings -C link-arg=-zmax-page-size=4096 --remap-path-prefix "$projectRoot=/offline_bank"
    if ($LASTEXITCODE -ne 0) { throw "Transporter payload link failed: $LASTEXITCODE" }
    $linkedPath = Join-Path $projectRoot 'target/armv6k-3ds/transporter/transporter-payload-link'
    if (-not (Test-Path -LiteralPath $linkedPath -PathType Leaf)) { throw "Missing linked ELF: $linkedPath" }
    Copy-Item -LiteralPath $linkedPath -Destination $elfPath -Force
    Get-FileHash -Algorithm SHA256 -LiteralPath $elfPath
    Write-Host 'Transporter hook image linked. It is not an installable patch by itself.'
} finally { Pop-Location }
