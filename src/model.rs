use anyhow::{Result, bail};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const INDEX_SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexFile {
    pub schema: u32,
    pub generated_at: String,
    #[serde(default)]
    pub packages: Vec<PackageManifest>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub arch: Option<String>,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub extract: Option<String>,
    #[serde(default)]
    pub strip_components: Option<usize>,
    #[serde(default)]
    pub bin: Vec<BinSpec>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub persist: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum BinSpec {
    Simple(String),
    Detailed {
        path: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl BinSpec {
    pub fn path(&self) -> &str {
        match self {
            Self::Simple(path) => path,
            Self::Detailed { path, .. } => path,
        }
    }

    pub fn command_name(&self) -> String {
        match self {
            Self::Simple(path) => stem(path),
            Self::Detailed { path, name, .. } => name.clone().unwrap_or_else(|| stem(path)),
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Simple(_) => Vec::new(),
            Self::Detailed { args, .. } => args.clone(),
        }
    }
}

fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn is_valid_command_name(name: &str) -> bool {
    if name.is_empty() || name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
    }) {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or("");
    !matches!(
        stem,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

impl PackageManifest {
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || !self.name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
            })
        {
            bail!("invalid package name '{}'", self.name);
        }
        if self.name.starts_with('.') || self.name.contains("..") {
            bail!("invalid package name '{}'", self.name);
        }
        Version::parse(&self.version)
            .map_err(|e| anyhow::anyhow!("invalid semver '{}': {e}", self.version))?;
        if self.url.trim().is_empty() {
            bail!("package '{}' has an empty url", self.name);
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("package '{}' has an invalid sha256", self.name);
        }
        if self.bin.is_empty() {
            bail!("package '{}' has no bin entries", self.name);
        }
        for bin in &self.bin {
            let bin_path = std::path::Path::new(bin.path());
            if bin.path().trim().is_empty()
                || bin.path().contains("..")
                || bin_path.is_absolute()
                || bin_path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                bail!("package '{}' has an invalid bin path", self.name);
            }
            let command = bin.command_name();
            if !is_valid_command_name(&command) {
                bail!(
                    "package '{}' has an invalid bin name '{}'",
                    self.name,
                    command
                );
            }
        }
        for persist in &self.persist {
            if persist.trim().is_empty()
                || persist.contains("..")
                || std::path::Path::new(persist).is_absolute()
            {
                bail!(
                    "package '{}' has an invalid persist path '{}'",
                    self.name,
                    persist
                );
            }
        }
        Ok(())
    }

    pub fn binary_names(&self) -> Vec<String> {
        self.bin.iter().map(BinSpec::command_name).collect()
    }

    pub fn version_obj(&self) -> Result<Version> {
        Ok(Version::parse(&self.version)?)
    }
}

pub fn compare_manifests(a: &PackageManifest, b: &PackageManifest) -> Ordering {
    match (a.version_obj(), b.version_obj()) {
        (Ok(av), Ok(bv)) => av.cmp(&bv),
        _ => a.version.cmp(&b.version),
    }
}

pub fn latest_manifest(mut manifests: Vec<PackageManifest>) -> Option<PackageManifest> {
    manifests.sort_by(compare_manifests);
    manifests.pop()
}
