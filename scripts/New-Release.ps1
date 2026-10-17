[CmdletBinding()]
param(
    [switch]$Publish
)
# Makes a release of both patches from the committed sources:
#   tests, build, independent verification, zip for the SD card, release
#   notes, and an annotated git tag on the commit it was built from.
# Beside them it ships the migration below v1.0.0 and the download while
# the official service runs, each as long as packages.psd1 lists it.
# Nothing leaves this machine unless -Publish is given, which pushes the tag
# and creates the GitHub release from the zips made by an earlier run.
# A release is only ever made whole: to test one, make it, and withdraw it
# (docs/building.md) if it fails. The version of a release is the workspace's in Cargo.toml; a release commit
# sets it there and in Cargo.lock (docs/building.md).
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$manifest = Join-Path $projectRoot 'Cargo.toml'
# The version under [workspace.package], the one every crate but offline-core takes.
$section = ''
$release = $null
foreach ($line in Get-Content -LiteralPath $manifest) {
    if ($line -match '^\s*\[([^\]]+)\]') { $section = $Matches[1] }
    elseif ($section -eq 'workspace.package' -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $release = 'v' + $Matches[1] }
}
if (-not $release) { throw 'Cargo.toml has no version under [workspace.package].' }
if ($release -notmatch '^v\d+\.\d+\.\d+$') { throw "The version in Cargo.toml must look like 1.0.0: $release" }
$Version = $release

function ConvertTo-Version([string]$tag) { return [version]$tag.Substring(1) }

# What ships beside the two patches: every package for Bank that
# packages.psd1 lists and whose time has not passed.
$packages = Import-PowerShellDataFile (Join-Path $PSScriptRoot 'packages.psd1')
$Migrate = $packages.ContainsKey('migration')
$Download = $packages.ContainsKey('download')
# The day the official service ends, in its own time zone.
$serviceEnds = 'February 25, 2027'
if (-not $Publish) {
    if ($Migrate -and (ConvertTo-Version $release) -ge [version]'1.0.0') {
        throw "Releases from v1.0.0 on ship no migration. Remove it first (docs/building.md#the-migration-from-earlier-releases)."
    }
    $ended = [datetime]::ParseExact($serviceEnds, 'MMMM d, yyyy', [Globalization.CultureInfo]::InvariantCulture)
    if ($Download -and (Get-Date).Date -gt $ended) {
        throw "The official service ended on $serviceEnds. Remove the download first (docs/building.md#the-download-from-the-official-server)."
    }
}
# What the guides of the migration and the download quote, from the code
# that shows it (each crate's `<crate>-guide` binary): the patch's name, and each
# result by name with its numbers.
function Get-Guide([string]$crate) {
    $lines = & cargo +stable run --quiet --locked --offline --manifest-path $manifest -p $crate --bin "$crate-guide"
    if ($LASTEXITCODE -ne 0) { throw "The guide facts of $crate failed: $LASTEXITCODE" }
    $facts = @{}
    $ways = [Collections.Generic.List[object]]::new()
    foreach ($line in $lines) {
        $key, $rest = $line -split ' ', 2
        if ($key -eq 'way') {
            $from, $size = $rest -split ' '
            $ways.Add([pscustomobject]@{ From = $from; Size = $size })
        } else {
            $facts[$key] = $rest
        }
    }
    $facts['ways'] = $ways.ToArray()
    return $facts
}
# Shown as the console shows it: two numbers, or only a first one.
function Format-Numbers([string]$numbers) { return '`' + $numbers + '`' }
# The earlier ways of storing the Bank, oldest first, as only the migration
# knows them: the last release of each, whose release stays published for
# whoever has to settle a Bank with it, and the second number of error 3
# with which this release refuses such a Bank.
$bankFormats = @()
if ($Migrate) {
    $migrationFacts = Get-Guide 'bank-migrate-payload'
    $bankFormats = @($migrationFacts.ways)
    if (-not $bankFormats.Count) { throw 'The migration names no earlier way.' }
}
$downloadFacts = if ($Download) { Get-Guide 'bank-download-payload' }
$name = "offline_bank-$Version"
# A release lies in a folder of its own, named for its version.
$releaseRoot = Join-Path $projectRoot "build/release/$Version"
$zip = Join-Path $releaseRoot "$name.zip"
$notesPath = Join-Path $releaseRoot "$name.md"
# The zips beside the patches have no version in their names: the migration
# converts whatever earlier Bank it finds, and the download belongs to the
# official service, not to a version of the patch.
$migrationName = if ($Migrate) { 'offline_bank-migration' } else { '' }
$migrationZip = if ($Migrate) { Join-Path $releaseRoot "$migrationName.zip" } else { '' }
$downloadName = if ($Download) { 'offline_bank-download' } else { '' }
$downloadZip = if ($Download) { Join-Path $releaseRoot "$downloadName.zip" } else { '' }
# The release notes link to other releases and to the pictures on main.
$repository = 'https://github.com/devEnju/offline_bank'
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
    if ($notes -match 'offline_bank-migration\.zip') {
        $assets += Join-Path $releaseRoot $Matches[0]
    }
    # And so does a download zip.
    if ($notes -match 'offline_bank-download\.zip') {
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
if ($Migrate) {
    foreach ($format in $bankFormats) {
        & git -C $projectRoot rev-parse --verify --quiet "refs/tags/$($format.From)" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "There is no released version $($format.From) to migrate from." }
    }
    # The notes show the migration's result for each earlier way as
    # pictures/ has it on main.
    $pictures = @($bankFormats | ForEach-Object { "$($_.From)-migration-converted.png" }) + 'offline-boxes.png'
    foreach ($picture in $pictures) {
        if (-not (Test-Path -LiteralPath (Join-Path $projectRoot "pictures/$picture") -PathType Leaf)) {
            throw "The notes need pictures/$picture."
        }
    }
}
$branch = Invoke-Git branch --show-current
if ($branch -ne 'main') { throw "Releases are made from main; this is '$branch'." }
# Files git does not track may lie in the folder; a tracked file must be as committed.
if (Invoke-Git status --porcelain --untracked-files=no) { throw 'There are uncommitted changes. Commit them first.' }
& git -C $projectRoot rev-parse --verify --quiet "refs/tags/$Version" | Out-Null
if ($LASTEXITCODE -eq 0) { throw "Tag $Version already exists. Set the new version in Cargo.toml and Cargo.lock first." }
$newest = @(Invoke-Git tag -l 'v*' | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' } | Sort-Object { ConvertTo-Version $_ })
if ($newest.Count -and (ConvertTo-Version $Version) -le (ConvertTo-Version $newest[-1])) {
    throw "$Version is not newer than $($newest[-1]). Set the new version in Cargo.toml and Cargo.lock first."
}
foreach ($path in @($zip, $notesPath, $migrationZip, $downloadZip) | Where-Object { $_ }) {
    if (Test-Path -LiteralPath $path) { throw "Output already exists and will not be replaced: $path" }
}

Push-Location -LiteralPath $projectRoot
try {
    # 2. Tests.
    Invoke-Checked 'tests' { cargo +stable test --workspace --locked --offline --quiet }
    Invoke-Checked 'test-build tests' { cargo +stable test -p bank-common -p bank-payload --features test-build --release --locked --offline --quiet }
    Invoke-Checked 'lint' { cargo +stable clippy --workspace --all-targets --locked --offline --quiet -- -D warnings }
    Invoke-Checked 'formatting' { cargo +stable fmt --all -- --check }

    # 3. The patches, built from these sources and independently verified.
    # Build-Patches.ps1 replaces the package of each kind in its folder
    # (packages.psd1),
    # writes its name into PACKAGE.txt (Get-PackageId.ps1: the hash of its
    # compiled code and of the builder's profile with the edits of the
    # original), and verifies it.
    function Get-Verified([string]$kind) {
        $directory = Join-Path $projectRoot $packages[$kind].Folder
        $id = (Get-Content -LiteralPath (Join-Path $directory 'PACKAGE.txt') -Raw).Trim()
        if ($id -ne (& (Join-Path $PSScriptRoot 'Get-PackageId.ps1') -Kind $kind)) {
            throw "Package $kind is not the one of the current sources."
        }
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
        # The title its folder on the SD card is named for, as the builder wrote it.
        $title = (Get-Content -LiteralPath (Join-Path $directory 'manifest.json') -Raw | ConvertFrom-Json).title_id.ToUpperInvariant()
        return [pscustomobject]@{ Name = $id; Directory = $directory; Hashes = $files; Title = $title }
    }
    # A package for Bank says the release's version on its error and result
    # screens. The migration puts the version of the Bank it converted in front.
    function Invoke-Build([hashtable]$arguments) {
        $arguments.Release = $true
        & (Join-Path $PSScriptRoot 'Build-Patches.ps1') @arguments | Out-Host
    }
    Invoke-Build @{}
    if ($Migrate) { Invoke-Build @{ Kind = 'migration' } }
    if ($Download) { Invoke-Build @{ Kind = 'download' } }
    # The verify scripts still refuse what they must.
    & (Join-Path $PSScriptRoot 'Test-Verify.ps1') | Out-Host
    $bank = Get-Verified 'bank'
    $transporter = Get-Verified 'transporter'
    $titles = [ordered]@{
        $bank.Title        = [pscustomobject]@{ Label = 'Pokemon Bank 1.5'; Package = $bank }
        $transporter.Title = [pscustomobject]@{ Label = 'Poke Transporter 1.5'; Package = $transporter }
    }
    $migration = if ($Migrate) { Get-Verified 'migration' }
    $downloaded = if ($Download) { Get-Verified 'download' }

    # 4. The zips, each laid out for the root of the SD card.
    $stage = Join-Path $releaseRoot $name
    $migrationStage = if ($Migrate) { Join-Path $releaseRoot $migrationName } else { '' }
    $downloadStage = if ($Download) { Join-Path $releaseRoot $downloadName } else { '' }
    foreach ($path in @($stage, $zip, $notesPath, $migrationStage, $migrationZip, $downloadStage, $downloadZip) | Where-Object { $_ }) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse }
    }
    New-Item -ItemType Directory -Force -Path $releaseRoot | Out-Null
    $date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
    $source = $commit
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

    # The guides, written once and shown twice: as Markdown in the release
    # notes and as plain text in the README.txt of each zip. A block is a
    # paragraph (P), lines without a blank between them (Lines), or a step
    # (Step) with a picture or a table. Plain gives other words for the zip
    # where the page links within itself; a table row's fourth cell does so
    # for its third.
    # GitHub's anchor of a heading.
    function Get-Anchor([string]$heading) {
        return '#' + (($heading.ToLowerInvariant() -replace '[^\p{L}\p{N} _-]', '') -replace ' ', '-')
    }
    function Get-ReleaseLink([string]$tag) { return "[$tag release]($repository/releases/tag/$tag)" }
    # The released versions from $from up to before $to, as "a, b or c".
    function Get-Versions([string]$from, [string]$to) {
        $tags = @(Invoke-Git tag -l 'v*' | Where-Object {
                $_ -match '^v\d+\.\d+\.\d+$' -and
                (-not $from -or (ConvertTo-Version $_) -ge (ConvertTo-Version $from)) -and
                (ConvertTo-Version $_) -lt (ConvertTo-Version $to)
            } | Sort-Object { ConvertTo-Version $_ })
        if ($tags.Count -le 1) { return [string]$tags }
        return ($tags[0..($tags.Count - 2)] -join ', ') + ' or ' + $tags[-1]
    }
    # Markdown as the README.txt says it: without pictures, links, marks and accents.
    function ConvertTo-PlainText([string]$text) {
        $text = $text -replace '!\[[^\]]*\]\([^)]*\)', '' -replace '\[([^\]]*)\]\([^)]*\)', '$1'
        return $text.Replace('**', '').Replace('`', '').Replace('é', 'e')
    }
    # Wraps at 76 columns, the first line after $first and the others after $rest.
    function Format-Wrapped([string]$text, [string]$first, [string]$rest) {
        $lines = [Collections.Generic.List[string]]::new()
        $line = $first
        $start = $true
        foreach ($word in $text -split ' ') {
            if ($start) { $line += $word; $start = $false }
            elseif ($line.Length + 1 + $word.Length -gt 76) { $lines.Add($line); $line = $rest + $word }
            else { $line += " $word" }
        }
        $lines.Add($line)
        return $lines
    }
    function Get-Plain($block, [string]$text) {
        if ($block['Plain']) { return $block['Plain'] }
        return ConvertTo-PlainText $text
    }
    function ConvertTo-MarkdownTable($table, [string]$indent) {
        $out = [Collections.Generic.List[string]]::new()
        $out.Add("$indent| $($table.Head -join ' | ') |")
        $out.Add("$indent|$(' --- |' * $table.Head.Count)")
        foreach ($row in $table.Rows) { $out.Add("$indent| $($row[0..($table.Head.Count - 1)] -join ' | ') |") }
        return $out
    }
    # The first cell as a column as wide as its longest entry up to 24
    # characters; a longer one stands on lines of its own.
    function ConvertTo-TextTable($table, [string]$indent) {
        $out = [Collections.Generic.List[string]]::new()
        $width = ($table.Rows | ForEach-Object { (ConvertTo-PlainText $_[0]).Length } | Where-Object { $_ -le 24 } | Measure-Object -Maximum).Maximum
        $rest = $indent + (' ' * ($width + 2))
        foreach ($row in $table.Rows) {
            $key = ConvertTo-PlainText $row[0]
            $what = if ($row.Count -gt 3) { $row[3] } else { ConvertTo-PlainText $row[2] }
            $text = "$(ConvertTo-PlainText $row[1]) $what"
            if ($key.Length -le $width) {
                $out.AddRange([string[]](Format-Wrapped $text ($indent + $key.PadRight($width + 2)) $rest))
            } else {
                $out.AddRange([string[]](Format-Wrapped $key $indent $indent))
                $out.AddRange([string[]](Format-Wrapped $text $rest $rest))
            }
        }
        return $out
    }
    # Steps follow each other without a blank line unless one has a picture or a table.
    function ConvertTo-Markdown($blocks) {
        $out = [Collections.Generic.List[string]]::new()
        $step = 0
        $tight = $false
        foreach ($block in $blocks) {
            $isStep = $block.Contains('Step')
            if ($out.Count -and -not ($tight -and $isStep)) { $out.Add('') }
            $tight = $false
            if ($isStep) {
                $step++
                $out.Add("$step. $($block.Step)")
                if ($block['Picture']) { $out.Add(''); $out.Add("   $($block['Picture'])") }
                elseif ($block['Table']) { $out.Add(''); $out.AddRange([string[]](ConvertTo-MarkdownTable $block['Table'] '   ')) }
                else { $tight = $true }
            } elseif ($block.Contains('Lines')) {
                $out.AddRange([string[]]$block.Lines)
            } else {
                $out.Add($block.P)
            }
        }
        return $out
    }
    function ConvertTo-Text($blocks) {
        $out = [Collections.Generic.List[string]]::new()
        $step = 0
        $inSteps = $false
        foreach ($block in $blocks) {
            $isStep = $block.Contains('Step')
            if ($out.Count -and -not ($inSteps -and $isStep)) { $out.Add('') }
            $inSteps = $isStep
            if ($isStep) {
                $step++
                $out.AddRange([string[]](Format-Wrapped (Get-Plain $block $block.Step) "$step. " '   '))
                if ($block['Table']) { $out.AddRange([string[]](ConvertTo-TextTable $block['Table'] '     ')) }
            } elseif ($block.Contains('Lines')) {
                foreach ($line in $block.Lines) { $out.AddRange([string[]](Format-Wrapped (ConvertTo-PlainText $line) '' '')) }
            } else {
                $out.AddRange([string[]](Format-Wrapped (Get-Plain $block $block.P) '' ''))
            }
        }
        return $out
    }

    $installing = Get-Anchor "Installing $Version"
    $comingHeading = 'Coming from a previous version'
    $coming = Get-Anchor $comingHeading
    $downloading = Get-Anchor 'Pokémon still on the official server'
    $install = "Install ${Version}: extract $name.zip from this release and copy its luma folder to the root of the SD card."
    # Each earlier way of storing is known by the last release of it.
    $lastOfEach = @($bankFormats | ForEach-Object { $_.From })
    $earlier = if ($lastOfEach.Count) { "$($lastOfEach[-1]) or before" } else { '' }
    # Without the migration, a Bank stored an earlier way goes through the
    # last release that shipped it.
    $migratedBy = if ($Migrate) { '' } else {
        Get-ReleaseLink (@(Invoke-Git tag -l 'v0.*' | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' } | Sort-Object { ConvertTo-Version $_ })[-1])
    }

    # Whose Bank goes which way: what they have, what they do, and what the zip says instead.
    $routes = [Collections.Generic.List[object]]::new()
    $routes.Add(@('Never used the offline patch', "[Install $Version]($installing)"))
    foreach ($format in $bankFormats) {
        $have = "An offline Bank from $($format.From) or before"
        $routes.Add(@($have, "[Convert it first]($coming), then install $Version", "convert it first with $migrationName.zip from this release, then install $Version"))
    }
    if (-not $Migrate) {
        $routes.Add(@('An offline Bank of an earlier release', "Convert it first with the migration of the $migratedBy, then install $Version"))
    }
    $routes.Add(@('An offline Bank from every other version', "[Install $Version]($installing)"))
    if ($Download) {
        $routes.Add(@('Pokémon on the official server and no offline Bank', "[Download them first]($downloading), then install $Version", "download them first with $downloadName.zip from this release, then install $Version"))
    }

    # What Bank says when the Bank on the SD card is not continued.
    $errors = @{
        Head = @('First number', 'Meaning', 'What to do')
        Rows = $(if ($Migrate) {
                @($bankFormats | ForEach-Object {
                        , @("``00000003`` (with ``$($_.Size)``)", "The Bank is from $($_.From) or before and not converted yet.", "[Convert it first]($coming).", "Convert it first with $migrationName.zip.")
                    })
            } else {
                @(, @('`00000003`', 'The Bank is from an earlier release and not converted yet.', "Convert it first with the migration of the $migratedBy."))
            })
    }

    # The migration from earlier releases. This goes when patches/bank-migrate goes.
    if ($Migrate) {
        # Who has to settle a Bank first puts back the last release of its way.
        $settleWith = 'Put back ' + (@($lastOfEach | ForEach-Object { "the $(Get-ReleaseLink $_) for a Bank from $_ or before" }) -join ', ')
        $migrationGuide = @(
            @{ P = "``$migrationName.zip`` converts a Bank that was last used with $earlier, once, and does nothing else. After converting, that release cannot open the Bank any more; the backup of step 1 is the only way back." }
            @{ Step = 'Back up Bank''s extra data with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint); for Bank, press X there to switch to extdata.' }
            @{ Step = "Extract ``$migrationName.zip`` and copy its ``luma`` folder to the root of the SD card." }
            @{ Step = "Start Pokémon Bank. The start screen says `"Ver. 1.5$($migrationFacts.package)`"." }
            @{
                Step    = "Press START. After preparation three lines appear: `"Press HOME to return to the HOME Menu`", the version your Bank is from with this one, as ``$($lastOfEach[-1])-$($Version.TrimStart('v'))``, and two numbers. The Bank itself does not open."
                Picture = (@($lastOfEach | ForEach-Object { "![The console showing a Bank of $_ converted, with $($migrationFacts.converted)]($repository/blob/main/pictures/$_-migration-converted.png)" }) -join ' ')
            }
            @{
                Step  = 'Look up the numbers:'
                Table = @{
                    Head = @('You see', 'Meaning', 'What to do')
                    Rows = @(
                        , @((Format-Numbers $migrationFacts.converted), 'Converted.', 'Go on with step 6.')
                        , @((Format-Numbers $migrationFacts.already), 'Already converted.', 'Go on with step 6.')
                        , @((Format-Numbers $migrationFacts.none), 'No Bank was found.', "Go on with step 6; $Version creates one.")
                        , @((Format-Numbers $migrationFacts['in-progress']), 'A Save and Quit was interrupted.', "$settleWith; start Bank once, then begin again at step 2.")
                        , @((Format-Numbers $migrationFacts.unsettled), 'The Bank needs a Save and Quit first.', "$settleWith; open Bank with a game, Save and Quit, then begin again at step 2.")
                        , @("$((Format-Numbers ($migrationFacts.failed -split ' ')[0])) to $((Format-Numbers ($migrationFacts.failed -split ' ')[1]))", 'A step failed.', 'Start Bank again with the migration files; that is always safe. If it keeps happening, restore your backup and report the version line and both numbers.')
                    )
                }
            }
            @{ Step = "Install $Version as described [below]($installing)."; Plain = $install }
            @{
                Step    = 'Start Pokémon Bank. Your boxes, Pokédex and Poké Miles are as before.'
                Picture = "![The boxes after the update]($repository/blob/main/pictures/offline-boxes.png)"
            }
            @{ P = 'The conversion needs about 1.5 MB of free space on the SD card. If the power fails while it runs, start it again: the old Bank stays untouched until the new files are complete.' }
        )
    }

    # The download from the official server. This goes when patches/bank-download goes.
    if ($Download) {
        $downloadGuide = @(
            @{ P = "``$downloadName.zip`` fetches the Bank you have on the official Pokémon Bank server into a new offline Bank, once, and does nothing else." }
            @{
                Lines = @(
                    "**Only until $serviceEnds.** That day the official service ends, and this patch cannot do anything any more."
                    '**An offline Bank you already have is never replaced or merged.** Nothing is contacted then.'
                    '**Your Pokémon stay on the server as well.** This makes a copy. Poké Miles come along as the server has them.'
                    '**You need an internet connection and your Nintendo Network ID on this console.** No game is needed: none is chosen, read or saved.'
                )
            }
            @{ Step = 'Back up Bank''s extra data and your game saves with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint); for Bank, press X there to switch to extdata.' }
            @{ Step = "Extract ``$downloadName.zip`` and copy its ``luma`` folder to the root of the SD card." }
            @{ Step = "Start Pokémon Bank. The start screen says `"Ver. 1.5$($downloadFacts.package)`"." }
            @{ Step = "Press START. It connects as the original does; there is no menu and nothing to choose. After the loading screens three lines appear: `"Press HOME to return to the HOME Menu`", ``$Version$($downloadFacts.package)``, and two numbers. The Bank itself does not open." }
            @{
                Step  = 'Look up the numbers:'
                Table = @{
                    Head = @('You see', 'Meaning', 'What to do')
                    Rows = @(
                        , @((Format-Numbers $downloadFacts.downloaded), 'Downloaded.', 'Go on with step 6.')
                        , @((Format-Numbers $downloadFacts.present), 'An offline Bank is already on the SD card. Nothing was contacted or changed.', 'Go on with step 6.')
                        , @((Format-Numbers $downloadFacts['no-bank']), 'The server holds no Bank for this console. Nothing was created or changed.', "Go on with step 6; $Version creates a Bank.")
                        , @('Other numbers', 'Writing failed.', 'Start Bank again with the download files; that is always safe. If it keeps happening, report the version line and both numbers.')
                        , @('A message of the original, then its start screen', 'No connection, maintenance, or the server is locked for a while. Nothing was written.', 'Try again later.')
                    )
                }
            }
            @{ Step = "Install $Version as described [below]($installing)."; Plain = $install }
            @{ Step = 'Start Pokémon Bank. Your boxes are there.' }
        )
    }

    # Each zip with its README.txt: the facts to trace it, and its guide.
    function Get-Readme([string]$heading) {
        $readme = [Collections.Generic.List[string]]::new()
        $readme.Add($heading)
        $readme.Add("Built $date from commit $source")
        $readme.Add('')
        $readme.AddRange([string[]]$disclaimer)
        $readme.Add('')
        return , $readme
    }
    $readme = Get-Readme "Pokemon Bank and Transporter Offline Patch $Version"
    $readme.Add('Copy the luma folder to the root of the SD card (merge it with the')
    $readme.Add('existing one) and switch on "Enable game patching" in the Luma3DS')
    $readme.Add('configuration (hold SELECT while powering on).')
    $readme.Add('')
    $readme.Add('The two files of a title only work as a pair. Back up your game saves')
    $readme.Add('first, and the extra data of Pokemon Bank before every update. Start')
    $readme.Add('Bank once before using Transporter. Always update Bank and Transporter')
    $readme.Add('together.')
    $readme.Add('')
    $readme.Add('Which part is for you:')
    foreach ($route in $routes) {
        $do = if ($route.Count -gt 2) { $route[2] } else { ConvertTo-PlainText $route[1] }
        $do = $do.Substring(0, 1).ToLowerInvariant() + $do.Substring(1)
        $readme.AddRange([string[]](Format-Wrapped "$(ConvertTo-PlainText $route[0]): $do." '- ' '  '))
    }
    $readme.Add('')
    $readme.Add('If Bank then shows an error with one of these as its first number:')
    $readme.AddRange([string[]](ConvertTo-TextTable $errors '  '))
    $readme.Add('')
    foreach ($title in $titles.Keys) {
        Add-Files $stage $title $titles[$title].Package $titles[$title].Label "$name.zip" $readme
    }
    [IO.File]::WriteAllLines((Join-Path $stage 'README.txt'), $readme)
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip)
    Remove-Item -LiteralPath $stage -Recurse

    if ($Migrate) {
        $steps = Get-Readme "Pokemon Bank Offline Patch: migration from $earlier to $Version"
        $steps.AddRange([string[]](ConvertTo-Text $migrationGuide))
        $steps.Add('')
        Add-Files $migrationStage $migration.Title $migration "Pokemon Bank 1.5, migration from $earlier" "$migrationName.zip" $steps
        [IO.File]::WriteAllLines((Join-Path $migrationStage 'README.txt'), $steps)
        [IO.Compression.ZipFile]::CreateFromDirectory($migrationStage, $migrationZip)
        Remove-Item -LiteralPath $migrationStage -Recurse
    }
    if ($Download) {
        $steps = Get-Readme "Pokemon Bank Offline Patch: download from the official server, for $Version"
        $steps.AddRange([string[]](ConvertTo-Text $downloadGuide))
        $steps.Add('')
        Add-Files $downloadStage $downloaded.Title $downloaded 'Pokemon Bank 1.5, download from the official server' "$downloadName.zip" $steps
        [IO.File]::WriteAllLines((Join-Path $downloadStage 'README.txt'), $steps)
        [IO.Compression.ZipFile]::CreateFromDirectory($downloadStage, $downloadZip)
        Remove-Item -LiteralPath $downloadStage -Recurse
    }

    # 5. Release notes: the same guides, and everything needed to trace this version.
    $notes = [Collections.Generic.List[string]]::new()
    $notes.Add("Pokémon Bank and Transporter Offline Patch $Version")
    $notes.Add('')
    $notes.Add('## Which part is for you')
    $notes.Add('')
    $notes.AddRange([string[]](ConvertTo-MarkdownTable @{ Head = @('You have', 'Do'); Rows = $routes } ''))
    if ($Migrate) {
        $notes.Add('')
        $notes.Add("## $comingHeading")
        $notes.Add('')
        $notes.AddRange([string[]](ConvertTo-Markdown $migrationGuide))
    }
    if ($Download) {
        $notes.Add('')
        $notes.Add('## Pokémon still on the official server')
        $notes.Add('')
        $notes.AddRange([string[]](ConvertTo-Markdown $downloadGuide))
    }
    $notes.Add('')
    $notes.Add("## Installing $Version")
    $notes.Add('')
    $notes.Add("Extract ``$name.zip`` and copy its ``luma`` folder to the root of the SD card. See the README of the repository for installation and the guides for each patch.")
    $notes.Add('')
    $notes.Add('If Bank then shows an error with one of these as its first number:')
    $notes.Add('')
    $notes.AddRange([string[]](ConvertTo-MarkdownTable $errors ''))
    $notes.AddRange([string[]]@(
            ''
            '<details>'
            '<summary>Files and hashes</summary>'
            ''
            '| | |'
        ))
    $notes.Add('| --- | --- |')
    $notes.Add("| Built | $date |")
    $notes.Add("| Commit | ``$source`` |")
    $notes.Add("| Bank package | ``$($bank.Name)`` |")
    $notes.Add("| Transporter package | ``$($transporter.Name)`` |")
    $notes.Add("| ``$name.zip`` | ``$(Get-Sha256 $zip)`` |")
    if ($Migrate) {
        $notes.Add("| Migration package | ``$($migration.Name)`` |")
        $notes.Add("| ``$migrationName.zip`` | ``$(Get-Sha256 $migrationZip)`` |")
    }
    if ($Download) {
        $notes.Add("| Download package | ``$($downloaded.Name)`` |")
        $notes.Add("| ``$downloadName.zip`` | ``$(Get-Sha256 $downloadZip)`` |")
    }
    $notes.Add('')
    $notes.Add('| In | File | SHA-256 |')
    $notes.Add('| --- | --- | --- |')
    $notes.AddRange($table)
    $notes.Add('')
    $notes.Add('</details>')
    [IO.File]::WriteAllLines($notesPath, $notes)

    # 6. The tag, on the commit the zips were built from.
    if ((Invoke-Git rev-parse HEAD) -ne $commit -or (Invoke-Git status --porcelain --untracked-files=no)) {
        throw 'The repository changed while the release was built; nothing was tagged.'
    }
    Invoke-Checked "tag $Version" { git -C $projectRoot tag -a $Version -F $notesPath --cleanup=verbatim $commit }
} finally { Pop-Location }

Write-Host ''
Write-Host "Archive: $zip"
if ($Migrate) { Write-Host "Archive: $migrationZip" }
if ($Download) { Write-Host "Archive: $downloadZip" }
Write-Host "Notes:   $notesPath"
Write-Host "Tag:     $Version on $commit (local; nothing was pushed)"
Write-Host ''
Write-Host 'Next: test the zips on a console. Then either'
Write-Host "  ./scripts/New-Release.ps1 -Publish"
Write-Host "or push the tag (git push origin $Version) and upload the zips and the notes by hand."
Write-Host "To withdraw before publishing: git tag -d $Version, and delete build/release/$Version."

