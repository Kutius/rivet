use crate::download;
use crate::index;
use crate::model::{BinSpec, PackageManifest};
use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;
use walkdir::WalkDir;

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub name: Option<String>,
    pub asset: Option<String>,
    pub bin: Option<String>,
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
    };
    let path = write_manifest_from_asset(root, &draft, asset, &repo, options.bin.as_deref())?;
    index::build_index(root, None)?;
    Ok(path)
}

pub fn update_github_packages(
    root: &Path,
    check: bool,
    only: Option<&str>,
) -> Result<Vec<(String, String, String)>> {
    let client = download::http_client()?;
    let mut changed = Vec::new();
    let mut errors = Vec::new();

    for path in manifest_files(root)? {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut manifest: PackageManifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if only.is_some_and(|name| manifest.name != name) {
            continue;
        }
        let Some((owner, repo)) = github_release_source(&manifest.url) else {
            continue;
        };
        let release: Release = match github_json(
            &client,
            &format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
        ) {
            Ok(release) => release,
            Err(error) => {
                errors.push(format!("{}: {error:#}", manifest.name));
                continue;
            }
        };
        let version = match release_version(&release.tag_name) {
            Ok(version) => version,
            Err(error) => {
                errors.push(format!("{}: {error:#}", manifest.name));
                continue;
            }
        };
        let current = match Version::parse(&manifest.version) {
            Ok(version) => version,
            Err(error) => {
                errors.push(format!(
                    "{}: invalid current version: {error}",
                    manifest.name
                ));
                continue;
            }
        };
        if version <= current {
            println!("{} {}: up to date", manifest.name, manifest.version);
            continue;
        }

        let asset = match choose_asset(&release.assets, None, &manifest.name, &repo) {
            Ok(asset) => asset,
            Err(error) => {
                errors.push(format!("{}: {error:#}", manifest.name));
                continue;
            }
        };
        if check {
            println!(
                "{}: {} -> {} ({})",
                manifest.name, manifest.version, version, asset.name
            );
            changed.push((
                manifest.name.clone(),
                manifest.version.clone(),
                version.to_string(),
            ));
            continue;
        }

        let guessed = match download_and_inspect(asset, &manifest.name, &repo, None) {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("{}: {error:#}", manifest.name));
                continue;
            }
        };
        let mut layout_changed = false;
        if guessed.extract != "none" {
            for bin in &manifest.bin {
                if !guessed.files.iter().any(|file| file == bin.path()) {
                    errors.push(format!(
                        "{}: new asset no longer contains '{}'; update the manifest manually",
                        manifest.name,
                        bin.path()
                    ));
                    layout_changed = true;
                }
            }
        }
        if layout_changed {
            continue;
        }
        manifest.version = version.to_string();
        manifest.url = asset.browser_download_url.clone();
        manifest.sha256 = guessed.sha256.clone();
        manifest.extract = Some(guessed.extract.clone());
        manifest.strip_components =
            (guessed.strip_components > 0).then_some(guessed.strip_components);
        if manifest.bin.len() == 1 && options_bin_can_change(&manifest.bin[0], &guessed) {
            manifest.bin = vec![guessed.bin];
        }
        let text = toml::to_string_pretty(&manifest)?;
        fs::write(&path, format!("{text}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
        println!(
            "{}: {} -> {} ({})",
            manifest.name, current, version, asset.name
        );
        changed.push((
            manifest.name.clone(),
            current.to_string(),
            version.to_string(),
        ));
    }

    if !check && !changed.is_empty() {
        index::build_index(root, None)?;
    }
    if !errors.is_empty() {
        bail!(
            "catalog update failed for {} package(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        );
    }
    Ok(changed)
}

fn options_bin_can_change(current: &BinSpec, guessed: &Inspected) -> bool {
    current.path() == guessed.bin.path() && guessed.extract == "none"
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
        url: asset.browser_download_url.clone(),
        sha256: guessed.sha256,
        extract: Some(guessed.extract),
        strip_components: (guessed.strip_components > 0).then_some(guessed.strip_components),
        bin: vec![guessed.bin],
        depends: Vec::new(),
        persist: Vec::new(),
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
            "{} already exists; use 'rivet repo update' or edit it manually",
            path.display()
        );
    }
    let text = toml::to_string_pretty(&manifest)?;
    fs::write(&path, format!("{text}\n")).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn download_and_inspect(
    asset: &Asset,
    package_name: &str,
    repo_name: &str,
    explicit_bin: Option<&str>,
) -> Result<Inspected> {
    let temp = std::env::temp_dir().join(format!(
        "rivet-repo-{}-{}",
        std::process::id(),
        sanitize_file_name(&asset.name)
    ));
    let _ = fs::remove_file(&temp);
    let result = (|| -> Result<Inspected> {
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
        let files = stripped.1;
        let bin = if kind == "none" {
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
            choose_bin(&files, package_name, repo_name, explicit_bin)?
        };
        Ok(Inspected {
            sha256,
            extract: kind,
            strip_components,
            bin,
            files,
        })
    })();
    let _ = fs::remove_file(&temp);
    result
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
    {
        return None;
    }
    let windows = lower.contains("windows")
        || lower.contains("win64")
        || lower.contains("win32")
        || lower.contains("_win")
        || lower.contains("-win");
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
        || lower.contains("win32")
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
    if lower.contains(&package_key) || lower.contains(&repo_key) {
        score += 2;
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
    if let Ok(version) = Version::parse(direct) {
        return Ok(version);
    }
    for candidate in tag.split(['-', '_']).rev() {
        let candidate = candidate.trim_start_matches(['v', 'V']);
        if let Ok(version) = Version::parse(candidate) {
            return Ok(version);
        }
    }
    bail!("release tag '{tag}' is not a semantic version")
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
    use super::{command_name, parse_repository, release_version};

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
    fn accepts_common_release_tag_forms() {
        assert_eq!(release_version("v0.26.1").unwrap().to_string(), "0.26.1");
        assert_eq!(release_version("jq-1.8.2").unwrap().to_string(), "1.8.2");
        assert_eq!(release_version("15.2.0").unwrap().to_string(), "15.2.0");
        assert!(release_version("latest").is_err());
    }
}
