use crate::archive;
use crate::cli::{Cli, Commands, ConfigCommands, RepoCommands, TapCommands};
use crate::config::{Config, Paths};
use crate::db::{Db, InstalledRecord, PackageRecord, ShimRecord};
use crate::download;
use crate::index;
use crate::lock;
use crate::model::{BinSpec, PackageManifest, compare_manifests};
use crate::repo;
use crate::store;
use crate::trust;
use anyhow::{Context, Result, bail};
use semver::Version;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn run(cli: Cli) -> Result<()> {
    download::init_crypto();
    let insecure = cli.insecure;
    let paths = Paths::discover()?;
    if let Commands::Repo { command } = &cli.command {
        return cmd_repo(command.clone());
    }
    if let Commands::Config {
        command: ConfigCommands::Path,
    } = &cli.command
    {
        println!("{}", paths.config_file.display());
        return Ok(());
    }
    paths.ensure_layout()?;
    let lock_guard = if is_mutating(&cli.command) {
        Some(lock::acquire(&paths)?)
    } else {
        None
    };
    let mut config = Config::load(&paths)?;
    if insecure {
        config.allow_unsigned_remote = true;
    }
    let db = Db::open(&paths.db_file)?;
    if lock_guard.is_some() {
        store::recover_interrupted(&paths, &db)?;
    }

    match cli.command {
        Commands::Update => cmd_update(&db, &paths, &config),
        Commands::Search { query } => cmd_search(&db, query.as_deref().unwrap_or("")),
        Commands::List => cmd_list(&db),
        Commands::Status { check } => cmd_status(&db, &paths, &config, check),
        Commands::Info { package } => cmd_info(&db, &package),
        Commands::Install {
            packages,
            no_deps,
            force,
        } => cmd_install(&db, &paths, &config, &packages, no_deps, force),
        Commands::Remove {
            packages,
            purge,
            force,
        } => cmd_remove(&db, &paths, &packages, purge, force),
        Commands::Upgrade { packages, all } => cmd_upgrade(&db, &paths, &config, &packages, all),
        Commands::Rollback { packages } => cmd_rollback(&db, &paths, &packages),
        Commands::Cleanup => cmd_cleanup(&db, &paths, &config),
        Commands::Doctor { fix } => cmd_doctor(&db, &paths, fix),
        Commands::Tap { command } => cmd_tap(&db, &paths, command, &config.trust()),
        Commands::Repo { command } => cmd_repo(command),
        Commands::Config { command } => cmd_config(&paths, command),
        Commands::Init => cmd_init(&paths),
    }
}

fn is_mutating(command: &Commands) -> bool {
    match command {
        Commands::Update
        | Commands::Install { .. }
        | Commands::Remove { .. }
        | Commands::Upgrade { .. }
        | Commands::Rollback { .. }
        | Commands::Cleanup
        | Commands::Init => true,
        Commands::Tap { command } => matches!(
            command,
            TapCommands::Add { .. } | TapCommands::Remove { .. }
        ),
        Commands::Config { command } => matches!(command, ConfigCommands::Set { .. }),
        Commands::Doctor { fix } => *fix,
        _ => false,
    }
}

fn cmd_update(db: &Db, paths: &Paths, config: &Config) -> Result<()> {
    let trust = config.trust();
    let has_main = config.main_index_location().is_some() || db.get_repo("main")?.is_some();
    if has_main {
        repo::sync_main(db, paths, config, &trust)?;
    } else {
        println!("main index is not configured; updating taps only");
    }
    repo::sync_taps(db, paths, &trust)?;
    Ok(())
}

fn cmd_search(db: &Db, query: &str) -> Result<()> {
    let records = db.search(query, 100)?;
    let mut unique: BTreeMap<(String, String), PackageRecord> = BTreeMap::new();
    for record in records {
        let key = (record.repo.clone(), record.manifest.name.clone());
        match unique.get(&key) {
            Some(existing) if compare_manifests(&record.manifest, &existing.manifest).is_le() => {}
            _ => {
                unique.insert(key, record);
            }
        }
    }
    if unique.is_empty() {
        println!("no packages found");
        return Ok(());
    }
    println!(
        "{:<24} {:<12} {:<14} {}",
        "NAME", "VERSION", "REPO", "DESCRIPTION"
    );
    for record in unique.values() {
        println!(
            "{:<24} {:<12} {:<14} {}",
            record.manifest.name,
            record.manifest.version,
            record.repo,
            truncate(&record.manifest.description, 70)
        );
    }
    Ok(())
}

fn cmd_list(db: &Db) -> Result<()> {
    let installed = db.list_installed()?;
    if installed.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    println!("{:<24} {:<12} {:<14} PATH", "NAME", "VERSION", "REPO");
    for item in installed {
        println!(
            "{:<24} {:<12} {:<14} {}",
            item.name, item.version, item.repo, item.store_path
        );
    }
    Ok(())
}

fn cmd_status(db: &Db, paths: &Paths, config: &Config, check: bool) -> Result<()> {
    if check {
        cmd_update(db, paths, config)?;
    }
    let installed = db.list_installed()?;
    if installed.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    println!(
        "{:<24} {:<12} {:<12} {}",
        "NAME", "INSTALLED", "LATEST", "STATUS"
    );
    let mut outdated = 0usize;
    for item in installed {
        let versions = db.package_versions(&item.repo, &item.name)?;
        let latest = versions
            .last()
            .map(|record| record.manifest.version.clone());
        let status = match latest {
            None => "missing metadata".to_string(),
            Some(ref latest) => {
                if Version::parse(&latest)? > Version::parse(&item.version)? {
                    outdated += 1;
                    "outdated".to_string()
                } else {
                    "ok".to_string()
                }
            }
        };
        println!(
            "{:<24} {:<12} {:<12} {}",
            item.name,
            item.version,
            latest.as_deref().unwrap_or("-"),
            status
        );
    }
    if outdated > 0 {
        println!("{outdated} package(s) can be upgraded with 'rv upgrade'");
    }
    Ok(())
}

fn cmd_info(db: &Db, spec: &str) -> Result<()> {
    let record = find_package(db, spec)?;
    let versions = db.package_versions(&record.repo, &record.manifest.name)?;
    println!("name:        {}", record.manifest.name);
    println!("repo:        {}", record.repo);
    println!("version:     {}", record.manifest.version);
    println!("description: {}", record.manifest.description);
    println!("homepage:    {}", record.manifest.homepage);
    println!("license:     {}", record.manifest.license);
    println!("url:         {}", record.manifest.url);
    println!("sha256:      {}", record.manifest.sha256);
    if !record.manifest.depends.is_empty() {
        println!("depends:     {}", record.manifest.depends.join(", "));
    }
    if !record.manifest.persist.is_empty() {
        println!("persist:     {}", record.manifest.persist.join(", "));
    }
    println!(
        "versions:    {}",
        versions
            .iter()
            .map(|v| v.manifest.version.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

fn cmd_install(
    db: &Db,
    paths: &Paths,
    config: &Config,
    specs: &[String],
    no_deps: bool,
    force: bool,
) -> Result<()> {
    if specs.is_empty() {
        bail!("specify at least one package");
    }
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for spec in specs {
        install_spec(
            db,
            paths,
            config,
            spec,
            no_deps,
            force,
            &mut visiting,
            &mut done,
        )?;
    }
    Ok(())
}

fn install_spec(
    db: &Db,
    paths: &Paths,
    config: &Config,
    spec: &str,
    no_deps: bool,
    force: bool,
    visiting: &mut HashSet<String>,
    done: &mut HashSet<String>,
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
            install_spec(db, paths, config, &dep_spec, false, force, visiting, done)?;
        }
    }

    install_one(db, paths, config, &record, force)?;
    visiting.remove(&key);
    done.insert(key);
    Ok(())
}

fn install_one(
    db: &Db,
    paths: &Paths,
    _config: &Config,
    record: &PackageRecord,
    force: bool,
) -> Result<()> {
    let manifest = &record.manifest;
    let existing = db.installed(&manifest.name)?;
    if let Some(current) = &existing {
        if current.version == manifest.version && !force {
            println!(
                "{} {} is already installed",
                manifest.name, manifest.version
            );
            return Ok(());
        }
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

    let result = (|| -> Result<()> {
        let url = download::resolve_artifact_url(&record.location, &manifest.url)?;
        println!(
            "Installing {} {} from {}",
            manifest.name, manifest.version, record.repo
        );
        let blob = download::cache_blob(&paths.cache, &url, &manifest.sha256)?;
        let kind = manifest
            .extract
            .clone()
            .or_else(|| archive_kind_from_url(&manifest.url));
        if matches!(kind.as_deref(), Some("none") | Some("raw")) {
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
            )?;
        }
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

    for persist in &manifest.persist {
        store::ensure_persist_junction(paths, &manifest.name, &manifest.version, persist)?;
    }

    store::switch_current(paths, &manifest.name, &manifest.version)?;

    let shims = reconcile_shims(db, paths, &manifest.name, manifest)?;

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
    )?;
    println!(
        "installed {} {} ({} command{})",
        manifest.name,
        manifest.version,
        manifest.bin.len(),
        if manifest.bin.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

fn reconcile_shims(
    db: &Db,
    paths: &Paths,
    package: &str,
    manifest: &PackageManifest,
) -> Result<Vec<ShimRecord>> {
    for shim in db.shims_for_package(package)? {
        store::remove_shim_files(paths, &shim.command)?;
    }
    let created = store::install_shims(paths, package, &manifest.bin)?;
    let mut records = Vec::new();
    for (command, _shim_path, sidecar_path) in &created {
        let target = store::current_link(paths, package).join(
            manifest
                .bin
                .iter()
                .find(|bin| bin.command_name() == *command)
                .map(BinSpec::path)
                .unwrap_or(""),
        );
        records.push(ShimRecord {
            command: command.clone(),
            package: package.to_string(),
            target: target.to_string_lossy().to_string(),
            sidecar: sidecar_path.to_string_lossy().to_string(),
        });
    }
    Ok(records)
}

fn cmd_remove(db: &Db, paths: &Paths, specs: &[String], purge: bool, force: bool) -> Result<()> {
    if specs.is_empty() {
        bail!("specify at least one package");
    }
    let mut names = Vec::new();
    for spec in specs {
        names.push(installed_name(db, spec)?);
    }
    if !force {
        for name in &names {
            let dependents = reverse_dependents(db, name)?;
            if !dependents.is_empty() {
                bail!(
                    "cannot remove '{name}': installed package(s) {} depend on it.\n  Remove the dependents first, or pass --force to remove it anyway.",
                    dependents.join(", ")
                );
            }
        }
    }
    for name in names {
        for shim in db.shims_for_package(&name)? {
            store::remove_shim_files(paths, &shim.command)?;
        }
        store::remove_current(paths, &name)?;
        db.remove_installed(&name)?;
        let app_store = paths.store.join(&name);
        if app_store.exists() {
            fs::remove_dir_all(&app_store)
                .with_context(|| format!("removing {}", app_store.display()))?;
        }
        if purge {
            let persist = paths.persist.join(&name);
            if persist.exists() {
                fs::remove_dir_all(&persist)
                    .with_context(|| format!("removing {}", persist.display()))?;
            }
        }
        println!(
            "removed {name}{}",
            if purge {
                " (including persist data)"
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn cmd_upgrade(db: &Db, paths: &Paths, config: &Config, specs: &[String], all: bool) -> Result<()> {
    let installed = db.list_installed()?;
    let targets: Vec<InstalledRecord> = if all || specs.is_empty() {
        installed
    } else {
        let mut result = Vec::new();
        for spec in specs {
            let name = installed_name(db, spec)?;
            if let Some(item) = installed.iter().find(|item| item.name == name) {
                result.push(item.clone());
            }
        }
        result
    };
    if targets.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    for item in targets {
        if item.pinned {
            println!("{} is pinned; skipping", item.name);
            continue;
        }
        let versions = db.package_versions(&item.repo, &item.name)?;
        let Some(latest) = versions.last() else {
            eprintln!("{}: no package metadata in {}", item.name, item.repo);
            continue;
        };
        if Version::parse(&latest.manifest.version)? <= Version::parse(&item.version)? {
            println!("{} {} is up to date", item.name, item.version);
            continue;
        }
        let mut visiting = HashSet::new();
        let mut done = HashSet::new();
        let spec = format!("{}/{}@{}", item.repo, item.name, latest.manifest.version);
        install_spec(
            db,
            paths,
            config,
            &spec,
            false,
            false,
            &mut visiting,
            &mut done,
        )?;
    }
    Ok(())
}

fn cmd_rollback(db: &Db, paths: &Paths, specs: &[String]) -> Result<()> {
    let installed = db.list_installed()?;
    let targets: Vec<InstalledRecord> = if specs.is_empty() {
        installed
    } else {
        let mut result = Vec::new();
        for spec in specs {
            let name = installed_name(db, spec)?;
            if let Some(item) = installed.iter().find(|item| item.name == name) {
                result.push(item.clone());
            }
        }
        result
    };
    for item in targets {
        let Some(previous) = item.previous_version.clone() else {
            println!("{}: no previous version", item.name);
            continue;
        };
        let previous_dir = store::version_dir(paths, &item.name, &previous);
        if !previous_dir.exists() {
            eprintln!("{}: previous version {} is missing", item.name, previous);
            continue;
        }
        store::switch_current(paths, &item.name, &previous)?;
        let manifest_path = previous_dir.join("rivet-manifest.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest: PackageManifest = serde_json::from_str(&manifest_text)?;
        let shims = reconcile_shims(db, paths, &item.name, &manifest)?;
        db.save_install_state(
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
        )?;
        println!("rolled back {} {} -> {}", item.name, item.version, previous);
    }
    Ok(())
}

fn cmd_cleanup(db: &Db, paths: &Paths, config: &Config) -> Result<()> {
    let installed = db.list_installed()?;
    let keep_previous = config.keep_versions.max(1);
    for item in &installed {
        let mut versions = store::list_version_dirs(paths, &item.name)?;
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
            } else {
                store::remove_version_dir(paths, &item.name, &version)?;
                println!("removed {} {}", item.name, version);
            }
        }
    }

    let installed_names: HashSet<String> = installed.iter().map(|item| item.name.clone()).collect();
    if paths.store.is_dir() {
        for entry in fs::read_dir(&paths.store)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !installed_names.contains(&name) {
                    fs::remove_dir_all(entry.path())?;
                    println!("removed orphan store {}", name);
                }
            }
        }
    }

    let mut keep_hashes = HashSet::new();
    for item in &installed {
        let versions = [Some(item.version.clone()), item.previous_version.clone()];
        for version in versions.into_iter().flatten() {
            let manifest_path =
                store::version_dir(paths, &item.name, &version).join("rivet-manifest.json");
            if let Ok(text) = fs::read_to_string(manifest_path) {
                if let Ok(manifest) = serde_json::from_str::<PackageManifest>(&text) {
                    keep_hashes.insert(manifest.sha256.to_ascii_lowercase());
                }
            }
        }
    }
    if paths.cache.is_dir() {
        for entry in fs::read_dir(&paths.cache)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".part") || !keep_hashes.contains(&name) {
                fs::remove_file(entry.path())?;
                println!("removed cache {}", name);
            }
        }
    }
    Ok(())
}

fn cmd_doctor(db: &Db, paths: &Paths, fix: bool) -> Result<()> {
    println!("root:      {}", paths.root.display());
    println!("database:  {}", paths.db_file.display());
    let mut ok = true;
    let mut repairable = 0usize;
    if !paths.bin.join("rv-shim.exe").exists() {
        println!(
            "bin:       missing {} (run 'rv init')",
            paths.bin.join("rv-shim.exe").display()
        );
        ok = false;
    } else {
        println!("bin:       ok");
    }
    if store::path_contains(paths) {
        println!("PATH:      ok");
    } else {
        println!(
            "PATH:      {} is not in PATH (run 'rv init')",
            paths.bin.display()
        );
        ok = false;
    }
    for item in db.list_installed()? {
        let store_path = Path::new(&item.store_path);
        let manifest_path = store_path.join("rivet-manifest.json");
        if !store_path.is_dir() || !manifest_path.is_file() {
            println!(
                "install:   missing store for {} at {} (cannot repair; reinstall it)",
                item.name, item.store_path
            );
            ok = false;
            continue;
        }
        let manifest: PackageManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        let link = store::current_link(paths, &item.name);
        let link_ok = match (fs::canonicalize(&link), fs::canonicalize(store_path)) {
            (Ok(link_target), Ok(store_target)) => link_target == store_target,
            _ => false,
        };
        if !link_ok {
            if fix {
                store::switch_current(paths, &item.name, &item.version)?;
                println!("fixed:     current link {} -> {}", item.name, item.version);
            } else {
                println!(
                    "current:   broken link for {} (repairable: rv doctor --fix)",
                    item.name
                );
                repairable += 1;
                ok = false;
            }
        }
        let missing: Vec<String> = manifest
            .bin
            .iter()
            .map(|bin| bin.command_name())
            .filter(|command| !paths.bin.join(format!("{command}.exe")).exists())
            .collect();
        if !missing.is_empty() {
            if fix {
                let shims = reconcile_shims(db, paths, &item.name, &manifest)?;
                db.save_install_state(&item, &shims)?;
                println!(
                    "fixed:     shims for {} ({})",
                    item.name,
                    missing.join(", ")
                );
            } else {
                println!(
                    "shim:      missing {} (repairable: rv doctor --fix)",
                    missing.join(", ")
                );
                repairable += 1;
                ok = false;
            }
        }
        for shim in db.shims_for_package(&item.name)? {
            if !Path::new(&shim.target).exists() {
                println!(
                    "shim:      broken {} -> {} (repairable: rv doctor --fix)",
                    shim.command, shim.target
                );
                repairable += 1;
                ok = false;
            }
        }
    }
    if ok {
        println!("status:    ok");
    } else if repairable > 0 {
        println!("status:    issues found ({repairable} repairable with 'rv doctor --fix')");
    } else {
        println!("status:    issues found");
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

fn cmd_tap(db: &Db, paths: &Paths, command: TapCommands, trust: &trust::Trust) -> Result<()> {
    match command {
        TapCommands::Add { name, url } => repo::add_tap(db, paths, &name, url.as_deref(), trust),
        TapCommands::List => {
            let repos = db.repos()?;
            if repos.iter().all(|repo| repo.kind != "tap") {
                println!("no taps configured");
            } else {
                for repo in repos.into_iter().filter(|repo| repo.kind == "tap") {
                    println!(
                        "{}\t{}\t{}",
                        repo.name,
                        repo.location,
                        repo.updated_at.unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        TapCommands::Remove { name } => repo::remove_tap(db, paths, &name),
    }
}

fn cmd_repo(command: RepoCommands) -> Result<()> {
    match command {
        RepoCommands::Build { path, output } => {
            let path = std::path::PathBuf::from(path);
            let output = output.map(std::path::PathBuf::from);
            let index = index::build_index(&path, output.as_deref())?;
            println!("built index with {} package versions", index.packages.len());
            Ok(())
        }
        RepoCommands::Keygen { out } => {
            let directory = out
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("keys"));
            let (private, public) = trust::keygen(&directory)?;
            let key_hex = fs::read_to_string(&public)?.trim().to_string();
            println!("private key: {}", private.display());
            println!("public key:  {}", public.display());
            println!("public key hex: {key_hex}");
            println!("trust this key with: rv config set trusted_key {key_hex}");
            Ok(())
        }
        RepoCommands::Sign { path, key } => {
            let path = std::path::PathBuf::from(path);
            let index_path = if path.is_dir() {
                path.join("index.json")
            } else {
                path
            };
            if !index_path.exists() {
                bail!("index file not found: {}", index_path.display());
            }
            let pair = trust::load_private_key(Path::new(&key))?;
            let data = fs::read(&index_path)?;
            let signature = trust::sign(&pair, &data);
            let signature_path = trust::signature_path(&index_path);
            fs::write(
                &signature_path,
                format!("{}\n", serde_json::to_string_pretty(&signature)?),
            )?;
            println!(
                "signed {} (key {})",
                index_path.display(),
                signature.public_key
            );
            Ok(())
        }
        RepoCommands::PublicKey { key } => {
            let pair = trust::load_private_key(Path::new(&key))?;
            println!("{}", trust::public_key_hex(&pair));
            Ok(())
        }
    }
}

fn cmd_config(paths: &Paths, command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Path => {
            println!("{}", paths.config_file.display());
            Ok(())
        }
        ConfigCommands::Get { key } => {
            let config = Config::load(paths)?;
            let value = match key.as_str() {
                "main_index" => config.main_index,
                "concurrency" => config.concurrency.to_string(),
                "keep_versions" => config.keep_versions.to_string(),
                "trusted_key" | "trusted_keys" => config.trusted_keys.join(","),
                "allow_unsigned_remote" => config.allow_unsigned_remote.to_string(),
                _ => bail!("unknown config key '{key}'"),
            };
            println!("{value}");
            Ok(())
        }
        ConfigCommands::Set { key, value } => {
            let mut config = Config::load(paths)?;
            match key.as_str() {
                "main_index" => config.main_index = value,
                "concurrency" => config.concurrency = value.parse()?,
                "keep_versions" => config.keep_versions = value.parse()?,
                "trusted_key" | "trusted_keys" => {
                    for key in value.split(',') {
                        let key = key.trim().to_ascii_lowercase();
                        if !key.is_empty() && !config.trusted_keys.contains(&key) {
                            config.trusted_keys.push(key);
                        }
                    }
                }
                "allow_unsigned_remote" => config.allow_unsigned_remote = value.parse()?,
                _ => bail!("unknown config key '{key}'"),
            }
            config.save(paths)?;
            println!(
                "{} = {}",
                key,
                match key.as_str() {
                    "main_index" => config.main_index,
                    "concurrency" => config.concurrency.to_string(),
                    "keep_versions" => config.keep_versions.to_string(),
                    "trusted_key" | "trusted_keys" => config.trusted_keys.join(","),
                    "allow_unsigned_remote" => config.allow_unsigned_remote.to_string(),
                    _ => String::new(),
                }
            );
            Ok(())
        }
    }
}

fn install_self(paths: &Paths) -> Result<()> {
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

fn cmd_init(paths: &Paths) -> Result<()> {
    paths.ensure_layout()?;
    install_self(paths)?;
    let bin = paths.bin.to_string_lossy().to_string();
    let current = user_path()?;
    let already = current.split(';').any(|part| {
        part.trim_end_matches('\\')
            .eq_ignore_ascii_case(bin.trim_end_matches('\\'))
    });
    if already {
        println!("{} is already in the user PATH", paths.bin.display());
        return Ok(());
    }
    let new_path = if current.trim().is_empty() {
        bin.clone()
    } else {
        format!("{};{}", current.trim_end_matches(';'), bin)
    };
    set_user_path(&new_path)?;
    println!(
        "added {} to the user PATH; open a new terminal to use it",
        paths.bin.display()
    );
    Ok(())
}

fn user_path() -> Result<String> {
    let output = Command::new("reg")
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .output()
        .context("querying user PATH with reg.exe")?;
    if !output.status.success() {
        return Ok(String::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if let Some(index) = line.find("REG_") {
            let rest = &line[index..];
            let mut parts = rest.splitn(2, char::is_whitespace);
            let _type = parts.next();
            if let Some(value) = parts.next() {
                return Ok(value.trim().to_string());
            }
        }
    }
    Ok(String::new())
}

fn set_user_path(value: &str) -> Result<()> {
    let status = Command::new("reg")
        .args([
            "add",
            "HKCU\\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            value,
            "/f",
        ])
        .status()
        .context("setting user PATH with reg.exe")?;
    if !status.success() {
        bail!("reg.exe failed to update user PATH");
    }
    Ok(())
}

fn find_package(db: &Db, spec: &str) -> Result<PackageRecord> {
    let (repo_name, name, version) = parse_spec(spec);
    if let Some(repo) = repo_name {
        let records = db.package_versions(&repo, &name)?;
        if records.is_empty() {
            bail!("package '{name}' not found in repo '{repo}'");
        }
        return select_record_from(records, version.as_deref(), spec);
    }

    let main_records = db.package_versions("main", &name)?;
    if !main_records.is_empty() {
        return select_record_from(main_records, version.as_deref(), spec);
    }
    let all = db.packages_by_name(&name)?;
    if all.is_empty() {
        bail!("package '{name}' not found; run 'rv update' or check 'rv search'");
    }
    let mut repos = all
        .iter()
        .map(|record| record.repo.clone())
        .collect::<Vec<_>>();
    repos.sort();
    repos.dedup();
    if repos.len() > 1 {
        bail!(
            "package '{name}' exists in multiple repos: {}. Use repo/name",
            repos.join(", ")
        );
    }
    select_record_from(all, version.as_deref(), spec)
}

fn find_dependency(db: &Db, preferred_repo: &str, dep: &str) -> Result<PackageRecord> {
    let (repo_name, name, version) = parse_spec(dep);
    let repo = repo_name.as_deref().unwrap_or(preferred_repo);
    let records = db.package_versions(repo, &name)?;
    if !records.is_empty() {
        return select_record_from(records, version.as_deref(), dep);
    }
    if repo != "main" {
        let main_records = db.package_versions("main", &name)?;
        if !main_records.is_empty() {
            return select_record_from(main_records, version.as_deref(), dep);
        }
    }
    bail!("dependency '{dep}' not found")
}

fn select_record_from(
    records: Vec<PackageRecord>,
    version: Option<&str>,
    spec: &str,
) -> Result<PackageRecord> {
    if let Some(version) = version {
        return records
            .into_iter()
            .find(|record| record.manifest.version == version)
            .ok_or_else(|| anyhow::anyhow!("package '{spec}' version not found"));
    }
    records
        .into_iter()
        .max_by(|a, b| compare_manifests(&a.manifest, &b.manifest))
        .ok_or_else(|| anyhow::anyhow!("package '{spec}' not found"))
}

fn parse_spec(spec: &str) -> (Option<String>, String, Option<String>) {
    let (body, version) = match spec.rsplit_once('@') {
        Some((body, version)) if !version.is_empty() => (body, Some(version.to_string())),
        _ => (spec, None),
    };
    let parts: Vec<&str> = body.split('/').collect();
    if parts.len() >= 3 {
        let name = parts.last().unwrap().to_string();
        let repo = parts[..parts.len() - 1].join("/");
        (Some(repo), name, version)
    } else if parts.len() == 2 {
        (Some(parts[0].to_string()), parts[1].to_string(), version)
    } else {
        (None, body.to_string(), version)
    }
}

fn installed_name(db: &Db, spec: &str) -> Result<String> {
    let (_, name, _) = parse_spec(spec);
    if db.installed(&name)?.is_some() {
        Ok(name)
    } else {
        bail!("package '{name}' is not installed")
    }
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

fn truncate(value: &str, length: usize) -> String {
    if value.chars().count() <= length {
        value.to_string()
    } else {
        let mut result: String = value.chars().take(length.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}
