[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $projectRoot
try {
    & cargo +nightly-2026-10-03 build -p bank-payload --lib --release --target targets/armv6k-3ds.json -Z json-target-spec -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem
    if ($LASTEXITCODE -ne 0) { throw "ARMv6K payload compilation failed: $LASTEXITCODE" }
    Write-Host 'ARMv6K payload-adapter compilation passed. This is not an installable Bank patch.'
} finally { Pop-Location }

