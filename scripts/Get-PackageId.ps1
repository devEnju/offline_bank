[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('bank', 'download', 'migration', 'test', 'transporter')][string]$Kind
)
# The name of a patch package: the first 16 hex digits of the SHA-256 over
# everything the patch is made from, which is the linked payload and the
# builder's profile with the edits of the original (some edits are plain
# words in the profile and leave the payload as it is). A Bank patch has two
# profile files: what belongs to the program, and the patch's own. The same
# sources give the same name; line endings of the profile do not count.
# A package lies in its folder with its name in PACKAGE.txt; the next build
# of that kind replaces it. The two patches of a release are in
# build/binary/bank and build/binary/transporter, the others in build/<kind>.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
# The crate a kind is built from. The test build is the Bank crate with a
# feature, in a folder of its own.
$crate = @{ 'bank' = 'bank'; 'test' = 'bank'; 'migration' = 'bank-migrate'; 'download' = 'bank-download'; 'transporter' = 'transporter' }[$Kind]
$elf = Join-Path $projectRoot "build/intermediate/$Kind/$crate-payload.elf"
$profiles = @{
    'bank'         = @('bank15.rs', 'bank15/offline.rs')
    'download'     = @('bank15.rs', 'bank15/download.rs')
    'migration'    = @('bank15.rs', 'bank15/migrate.rs')
    'test'         = @('bank15.rs', 'bank15/offline.rs', 'bank15/test.rs')
    'transporter'  = @('transporter15.rs')
}[$Kind] | ForEach-Object { Join-Path $projectRoot "crates/patch-builder/src/$_" }
foreach ($path in @($elf) + $profiles) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing package source: $path" }
}
$source = [Collections.Generic.List[byte]]::new()
$source.AddRange([IO.File]::ReadAllBytes($elf))
foreach ($file in $profiles) {
    foreach ($byte in [IO.File]::ReadAllBytes($file)) {
        if ($byte -ne 13) { $source.Add($byte) }
    }
}
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $hash = -join ($sha.ComputeHash($source.ToArray()) | ForEach-Object { $_.ToString('x2') })
} finally { $sha.Dispose() }
$hash.Substring(0, 16)
