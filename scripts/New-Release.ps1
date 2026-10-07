[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$Draft,
    [switch]$Publish
)
# Makes a release of both patches from the committed sources:
#   tests, build, independent verification, zip for the SD card, release
#   notes, and an annotated git tag on the commit it was built from.
# Nothing leaves this machine unless -Publish is given, which pushes the tag
# and creates the GitHub release from a zip made by an earlier run.
# -Draft packs the working tree as it is for testing: no tag, marked as draft.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($Version -notmatch '^v\d+\.\d+\.\d+$') { throw "Version must look like v1.0.0: $Version" }
if ($Draft -and $Publish) { throw 'A draft cannot be published.' }

$name = if ($Draft) { "offline_bank-$Version-draft" } else { "offline_bank-$Version" }
$releaseRoot = Join-Path $projectRoot 'build/release'
$zip = Join-Path $releaseRoot "$name.zip"
$notesPath = Join-Path $releaseRoot "$name.md"

function Invoke-Git {
    $output = & git -C $projectRoot @args
    if ($LASTEXITCODE -ne 0) { throw "git $($args -join ' ') failed: $LASTEXITCODE" }
    return $output
}
function Invoke-Checked([string]$what, [scriptblock]$command) {
    Write-Host "== $what"
    & $command | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$what failed: $LASTEXITCODE" }
}
function Get-Sha256([string]$path) {
    return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

if ($Publish) {
    # Second step, after the console test: the tag and the zip of an earlier run.
    foreach ($path in @($zip, $notesPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Run without -Publish first; missing $path" }
    }
    $tagged = Invoke-Git rev-parse --verify --quiet "refs/tags/$Version^{commit}"
    $notes = Get-Content -LiteralPath $notesPath -Raw
    if ($notes -notmatch [regex]::Escape($tagged)) { throw "The notes do not name the commit of tag $Version ($tagged)." }
    if ($notes -notmatch [regex]::Escape((Get-Sha256 $zip))) { throw 'The zip changed since its release notes were written.' }
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'The gh tool is not installed; upload by hand (see docs/building.md).' }
    Invoke-Checked "push tag $Version" { git -C $projectRoot push origin "refs/tags/$Version" }
    Push-Location -LiteralPath $projectRoot
    try {
        Invoke-Checked "create release $Version" {
            gh release create $Version $zip --title $Version --notes-file $notesPath --verify-tag
        }
    } finally { Pop-Location }
    Write-Host "Published $Version."
    return
}

# 1. State of the repository.
$commit = Invoke-Git rev-parse HEAD
$dirty = [bool](Invoke-Git status --porcelain)
if (-not $Draft) {
    $branch = Invoke-Git branch --show-current
    if ($branch -ne 'main') { throw "Releases are made from main; this is '$branch'." }
    if ($dirty) { throw 'There are uncommitted changes. Commit them, or use -Draft for a test build.' }
    & git -C $projectRoot rev-parse --verify --quiet "refs/tags/$Version" | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "Tag $Version already exists." }
    foreach ($path in @($zip, $notesPath)) {
        if (Test-Path -LiteralPath $path) { throw "Output already exists and will not be replaced: $path" }
    }
}

Push-Location -LiteralPath $projectRoot
try {
    # 2. Tests.
    Invoke-Checked 'tests' { cargo +stable test --workspace --locked --offline --quiet }
    Invoke-Checked 'lint' { cargo +stable clippy --workspace --all-targets --locked --offline --quiet -- -D warnings }
    Invoke-Checked 'formatting' { cargo +stable fmt --all -- --check }

    # 3. Both patches, built from these sources and independently verified.
    # A package is named after the hash of its compiled code, so an existing
    # one with that name was built from the same code and is reused.
    function Get-Package([string]$kind, [string]$title) {
        & (Join-Path $PSScriptRoot "Build-${title}Payload.ps1") | Out-Host
        $elf = Join-Path $projectRoot "build/intermediate/$kind/$kind-payload.elf"
        $id = (Get-Sha256 $elf).Substring(0, 16)
        $directory = Join-Path $projectRoot "build/$kind/$id"
        if (-not (Test-Path -LiteralPath $directory)) {
            & (Join-Path $PSScriptRoot "Build-${title}Patch.ps1") | Out-Host
        }
        $verificationPath = Join-Path $directory 'verification.json'
        $passed = (Test-Path -LiteralPath $verificationPath) -and
            ((Get-Content -LiteralPath $verificationPath -Raw | ConvertFrom-Json).status -eq 'passed')
        if (-not $passed) {
            & (Join-Path $PSScriptRoot "Verify-${title}Patch.ps1") -Package $id | Out-Host
        }
        $verification = Get-Content -LiteralPath $verificationPath -Raw | ConvertFrom-Json
        if ($verification.status -ne 'passed') { throw "Package $kind/$id did not pass verification." }
        $files = [ordered]@{
            'code.ips'     = $verification.ips_sha256
            'exheader.bin' = $verification.paired_exheader_sha256
        }
        foreach ($file in $files.Keys) {
            if ((Get-Sha256 (Join-Path $directory $file)) -ne $files[$file]) {
                throw "Package $kind/${id}: $file changed after verification."
            }
        }
        return [pscustomobject]@{ Name = $id; Directory = $directory; Hashes = $files }
    }
    $titles = [ordered]@{
        '00040000000C9B00' = [pscustomobject]@{ Label = 'Pokemon Bank 1.5'; Package = (Get-Package 'bank' 'Bank') }
        '00040000000C9C00' = [pscustomobject]@{ Label = 'Poke Transporter 1.5'; Package = (Get-Package 'transporter' 'Transporter') }
    }

    # 4. The zip, laid out for the root of the SD card.
    $stage = Join-Path $releaseRoot $name
    foreach ($path in @($stage, $zip, $notesPath)) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse }
    }
    $date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
    $source = if ($dirty) { "$commit plus uncommitted changes" } else { $commit }
    $readme = [Collections.Generic.List[string]]::new()
    $readme.Add("Pokemon Bank and Transporter Offline Patch $Version" + $(if ($Draft) { ' (DRAFT, not a release)' }))
    $readme.Add("Built $date from commit $source")
    $readme.Add('')
    $readme.Add('Unofficial fan project. Not affiliated with or endorsed by Nintendo,')
    $readme.Add('The Pokemon Company, GAME FREAK or Creatures. Provided as is, without')
    $readme.Add('warranty; use at your own risk.')
    $readme.Add('')
    $readme.Add('Copy the luma folder to the root of the SD card (merge it with the')
    $readme.Add('existing one) and switch on "Enable game patching" in the Luma3DS')
    $readme.Add('configuration (hold SELECT while powering on).')
    $readme.Add('')
    $readme.Add('The two files of a title only work as a pair. Back up your game saves')
    $readme.Add('and the extra data of Pokemon Bank first, and start Bank once before')
    $readme.Add('using Transporter.')
    $readme.Add('')
    $table = [Collections.Generic.List[string]]::new()
    foreach ($title in $titles.Keys) {
        $entry = $titles[$title]
        $target = Join-Path $stage "luma/titles/$title"
        New-Item -ItemType Directory -Path $target -Force | Out-Null
        $readme.Add("$($entry.Label), package $($entry.Package.Name)")
        foreach ($file in $entry.Package.Hashes.Keys) {
            Copy-Item -LiteralPath (Join-Path $entry.Package.Directory $file) -Destination $target
            $readme.Add("  luma/titles/$title/$file")
            $readme.Add("    SHA-256 $($entry.Package.Hashes[$file])")
            $table.Add("| ``luma/titles/$title/$file`` | ``$($entry.Package.Hashes[$file])`` |")
        }
    }
    [IO.File]::WriteAllLines((Join-Path $stage 'README.txt'), $readme)
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip)
    Remove-Item -LiteralPath $stage -Recurse

    # 5. Release notes: everything needed to trace this version.
    $notes = [Collections.Generic.List[string]]::new()
    $notes.Add("Pokémon Bank and Transporter Offline Patch $Version" + $(if ($Draft) { ' (draft)' }))
    $notes.Add('')
    $notes.Add('Extract the zip and copy its `luma` folder to the root of the SD card. See the README of the repository for installation and the guides for each patch.')
    $notes.Add('')
    $notes.Add('| | |')
    $notes.Add('| --- | --- |')
    $notes.Add("| Built | $date |")
    $notes.Add("| Commit | ``$source`` |")
    $notes.Add("| Bank package | ``$($titles['00040000000C9B00'].Package.Name)`` |")
    $notes.Add("| Transporter package | ``$($titles['00040000000C9C00'].Package.Name)`` |")
    $notes.Add("| ``$name.zip`` | ``$(Get-Sha256 $zip)`` |")
    $notes.Add('')
    $notes.Add('| File | SHA-256 |')
    $notes.Add('| --- | --- |')
    $notes.AddRange($table)
    [IO.File]::WriteAllLines($notesPath, $notes)

    # 6. The tag, on the commit the zip was built from.
    if (-not $Draft) {
        if ((Invoke-Git rev-parse HEAD) -ne $commit -or (Invoke-Git status --porcelain)) {
            throw 'The repository changed while the release was built; nothing was tagged.'
        }
        Invoke-Checked "tag $Version" { git -C $projectRoot tag -a $Version -F $notesPath --cleanup=verbatim $commit }
    }
} finally { Pop-Location }

Write-Host ''
Write-Host "Archive: $zip"
Write-Host "Notes:   $notesPath"
if ($Draft) {
    Write-Host 'Draft: no tag was created. Do not publish this file.'
} else {
    Write-Host "Tag:     $Version on $commit (local; nothing was pushed)"
    Write-Host ''
    Write-Host 'Next: test the zip on a console. Then either'
    Write-Host "  ./scripts/New-Release.ps1 -Version $Version -Publish"
    Write-Host "or push the tag (git push origin $Version) and upload the zip and the notes by hand."
    Write-Host "To withdraw before publishing: git tag -d $Version, and delete the two files."
}
