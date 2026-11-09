# Copies this repository's committed files into the release repository
# (decisions D79): the same code, without the internal documents. Run it from
# this repository:  .\dist\export.ps1 -To <release repository folder>
#
# It refuses to run with uncommitted changes here (the export must match a
# commit) or there (it would overwrite them). Files the release repository
# tracks that are no longer here, or are excluded, are deleted there. It
# doesn't commit, tag or push anything itself.
param(
    [Parameter(Mandatory = $true)] [string] $To
)

# Not $ErrorActionPreference = 'Stop' (see release.ps1): native commands are
# checked by exit code; file commands use -ErrorAction Stop.
Set-Location (Split-Path $PSScriptRoot -Parent) -ErrorAction Stop

# Development only: never in the release repository. Add new ones here.
$excluded = @(
    'docs/PRD.md',
    'docs/decisions.md',
    'docs/format-notes.md',
    'docs/progress.md',
    'crates/h5tags-core/tests/real_builds_survey.rs',
    'crates/h5tags-core/tests/real_cache_survey.rs',
    'crates/h5tags-core/tests/real_partial_read_survey.rs',
    'crates/h5tags-core/tests/real_references_survey.rs',
    'dist/export.ps1'
)

if (git status --porcelain) {
    throw "There are uncommitted changes here: commit them first, so the export matches a commit."
}
$to = (Resolve-Path $To -ErrorAction Stop).Path
if (-not (Test-Path (Join-Path $to '.git'))) {
    throw "$to isn't a git repository."
}
if (git -C $to status --porcelain) {
    throw "There are uncommitted changes in $to; commit or discard them first."
}

$files = git ls-files
if ($LASTEXITCODE -ne 0) { throw "git ls-files failed." }
$kept = $files | Where-Object { $excluded -notcontains $_ }

foreach ($f in $kept) {
    $target = Join-Path $to $f
    $dir = Split-Path $target -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir -ErrorAction Stop | Out-Null }
    Copy-Item -LiteralPath $f -Destination $target -Force -ErrorAction Stop
}

# Tracked there, but not exported: gone from here, or excluded.
$theirs = git -C $to ls-files
if ($LASTEXITCODE -ne 0) { throw "git ls-files failed in $to." }
$removed = @($theirs | Where-Object { $kept -notcontains $_ })
foreach ($f in $removed) {
    Remove-Item -LiteralPath (Join-Path $to $f) -Force -ErrorAction Stop
}

$commit = git rev-parse --short HEAD
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Write-Host "Exported $($kept.Count) files of commit $commit (version $version) to $to"
Write-Host "Left out: $($excluded -join ', ')"
if ($removed.Count -gt 0) { Write-Host "Deleted there: $($removed -join ', ')" }
Write-Host ""
Write-Host "Next, in $to :"
Write-Host "  1. git status, and look at what changed"
Write-Host "  2. git add -A; git commit -m `"Ausar Tag Tools $version`""
Write-Host "  3. git tag v$version; git push; git push origin v$version"
Write-Host "  4. .\dist\release.ps1 (builds the zip from that commit)"
