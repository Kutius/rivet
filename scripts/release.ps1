param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root "dist"
$release = Join-Path $root "target\release"
$manifest = Join-Path $root "Cargo.toml"

$versionLine = Select-String -Path $manifest -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionLine) {
    throw "could not read version from $manifest"
}
$version = $versionLine.Matches[0].Groups[1].Value
$archiveName = "rivet-v$version-windows-x64.zip"

if (-not $SkipBuild) {
    & cargo build --offline --release --bins
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }
}

foreach ($binary in @("rv.exe", "rv-shim.exe")) {
    $path = Join-Path $release $binary
    if (-not (Test-Path $path)) {
        throw "missing release binary: $path"
    }
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
Get-ChildItem -Path $dist -Filter "rivet-v*-windows-x64.zip" -File -ErrorAction SilentlyContinue | Remove-Item -Force
Remove-Item (Join-Path $dist "SHA256SUMS") -Force -ErrorAction SilentlyContinue

$stage = Join-Path $dist ".stage"
if (Test-Path $stage) {
    Remove-Item $stage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item (Join-Path $release "rv.exe") $stage
Copy-Item (Join-Path $release "rv-shim.exe") $stage
Copy-Item (Join-Path $root "README.md") $stage
Copy-Item (Join-Path $root "LICENSE") $stage
foreach ($doc in @("CHANGELOG.md", "SECURITY.md")) {
    $source = Join-Path $root $doc
    if (Test-Path $source) {
        Copy-Item $source $stage
    }
}
$docs = Join-Path $root "docs"
if (Test-Path $docs) {
    Copy-Item $docs (Join-Path $stage "docs") -Recurse
}

$stageChecksums = foreach ($file in (Get-ChildItem $stage -File | Sort-Object Name)) {
    $fileHash = (Get-FileHash $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$fileHash  $($file.Name)"
}
$stageChecksums | Set-Content -Path (Join-Path $stage "SHA256SUMS") -Encoding ascii

$archive = Join-Path $dist $archiveName
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $archive -CompressionLevel Optimal
Remove-Item $stage -Recurse -Force

Copy-Item (Join-Path $release "rv.exe") (Join-Path $dist "rv.exe") -Force
Copy-Item (Join-Path $release "rv-shim.exe") (Join-Path $dist "rv-shim.exe") -Force

$checksumLines = foreach ($file in @("rv.exe", "rv-shim.exe", $archiveName)) {
    $path = Join-Path $dist $file
    $fileHash = (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$fileHash  $file"
}
$checksumLines | Set-Content -Path (Join-Path $dist "SHA256SUMS") -Encoding ascii

$hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output "built $archive"
Write-Output "sha256 $hash"
