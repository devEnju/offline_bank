[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    # The one earlier version whose Bank this release converts. Only for the
    # release that changes how the Bank is stored; adds the migration zip.
    [string]$MigrateFrom = '',
    [switch]$Draft,
    [switch]$Publish
)
# Makes a release of both patches from the committed sources:
#   tests, build, independent verification, zip for the SD card, release
#   notes, and an annotated git tag on the commit it was built from.
# Nothing leaves this machine unless -Publish is given, which pushes the tag
# and creates the GitHub release from the zips made by an earlier run.
# -Draft packs the working tree as it is for testing: no tag, marked as draft.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ($Version -notmatch '^v\d+\.\d+\.\d+$') { throw "Version must look like v1.0.0: $Version" }
if ($Draft -and $Publish) { throw 'A draft cannot be published.' }

# The first version that stores the Bank the way this one does. A Bank last
# used with an older version is not continued by a newer one; the release
# named here ships the migration for the version before it, and no other
# release ships one.
$bankFilesSince = 'v0.3.0'
if ($MigrateFrom) {
    if ($MigrateFrom -notmatch '^v\d+\.\d+\.\d+$') { throw "MigrateFrom must look like v1.0.0: $MigrateFrom" }
    if ($Publish) { throw '-Publish finds the migration zip of the earlier run by itself.' }
    if ($Version -ne $bankFilesSince) {
        throw "Only $bankFilesSince, which changed how the Bank is stored, ships a migration."
    }
}

$suffix = if ($Draft) { '-draft' } else { '' }
$name = "offline_bank-$Version$suffix"
$releaseRoot = Join-Path $projectRoot 'build/release'
$zip = Join-Path $releaseRoot "$name.zip"
$notesPath = Join-Path $releaseRoot "$name.md"
# The migration zip carries the version it converts from.
$migrationName = if ($MigrateFrom) { "offline_bank-$MigrateFrom-migration$suffix" } else { '' }
$migrationZip = if ($MigrateFrom) { Join-Path $releaseRoot "$migrationName.zip" } else { '' }
$bankTitle = '00040000000C9B00'
$disclaimer = @(
    'Unofficial fan project. Not affiliated with or endorsed by Nintendo,'
    'The Pokemon Company, GAME FREAK or Creatures. Provided as is, without'
    'warranty; use at your own risk.'
)

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
    # Second step, after the console test: the tag and the zips of an earlier run.
    foreach ($path in @($zip, $notesPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Run without -Publish first; missing $path" }
    }
    $tagged = Invoke-Git rev-parse --verify --quiet "refs/tags/$Version^{commit}"
    $notes = Get-Content -LiteralPath $notesPath -Raw
    if ($notes -notmatch [regex]::Escape($tagged)) { throw "The notes do not name the commit of tag $Version ($tagged)." }
    $assets = @($zip)
    # A migration zip the notes name belongs to this release as well.
    if ($notes -match 'offline_bank-v\d+\.\d+\.\d+-migration\.zip') {
        $assets += Join-Path $releaseRoot $Matches[0]
    }
    foreach ($asset in $assets) {
        if (-not (Test-Path -LiteralPath $asset -PathType Leaf)) { throw "Missing $asset" }
        if ($notes -notmatch [regex]::Escape((Get-Sha256 $asset))) {
            throw "$(Split-Path -Leaf $asset) changed since its release notes were written."
        }
    }
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'The gh tool is not installed; upload by hand (see docs/building.md).' }
    Invoke-Checked "push tag $Version" { git -C $projectRoot push origin "refs/tags/$Version" }
    Push-Location -LiteralPath $projectRoot
    try {
        Invoke-Checked "create release $Version" {
            gh release create $Version @assets --title $Version --notes-file $notesPath --verify-tag
        }
    } finally { Pop-Location }
    Write-Host "Published $Version."
    return
}

# 1. State of the repository.
$commit = Invoke-Git rev-parse HEAD
$dirty = [bool](Invoke-Git status --porcelain)
if ($MigrateFrom) {
    & git -C $projectRoot rev-parse --verify --quiet "refs/tags/$MigrateFrom" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "There is no released version $MigrateFrom to migrate from." }
}
if (-not $Draft) {
    $branch = Invoke-Git branch --show-current
    if ($branch -ne 'main') { throw "Releases are made from main; this is '$branch'." }
    if ($dirty) { throw 'There are uncommitted changes. Commit them, or use -Draft for a test build.' }
    & git -C $projectRoot rev-parse --verify --quiet "refs/tags/$Version" | Out-Null
    if ($LASTEXITCODE -eq 0) { throw "Tag $Version already exists." }
    foreach ($path in @($zip, $notesPath, $migrationZip) | Where-Object { $_ }) {
        if (Test-Path -LiteralPath $path) { throw "Output already exists and will not be replaced: $path" }
    }
}

Push-Location -LiteralPath $projectRoot
try {
    # 2. Tests.
    Invoke-Checked 'tests' { cargo +stable test --workspace --locked --offline --quiet }
    Invoke-Checked 'lint' { cargo +stable clippy --workspace --all-targets --locked --offline --quiet -- -D warnings }
    Invoke-Checked 'formatting' { cargo +stable fmt --all -- --check }

    # 3. The patches, built from these sources and independently verified.
    # A package is named after the hash of its compiled code and of the
    # builder's profile with the edits of the original (Get-PackageId.ps1),
    # so an existing one with that name was built from the same sources and
    # is reused.
    function Get-Verified([string]$kind, [string]$id) {
        $directory = Join-Path $projectRoot "build/$kind/$id"
        $verification = Get-Content -LiteralPath (Join-Path $directory 'verification.json') -Raw | ConvertFrom-Json
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
    function Get-Package([string]$kind, [string]$title) {
        & (Join-Path $PSScriptRoot "Build-${title}Payload.ps1") | Out-Host
        $id = & (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind $kind
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
        return Get-Verified $kind $id
    }
    $titles = [ordered]@{
        $bankTitle         = [pscustomobject]@{ Label = 'Pokemon Bank 1.5'; Package = (Get-Package 'bank' 'Bank') }
        '00040000000C9C00' = [pscustomobject]@{ Label = 'Poke Transporter 1.5'; Package = (Get-Package 'transporter' 'Transporter') }
    }
    # The migration patch builds and verifies itself and prints its name.
    $migration = if ($MigrateFrom) {
        Get-Verified 'bank-migrate' (& (Join-Path $PSScriptRoot 'Build-BankMigrationPatch.ps1'))
    }

    # 4. The zips, each laid out for the root of the SD card.
    $stage = Join-Path $releaseRoot $name
    $migrationStage = if ($MigrateFrom) { Join-Path $releaseRoot $migrationName } else { '' }
    foreach ($path in @($stage, $zip, $notesPath, $migrationStage, $migrationZip) | Where-Object { $_ }) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse }
    }
    $date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
    $source = if ($dirty) { "$commit plus uncommitted changes" } else { $commit }
    $draftMark = if ($Draft) { ' (DRAFT, not a release)' } else { '' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $table = [Collections.Generic.List[string]]::new()
    # Copies a package into a staged luma folder and lists its files.
    function Add-Files([string]$stageRoot, [string]$title, $package, [string]$label, [string]$archive, $readme) {
        $target = Join-Path $stageRoot "luma/titles/$title"
        New-Item -ItemType Directory -Path $target -Force | Out-Null
        $readme.Add("$label, package $($package.Name)")
        foreach ($file in $package.Hashes.Keys) {
            Copy-Item -LiteralPath (Join-Path $package.Directory $file) -Destination $target
            $readme.Add("  luma/titles/$title/$file")
            $readme.Add("    SHA-256 $($package.Hashes[$file])")
            $table.Add("| ``$archive`` | ``luma/titles/$title/$file`` | ``$($package.Hashes[$file])`` |")
        }
    }

    $readme = [Collections.Generic.List[string]]::new()
    $readme.Add("Pokemon Bank and Transporter Offline Patch $Version$draftMark")
    $readme.Add("Built $date from commit $source")
    $readme.Add('')
    $readme.AddRange([string[]]$disclaimer)
    $readme.Add('')
    $readme.Add('Copy the luma folder to the root of the SD card (merge it with the')
    $readme.Add('existing one) and switch on "Enable game patching" in the Luma3DS')
    $readme.Add('configuration (hold SELECT while powering on).')
    $readme.Add('')
    $readme.Add('The two files of a title only work as a pair. Back up your game saves')
    $readme.Add('and the extra data of Pokemon Bank first, and start Bank once before')
    $readme.Add('using Transporter. Always update Bank and Transporter together.')
    $readme.Add('')
    $readme.Add('If you already have an offline Bank: this version continues a Bank')
    $readme.Add("that was last used with $bankFilesSince or later, and no older one.")
    if ($MigrateFrom) {
        $readme.Add("Coming from ${MigrateFrom}? Convert your Bank first with")
        $readme.Add("$migrationName.zip from this release.")
        $readme.Add("Older than ${MigrateFrom}? Install $MigrateFrom first and start Bank once.")
    }
    $readme.Add('')
    foreach ($title in $titles.Keys) {
        Add-Files $stage $title $titles[$title].Package $titles[$title].Label "$name.zip" $readme
    }
    [IO.File]::WriteAllLines((Join-Path $stage 'README.txt'), $readme)
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip)
    Remove-Item -LiteralPath $stage -Recurse

    # The migration from v0.2.1: its own zip, with everything its user has
    # to know. This block goes when patches/bank-migrate goes.
    if ($MigrateFrom) {
        $steps = [Collections.Generic.List[string]]::new()
        $steps.Add("Pokemon Bank Offline Patch: migration from $MigrateFrom to $Version$draftMark")
        $steps.Add("Built $date from commit $source")
        $steps.Add('')
        $steps.AddRange([string[]]$disclaimer)
        $steps.Add('')
        $steps.Add("ONLY for a Bank that was last used with $MigrateFrom. It converts that")
        $steps.Add("Bank for $Version and does nothing else. It is not for any other version.")
        $steps.Add("- Older than ${MigrateFrom}: install $MigrateFrom first and start Bank once.")
        $steps.Add("- Never used the offline patch: you do not need this. Install $Version.")
        $steps.Add('')
        $steps.Add("1. With $MigrateFrom still installed, start Pokemon Bank once and make sure")
        $steps.Add('   it opens.')
        $steps.Add('2. Back up the extra data (extdata) of Pokemon Bank and your game saves,')
        $steps.Add('   for example with Checkpoint.')
        $steps.Add('3. Copy the luma folder of this zip to the root of the SD card. It')
        $steps.Add('   replaces the two files of Pokemon Bank.')
        $steps.Add('4. Start Pokemon Bank and press START. After a loading screen two')
        $steps.Add('   numbers appear. This patch never opens the Bank; the numbers are all')
        $steps.Add('   it shows.')
        $steps.Add('     0000600D 00000001  Converted. Go on with step 5.')
        $steps.Add('     0000600D 00000002  Already converted. Go on with step 5.')
        $steps.Add("     0000600D 00000000  No Bank was found. Go on with step 5; $Version")
        $steps.Add('                        creates one.')
        $steps.Add("     00000BAD 00000007  A Save and Quit was interrupted. Put $MigrateFrom")
        $steps.Add('                        back, start Bank once, then begin again at step 3.')
        $steps.Add('     00000BA1 to        A step failed. Start Pokemon Bank again with these')
        $steps.Add('     00000BA4           files; that is always safe. If it keeps happening,')
        $steps.Add('                        restore your backup and report both numbers.')
        $steps.Add("5. Install ${Version}: copy the luma folder of $name.zip to")
        $steps.Add('   the SD card. It replaces these files and updates Poke Transporter.')
        $steps.Add('6. Start Pokemon Bank. Your boxes, Pokedex and Poke Miles are as before.')
        $steps.Add('')
        $steps.Add('The conversion needs about 1.5 MB of free space on the SD card. If the')
        $steps.Add('power fails while it runs, start it again; the old Bank stays untouched')
        $steps.Add("until the new files are complete. After converting, $MigrateFrom cannot open")
        $steps.Add('the Bank any more; the backup from step 2 is the way back.')
        $steps.Add('')
        Add-Files $migrationStage $bankTitle $migration "Pokemon Bank 1.5, migration from $MigrateFrom" "$migrationName.zip" $steps
        [IO.File]::WriteAllLines((Join-Path $migrationStage 'README.txt'), $steps)
        [IO.Compression.ZipFile]::CreateFromDirectory($migrationStage, $migrationZip)
        Remove-Item -LiteralPath $migrationStage -Recurse
    }

    # 5. Release notes: everything needed to trace this version.
    $notes = [Collections.Generic.List[string]]::new()
    $notes.Add("Pokémon Bank and Transporter Offline Patch $Version" + $(if ($Draft) { ' (draft)' }))
    $notes.Add('')
    $notes.Add("Extract ``$name.zip`` and copy its ``luma`` folder to the root of the SD card. See the README of the repository for installation and the guides for each patch.")
    $notes.Add('')
    $notes.Add("If you already have an offline Bank: this version continues a Bank that was last used with $bankFilesSince or later, and no older one.")
    if ($MigrateFrom) {
        $notes.Add('')
        $notes.Add("**Coming from ${MigrateFrom}?** Convert your Bank first with ``$migrationName.zip``; the steps are in its ``README.txt``. It is only for a Bank last used with $MigrateFrom. Older than ${MigrateFrom}: install $MigrateFrom first and start Bank once.")
    }
    $notes.Add('')
    $notes.Add('| | |')
    $notes.Add('| --- | --- |')
    $notes.Add("| Built | $date |")
    $notes.Add("| Commit | ``$source`` |")
    $notes.Add("| Bank package | ``$($titles[$bankTitle].Package.Name)`` |")
    $notes.Add("| Transporter package | ``$($titles['00040000000C9C00'].Package.Name)`` |")
    $notes.Add("| ``$name.zip`` | ``$(Get-Sha256 $zip)`` |")
    if ($MigrateFrom) {
        $notes.Add("| Migration package | ``$($migration.Name)`` |")
        $notes.Add("| ``$migrationName.zip`` | ``$(Get-Sha256 $migrationZip)`` |")
    }
    $notes.Add('')
    $notes.Add('| In | File | SHA-256 |')
    $notes.Add('| --- | --- | --- |')
    $notes.AddRange($table)
    [IO.File]::WriteAllLines($notesPath, $notes)

    # 6. The tag, on the commit the zips were built from.
    if (-not $Draft) {
        if ((Invoke-Git rev-parse HEAD) -ne $commit -or (Invoke-Git status --porcelain)) {
            throw 'The repository changed while the release was built; nothing was tagged.'
        }
        Invoke-Checked "tag $Version" { git -C $projectRoot tag -a $Version -F $notesPath --cleanup=verbatim $commit }
    }
} finally { Pop-Location }

Write-Host ''
Write-Host "Archive: $zip"
if ($MigrateFrom) { Write-Host "Archive: $migrationZip" }
Write-Host "Notes:   $notesPath"
if ($Draft) {
    Write-Host 'Draft: no tag was created. Do not publish these files.'
} else {
    Write-Host "Tag:     $Version on $commit (local; nothing was pushed)"
    Write-Host ''
    Write-Host 'Next: test the zips on a console. Then either'
    Write-Host "  ./scripts/New-Release.ps1 -Version $Version -Publish"
    Write-Host "or push the tag (git push origin $Version) and upload the zips and the notes by hand."
    Write-Host "To withdraw before publishing: git tag -d $Version, and delete the files."
}
