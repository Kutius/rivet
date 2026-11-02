use crate::config::Paths;
use crate::model::BinSpec;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShimSpec {
    pub target: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

pub fn version_dir(paths: &Paths, name: &str, version: &str) -> PathBuf {
    paths.store.join(name).join(version)
}

pub fn app_dir(paths: &Paths, name: &str) -> PathBuf {
    paths.apps.join(name)
}

pub fn current_link(paths: &Paths, name: &str) -> PathBuf {
    app_dir(paths, name).join("current")
}

pub fn ensure_app_dir(paths: &Paths, name: &str) -> Result<()> {
    fs::create_dir_all(app_dir(paths, name))?;
    Ok(())
}

pub fn switch_current(paths: &Paths, name: &str, version: &str) -> Result<()> {
    let target = version_dir(paths, name, version);
    if !target.is_dir() {
        bail!("version directory does not exist: {}", target.display());
    }
    ensure_app_dir(paths, name)?;
    let link = current_link(paths, name);
    remove_link(&link)?;
    junction::create(&target, &link).with_context(|| {
        format!(
            "creating junction {} -> {}",
            link.display(),
            target.display()
        )
    })?;
    Ok(())
}

pub fn remove_current(paths: &Paths, name: &str) -> Result<()> {
    remove_link(&current_link(paths, name))
}

fn remove_link(link: &Path) -> Result<()> {
    if fs::symlink_metadata(link).is_err() {
        return Ok(());
    }
    let _ = junction::delete(link);
    if let Ok(metadata) = fs::symlink_metadata(link) {
        if metadata.file_type().is_symlink() {
            fs::remove_file(link).with_context(|| format!("removing link {}", link.display()))?;
        } else {
            fs::remove_dir(link)
                .with_context(|| format!("removing directory {}", link.display()))?;
        }
    }
    Ok(())
}

pub fn ensure_base_shim(paths: &Paths) -> Result<PathBuf> {
    let current = std::env::current_exe().context("locating current executable")?;
    let source = current
        .parent()
        .context("current executable has no parent")?
        .join("rv-shim.exe");
    if !source.exists() {
        bail!(
            "rv-shim.exe not found next to {}. Build both binaries with 'cargo build --bins'.",
            current.display()
        );
    }
    let destination = paths.bin.join("rv-shim.exe");
    fs::create_dir_all(&paths.bin)?;
    let needs_copy = if destination.exists() {
        let source_hash = crate::download::sha256_file(&source)?;
        let dest_hash = crate::download::sha256_file(&destination)?;
        source_hash != dest_hash
    } else {
        true
    };
    if needs_copy {
        fs::copy(&source, &destination).with_context(|| {
            format!("copying {} to {}", source.display(), destination.display())
        })?;
    }
    Ok(destination)
}

pub fn install_shims(
    paths: &Paths,
    package: &str,
    bins: &[BinSpec],
) -> Result<Vec<(String, PathBuf, PathBuf)>> {
    let base = ensure_base_shim(paths)?;
    let mut created = Vec::new();
    for bin in bins {
        let command = bin.command_name();
        let target = current_link(paths, package).join(bin.path());
        let sidecar_path = paths.bin.join(format!("{command}.rvshim"));
        let shim_path = paths.bin.join(format!("{command}.exe"));
        remove_shim_files(paths, &command)?;
        match fs::hard_link(&base, &shim_path) {
            Ok(()) => {}
            Err(_) => {
                fs::copy(&base, &shim_path).with_context(|| {
                    format!(
                        "creating shim {} from {}",
                        shim_path.display(),
                        base.display()
                    )
                })?;
            }
        }
        let spec = ShimSpec {
            target: target.to_string_lossy().to_string(),
            args: bin.args(),
            cwd: None,
            env: BTreeMap::new(),
        };
        fs::write(&sidecar_path, serde_json::to_string_pretty(&spec)?)
            .with_context(|| format!("writing {}", sidecar_path.display()))?;
        created.push((command, shim_path, sidecar_path));
    }
    Ok(created)
}

pub fn remove_shim_files(paths: &Paths, command: &str) -> Result<()> {
    for extension in ["exe", "cmd", "bat", "ps1", "rvshim"] {
        let path = paths.bin.join(format!("{command}.{extension}"));
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}

pub fn path_contains(paths: &Paths) -> bool {
    let Ok(current) = std::env::var("PATH") else {
        return false;
    };
    let needle = paths.bin.to_string_lossy().to_ascii_lowercase();
    current
        .split(';')
        .any(|part| part.trim_end_matches('\\').to_ascii_lowercase() == needle)
}

pub fn list_version_dirs(paths: &Paths, name: &str) -> Result<Vec<String>> {
    let dir = paths.store.join(name);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                result.push(name.to_string());
            }
        }
    }
    result.sort();
    Ok(result)
}

pub fn remove_version_dir(paths: &Paths, name: &str, version: &str) -> Result<()> {
    let dir = version_dir(paths, name, version);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    Ok(())
}

pub fn ensure_persist_junction(
    paths: &Paths,
    package: &str,
    version: &str,
    relative: &str,
) -> Result<()> {
    let persist = paths.persist.join(package).join(relative);
    let installed = version_dir(paths, package, version).join(relative);
    if let Some(parent) = persist.parent() {
        fs::create_dir_all(parent)?;
    }
    if installed.exists() {
        if !persist.exists() {
            fs::rename(&installed, &persist).with_context(|| {
                format!("moving {} to {}", installed.display(), persist.display())
            })?;
        } else if persist.is_dir() {
            if fs::read_dir(&persist)?.next().is_none() && installed.is_dir() {
                for entry in fs::read_dir(&installed)? {
                    let entry = entry?;
                    fs::rename(entry.path(), persist.join(entry.file_name()))?;
                }
            }
            if installed.is_dir() {
                fs::remove_dir_all(&installed)?;
            } else {
                fs::remove_file(&installed)?;
            }
        } else {
            fs::remove_file(&installed)?;
        }
    } else if !persist.exists() {
        fs::create_dir_all(&persist)?;
    }
    if let Some(parent) = installed.parent() {
        fs::create_dir_all(parent)?;
    }
    if installed.exists() {
        if installed.is_dir() {
            junction::delete(&installed)?;
        } else {
            fs::remove_file(&installed)?;
        }
    }
    junction::create(&persist, &installed).with_context(|| {
        format!(
            "creating persist junction {} -> {}",
            installed.display(),
            persist.display()
        )
    })?;
    Ok(())
}

/// Remove artifacts of an interrupted install: staging directories under the
/// store and shim files that have no database record. Safe to run at the start
/// of every mutating command.
pub fn recover_interrupted(paths: &Paths, db: &crate::db::Db) -> Result<()> {
    if paths.store.is_dir() {
        for app in fs::read_dir(&paths.store)? {
            let app = app?;
            if !app.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(app.path())? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(".tmp-") {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    let known: std::collections::HashSet<String> = db
        .all_shims()?
        .into_iter()
        .map(|shim| shim.command.to_ascii_lowercase())
        .collect();
    if paths.bin.is_dir() {
        for entry in fs::read_dir(&paths.bin)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if let Some(command) = file_name.strip_suffix(".rvshim") {
                if !known.contains(&command.to_ascii_lowercase()) {
                    let _ = fs::remove_file(paths.bin.join(format!("{command}.exe")));
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(())
}

pub fn remove_tree(path: &Path) -> Result<()> {
    if path.exists() {
        if path.is_dir() {
            fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))?;
        } else {
            fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}
