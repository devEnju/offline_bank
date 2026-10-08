[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('bank', 'transporter')][string]$Kind
)
# The name of a patch package: the first 16 hex digits of the SHA-256 over
# everything the patch is made from, which is the linked payload and the
# builder's profile with the edits of the original (some edits are plain
# words in the profile and leave the payload as it is). The same sources give
# the same name; line endings of the profile do not count.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$elf = Join-Path $projectRoot "build/intermediate/$Kind/$Kind-payload.elf"
$profile = Join-Path $projectRoot ("crates/patch-builder/src/{0}15.rs" -f $Kind)
foreach ($path in @($elf, $profile)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing package source: $path" }
}
$source = [Collections.Generic.List[byte]]::new()
$source.AddRange([IO.File]::ReadAllBytes($elf))
foreach ($byte in [IO.File]::ReadAllBytes($profile)) {
    if ($byte -ne 13) { $source.Add($byte) }
}
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $hash = -join ($sha.ComputeHash($source.ToArray()) | ForEach-Object { $_.ToString('x2') })
} finally { $sha.Dispose() }
$hash.Substring(0, 16)
