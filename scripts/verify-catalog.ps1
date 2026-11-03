[CmdletBinding()]
param(
    [string]$Root = "repo/main",
    [string]$Rv = "",
    [string]$Repo = "",
    [int]$TimeoutSeconds = 30,
    [switch]$SkipSmoke,
    [switch]$Keep
)

$ErrorActionPreference = "Stop"
$project = Split-Path $PSScriptRoot -Parent
if (-not $Rv) { $Rv = Join-Path $project "target\release\rv.exe" }
if (-not $Repo) { $Repo = Join-Path $project "target\release\rv-repo.exe" }
foreach ($exe in @($Rv, $Repo)) {
    if (-not (Test-Path $exe)) {
        throw "missing $exe; run: cargo build --release --bin rv --bin rv-repo"
    }
}

$Root = (Resolve-Path $Root).Path
$temp = Join-Path ([IO.Path]::GetTempPath()) ("rivet-verify-" + [guid]::NewGuid().ToString("N"))
$catalog = Join-Path $temp "catalog"
$rivetRoot = Join-Path $temp "root"
New-Item -ItemType Directory -Force -Path $catalog, $rivetRoot | Out-Null
& $Repo build $Root --output (Join-Path $catalog "index.json")
if ($LASTEXITCODE -ne 0) { throw "rv-repo build failed" }
if (Test-Path (Join-Path $Root "artifacts")) {
    Copy-Item (Join-Path $Root "artifacts") (Join-Path $catalog "artifacts") -Recurse
}

$oldRoot = $env:RIVET_ROOT
$oldIndex = $env:RIVET_MAIN_INDEX
$env:RIVET_ROOT = $rivetRoot
$env:RIVET_MAIN_INDEX = Join-Path $catalog "index.json"

try {
    & $Rv update
    if ($LASTEXITCODE -ne 0) { throw "rivet update failed" }

    $index = Get-Content $env:RIVET_MAIN_INDEX -Raw | ConvertFrom-Json
    $packages = @($index.packages)
    $names = @($packages | ForEach-Object name | Sort-Object -Unique)
    $failed = @()
    foreach ($name in $names) {
        $ok = $false
        for ($attempt = 1; $attempt -le 3; $attempt++) {
            & $Rv install $name
            if ($LASTEXITCODE -eq 0) { $ok = $true; break }
            Start-Sleep -Seconds 5
        }
        if (-not $ok) { $failed += $name }
    }
    if ($failed.Count -gt 0) {
        throw "install failed for: $($failed -join ', ')"
    }

    & $Rv verify
    if ($LASTEXITCODE -ne 0) { throw "rivet verify failed" }

    if (-not $SkipSmoke) {
        $commands = @{}
        foreach ($package in $packages) {
            foreach ($bin in $package.bin) {
                $command = if ($bin -is [string]) {
                    [IO.Path]::GetFileNameWithoutExtension($bin)
                } elseif ($bin.name) {
                    $bin.name
                } else {
                    [IO.Path]::GetFileNameWithoutExtension($bin.path)
                }
                $commands[$command] = $true
            }
        }
        $versionArgs = @{
            "air" = @("-v")
            "cargo-binstall" = @("-V")
            "dasel" = @("version")
            "restic" = @("version")
        }
        $smokeFailed = @()
        foreach ($command in ($commands.Keys | Sort-Object)) {
            if ($command -eq "hello") { continue }
            $exe = Join-Path $rivetRoot "bin\$command.exe"
            if (-not (Test-Path $exe)) { $smokeFailed += "$command missing"; continue }
            $psi = [Diagnostics.ProcessStartInfo]::new()
            $psi.FileName = $exe
            $versionArguments = if ($versionArgs.ContainsKey($command)) { $versionArgs[$command] } else { @("--version") }
            foreach ($argument in $versionArguments) { $psi.ArgumentList.Add($argument) }
            $psi.UseShellExecute = $false
            $psi.RedirectStandardOutput = $true
            $psi.RedirectStandardError = $true
            $psi.CreateNoWindow = $true
            $process = [Diagnostics.Process]::Start($psi)
            if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
                $process.Kill()
                $smokeFailed += "$command timeout"
                continue
            }
            $output = ($process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()).Trim()
            if ($process.ExitCode -ne 0) {
                $smokeFailed += "$command exit $($process.ExitCode)"
            } else {
                $firstLine = ($output -split "\r?\n" | Select-Object -First 1)
                Write-Host ("ok  {0,-16} {1}" -f $command, $firstLine)
            }
        }
        if ($smokeFailed.Count -gt 0) {
            $smokeFailed | ForEach-Object { Write-Host "FAIL $_" }
            throw "smoke test failed for $($smokeFailed.Count) command(s)"
        }
    }

    Write-Host "catalog verify: ok ($($packages.Count) versions, $($names.Count) packages)"
} finally {
    $env:RIVET_ROOT = $oldRoot
    $env:RIVET_MAIN_INDEX = $oldIndex
    if (-not $Keep) {
        Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "kept: $temp"
    }
}
