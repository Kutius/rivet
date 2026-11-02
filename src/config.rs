use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub main_index: String,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_keep_versions")]
    pub keep_versions: usize,
}

fn default_concurrency() -> usize {
    4
}

fn default_keep_versions() -> usize {
    1
}

impl Default for Config {
    fn default() -> Self {
        Self {
            main_index: String::new(),
            concurrency: default_concurrency(),
            keep_versions: default_keep_versions(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub store: PathBuf,
    pub apps: PathBuf,
    pub persist: PathBuf,
    pub cache: PathBuf,
    pub taps: PathBuf,
    pub config_file: PathBuf,
    pub db_file: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let root = match std::env::var_os("RIVET_ROOT") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => dirs::data_local_dir()
                .context("could not determine LOCALAPPDATA")?
                .join("Rivet"),
        };
        Ok(Self::from_root(root))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            bin: root.join("bin"),
            store: root.join("store"),
            apps: root.join("apps"),
            persist: root.join("persist"),
            cache: root.join("cache").join("blobs"),
            taps: root.join("taps"),
            config_file: root.join("config.toml"),
            db_file: root.join("state.db"),
            root,
        }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            &self.root,
            &self.bin,
            &self.store,
            &self.apps,
            &self.persist,
            &self.cache,
            &self.taps,
        ] {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(())
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        paths.ensure_layout()?;
        let mut config = if !paths.config_file.exists() {
            let config = Config::default();
            config.save(paths)?;
            config
        } else {
            let text = fs::read_to_string(&paths.config_file)
                .with_context(|| format!("reading {}", paths.config_file.display()))?;
            toml::from_str(&text)
                .with_context(|| format!("parsing {}", paths.config_file.display()))?
        };
        if let Ok(main_index) = std::env::var("RIVET_MAIN_INDEX") {
            if !main_index.trim().is_empty() {
                config.main_index = main_index;
            }
        }
        if config.concurrency == 0 {
            config.concurrency = default_concurrency();
        }
        Ok(config)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure_layout()?;
        let text = toml::to_string_pretty(self)?;
        fs::write(&paths.config_file, text)
            .with_context(|| format!("writing {}", paths.config_file.display()))?;
        Ok(())
    }

    pub fn main_index_location(&self) -> Option<String> {
        let value = self.main_index.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }
}

pub fn resolve_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}
