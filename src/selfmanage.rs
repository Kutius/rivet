use crate::archive;
use crate::config::Paths;
use crate::download;
use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn update(paths: &Paths, force: bool) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let body = download::get_text("https://api.github.com/repos/Kutius/rivet/releases?per_page=20")
        .context("querying GitHub releases")?;
    let releases: Vec<Release> =
        serde_json::from_str(&body).context("parsing GitHub release metadata")?;
    let mut candidates: Vec<(Version, Release)> = releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            Version::parse(release.tag_name.trim_start_matches('v'))
                .ok()
                .map(|version| (version, release))
        })
        .collect();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    let (latest, release) = candidates
        .into_iter()
        .next()
        .context("no published Rivet release found")?;

    if latest < current {
        println!(
            "rv {current} is newer than the latest published release ({latest}); nothing to do"
        );
        return Ok(());
    }
    if !force && latest == current {
        println!("rv {current} is already up to date");
        return Ok(());
    }

    let version = latest.to_string();
    let zip_name = format!("rivet-v{version}-windows-x64.zip");
    let zip_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == zip_name)
        .with_context(|| format!("release {version} does not contain {zip_name}"))?;
    let sums_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .with_context(|| format!("release {version} does not contain SHA256SUMS"))?;

    let temp = std::env::temp_dir().join(format!("rivet-self-update-{}", std::process::id()));
    if temp.exists() {
        fs::remove_dir_all(&temp)?;
    }
    fs::create_dir_all(&temp)?;
    let sums_path = temp.join("SHA256SUMS");
    let zip_path = temp.join(&zip_name);
    download::download_to(&sums_asset.browser_download_url, &sums_path)?;
    download::download_to(&zip_asset.browser_download_url, &zip_path)?;

    let sums = fs::read_to_string(&sums_path)?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            (name == zip_name).then(|| hash.to_ascii_lowercase())
        })
        .with_context(|| format!("SHA256SUMS does not list {zip_name}"))?;
    let actual = download::sha256_file(&zip_path)?;
    if actual != expected {
        bail!("sha256 mismatch for {zip_name}: expected {expected}, got {actual}");
    }

    let staging = temp.join("stage");
    archive::extract(&zip_path, &staging, Some("zip"), 0)?;
    let new_rv = staging.join("rv.exe");
    if !new_rv.is_file() || !staging.join("rv-shim.exe").is_file() {
        bail!("release archive does not contain rv.exe and rv-shim.exe");
    }

    let log = std::env::temp_dir().join(format!("rivet-self-update-{}.log", std::process::id()));
    let script = format!(
        "$ErrorActionPreference = 'Stop'
$parent = Get-Process -Id {pid} -ErrorAction SilentlyContinue
if ($null -ne $parent) {{ $parent.WaitForExit() }}
Start-Sleep -Milliseconds 150
try {{ & {rv} init --no-path *> {log}; $code = $LASTEXITCODE; if ($code -ne 0) {{ Add-Content -Path {log} -Value ('exit code ' + $code) }} }} catch {{ Add-Content -Path {log} -Value $_ }}
Remove-Item -LiteralPath {temp} -Recurse -Force -ErrorAction SilentlyContinue",
        pid = std::process::id(),
        rv = ps_quote(&new_rv.to_string_lossy()),
        log = ps_quote(&log.to_string_lossy()),
        temp = ps_quote(&temp.to_string_lossy()),
    );
    spawn_hidden_powershell(&script)?;

    println!("updating rv {current} -> {latest}");
    println!("the update will finish after this process exits");
    println!("log: {}", log.display());
    println!("root: {}", paths.root.display());
    Ok(())
}

pub fn schedule_uninstall(paths: &Paths) -> Result<std::path::PathBuf> {
    let log = std::env::temp_dir().join(format!("rivet-self-uninstall-{}.log", std::process::id()));
    let script = format!(
        "$ErrorActionPreference = 'Stop'
$parent = Get-Process -Id {pid} -ErrorAction SilentlyContinue
if ($null -ne $parent) {{ $parent.WaitForExit() }}
Start-Sleep -Milliseconds 200
try {{ Remove-Item -LiteralPath {root} -Recurse -Force -ErrorAction Stop }} catch {{ Add-Content -Path {log} -Value $_ }}",
        pid = std::process::id(),
        root = ps_quote(&paths.root.to_string_lossy()),
        log = ps_quote(&log.to_string_lossy()),
    );
    spawn_hidden_powershell(&script)?;
    Ok(log)
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn spawn_hidden_powershell(script: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("starting the deferred update helper")?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_hidden_powershell(_script: &str) -> Result<()> {
    bail!("self-update and self-uninstall are only supported on Windows")
}
