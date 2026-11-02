param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$project = Split-Path -Parent $PSScriptRoot
Push-Location $project
try {
    if ($Release) {
        cargo build --release --bins
        $rv = Join-Path $project "target\release\rv.exe"
    } else {
        cargo build --bins
        $rv = Join-Path $project "target\debug\rv.exe"
    }

    $root = Join-Path $env:TEMP "rivet-demo"
    if (Test-Path $root) {
        Remove-Item $root -Recurse -Force
    }
    $env:RIVET_ROOT = $root
    $env:RIVET_MAIN_INDEX = (Resolve-Path (Join-Path $project "repo\main\index.json")).Path

    & $rv update
    & $rv search hello
    & $rv install hello
    & $rv list
    & (Join-Path $root "bin\hello.cmd") demo
}
finally {
    Pop-Location
}
