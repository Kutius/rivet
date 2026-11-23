mod admin;
mod health;
mod lifecycle;
mod query;

use crate::cli::{Cli, Commands, ConfigCommands, TapCommands};
use crate::config::{Config, Paths};
use crate::db::{Db, FontRecord, InstalledRecord, PackageRecord, ShimRecord};
use crate::download;
use crate::font;
use crate::lock;
use crate::model::{BinSpec, PackageManifest, compare_manifests};
use crate::selfmanage;
use crate::store;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub(super) struct Runtime {
    pub db: Db,
    pub paths: Paths,
    pub config: Config,
    pub verbose: bool,
    pub json: bool,
}

impl Runtime {
    pub fn log(&self, message: impl std::fmt::Display) {
        if self.verbose {
            eprintln!("verbose: {message}");
        }
    }
}

pub fn run(cli: Cli) -> Result<()> {
    download::init_crypto();
    let verbose = cli.verbose;
    let insecure = cli.insecure;
    let json = cli.json;
    let paths = Paths::discover()?;
    if let Commands::SelfUpdate { force } = &cli.command {
        return selfmanage::update(&paths, *force);
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
        if verbose {
            eprintln!(
                "verbose: acquiring mutation lock {}",
                lock::lock_path(&paths).display()
            );
        }
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

    let rt = Runtime {
        db,
        paths,
        config,
        verbose,
        json,
    };

    match cli.command {
        Commands::Update => admin::cmd_update(&rt),
        Commands::Search { query } => query::cmd_search(&rt, query.as_deref().unwrap_or("")),
        Commands::List => query::cmd_list(&rt),
        Commands::Status { check } => query::cmd_status(&rt, check),
        Commands::Info { package } => query::cmd_info(&rt, &package),
        Commands::Install {
            packages,
            no_deps,
            force,
        } => lifecycle::cmd_install(&rt, &packages, no_deps, force),
        Commands::Remove {
            packages,
            purge,
            force,
        } => lifecycle::cmd_remove(&rt, &packages, purge, force),
        Commands::Upgrade {
            packages,
            all,
            force,
        } => lifecycle::cmd_upgrade(&rt, &packages, all, force),
        Commands::Pin { packages } => lifecycle::cmd_pin(&rt, &packages, true),
        Commands::Unpin { packages } => lifecycle::cmd_pin(&rt, &packages, false),
        Commands::Verify => health::cmd_verify(&rt),
        Commands::Rollback { packages } => lifecycle::cmd_rollback(&rt, &packages),
        Commands::Cleanup { dry_run } => lifecycle::cmd_cleanup(&rt, dry_run),
        Commands::Doctor { fix } => health::cmd_doctor(&rt, fix),
        Commands::Tap { command } => admin::cmd_tap(&rt, command),
        Commands::Config { command } => admin::cmd_config(&rt.paths, command),
        Commands::Init { no_path } => selfmanage::init(&rt.paths, no_path),
        Commands::SelfUpdate { .. } => unreachable!("self-update is handled before database open"),
        Commands::SelfUninstall { yes } => admin::cmd_self_uninstall(&rt, yes),
    }
}

fn is_mutating(command: &Commands) -> bool {
    match command {
        Commands::Update
        | Commands::Install { .. }
        | Commands::Remove { .. }
        | Commands::Upgrade { .. }
        | Commands::Rollback { .. }
        | Commands::Pin { .. }
        | Commands::Unpin { .. }
        | Commands::Init { .. }
        | Commands::SelfUninstall { .. } => true,
        Commands::Status { check } => *check,
        Commands::Cleanup { dry_run } => !dry_run,
        Commands::Tap { command } => matches!(
            command,
            TapCommands::Add { .. } | TapCommands::Remove { .. }
        ),
        Commands::Config { command } => matches!(command, ConfigCommands::Set { .. }),
        Commands::Doctor { fix } => *fix,
        _ => false,
    }
}

pub(super) fn reconcile_shims(
    db: &Db,
    paths: &Paths,
    package: &str,
    manifest: &PackageManifest,
) -> Result<Vec<ShimRecord>> {
    store::preflight_shims(paths, package, &manifest.bin)?;
    for shim in db.shims_for_package(package)? {
        store::remove_shim_files(paths, &shim.command)?;
    }
    let created = store::install_shims(paths, package, &manifest.bin)?;
    let mut records = Vec::new();
    for shim in &created {
        let target = store::current_link(paths, package).join(
            manifest
                .bin
                .iter()
                .find(|bin| bin.command_name() == shim.command)
                .map(BinSpec::path)
                .unwrap_or(""),
        );
        records.push(ShimRecord {
            command: shim.command.clone(),
            package: package.to_string(),
            target: target.to_string_lossy().to_string(),
            sidecar: shim
                .sidecar
                .as_ref()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
        });
    }
    Ok(records)
}

pub(super) fn reconcile_fonts(
    db: &Db,
    package: &str,
    sources: &[PathBuf],
) -> Result<Vec<FontRecord>> {
    let old = db.fonts_for_package(package)?;
    let font_dir = font::font_dir()?;
    let registry_key = font::registry_key();
    if sources.is_empty() {
        font::remove(&old, &font_dir, &registry_key)?;
        return Ok(Vec::new());
    }
    let all = db.all_fonts()?;
    font::replace(package, sources, &old, &all, &font_dir, &registry_key)
}

pub(super) fn manifest_font_sources(
    root: &Path,
    manifest: &PackageManifest,
) -> Result<Vec<PathBuf>> {
    if manifest.fonts.is_empty() {
        Ok(Vec::new())
    } else {
        font::resolve(root, &manifest.fonts)
    }
}

pub(super) fn select_installed_targets(
    db: &Db,
    specs: &[String],
    all: bool,
) -> Result<Vec<InstalledRecord>> {
    let installed = db.list_installed()?;
    if all || specs.is_empty() {
        return Ok(installed);
    }
    let mut result = Vec::new();
    for spec in specs {
        let name = installed_name(db, spec)?;
        if let Some(item) = installed.iter().find(|item| item.name == name) {
            result.push(item.clone());
        }
    }
    Ok(result)
}

pub(super) fn find_package(db: &Db, spec: &str) -> Result<PackageRecord> {
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
        bail!("package '{name}' not found; run 'rivet update' or check 'rivet search'");
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

pub(super) fn find_dependency(db: &Db, preferred_repo: &str, dep: &str) -> Result<PackageRecord> {
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

pub(super) fn parse_spec(spec: &str) -> (Option<String>, String, Option<String>) {
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

pub(super) fn installed_name(db: &Db, spec: &str) -> Result<String> {
    let (_, name, _) = parse_spec(spec);
    if db.installed(&name)?.is_some() {
        Ok(name)
    } else {
        bail!("package '{name}' is not installed")
    }
}

#[cfg(test)]
mod tests {
    use super::is_mutating;
    use crate::cli::Commands;

    #[test]
    fn cleanup_dry_run_and_status_check_locking() {
        assert!(!is_mutating(&Commands::Cleanup { dry_run: true }));
        assert!(is_mutating(&Commands::Cleanup { dry_run: false }));
        assert!(!is_mutating(&Commands::Status { check: false }));
        assert!(is_mutating(&Commands::Status { check: true }));
    }
}
