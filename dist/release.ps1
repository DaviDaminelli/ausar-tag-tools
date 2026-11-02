# Builds the Windows release zip on this PC (decisions D65): one build per
# version, published once and never replaced, so every tester downloads the
# same file. Run it from the repository folder:  .\dist\release.ps1
#
# It refuses to run with uncommitted changes, or when the current commit
# isn't tagged v<version> (the version in Cargo.toml), so the zip always
# matches a commit. It doesn't push or publish anything itself.

# Not $ErrorActionPreference = 'Stop': Windows PowerShell 5.1 turns cargo's
# progress lines on stderr into errors when output is redirected. Native
# commands are checked by exit code; file commands use -ErrorAction Stop.
Set-Location (Split-Path $PSScriptRoot -Parent) -ErrorAction Stop

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$tag = "v$version"
Write-Host "Version $version (tag $tag)"

if (git status --porcelain) {
    throw "There are uncommitted changes: commit them first, so the zip matches a commit."
}
$tagged = git tag --points-at HEAD
if ($tagged -notcontains $tag) {
    throw "The current commit isn't tagged $tag. Tag it first:  git tag $tag"
}

cargo test --workspace --locked
if ($LASTEXITCODE -ne 0) { throw "Tests failed." }
cargo build --release --locked --bin ausar-tag-cli --bin ausar-tag-mcp
if ($LASTEXITCODE -ne 0) { throw "The build failed." }

$name = "ausar-tag-tools-$version-windows"
$out = Join-Path target 'dist'
$folder = Join-Path $out $name
$zip = Join-Path $out "$name.zip"
if (Test-Path $folder) { Remove-Item $folder -Recurse -Force -ErrorAction Stop }
if (Test-Path $zip) { Remove-Item $zip -Force -ErrorAction Stop }
New-Item -ItemType Directory -Force $folder -ErrorAction Stop | Out-Null
Copy-Item target\release\ausar-tag-cli.exe, target\release\ausar-tag-mcp.exe, README.md, docs\SETUP.md, dist\h5tags.example.toml $folder -ErrorAction Stop
Compress-Archive -Path $folder -DestinationPath $zip -ErrorAction Stop

$pre = if ($version.Contains('-')) { ' (mark it as a pre-release)' } else { '' }
# GitHub shows each attached file's SHA-256 on the release page too; this one
# goes in the notes, where testers read it (check: Get-FileHash <zip>).
$sha256 = (Get-FileHash $zip -Algorithm SHA256 -ErrorAction Stop).Hash.ToLower()
Write-Host ""
Write-Host "Built $zip"
Write-Host "SHA-256 $sha256"
Write-Host ""
Write-Host "For the release notes:  $name.zip  SHA-256: $sha256"
Write-Host ""
Write-Host "To publish:"
Write-Host "  1. git push origin $tag"
Write-Host "  2. On GitHub: Releases -> Draft a new release -> choose $tag$pre, title 'Ausar Tag Tools $version',"
Write-Host "     attach $name.zip, publish."
Write-Host "     Or, with the GitHub CLI: gh release create $tag `"$zip`" --title `"Ausar Tag Tools $version`"$(if ($pre) { ' --prerelease' })"
Write-Host "  3. Optional, a Linux zip: Actions -> Linux zip -> Run workflow, with $tag."
