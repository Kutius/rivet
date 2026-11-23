use super::{
    Runtime, find_dependency, find_package, installed_name, manifest_font_sources, parse_spec,
    reconcile_fonts, reconcile_shims, select_installed_targets,
};
use crate::archive;
use crate::db::{Db, InstalledRecord, PackageRecord};
use crate::download;
use crate::font;
use crate::model::{BinSpec, PackageManifest};
use crate::store;
use crate::ui;
use anyhow::{Context, Result, bail};
use semver::Version;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(super) fn cmd_install(
    rt: &Runtime,
    specs: &[String],
    no_deps: bool,
    force: bool,
) -> Result<()> {
    if specs.is_empty() {
        bail!("specify at least one package");
    }
    let started = Instant::now();
    let plan = resolve_install_plan(&rt.db, specs, no_deps)?;
    prefetch_plan(rt, &plan, force)?;
    let mut installed_count = 0usize;
    for record in &plan {
        if install_one(rt, record, force)? {
            installed_count += 1;
        }
    }
    ui::print_summary("installed", installed_count, started.elapsed());
    Ok(())
}

fn resolve_install_plan(db: &Db, specs: &[String], no_deps: bool) -> Result<Vec<PackageRecord>> {
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    let mut plan = Vec::new();
    for spec in specs {
        collect_plan(db, spec, no_deps, &mut visiting, &mut done, &mut plan)?;
    }
    Ok(plan)
}

fn collect_plan(
    db: &Db,
    spec: &str,
    no_deps: bool,
    visiting: &mut HashSet<String>,
    done: &mut HashSet<String>,
    plan: &mut Vec<PackageRecord>,
) -> Result<()> {
    let record = find_package(db, spec)?;
    let key = format!("{}/{}", record.repo, record.manifest.name);
    if done.contains(&key) {
        return Ok(());
    }
    if !visiting.insert(key.clone()) {
        bail!("dependency cycle involving {key}");
    }

    if !no_deps {
        for dep in &record.manifest.depends {
            let dep_record = find_dependency(db, &record.repo, dep)?;
            let dep_spec = format!(
                "{}/{}@{}",
                dep_record.repo, dep_record.manifest.name, dep_record.manifest.version
            );
            collect_plan(db, &dep_spec, false, visiting, done, plan)?;
        }
    }

    visiting.remove(&key);
    done.insert(key);
    plan.push(record);
    Ok(())
}

fn prefetch_plan(rt: &Runtime, plan: &[PackageRecord], force: bool) -> Result<()> {
    let mut items = Vec::new();
    for record in plan {
        let already = rt
            .db
            .installed(&record.manifest.name)?
            .is_some_and(|item| item.version == record.manifest.version && !force);
        if already {
            continue;
        }
        let url = download::resolve_artifact_url(&record.location, &record.manifest.url)?;
        items.push((url, record.manifest.sha256.clone()));
    }
    download::prefetch_blobs(&rt.paths.cache, &items, rt.config.concurrency, rt.verbose)
}

fn install_one(rt: &Runtime, record: &PackageRecord, force: bool) -> Result<bool> {
    let started = Instant::now();
    let db = &rt.db;
    let paths = &rt.paths;
    let manifest = &record.manifest;
    let existing = db.installed(&manifest.name)?;
    if let Some(current) = &existing
        && current.version == manifest.version
        && !force
    {
        println!(
            "{} {} is already installed",
            manifest.name, manifest.version
        );
        return Ok(false);
    }

    let mut seen_commands = HashSet::new();
    for bin in &manifest.bin {
        let command = bin.command_name();
        let command_key = command.to_ascii_lowercase();
        if !seen_commands.insert(command_key.clone()) {
            bail!(
                "package '{}' declares duplicate command '{}'",
                manifest.name,
                command
            );
        }
        for shim in db.all_shims()? {
            if shim.command.to_ascii_lowercase() == command_key && shim.package != manifest.name {
                bail!(
                    "command '{}' is already provided by package '{}'",
                    command,
                    shim.package
                );
            }
        }
    }

    let version_dir = store::version_dir(paths, &manifest.name, &manifest.version);
    let staging = paths.store.join(&manifest.name).join(format!(
        ".tmp-{}-{}",
        manifest.version,
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let mut font_relatives: Vec<PathBuf> = Vec::new();
    let mut from_cache = false;
    let result = (|| -> Result<()> {
        let url = download::resolve_artifact_url(&record.location, &manifest.url)?;
        rt.log(format!("resolved artifact url: {url}"));
        if !ui::stderr_is_tty() {
            println!(
                "Installing {} {} from {}",
                manifest.name, manifest.version, record.repo
            );
        }
        from_cache = paths
            .cache
            .join(manifest.sha256.to_ascii_lowercase())
            .is_file();
        let mut download_bar = ui::ProgressBar::new(format!(
            "Downloading {} {}",
            manifest.name, manifest.version
        ));
        let blob = download::cache_blob_with_progress(
            &paths.cache,
            &url,
            &manifest.sha256,
            &mut |current, total| download_bar.update(current, total),
        )
        .with_context(|| format!("downloading {}", url))?;
        download_bar.clear();

        let extract_spinner = ui::Spinner::start(format!(
            "Extracting {} {}...",
            manifest.name, manifest.version
        ));
        let kind = manifest
            .extract
            .clone()
            .or_else(|| archive_kind_from_url(&manifest.url));
        if matches!(kind.as_deref(), Some("none") | Some("raw")) {
            if !manifest.fonts.is_empty() {
                bail!("font packages must use an archive (zip or tar.gz)");
            }
            let raw_name = manifest
                .bin
                .first()
                .map(BinSpec::path)
                .unwrap_or("artifact.bin");
            let raw_path = staging.join(raw_name);
            if let Some(parent) = raw_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&blob, &raw_path)
                .with_context(|| format!("copying raw artifact to {}", raw_path.display()))?;
        } else {
            archive::extract(
                &blob,
                &staging,
                kind.as_deref(),
                manifest.strip_components.unwrap_or(0),
            )
            .with_context(|| format!("extracting {}", manifest.url))?;
        }
        extract_spinner.clear();
        for bin in &manifest.bin {
            let bin_path = staging.join(bin.path());
            if !bin_path.is_file() {
                bail!(
                    "package '{}' is missing declared bin '{}' after extraction",
                    manifest.name,
                    bin.path()
                );
            }
        }
        if !manifest.fonts.is_empty() {
            let staging_root = staging
                .canonicalize()
                .with_context(|| format!("resolving package root {}", staging.display()))?;
            for path in font::resolve(&staging, &manifest.fonts)? {
                let relative = path
                    .strip_prefix(&staging_root)
                    .with_context(|| format!("resolving font path {}", path.display()))?;
                font_relatives.push(relative.to_path_buf());
            }
        }
        fs::write(
            staging.join("rivet-manifest.json"),
            serde_json::to_string_pretty(manifest)?,
        )?;
        fs::write(
            staging.join("rivet-install.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "repo": record.repo,
                "url": url,
                "installed_at": chrono::Utc::now().to_rfc3339(),
            }))?,
        )?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if version_dir.exists() {
        if existing
            .as_ref()
            .is_some_and(|item| item.version == manifest.version)
        {
            store::remove_current(paths, &manifest.name)?;
        }
        fs::remove_dir_all(&version_dir)?;
    }
    if let Some(parent) = version_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&staging, &version_dir)
        .with_context(|| format!("activating {}", version_dir.display()))?;

    let font_sources: Vec<PathBuf> = font_relatives
        .iter()
        .map(|relative| version_dir.join(relative))
        .collect();

    // Preflight font and shim preconditions before mutating `current`, shims, or fonts.
    if !font_sources.is_empty() {
        let old_fonts = db.fonts_for_package(&manifest.name)?;
        let all_fonts = db.all_fonts()?;
        let font_dir = font::font_dir()?;
        font::validate_replace(
            &manifest.name,
            &font_sources,
            &old_fonts,
            &all_fonts,
            &font_dir,
        )?;
    }
    store::preflight_shims(paths, &manifest.name, &manifest.bin)?;

    let activate_spinner = ui::Spinner::start(format!(
        "Activating {} {}...",
        manifest.name, manifest.version
    ));
    for persist in &manifest.persist {
        store::ensure_persist_junction(paths, &manifest.name, &manifest.version, persist)?;
    }

    store::switch_current(paths, &manifest.name, &manifest.version)?;

    let shims = reconcile_shims(db, paths, &manifest.name, manifest)?;
    let fonts = reconcile_fonts(db, &manifest.name, &font_sources)?;

    let previous = existing.as_ref().and_then(|item| {
        if item.version != manifest.version {
            Some(item.version.clone())
        } else {
            item.previous_version.clone()
        }
    });
    db.save_install_state(
        &InstalledRecord {
            name: manifest.name.clone(),
            repo: record.repo.clone(),
            version: manifest.version.clone(),
            previous_version: previous,
            store_path: version_dir.to_string_lossy().to_string(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            pinned: existing.as_ref().is_some_and(|item| item.pinned),
        },
        &shims,
        &fonts,
    )?;
    activate_spinner.clear();
    let mut parts = Vec::new();
    if !manifest.bin.is_empty() {
        parts.push(format!(
            "{} command{}",
            manifest.bin.len(),
            if manifest.bin.len() == 1 { "" } else { "s" }
        ));
    }
    if !font_sources.is_empty() {
        parts.push(format!(
            "{} font{}",
            font_sources.len(),
            if font_sources.len() == 1 { "" } else { "s" }
        ));
    }
    let elapsed = ui::format_duration(started.elapsed());
    let detail = if from_cache {
        format!("· {elapsed}, cached")
    } else {
        format!("· {elapsed}")
    };
    ui::print_done(
        &format!(
            "installed {} {} ({})",
            manifest.name,
            manifest.version,
            parts.join(", ")
        ),
        Some(&detail),
    );
    let commands = manifest.binary_names();
    if !commands.is_empty() {
        ui::print_hint(&format!("→ ready to run: {}", commands.join(", ")));
    }
    Ok(true)
}

pub(super) fn cmd_remove(rt: &Runtime, specs: &[String], purge: bool, force: bool) -> Result<()> {
    if specs.is_empty() {
        bail!("specify at least one package");
    }
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for spec in specs {
        let (_, name, _) = parse_spec(spec);
        if !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }
        if rt.db.installed(&name)?.is_none() {
            println!("{name} is not installed");
            continue;
        }
        names.push(name);
    }
    if names.is_empty() {
        return Ok(());
    }
    if !force {
        let removing: HashSet<String> =
            names.iter().map(|name| name.to_ascii_lowercase()).collect();
        for name in &names {
            let dependents: Vec<String> = reverse_dependents(&rt.db, name)?
                .into_iter()
                .filter(|dep| !removing.contains(&dep.to_ascii_lowercase()))
                .collect();
            if !dependents.is_empty() {
                bail!(
                    "cannot remove '{name}': installed package(s) {} depend on it.\n  Remove the dependents first, or pass --force to remove it anyway.",
                    dependents.join(", ")
                );
            }
        }
    }
    let started = Instant::now();
    let font_dir = font::font_dir()?;
    let registry_key = font::registry_key();
    let removed_count = names.len();
    for name in names {
        let pkg_started = Instant::now();
        let remove_spinner = ui::Spinner::start(format!("Removing {name}..."));
        let fonts = rt.db.fonts_for_package(&name)?;
        if !fonts.is_empty() {
            font::remove(&fonts, &font_dir, &registry_key)?;
        }
        for shim in rt.db.shims_for_package(&name)? {
            store::remove_shim_files(&rt.paths, &shim.command)?;
        }
        store::remove_current(&rt.paths, &name)?;
        rt.db.remove_installed(&name)?;
        let app_store = rt.paths.store.join(&name);
        if app_store.exists() {
            fs::remove_dir_all(&app_store)
                .with_context(|| format!("removing {}", app_store.display()))?;
        }
        let persist = rt.paths.persist.join(&name);
        let kept_persist = if purge {
            if persist.exists() {
                fs::remove_dir_all(&persist)
                    .with_context(|| format!("removing {}", persist.display()))?;
            }
            false
        } else {
            persist.exists()
        };
        remove_spinner.clear();
        let detail = format!("· {}", ui::format_duration(pkg_started.elapsed()));
        ui::print_done(
            &format!(
                "removed {name}{}",
                if purge {
                    " (including persist data)"
                } else {
                    ""
                }
            ),
            Some(&detail),
        );
        if kept_persist {
            ui::print_hint(&format!(
                "kept your data in {} (pass --purge to remove it too)",
                persist.display()
            ));
        }
    }
    ui::print_summary("removed", removed_count, started.elapsed());
    Ok(())
}

pub(super) fn cmd_upgrade(rt: &Runtime, specs: &[String], all: bool, force: bool) -> Result<()> {
    let explicit = !all && !specs.is_empty();
    let targets = select_installed_targets(&rt.db, specs, all)?;
    if targets.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    if explicit && !force {
        let pinned: Vec<String> = targets
            .iter()
            .filter(|item| item.pinned)
            .map(|item| format!("{} {}", item.name, item.version))
            .collect();
        if !pinned.is_empty() {
            bail!(
                "package(s) {} are pinned; pass --force to upgrade them",
                pinned.join(", ")
            );
        }
    }

    let mut upgrade_specs = Vec::new();
    for item in targets {
        if item.pinned {
            if !explicit {
                println!("{} is pinned; skipping", item.name);
                continue;
            }
            println!("{} is pinned; upgrading anyway (--force)", item.name);
        }
        let versions = rt.db.package_versions(&item.repo, &item.name)?;
        let Some(latest) = versions.last() else {
            eprintln!("{}: no package metadata in {}", item.name, item.repo);
            continue;
        };
        if Version::parse(&latest.manifest.version)? <= Version::parse(&item.version)? {
            println!("{} {} is up to date", item.name, item.version);
            continue;
        }
        upgrade_specs.push(format!(
            "{}/{}@{}",
            item.repo, item.name, latest.manifest.version
        ));
    }
    if upgrade_specs.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    let plan = resolve_install_plan(&rt.db, &upgrade_specs, false)?;
    prefetch_plan(rt, &plan, false)?;
    let mut upgraded_count = 0usize;
    for record in &plan {
        if install_one(rt, record, false)? {
            upgraded_count += 1;
        }
    }
    ui::print_summary("upgraded", upgraded_count, started.elapsed());
    Ok(())
}

pub(super) fn cmd_pin(rt: &Runtime, specs: &[String], pinned: bool) -> Result<()> {
    if specs.is_empty() {
        bail!("specify at least one package");
    }
    for spec in specs {
        let name = installed_name(&rt.db, spec)?;
        let item = rt
            .db
            .installed(&name)?
            .ok_or_else(|| anyhow::anyhow!("package '{name}' is not installed"))?;
        if item.pinned == pinned {
            println!(
                "{} {} is already {}",
                item.name,
                item.version,
                if pinned { "pinned" } else { "unpinned" }
            );
            continue;
        }
        rt.db.set_pinned(&name, pinned)?;
        println!(
            "{} {} {}",
            item.name,
            item.version,
            if pinned { "pinned" } else { "unpinned" }
        );
    }
    Ok(())
}

pub(super) fn cmd_rollback(rt: &Runtime, specs: &[String]) -> Result<()> {
    let targets = select_installed_targets(&rt.db, specs, false)?;
    for item in targets {
        let Some(previous) = item.previous_version.clone() else {
            println!("{}: no previous version", item.name);
            continue;
        };
        let previous_dir = store::version_dir(&rt.paths, &item.name, &previous);
        if !previous_dir.exists() {
            eprintln!("{}: previous version {} is missing", item.name, previous);
            continue;
        }
        store::switch_current(&rt.paths, &item.name, &previous)?;
        let manifest_path = previous_dir.join("rivet-manifest.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: PackageManifest = serde_json::from_str(&manifest_text)?;
        let shims = reconcile_shims(&rt.db, &rt.paths, &item.name, &manifest)?;
        let font_sources = manifest_font_sources(&previous_dir, &manifest)?;
        let fonts = reconcile_fonts(&rt.db, &item.name, &font_sources)?;
        rt.db.save_install_state(
            &InstalledRecord {
                name: item.name.clone(),
                repo: item.repo.clone(),
                version: previous.clone(),
                previous_version: Some(item.version.clone()),
                store_path: previous_dir.to_string_lossy().to_string(),
                installed_at: chrono::Utc::now().to_rfc3339(),
                pinned: item.pinned,
            },
            &shims,
            &fonts,
        )?;
        println!("rolled back {} {} -> {}", item.name, item.version, previous);
    }
    Ok(())
}

pub(super) fn cmd_cleanup(rt: &Runtime, dry_run: bool) -> Result<()> {
    let installed = rt.db.list_installed()?;
    let keep_previous = rt.config.keep_versions.max(1);
    for item in &installed {
        let mut versions = store::list_version_dirs(&rt.paths, &item.name)?;
        versions.sort_by(|a, b| match (Version::parse(a), Version::parse(b)) {
            (Ok(av), Ok(bv)) => av.cmp(&bv),
            _ => a.cmp(b),
        });
        let mut keep: HashSet<String> = HashSet::new();
        keep.insert(item.version.clone());
        if let Some(previous) = &item.previous_version {
            keep.insert(previous.clone());
        }
        let mut extra = Vec::new();
        for version in versions.into_iter().rev() {
            if keep.contains(&version) {
                continue;
            }
            if extra.len() < keep_previous.saturating_sub(1) {
                extra.push(version);
            } else if dry_run {
                println!("would remove {} {}", item.name, version);
            } else {
                store::remove_version_dir(&rt.paths, &item.name, &version)?;
                println!("removed {} {}", item.name, version);
            }
        }
    }

    let installed_names: HashSet<String> = installed.iter().map(|item| item.name.clone()).collect();
    if rt.paths.store.is_dir() {
        for entry in fs::read_dir(&rt.paths.store)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !installed_names.contains(&name) {
                    if dry_run {
                        println!("would remove orphan store {}", name);
                    } else {
                        fs::remove_dir_all(entry.path())?;
                        println!("removed orphan store {}", name);
                    }
                }
            }
        }
    }

    let mut keep_hashes = HashSet::new();
    for item in &installed {
        let versions = [Some(item.version.clone()), item.previous_version.clone()];
        for version in versions.into_iter().flatten() {
            let manifest_path =
                store::version_dir(&rt.paths, &item.name, &version).join("rivet-manifest.json");
            if let Ok(text) = fs::read_to_string(manifest_path)
                && let Ok(manifest) = serde_json::from_str::<PackageManifest>(&text)
            {
                keep_hashes.insert(manifest.sha256.to_ascii_lowercase());
            }
        }
    }
    if rt.paths.cache.is_dir() {
        for entry in fs::read_dir(&rt.paths.cache)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".part") || !keep_hashes.contains(&name) {
                if dry_run {
                    println!("would remove cache {}", name);
                } else {
                    fs::remove_file(entry.path())?;
                    println!("removed cache {}", name);
                }
            }
        }
    }
    Ok(())
}

fn reverse_dependents(db: &Db, target: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for item in db.list_installed()? {
        if item.name == target {
            continue;
        }
        let manifest_path = Path::new(&item.store_path).join("rivet-manifest.json");
        let Ok(text) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<PackageManifest>(&text) else {
            continue;
        };
        if manifest
            .depends
            .iter()
            .any(|dep| parse_spec(dep).1 == target)
        {
            result.push(item.name.clone());
        }
    }
    Ok(result)
}

fn archive_kind_from_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Some("zip".to_string())
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Some("tar.gz".to_string())
    } else if lower.ends_with(".tar") {
        Some("tar".to_string())
    } else if lower.ends_with(".7z") {
        Some("7z".to_string())
    } else {
        None
    }
}
