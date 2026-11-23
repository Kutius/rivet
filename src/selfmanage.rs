use crate::archive;
use crate::config::{Config, Paths};
use crate::download;
use crate::winreg;
use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

const ENV_REGISTRY_KEY: &str = "Environment";

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

pub fn install_self(paths: &Paths) -> Result<()> {
    let current = std::env::current_exe().context("locating current executable")?;
    let rv_destination = paths.bin.join("rv.exe");
    let same = current
        .canonicalize()
        .ok()
        .zip(rv_destination.canonicalize().ok())
        .is_some_and(|(left, right)| left == right);
    if !same {
        fs::copy(&current, &rv_destination).with_context(|| {
            format!(
                "copying {} to {}",
                current.display(),
                rv_destination.display()
            )
        })?;
    }
    install_powershell_alias(&rv_destination, &paths.bin.join("rivet.exe"))?;
    let shim_source = current
        .parent()
        .context("current executable has no parent")?
        .join("rv-shim.exe");
    if shim_source.exists() {
        let shim_destination = paths.bin.join("rv-shim.exe");
        let same_shim = shim_source
            .canonicalize()
            .ok()
            .zip(shim_destination.canonicalize().ok())
            .is_some_and(|(left, right)| left == right);
        if !same_shim {
            fs::copy(&shim_source, &shim_destination).with_context(|| {
                format!(
                    "copying {} to {}",
                    shim_source.display(),
                    shim_destination.display()
                )
            })?;
        }
    }
    Ok(())
}

pub fn install_powershell_alias(rv: &Path, rivet: &Path) -> Result<()> {
    let unchanged = rv
        .canonicalize()
        .ok()
        .zip(rivet.canonicalize().ok())
        .is_some_and(|(left, right)| left == right)
        || (rv.exists()
            && rivet.exists()
            && download::sha256_file(rv)? == download::sha256_file(rivet)?);
    if !unchanged {
        if rivet.exists() {
            let _ = fs::remove_file(rivet);
        }
        if fs::hard_link(rv, rivet).is_err() {
            fs::copy(rv, rivet)
                .with_context(|| format!("copying {} to {}", rv.display(), rivet.display()))?;
        }
    }
    Ok(())
}

pub fn init(paths: &Paths, no_path: bool) -> Result<()> {
    paths.ensure_layout()?;
    install_self(paths)?;
    let config = Config::load(paths)?;
    if no_path {
        println!(
            "installed {}, {}, and {}; PATH not changed (run 'rivet init' to add {})",
            paths.bin.join("rv.exe").display(),
            paths.bin.join("rivet.exe").display(),
            paths.bin.join("rv-shim.exe").display(),
            paths.bin.display()
        );
        print_catalog(&config);
        return Ok(());
    }
    let bin = paths.bin.to_string_lossy().to_string();
    let current = user_path()?;
    if path_has_entry(&current, &bin) {
        println!("{} is already in the user PATH", paths.bin.display());
        print_catalog(&config);
        return Ok(());
    }
    let new_path = append_path(&current, &bin);
    set_user_path(&new_path)?;
    if let Err(error) = winreg::broadcast_setting_change("Environment") {
        eprintln!("warning: PATH was updated but the environment broadcast failed: {error}");
    }
    println!(
        "added {} to the user PATH; open a new terminal and run 'rivet'",
        paths.bin.display()
    );
    print_catalog(&config);
    Ok(())
}

pub fn remove_bin_from_user_path(paths: &Paths) -> Result<()> {
    let bin = paths.bin.to_string_lossy().to_string();
    let current = user_path()?;
    if path_has_entry(&current, &bin) {
        set_user_path(&remove_path_entry(&current, &bin))?;
        if let Err(error) = winreg::broadcast_setting_change("Environment") {
            eprintln!("warning: PATH was updated but the environment broadcast failed: {error}");
        }
    }
    Ok(())
}

fn print_catalog(config: &Config) {
    if let Some(location) = config.main_index_location() {
        println!("catalog: {location}");
    }
}

fn path_has_entry(current: &str, bin: &str) -> bool {
    current.split(';').any(|part| {
        part.trim()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(bin.trim_end_matches('\\'))
    })
}

fn append_path(current: &str, bin: &str) -> String {
    if current.trim().is_empty() {
        bin.to_string()
    } else {
        format!("{};{}", current.trim_end_matches(';'), bin)
    }
}

fn remove_path_entry(current: &str, bin: &str) -> String {
    current
        .split(';')
        .filter(|part| {
            !part
                .trim()
                .trim_end_matches('\\')
                .eq_ignore_ascii_case(bin.trim_end_matches('\\'))
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn user_path() -> Result<String> {
    Ok(winreg::get_hkcu_string(ENV_REGISTRY_KEY, "Path")?.unwrap_or_default())
}

fn set_user_path(value: &str) -> Result<()> {
    winreg::set_hkcu_string(ENV_REGISTRY_KEY, "Path", value, true)
}

#[cfg(not(windows))]
fn spawn_hidden_powershell(_script: &str) -> Result<()> {
    bail!("self-update and self-uninstall are only supported on Windows")
}

#[cfg(test)]
mod tests {
    use super::{append_path, install_powershell_alias, path_has_entry, remove_path_entry};
    use std::fs;

    #[test]
    fn rivet_alias_tracks_the_rv_binary() {
        let directory = tempfile::tempdir().unwrap();
        let rv = directory.path().join("rv.exe");
        let rivet = directory.path().join("rivet.exe");
        fs::write(&rv, b"first").unwrap();
        install_powershell_alias(&rv, &rivet).unwrap();
        assert_eq!(fs::read(&rivet).unwrap(), b"first");
        fs::write(&rv, b"second").unwrap();
        install_powershell_alias(&rv, &rivet).unwrap();
        assert_eq!(fs::read(&rivet).unwrap(), b"second");
    }

    #[test]
    fn path_entry_matches_case_and_trailing_slashes() {
        let current = r"C:\Tools;C:\Users\me\AppData\Local\Rivet\bin\";
        assert!(path_has_entry(
            current,
            r"c:\users\me\appdata\local\rivet\bin"
        ));
        assert!(!path_has_entry(current, r"C:\Other"));
    }

    #[test]
    fn append_path_does_not_add_an_extra_separator() {
        assert_eq!(append_path("", r"C:\Rivet\bin"), r"C:\Rivet\bin");
        assert_eq!(
            append_path(r"C:\Tools;", r"C:\Rivet\bin"),
            r"C:\Tools;C:\Rivet\bin"
        );
    }

    #[test]
    fn remove_path_entry_matches_case_and_trailing_slashes() {
        assert_eq!(
            remove_path_entry(r"C:\Tools;C:\Rivet\bin\;D:\Other", r"c:\rivet\bin"),
            r"C:\Tools;D:\Other"
        );
    }
}
