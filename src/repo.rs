use crate::config::{Config, Paths};
use crate::db::{Db, RepoRecord};
use crate::index;
use crate::trust::Trust;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn validate_tap_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        bail!("invalid tap name '{name}'");
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            bail!("invalid tap name '{name}'");
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            bail!("invalid tap name '{name}'");
        }
    }
    Ok(())
}

pub fn sanitize_tap_name(name: &str) -> String {
    let sanitized = name.replace(['/', '\\'], "__");
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "__invalid".to_string()
    } else {
        sanitized
    }
}

pub fn tap_dir(paths: &Paths, name: &str) -> PathBuf {
    paths.taps.join(sanitize_tap_name(name))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn sync_main(db: &Db, paths: &Paths, config: &Config, trust: &Trust) -> Result<()> {
    let location = if let Some(value) = config.main_index_location() {
        value
    } else if let Some(repo) = db.get_repo("main")? {
        repo.location
    } else {
        bail!(
            "main index is not configured. Set RIVET_MAIN_INDEX or run:\n  rv config set main_index C:\\path\\to\\index.json"
        );
    };
    let index = index::load_index(&location, trust)?;
    db.upsert_repo(&RepoRecord {
        name: "main".to_string(),
        kind: "main".to_string(),
        location: location.clone(),
        priority: 0,
        enabled: true,
        updated_at: Some(now()),
    })?;
    db.replace_repo_packages("main", &index)?;
    println!(
        "main: {} package versions from {}",
        index.package_count(),
        location
    );
    let _ = paths;
    Ok(())
}

pub fn sync_taps(db: &Db, paths: &Paths, trust: &Trust) -> Result<()> {
    for repo in db.repos()? {
        if repo.kind != "tap" {
            continue;
        }
        let dir = tap_dir(paths, &repo.name);
        if !dir.exists() {
            eprintln!("tap {}: directory missing, skipping", repo.name);
            continue;
        }
        if dir.join(".git").exists() {
            let output = Command::new("git")
                .args(["-C"])
                .arg(&dir)
                .args(["pull", "--ff-only", "-q"])
                .output()
                .with_context(|| format!("running git pull for tap {}", repo.name))?;
            if !output.status.success() {
                eprintln!(
                    "tap {}: git pull failed; using cached index ({})",
                    repo.name,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }
        let index_path = dir.join("index.json");
        let index = if index_path.exists() {
            index::load_index(&index_path.to_string_lossy(), trust)?
        } else {
            index::build_index(&dir, Some(&index_path))?
        };
        db.upsert_repo(&RepoRecord {
            name: repo.name.clone(),
            kind: "tap".to_string(),
            location: index_path.to_string_lossy().to_string(),
            priority: repo.priority,
            enabled: true,
            updated_at: Some(now()),
        })?;
        db.replace_repo_packages(&repo.name, &index)?;
        println!(
            "tap {}: {} package versions",
            repo.name,
            index.package_count()
        );
    }
    Ok(())
}

pub fn add_tap(db: &Db, paths: &Paths, name: &str, url: Option<&str>, trust: &Trust) -> Result<()> {
    let name = name.trim().trim_end_matches('/');
    validate_tap_name(name)?;
    let repo_url = match url {
        Some(value) => value.to_string(),
        None if name.contains('/') => format!("https://github.com/{name}.git"),
        _ => bail!("tap URL is required for a non owner/repo name"),
    };
    let dir = tap_dir(paths, name);
    if dir.exists() {
        bail!("tap '{}' already exists at {}", name, dir.display());
    }
    fs::create_dir_all(&paths.taps)?;
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "-q", &repo_url])
        .arg(&dir)
        .output()
        .with_context(|| format!("cloning {repo_url}"))?;
    if !output.status.success() {
        bail!(
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let index_path = dir.join("index.json");
    let index = if index_path.exists() {
        index::load_index(&index_path.to_string_lossy(), trust)?
    } else {
        index::build_index(&dir, Some(&index_path))?
    };
    db.upsert_repo(&RepoRecord {
        name: name.to_string(),
        kind: "tap".to_string(),
        location: index_path.to_string_lossy().to_string(),
        priority: 100,
        enabled: true,
        updated_at: Some(now()),
    })?;
    db.replace_repo_packages(name, &index)?;
    println!(
        "added tap {} ({} package versions)",
        name,
        index.package_count()
    );
    Ok(())
}

pub fn remove_tap(db: &Db, paths: &Paths, name: &str) -> Result<()> {
    validate_tap_name(name)?;
    let dir = tap_dir(paths, name);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("removing tap directory {}", dir.display()))?;
    }
    db.remove_repo(name)?;
    println!("removed tap {name}");
    Ok(())
}

pub fn git_head(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}
