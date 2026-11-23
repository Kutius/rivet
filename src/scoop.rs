use crate::index;
use crate::model::{BinSpec, PackageManifest};
use anyhow::{Context, Result, bail};
use semver::Version;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct ImportReport {
    pub imported: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

/// Convert a single Scoop JSON manifest into a strict, portable-only Rivet
/// `PackageManifest`. Rejects manifests that rely on installer scripts,
/// PowerShell hooks, environment mutations, or unsupported archive formats.
pub fn convert_manifest(name: &str, text: &str) -> Result<PackageManifest> {
    let value: Value =
        serde_json::from_str(text).with_context(|| format!("parsing Scoop manifest '{name}'"))?;
    let object = value
        .as_object()
        .with_context(|| format!("Scoop manifest '{name}' is not a JSON object"))?;

    for forbidden in [
        "pre_install",
        "post_install",
        "pre_uninstall",
        "post_uninstall",
        "installer",
        "uninstaller",
        "innosetup",
        "env_add_path",
        "env_set",
        "psmodule",
    ] {
        if object.contains_key(forbidden) {
            bail!("uses unsupported Scoop hook/field '{forbidden}'");
        }
    }

    let raw_version = object
        .get("version")
        .and_then(Value::as_str)
        .context("missing 'version'")?;
    let version = parse_version_lenient(raw_version)
        .with_context(|| format!("version '{raw_version}' is not semver-compatible"))?;

    let arch64 = object
        .get("architecture")
        .and_then(|v| v.get("64bit"))
        .and_then(Value::as_object);

    if let Some(arch) = arch64 {
        for forbidden in ["pre_install", "post_install", "installer", "uninstaller"] {
            if arch.contains_key(forbidden) {
                bail!("64bit section uses unsupported Scoop hook/field '{forbidden}'");
            }
        }
    }

    let url = pick_single_string(arch64, object, "url")
        .context("missing single string 'url' (multi-URL manifests are not supported)")?;
    let raw_hash =
        pick_single_string(arch64, object, "hash").context("missing single string 'hash'")?;
    let sha256 = normalize_sha256(&raw_hash)?;

    let extract = infer_supported_extract(&url)?;

    let extract_dir = pick_single_string(arch64, object, "extract_dir")
        .map(|value| value.replace('\\', "/").trim_matches('/').to_string())
        .filter(|value| !value.is_empty());

    let bin_value = arch64
        .and_then(|arch| arch.get("bin"))
        .or_else(|| object.get("bin"))
        .context("missing 'bin' entries")?;
    let bin = parse_bins(bin_value, extract_dir.as_deref())?;
    if bin.is_empty() {
        bail!("no usable 'bin' entries");
    }

    let persist = match object.get("persist") {
        Some(Value::String(path)) => vec![normalize_rel(path)?],
        Some(Value::Array(items)) => {
            let mut result = Vec::new();
            for item in items {
                match item {
                    Value::String(path) => result.push(normalize_rel(path)?),
                    Value::Array(pair) => {
                        if let Some(path) = pair.first().and_then(Value::as_str) {
                            result.push(normalize_rel(path)?);
                        }
                    }
                    _ => {}
                }
            }
            result
        }
        _ => Vec::new(),
    };

    let description = object
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let homepage = object
        .get("homepage")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let license = match object.get("license") {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Object(map)) => map
            .get("identifier")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    };

    let manifest = PackageManifest {
        name: name.to_ascii_lowercase(),
        version: version.to_string(),
        description,
        homepage,
        license,
        tags: Vec::new(),
        arch: Some("x64".to_string()),
        asset: None,
        url,
        sha256,
        extract: Some(extract),
        strip_components: None,
        bin,
        fonts: Vec::new(),
        depends: Vec::new(),
        persist,
        test_args: Vec::new(),
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Import one Scoop `.json` manifest or a directory of Scoop manifests into
/// `<root>/packages/`, skipping packages that already exist or fail strict
/// portable-only filtering.
pub fn import_from_path(
    root: &Path,
    source: &Path,
    dry_run: bool,
    limit: Option<usize>,
) -> Result<ImportReport> {
    let packages_dir = if root
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("packages"))
    {
        root.to_path_buf()
    } else {
        root.join("packages")
    };
    fs::create_dir_all(&packages_dir)?;

    let mut existing = HashSet::new();
    for entry in fs::read_dir(&packages_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.path().extension().and_then(|v| v.to_str()) == Some("toml")
            && let Ok(text) = fs::read_to_string(entry.path())
            && let Ok(manifest) = toml::from_str::<PackageManifest>(&text)
        {
            existing.insert(manifest.name.to_ascii_lowercase());
        }
    }

    let mut json_files: Vec<PathBuf> = if source.is_file() {
        vec![source.to_path_buf()]
    } else if source.is_dir() {
        let bucket = if source.join("bucket").is_dir() {
            source.join("bucket")
        } else {
            source.to_path_buf()
        };
        let mut files = Vec::new();
        for entry in fs::read_dir(&bucket)
            .with_context(|| format!("reading Scoop directory {}", bucket.display()))?
        {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().and_then(|v| v.to_str()) == Some("json")
            {
                files.push(entry.path());
            }
        }
        files.sort();
        files
    } else {
        bail!("Scoop source not found: {}", source.display());
    };

    let mut report = ImportReport::default();
    for file in json_files.drain(..) {
        if limit.is_some_and(|max| report.imported.len() >= max) {
            break;
        }
        let Some(name) = file.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        let name_lower = name.to_ascii_lowercase();
        if existing.contains(&name_lower) {
            report
                .skipped
                .push((name_lower, "already in catalog".to_string()));
            continue;
        }
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) => {
                report.skipped.push((name_lower, format!("{error}")));
                continue;
            }
        };
        let manifest = match convert_manifest(&name_lower, &text) {
            Ok(manifest) => manifest,
            Err(error) => {
                report.skipped.push((name_lower, format!("{error:#}")));
                continue;
            }
        };
        if dry_run {
            println!("would import {} {}", manifest.name, manifest.version);
        } else {
            let dest = packages_dir.join(format!("{}.toml", manifest.name));
            let rendered = toml::to_string_pretty(&manifest)?;
            fs::write(&dest, format!("{rendered}\n"))
                .with_context(|| format!("writing {}", dest.display()))?;
            println!(
                "imported {} {} -> {}",
                manifest.name,
                manifest.version,
                dest.display()
            );
        }
        existing.insert(manifest.name.clone());
        report.imported.push(manifest.name);
    }

    if !dry_run && !report.imported.is_empty() {
        index::build_index(root, None)?;
    }
    Ok(report)
}

fn pick_single_string(
    arch: Option<&serde_json::Map<String, Value>>,
    root: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<String> {
    let value = arch.and_then(|m| m.get(key)).or_else(|| root.get(key))?;
    value.as_str().map(ToString::to_string)
}

fn normalize_sha256(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if let Some((alg, hash)) = trimmed.split_once(':') {
        if !alg.eq_ignore_ascii_case("sha256") {
            bail!("unsupported hash algorithm '{alg}' (only sha256 is accepted)");
        }
        return normalize_sha256(hash);
    }
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid sha256 hash '{trimmed}'");
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn infer_supported_extract(url: &str) -> Result<String> {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let lower = clean.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Ok("zip".to_string())
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Ok("tar.gz".to_string())
    } else if lower.ends_with(".tar") {
        Ok("tar".to_string())
    } else if lower.ends_with(".exe") {
        Ok("none".to_string())
    } else {
        bail!("unsupported archive format in url '{url}'")
    }
}

fn parse_version_lenient(raw: &str) -> Result<Version> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    if let Ok(v) = Version::parse(trimmed) {
        return Ok(v);
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() == 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        return Ok(Version::parse(&format!("{trimmed}.0"))?);
    }
    bail!("cannot parse '{raw}' as semver")
}

fn normalize_rel(path: &str) -> Result<String> {
    let clean = path.replace('\\', "/");
    let clean = clean.trim_start_matches("./").trim_matches('/');
    if clean.is_empty() || clean.contains("..") {
        bail!("invalid relative path '{path}'");
    }
    Ok(clean.to_string())
}

fn join_extract_dir(path: &str, extract_dir: Option<&str>) -> Result<String> {
    let rel = normalize_rel(path)?;
    Ok(match extract_dir {
        Some(dir) => format!("{dir}/{rel}"),
        None => rel,
    })
}

fn parse_bins(value: &Value, extract_dir: Option<&str>) -> Result<Vec<BinSpec>> {
    match value {
        Value::String(path) => Ok(vec![BinSpec::Simple(join_extract_dir(path, extract_dir)?)]),
        Value::Array(items) => {
            let mut result = Vec::new();
            for item in items {
                match item {
                    Value::String(path) => {
                        result.push(BinSpec::Simple(join_extract_dir(path, extract_dir)?));
                    }
                    Value::Array(tuple) => {
                        let Some(path) = tuple.first().and_then(Value::as_str) else {
                            continue;
                        };
                        let name = tuple
                            .get(1)
                            .and_then(Value::as_str)
                            .map(ToString::to_string);
                        let args = tuple
                            .get(2)
                            .and_then(Value::as_str)
                            .map(|s| s.split_whitespace().map(ToString::to_string).collect())
                            .unwrap_or_default();
                        result.push(BinSpec::Detailed {
                            path: join_extract_dir(path, extract_dir)?,
                            name,
                            args,
                        });
                    }
                    _ => {}
                }
            }
            Ok(result)
        }
        _ => bail!("invalid 'bin' field"),
    }
}

#[cfg(test)]
mod tests {
    use super::convert_manifest;

    #[test]
    fn converts_portable_64bit_scoop_manifest() {
        let json = r#"{
            "version": "1.2.3",
            "description": "Fast tool",
            "homepage": "https://example.com",
            "license": "MIT",
            "architecture": {
                "64bit": {
                    "url": "https://example.com/tool-1.2.3-x64.zip",
                    "hash": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    "extract_dir": "tool-1.2.3"
                }
            },
            "bin": [["bin\\tool.exe", "tl", "--color"]],
            "persist": "config"
        }"#;
        let manifest = convert_manifest("tool", json).unwrap();
        assert_eq!(manifest.name, "tool");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.extract.as_deref(), Some("zip"));
        assert_eq!(manifest.bin.len(), 1);
        assert_eq!(manifest.bin[0].path(), "tool-1.2.3/bin/tool.exe");
        assert_eq!(manifest.bin[0].command_name(), "tl");
        assert_eq!(manifest.bin[0].args(), vec!["--color".to_string()]);
        assert_eq!(manifest.persist, vec!["config".to_string()]);
    }

    #[test]
    fn rejects_scoop_manifests_with_powershell_hooks_or_non_sha256() {
        let hooked = r#"{
            "version": "1.0.0",
            "url": "https://example.com/a.zip",
            "hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "bin": "a.exe",
            "post_install": ["Write-Host 'hi'"]
        }"#;
        assert!(convert_manifest("hooked", hooked).is_err());

        let md5 = r#"{
            "version": "1.0.0",
            "url": "https://example.com/a.zip",
            "hash": "md5:d41d8cd98f00b204e9800998ecf8427e",
            "bin": "a.exe"
        }"#;
        assert!(convert_manifest("md5pkg", md5).is_err());
    }
}
