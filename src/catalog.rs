use crate::download;
use crate::index;
use crate::model::{BinSpec, PackageManifest};
use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use url::Url;
use walkdir::WalkDir;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub check: bool,
    pub only: Option<String>,
    pub concurrency: usize,
    pub allow_failures: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            check: false,
            only: None,
            concurrency: 8,
            allow_failures: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct UpdateReport {
    pub changed: Vec<(String, String, String)>,
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub name: Option<String>,
    pub asset: Option<String>,
    pub bin: Option<String>,
    pub test_args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceEntry {
    pub repository: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub bin: Option<String>,
    #[serde(default)]
    pub test_args: Vec<String>,
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct Repository {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    html_url: String,
    #[serde(default)]
    license: Option<License>,
}

#[derive(Debug, Deserialize)]
struct License {
    #[serde(default)]
    spdx_id: Option<String>,
}

pub fn add_github_package(root: &Path, repository: &str, options: &AddOptions) -> Result<PathBuf> {
    let path = add_github_package_inner(root, repository, options)?;
    index::build_index(root, None)?;
    Ok(path)
}

fn add_github_package_inner(
    root: &Path,
    repository: &str,
    options: &AddOptions,
) -> Result<PathBuf> {
    let (owner, repo) = parse_repository(repository)?;
    let client = download::http_client()?;
    let info: Repository = github_json(
        &client,
        &format!("https://api.github.com/repos/{owner}/{repo}"),
    )?;
    let release: Release = github_json(
        &client,
        &format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
    )?;
    let version = release_version(&release.tag_name)?;
    let package_name = options
        .name
        .clone()
        .unwrap_or_else(|| repo.to_ascii_lowercase());
    let asset = choose_asset(
        &release.assets,
        options.asset.as_deref(),
        &package_name,
        &repo,
    )?;

    let draft = ManifestDraft {
        name: package_name,
        version: version.to_string(),
        description: info.description.unwrap_or_default(),
        homepage: info
            .homepage
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(info.html_url),
        license: info
            .license
            .and_then(|license| license.spdx_id)
            .filter(|value| value != "NOASSERTION")
            .unwrap_or_default(),
        asset: options.asset.clone(),
        test_args: options.test_args.clone(),
    };
    write_manifest_from_asset(root, &draft, asset, &repo, options.bin.as_deref())
}

pub fn update_github_packages(root: &Path, options: &UpdateOptions) -> Result<UpdateReport> {
    let client = download::http_client()?;
    let mut work = Vec::new();
    for path in manifest_files(root)? {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let manifest: PackageManifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if options
            .only
            .as_deref()
            .is_some_and(|name| manifest.name != name)
        {
            continue;
        }
        let Some((owner, repo)) = github_release_source(&manifest.url) else {
            continue;
        };
        work.push((path, manifest, owner, repo));
    }

    enum Outcome {
        UpToDate(String),
        Changed {
            message: String,
            entry: (String, String, String),
        },
        Failed {
            name: String,
            error: String,
        },
    }

    let workers = options.concurrency.max(1).min(work.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Outcome)>> = Mutex::new(Vec::with_capacity(work.len()));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, manifest, owner, repo)) = work.get(index) else {
                        break;
                    };
                    let outcome = match update_single_package(
                        &client,
                        path,
                        manifest,
                        owner,
                        repo,
                        options.check,
                    ) {
                        Ok(SingleUpdate::UpToDate(msg)) => Outcome::UpToDate(msg),
                        Ok(SingleUpdate::Changed(message, entry)) => {
                            Outcome::Changed { message, entry }
                        }
                        Err(error) => Outcome::Failed {
                            name: manifest.name.clone(),
                            error: format!("{error:#}"),
                        },
                    };
                    results.lock().unwrap().push((index, outcome));
                }
            });
        }
    });

    let mut ordered = results.into_inner().unwrap();
    ordered.sort_by_key(|(index, _)| *index);

    let mut report = UpdateReport::default();
    for (_, outcome) in ordered {
        match outcome {
            Outcome::UpToDate(message) => println!("{message}"),
            Outcome::Changed { message, entry } => {
                println!("{message}");
                report.changed.push(entry);
            }
            Outcome::Failed { name, error } => {
                eprintln!("failed {name}: {error}");
                report.failed.push((name, error));
            }
        }
    }

    if !options.check && !report.changed.is_empty() {
        index::build_index(root, None)?;
    }
    if !report.failed.is_empty() && !options.allow_failures {
        let details = report
            .failed
            .iter()
            .map(|(name, error)| format!("{name}: {error}"))
            .collect::<Vec<_>>()
            .join("\n  ");
        bail!(
            "catalog update failed for {} package(s):\n  {details}",
            report.failed.len()
        );
    }
    Ok(report)
}

enum SingleUpdate {
    UpToDate(String),
    Changed(String, (String, String, String)),
}

fn update_single_package(
    client: &reqwest::blocking::Client,
    path: &Path,
    manifest: &PackageManifest,
    owner: &str,
    repo: &str,
    check: bool,
) -> Result<SingleUpdate> {
    let mut manifest = manifest.clone();
    let release: Release = github_json(
        client,
        &format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
    )?;
    let version = release_version(&release.tag_name)?;
    let current = Version::parse(&manifest.version)
        .map_err(|error| anyhow::anyhow!("invalid current version: {error}"))?;
    if version <= current {
        return Ok(SingleUpdate::UpToDate(format!(
            "{} {}: up to date",
            manifest.name, manifest.version
        )));
    }

    let requested_asset = manifest
        .asset
        .as_deref()
        .map(|pattern| pattern.replace("{version}", &version.to_string()));
    let asset = choose_asset(
        &release.assets,
        requested_asset.as_deref(),
        &manifest.name,
        repo,
    )?;
    if check {
        return Ok(SingleUpdate::Changed(
            format!(
                "{}: {} -> {} ({})",
                manifest.name, manifest.version, version, asset.name
            ),
            (
                manifest.name.clone(),
                manifest.version.clone(),
                version.to_string(),
            ),
        ));
    }

    if manifest.bin.is_empty() {
        let info = download_archive(asset)?;
        let missing: Vec<&String> = manifest
            .fonts
            .iter()
            .filter(|entry| !font_entry_present(&info.files, entry))
            .collect();
        if !missing.is_empty() {
            bail!(
                "new asset no longer contains font path(s) {}; update the manifest manually",
                missing
                    .iter()
                    .map(|entry| entry.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        manifest.version = version.to_string();
        manifest.url = asset.browser_download_url.clone();
        manifest.sha256 = info.sha256;
        manifest.extract = Some(info.extract);
        manifest.strip_components = (info.strip_components > 0).then_some(info.strip_components);
        let text = toml::to_string_pretty(&manifest)?;
        fs::write(path, format!("{text}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
        return Ok(SingleUpdate::Changed(
            format!(
                "{}: {} -> {} ({})",
                manifest.name, current, version, asset.name
            ),
            (
                manifest.name.clone(),
                current.to_string(),
                version.to_string(),
            ),
        ));
    }

    let guessed = download_and_inspect(asset, &manifest.name, repo, None)?;
    if guessed.extract != "none" {
        match reconcile_updated_bins(
            &manifest.bin,
            &manifest.version,
            &version.to_string(),
            &guessed.files,
        ) {
            Ok(updated_bins) => manifest.bin = updated_bins,
            Err(missing) => {
                bail!(
                    "new asset no longer contains {}; update the manifest manually",
                    missing
                        .iter()
                        .map(|p| format!("'{p}'"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
    manifest.version = version.to_string();
    manifest.url = asset.browser_download_url.clone();
    manifest.sha256 = guessed.sha256;
    manifest.extract = Some(guessed.extract);
    manifest.strip_components = (guessed.strip_components > 0).then_some(guessed.strip_components);
    let text = toml::to_string_pretty(&manifest)?;
    fs::write(path, format!("{text}\n")).with_context(|| format!("writing {}", path.display()))?;
    Ok(SingleUpdate::Changed(
        format!(
            "{}: {} -> {} ({})",
            manifest.name, current, version, asset.name
        ),
        (
            manifest.name.clone(),
            current.to_string(),
            version.to_string(),
        ),
    ))
}

pub fn sync_github_packages(
    root: &Path,
    sources: &Path,
    dry_run: bool,
    limit: Option<usize>,
    allow_failures: bool,
) -> Result<SyncReport> {
    let entries = read_sources(sources)?;
    let mut report = SyncReport::default();
    let mut existing_sources = HashSet::new();
    let mut existing_names = HashMap::new();
    for path in manifest_files(root)? {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let manifest: PackageManifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        existing_names.insert(manifest.name.clone(), path.clone());
        if let Some((owner, repo)) = github_release_source(&manifest.url) {
            existing_sources.insert((owner.to_ascii_lowercase(), repo.to_ascii_lowercase()));
        }
    }

    let mut attempted = 0usize;
    for entry in entries {
        let (owner, repo) = match parse_repository(&entry.repository) {
            Ok(value) => value,
            Err(error) => {
                report
                    .failed
                    .push((entry.repository.clone(), format!("{error:#}")));
                continue;
            }
        };
        let key = (owner.to_ascii_lowercase(), repo.to_ascii_lowercase());
        let label = format!("{owner}/{repo}");
        if existing_sources.contains(&key) {
            report.skipped.push(label);
            continue;
        }
        let name = entry
            .name
            .clone()
            .unwrap_or_else(|| repo.to_ascii_lowercase());
        if let Some(path) = existing_names.get(&name) {
            report.failed.push((
                label.clone(),
                format!(
                    "package name '{name}' is already used by {}",
                    path.display()
                ),
            ));
            continue;
        }
        if limit.is_some_and(|limit| attempted >= limit) {
            break;
        }
        attempted += 1;
        if dry_run {
            println!("would add {label} as {name}");
            report.added.push(label);
            existing_sources.insert(key);
            existing_names.insert(name, PathBuf::from("<source list>"));
            continue;
        }
        println!("adding {label} as {name}");
        let options = AddOptions {
            name: entry.name,
            asset: entry.asset,
            bin: entry.bin,
            test_args: entry.test_args,
        };
        match add_github_package_inner(root, &label, &options) {
            Ok(path) => {
                println!("added {label} ({})", path.display());
                report.added.push(label);
                existing_sources.insert(key);
                existing_names.insert(name, path);
            }
            Err(error) => {
                eprintln!("failed {label}: {error:#}");
                report.failed.push((label, format!("{error:#}")));
            }
        }
    }

    if !dry_run && !report.added.is_empty() {
        index::build_index(root, None)?;
        println!("rebuilt index with {} new package(s)", report.added.len());
    }
    if !report.failed.is_empty() && !allow_failures {
        let details = report
            .failed
            .iter()
            .map(|(repository, error)| format!("{repository}: {error}"))
            .collect::<Vec<_>>()
            .join(
                "
  ",
            );
        bail!(
            "{} source(s) failed:
  {details}",
            report.failed.len()
        );
    }
    Ok(report)
}

fn read_sources(path: &Path) -> Result<Vec<SourceEntry>> {
    #[derive(Deserialize, Default)]
    struct SourcesFile {
        #[serde(default)]
        repositories: Vec<String>,
        #[serde(default)]
        source: Vec<SourceEntry>,
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("reading source list {}", path.display()))?;
    let file: SourcesFile =
        toml::from_str(&text).with_context(|| format!("parsing source list {}", path.display()))?;
    let mut entries: Vec<SourceEntry> = file
        .repositories
        .into_iter()
        .map(|repository| SourceEntry {
            repository,
            name: None,
            asset: None,
            bin: None,
            test_args: Vec::new(),
        })
        .collect();
    for entry in file.source {
        if let Some(existing) = entries
            .iter_mut()
            .find(|existing| existing.repository.eq_ignore_ascii_case(&entry.repository))
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        bail!("no sources found in {}", path.display());
    }
    Ok(entries)
}

fn reconcile_updated_bins(
    bins: &[BinSpec],
    old_version: &str,
    new_version: &str,
    files: &[String],
) -> std::result::Result<Vec<BinSpec>, Vec<String>> {
    let mut updated = Vec::with_capacity(bins.len());
    let mut missing = Vec::new();
    for bin in bins {
        if files.iter().any(|file| file == bin.path()) {
            updated.push(bin.clone());
            continue;
        }
        if !old_version.is_empty() && bin.path().contains(old_version) {
            let candidate = bin.path().replace(old_version, new_version);
            if files.iter().any(|file| file == &candidate) {
                let replacement = match bin {
                    BinSpec::Simple(_) => BinSpec::Simple(candidate),
                    BinSpec::Detailed { name, args, .. } => BinSpec::Detailed {
                        path: candidate,
                        name: name.clone(),
                        args: args.clone(),
                    },
                };
                updated.push(replacement);
                continue;
            }
        }
        missing.push(bin.path().to_string());
    }
    if missing.is_empty() {
        Ok(updated)
    } else {
        Err(missing)
    }
}

struct Inspected {
    sha256: String,
    extract: String,
    strip_components: usize,
    bin: BinSpec,
    files: Vec<String>,
}

struct ManifestDraft {
    name: String,
    version: String,
    description: String,
    homepage: String,
    license: String,
    asset: Option<String>,
    test_args: Vec<String>,
}

fn write_manifest_from_asset(
    root: &Path,
    draft: &ManifestDraft,
    asset: &Asset,
    repo_name: &str,
    explicit_bin: Option<&str>,
) -> Result<PathBuf> {
    let guessed = download_and_inspect(asset, &draft.name, repo_name, explicit_bin)?;
    let manifest = PackageManifest {
        name: draft.name.clone(),
        version: draft.version.clone(),
        description: draft.description.clone(),
        homepage: draft.homepage.clone(),
        license: draft.license.clone(),
        tags: Vec::new(),
        arch: Some("x64".to_string()),
        asset: draft.asset.clone(),
        url: asset.browser_download_url.clone(),
        sha256: guessed.sha256,
        extract: Some(guessed.extract),
        strip_components: (guessed.strip_components > 0).then_some(guessed.strip_components),
        bin: vec![guessed.bin],
        fonts: Vec::new(),
        depends: Vec::new(),
        persist: Vec::new(),
        test_args: draft.test_args.clone(),
    };
    manifest
        .validate()
        .with_context(|| format!("validating generated manifest for {}", draft.name))?;
    let packages = if root
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("packages"))
    {
        root.to_path_buf()
    } else {
        root.join("packages")
    };
    fs::create_dir_all(&packages)?;
    let path = packages.join(format!("{}.toml", draft.name));
    if path.exists() {
        bail!(
            "{} already exists; use 'rv-repo update' or edit it manually",
            path.display()
        );
    }
    let text = toml::to_string_pretty(&manifest)?;
    fs::write(&path, format!("{text}\n")).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

struct ArchiveInfo {
    sha256: String,
    extract: String,
    strip_components: usize,
    files: Vec<String>,
}

fn download_archive(asset: &Asset) -> Result<ArchiveInfo> {
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = std::env::temp_dir().join(format!(
        "rivet-repo-{}-{}-{}",
        std::process::id(),
        seq,
        sanitize_file_name(&asset.name)
    ));
    let _ = fs::remove_file(&temp);
    let result = (|| -> Result<ArchiveInfo> {
        download::download_to(&asset.browser_download_url, &temp)?;
        let sha256 = download::sha256_file(&temp)?;
        let kind = archive_kind(&asset.name)?;
        let files = if kind == "none" {
            Vec::new()
        } else {
            archive_files(&temp, &kind)?
        };
        let stripped = strip_root(&files);
        let strip_components = if stripped.0 { 1 } else { 0 };
        Ok(ArchiveInfo {
            sha256,
            extract: kind,
            strip_components,
            files: stripped.1,
        })
    })();
    let _ = fs::remove_file(&temp);
    result
}

fn download_and_inspect(
    asset: &Asset,
    package_name: &str,
    repo_name: &str,
    explicit_bin: Option<&str>,
) -> Result<Inspected> {
    let info = download_archive(asset)?;
    let bin = if info.extract == "none" {
        let path = match explicit_bin {
            Some(value) => normalize_bin(value)?,
            None => format!("{package_name}.exe"),
        };
        let command = command_name(&path, package_name);
        BinSpec::Detailed {
            path,
            name: Some(command),
            args: Vec::new(),
        }
    } else {
        choose_bin(&info.files, package_name, repo_name, explicit_bin)?
    };
    Ok(Inspected {
        sha256: info.sha256,
        extract: info.extract,
        strip_components: info.strip_components,
        bin,
        files: info.files,
    })
}

fn choose_bin(
    files: &[String],
    package_name: &str,
    repo_name: &str,
    explicit: Option<&str>,
) -> Result<BinSpec> {
    let available: Vec<&String> = files
        .iter()
        .filter(|file| file.to_ascii_lowercase().ends_with(".exe"))
        .collect();
    let chosen = if let Some(explicit) = explicit {
        let path = normalize_bin(explicit)?;
        available
            .iter()
            .find(|file| file.as_str() == path)
            .map(|file| (*file).clone())
            .with_context(|| {
                format!(
                    "'{path}' was not found in the archive; available executables: {}",
                    available
                        .iter()
                        .map(|file| file.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?
    } else if available.len() == 1 {
        available[0].clone()
    } else {
        let exact: Vec<&&String> = available
            .iter()
            .filter(|file| {
                let stem = std::path::Path::new(file)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                stem.eq_ignore_ascii_case(package_name) || stem.eq_ignore_ascii_case(repo_name)
            })
            .collect();
        if exact.len() == 1 {
            (*exact[0]).clone()
        } else {
            bail!(
                "archive contains multiple executables ({}); pass --bin <path>",
                available
                    .iter()
                    .map(|file| file.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    };
    let command = command_name(&chosen, package_name);
    Ok(BinSpec::Detailed {
        path: chosen,
        name: Some(command),
        args: Vec::new(),
    })
}

fn choose_asset<'a>(
    assets: &'a [Asset],
    requested: Option<&str>,
    package_name: &str,
    repo_name: &str,
) -> Result<&'a Asset> {
    if let Some(requested) = requested {
        let matches: Vec<&Asset> = assets
            .iter()
            .filter(|asset| {
                asset
                    .name
                    .to_ascii_lowercase()
                    .contains(&requested.to_ascii_lowercase())
            })
            .collect();
        return match matches.as_slice() {
            [asset] => Ok(asset),
            [] => bail!("no release asset matched '{requested}'"),
            _ => bail!(
                "multiple release assets matched '{requested}': {}",
                matches
                    .iter()
                    .map(|asset| asset.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
    }

    let mut ranked: Vec<(&Asset, i32)> = assets
        .iter()
        .filter_map(|asset| {
            let score = asset_score(&asset.name, package_name, repo_name)?;
            Some((asset, score))
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.name.cmp(&right.0.name))
    });
    let Some((best, best_score)) = ranked.first() else {
        let executables: Vec<&Asset> = assets
            .iter()
            .filter(|asset| {
                let lower = asset.name.to_ascii_lowercase();
                let is_32_bit = (lower.contains("x86")
                    && !lower.contains("x86_64")
                    && !lower.contains("x86-64"))
                    || lower.contains("i386")
                    || lower.contains("i686");
                lower.ends_with(".exe")
                    && !lower.contains("arm")
                    && !lower.contains("aarch64")
                    && !is_32_bit
            })
            .collect();
        if let [asset] = executables.as_slice() {
            return Ok(asset);
        }
        bail!("no Windows x64 release asset found; pass --asset <name>");
    };
    let best = *best;
    if ranked.get(1).is_some_and(|(_, score)| score == best_score) {
        bail!(
            "multiple Windows x64 release assets scored equally: {}; pass --asset <name>",
            ranked
                .iter()
                .filter(|(_, score)| score == best_score)
                .map(|(asset, _)| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(best)
}

fn asset_score(name: &str, package_name: &str, repo_name: &str) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".sha256")
        || lower.ends_with(".sha256sum")
        || lower.ends_with(".sig")
        || lower.ends_with(".asc")
        || lower.ends_with(".json")
        || lower.ends_with(".txt")
        || lower.ends_with(".deb")
        || lower.ends_with(".rpm")
        || lower.ends_with(".msi")
        || lower.ends_with(".msix")
        || lower.ends_with(".dmg")
        || lower.ends_with(".pkg")
        || lower.ends_with(".7z")
        || lower.contains("source")
        || lower.contains("installer")
        || lower.contains("setup")
        || lower.contains("-libs")
        || lower.contains("_libs")
    {
        return None;
    }
    let windows = !lower.contains("darwin")
        && (lower.contains("windows")
            || lower.contains("win64")
            || lower.contains("win32")
            || lower.contains("_win")
            || lower.contains("-win")
            || lower.contains("win-")
            || lower.contains("win_"));
    let x64 = lower.contains("x86_64")
        || lower.contains("amd64")
        || lower.contains("x64")
        || lower.contains("win64");
    if !windows || !x64 {
        return None;
    }
    if lower.contains("aarch64")
        || lower.contains("arm64")
        || lower.contains("armv")
        || lower.contains("i386")
        || lower.contains("i686")
        || (lower.contains("win32") && !x64)
    {
        return None;
    }
    let extension = if lower.ends_with(".zip") {
        6
    } else if lower.ends_with(".exe") {
        5
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        2
    } else {
        return None;
    };
    let mut score = 80 + extension;
    if lower.contains("msvc") {
        score += 8;
    } else if lower.contains("gnu") {
        score += 4;
    }
    if lower.contains("windows") && lower.contains("amd64") {
        score += 5;
    }
    let package_key = package_name.to_ascii_lowercase();
    let repo_key = repo_name.to_ascii_lowercase();
    let main_asset = lower == package_key
        || lower.starts_with(&format!("{package_key}-"))
        || lower.starts_with(&format!("{package_key}_"))
        || lower.starts_with(&format!("{package_key}."));
    if main_asset {
        score += 10;
    }
    if lower.contains(&package_key) || lower.contains(&repo_key) {
        score += 2;
    }
    if lower.contains("baseline") || lower.contains("profile") || lower.contains(".full.") {
        score -= 5;
    }
    Some(score)
}

fn archive_kind(name: &str) -> Result<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Ok("zip".to_string())
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Ok("tar.gz".to_string())
    } else if lower.ends_with(".tar") {
        Ok("tar".to_string())
    } else if lower.ends_with(".exe") {
        Ok("none".to_string())
    } else {
        bail!("unsupported release asset type: {name}")
    }
}

fn archive_files(path: &Path, kind: &str) -> Result<Vec<String>> {
    let files = match kind {
        "zip" => {
            let mut archive = zip::ZipArchive::new(File::open(path)?)?;
            let mut files = Vec::new();
            for index in 0..archive.len() {
                let entry = archive.by_index(index)?;
                if entry.is_dir() {
                    continue;
                }
                if let Some(name) = entry.enclosed_name() {
                    let name = name.to_string_lossy().replace('\\', "/");
                    if !name.starts_with("__MACOSX/") {
                        files.push(name);
                    }
                }
            }
            files
        }
        "tar.gz" => {
            let decoder = flate2::read::GzDecoder::new(File::open(path)?);
            tar_files(decoder)?
        }
        "tar" => tar_files(File::open(path)?)?,
        _ => bail!("cannot inspect {kind} archive"),
    };
    Ok(files)
}

fn tar_files<R: Read>(reader: R) -> Result<Vec<String>> {
    let mut archive = tar::Archive::new(reader);
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?;
        files.push(path.to_string_lossy().replace('\\', "/"));
    }
    Ok(files)
}

fn strip_root(files: &[String]) -> (bool, Vec<String>) {
    let Some(first) = files.first() else {
        return (false, Vec::new());
    };
    let Some(root) = first.split('/').next() else {
        return (false, files.to_vec());
    };
    if root.is_empty()
        || files
            .iter()
            .any(|file| !file.starts_with(&format!("{root}/")))
    {
        return (false, files.to_vec());
    }
    (
        true,
        files
            .iter()
            .map(|file| {
                file.split_once('/')
                    .map_or("", |(_, rest)| rest)
                    .to_string()
            })
            .collect(),
    )
}

fn font_entry_present(files: &[String], entry: &str) -> bool {
    let entry = entry.replace('\\', "/");
    let entry = entry.strip_prefix("./").unwrap_or(&entry);
    let entry = entry.trim_matches('/');
    if entry.is_empty() || entry == "." {
        return files.iter().any(|file| is_font_file_name(file));
    }
    if is_font_file_name(entry) {
        return files.iter().any(|file| file == entry);
    }
    let prefix = format!("{entry}/");
    files
        .iter()
        .any(|file| file.starts_with(&prefix) && is_font_file_name(file))
}

fn is_font_file_name(value: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
}

fn command_name(path: &str, package_name: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(package_name);
    let lower = stem.to_ascii_lowercase();
    let package = package_name.to_ascii_lowercase();
    if lower == package
        || lower
            .strip_prefix(&package)
            .is_some_and(|suffix| suffix.starts_with(['-', '_', '.']))
    {
        return package_name.to_string();
    }
    stem.to_string()
}

fn normalize_bin(value: &str) -> Result<String> {
    let value = value.replace('\\', "/");
    let value = value.trim_start_matches("./");
    if value.is_empty() || value.contains("..") || value.starts_with('/') {
        bail!("invalid --bin path '{value}'");
    }
    Ok(value.to_string())
}

fn sanitize_file_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_repository(value: &str) -> Result<(String, String)> {
    let value = value.trim().trim_end_matches('/');
    let path = if let Ok(url) = Url::parse(value) {
        if url.host_str() != Some("github.com") {
            bail!("expected a github.com repository, got {value}");
        }
        url.path()
            .trim_matches('/')
            .trim_end_matches(".git")
            .to_string()
    } else {
        value
            .trim_start_matches("github.com/")
            .trim_end_matches(".git")
            .to_string()
    };
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        bail!("expected owner/repo, got {value}");
    }
    let valid = |part: &str| {
        part.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    };
    if !valid(parts[0]) || !valid(parts[1]) {
        bail!("invalid GitHub repository name: {value}");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn github_release_source(value: &str) -> Option<(String, String)> {
    let url = Url::parse(value).ok()?;
    if url.host_str() != Some("github.com") {
        return None;
    }
    let parts: Vec<&str> = url.path().trim_matches('/').split('/').collect();
    if parts.len() >= 5 && parts[2] == "releases" && parts[3] == "download" {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn release_version(tag: &str) -> Result<Version> {
    let direct = tag.trim_start_matches(['v', 'V']);
    if let Some(version) = parse_semver_lenient(direct) {
        return Ok(version);
    }
    for candidate in tag.split(['-', '_', '@']).rev() {
        let candidate = candidate.trim_start_matches(['v', 'V']);
        if let Some(version) = parse_semver_lenient(candidate) {
            return Ok(version);
        }
    }
    bail!("release tag '{tag}' is not a semantic version")
}

fn parse_semver_lenient(value: &str) -> Option<Version> {
    if let Ok(version) = Version::parse(value) {
        return Some(version);
    }
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        return Version::parse(&format!("{value}.0")).ok();
    }
    None
}

fn github_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<T> {
    let mut request = client
        .get(url)
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.bearer_auth(token);
    }
    let response = request.send()?.error_for_status()?;
    let text = response.text()?;
    serde_json::from_str(&text).with_context(|| format!("parsing GitHub response from {url}"))
}

fn manifest_files(root: &Path) -> Result<Vec<PathBuf>> {
    let packages = if root.join("packages").is_dir() {
        root.join("packages")
    } else {
        root.to_path_buf()
    };
    let mut files = Vec::new();
    for entry in WalkDir::new(&packages).min_depth(1).max_depth(2) {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("toml")
        {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{
        asset_score, command_name, font_entry_present, parse_repository, read_sources,
        reconcile_updated_bins, release_version,
    };
    use crate::model::BinSpec;

    #[test]
    fn parses_github_repository_forms() {
        assert_eq!(
            parse_repository("BurntSushi/ripgrep").unwrap(),
            ("BurntSushi".to_string(), "ripgrep".to_string())
        );
        assert_eq!(
            parse_repository("https://github.com/jqlang/jq/").unwrap(),
            ("jqlang".to_string(), "jq".to_string())
        );
        assert_eq!(
            parse_repository("https://github.com/jqlang/jq.git").unwrap(),
            ("jqlang".to_string(), "jq".to_string())
        );
        assert!(parse_repository("https://example.com/a/b").is_err());
        assert!(parse_repository("owner").is_err());
    }

    #[test]
    fn strips_platform_suffixes_from_command_names() {
        assert_eq!(command_name("yq_windows_amd64.exe", "yq"), "yq");
        assert_eq!(command_name("rg.exe", "ripgrep"), "rg");
    }

    #[test]
    fn ranks_main_windows_assets() {
        assert!(
            asset_score(
                "duckdb-shared-libs-windows-amd64.tar.gz",
                "duckdb",
                "duckdb"
            )
            .is_none()
        );
        let deno = asset_score("deno-x86_64-pc-windows-msvc.zip", "deno", "deno").unwrap();
        let denort = asset_score("denort-x86_64-pc-windows-msvc.zip", "deno", "deno").unwrap();
        assert!(deno > denort);
        let bun = asset_score("bun-windows-x64.zip", "bun", "bun").unwrap();
        let baseline = asset_score("bun-windows-x64-baseline.zip", "bun", "bun").unwrap();
        assert!(bun > baseline);
        assert!(asset_score("bun-darwin-x64.zip", "bun", "bun").is_none());
    }

    #[test]
    fn checks_font_paths_in_release_archives() {
        let files = vec![
            "fonts/ttf/Demo-Regular.ttf".to_string(),
            "README.txt".to_string(),
        ];
        assert!(font_entry_present(&files, "fonts/ttf"));
        assert!(font_entry_present(&files, "fonts/ttf/Demo-Regular.ttf"));
        assert!(font_entry_present(&files, "."));
        assert!(!font_entry_present(&files, "fonts/otf"));
        assert!(!font_entry_present(&files, "Missing.ttf"));
    }

    #[test]
    fn reads_compact_and_detailed_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sources.toml");
        std::fs::write(
            &path,
            r#"
repositories = ["BurntSushi/ripgrep", "jqlang/jq"]

[[source]]
repository = "jqlang/jq"
asset = "jq-windows-amd64.exe"
"#,
        )
        .unwrap();
        let entries = read_sources(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].repository, "BurntSushi/ripgrep");
        assert_eq!(entries[1].repository, "jqlang/jq");
        assert_eq!(entries[1].asset.as_deref(), Some("jq-windows-amd64.exe"));
    }

    #[test]
    fn accepts_common_release_tag_forms() {
        assert_eq!(release_version("v0.26.1").unwrap().to_string(), "0.26.1");
        assert_eq!(release_version("jq-1.8.2").unwrap().to_string(), "1.8.2");
        assert_eq!(release_version("15.2.0").unwrap().to_string(), "15.2.0");
        assert_eq!(release_version("3.13").unwrap().to_string(), "3.13.0");
        assert!(release_version("latest").is_err());
        assert!(release_version("2026-11-02").is_err());
    }

    #[test]
    fn reconciles_versioned_binary_names_on_update() {
        let bins = vec![BinSpec::Detailed {
            path: "restic_0.19.1_windows_amd64.exe".to_string(),
            name: Some("restic".to_string()),
            args: vec!["--quiet".to_string()],
        }];
        let files = vec!["restic_0.19.2_windows_amd64.exe".to_string()];
        let updated = reconcile_updated_bins(&bins, "0.19.1", "0.19.2", &files).unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].path(), "restic_0.19.2_windows_amd64.exe");
        assert_eq!(updated[0].command_name(), "restic");
        assert_eq!(updated[0].args(), vec!["--quiet".to_string()]);

        let unrelated = vec!["other.exe".to_string()];
        let err = reconcile_updated_bins(&bins, "0.19.1", "0.19.2", &unrelated).unwrap_err();
        assert_eq!(err, vec!["restic_0.19.1_windows_amd64.exe".to_string()]);
    }
}
