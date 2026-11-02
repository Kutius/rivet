use crate::model::{INDEX_SCHEMA, IndexFile, PackageManifest};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn build_index(directory: &Path, output: Option<&Path>) -> Result<IndexFile> {
    let packages_dir = if directory.join("packages").is_dir() {
        directory.join("packages")
    } else {
        directory.to_path_buf()
    };
    if !packages_dir.is_dir() {
        bail!("packages directory not found: {}", packages_dir.display());
    }

    let mut packages = Vec::new();
    for entry in WalkDir::new(&packages_dir).min_depth(1).max_depth(2) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let manifest: PackageManifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        manifest
            .validate()
            .with_context(|| format!("validating {}", path.display()))?;
        packages.push(manifest);
    }
    if packages.is_empty() {
        bail!("no packages found in {}", packages_dir.display());
    }
    packages.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| crate::model::compare_manifests(a, b))
    });

    for pair in packages.windows(2) {
        if pair[0].name == pair[1].name && pair[0].version == pair[1].version {
            bail!("duplicate package {}@{}", pair[0].name, pair[0].version);
        }
    }

    let index = IndexFile {
        schema: INDEX_SCHEMA,
        generated_at: chrono::Utc::now().to_rfc3339(),
        packages,
    };
    let output = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| directory.join("index.json"));
    let json = serde_json::to_string_pretty(&index)?;
    fs::write(&output, format!("{json}\n"))
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(index)
}

pub fn parse_index(text: &str, source: &str) -> Result<IndexFile> {
    let index: IndexFile =
        serde_json::from_str(text).with_context(|| format!("parsing index from {source}"))?;
    if index.schema != INDEX_SCHEMA {
        bail!("unsupported index schema {} in {source}", index.schema);
    }
    for package in &index.packages {
        package.validate().with_context(|| {
            format!(
                "validating {}/{} from {source}",
                package.name, package.version
            )
        })?;
    }
    Ok(index)
}

pub fn load_index(location: &str) -> Result<IndexFile> {
    if crate::download::is_http(location) {
        let text = crate::download::get_text(location)?;
        parse_index(&text, location)
    } else {
        let path = crate::download::local_path(location)?;
        if path.is_dir() {
            let index_path = path.join("index.json");
            if index_path.exists() {
                return load_index(&index_path.to_string_lossy());
            }
            return build_index(&path, Some(&index_path));
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        parse_index(&text, &path.display().to_string())
    }
}

pub fn index_location_from_dir(directory: &Path) -> PathBuf {
    directory.join("index.json")
}
