[CmdletBinding()]
param(
    [string]$Version = "",
    [switch]$NoPath,
    [switch]$NoUpdate
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
}

$repo = "Kutius/rivet"

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "Rivet requires 64-bit Windows."
}

if (-not $Version) {
    $releases = Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$repo/releases?per_page=20"
    $release = $releases | Where-Object { -not $_.draft } | Select-Object -First 1
    if (-not $release) {
        throw "No Rivet release was found. Pass -Version explicitly."
    }
    $Version = $release.tag_name
}
$Version = $Version.TrimStart("v")

$archive = "rivet-v$Version-windows-x64.zip"
$base = "https://github.com/$repo/releases/download/v$Version"
$temp = Join-Path ([IO.Path]::GetTempPath()) ("rivet-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $temp | Out-Null

try {
    $sums = Join-Path $temp "SHA256SUMS"
    $zip = Join-Path $temp $archive
    Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile $sums
    Invoke-WebRequest -UseBasicParsing "$base/$archive" -OutFile $zip

    $line = Get-Content $sums | Where-Object { $_ -match [regex]::Escape($archive) } | Select-Object -First 1
    if (-not $line) {
        throw "SHA256SUMS does not list $archive."
    }
    $expected = ($line -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "SHA256 mismatch for $archive. Expected $expected, got $actual."
    }

    $extract = Join-Path $temp "x"
    Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force
    $rv = Join-Path $extract "rv.exe"
    if (-not (Test-Path $rv)) {
        throw "The release archive does not contain rv.exe."
    }

    $initArgs = @("init")
    if ($NoPath) {
        $initArgs += "--no-path"
    }
    & $rv @initArgs
    if ($LASTEXITCODE -ne 0) {
        throw "rv init failed with exit code $LASTEXITCODE."
    }

    $root = if ($env:RIVET_ROOT) { $env:RIVET_ROOT } else { Join-Path $env:LOCALAPPDATA "Rivet" }
    $installed = Join-Path $root "bin\rv.exe"
    if (-not (Test-Path $installed)) {
        throw "rv init did not create $installed."
    }
    if (-not (Test-Path (Join-Path $root "bin\rivet.exe"))) {
        throw "rv init did not create rivet.exe."
    }

    if (-not $NoUpdate) {
        & $installed update
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "The catalog update failed. Run 'rivet update' after fixing the network."
        }
    }

    & $installed --version
    Write-Host ""
    Write-Host "Rivet is installed."
    if ($NoPath) {
        Write-Host "Add $root\bin to PATH, or run $installed directly."
    } else {
        Write-Host "Open a new terminal and run: rivet --version"
    }
    Write-Host "Then try: rivet search ripgrep"
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
