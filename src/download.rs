use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn http_client() -> Result<Client> {
    init_crypto();
    Client::builder()
        .user_agent(concat!("rivet/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .context("building HTTP client")
}

pub fn is_http(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn is_file_url(value: &str) -> bool {
    value.to_ascii_lowercase().starts_with("file://")
}

pub fn is_remote(value: &str) -> bool {
    is_http(value) || is_file_url(value)
}

pub fn local_path(value: &str) -> Result<PathBuf> {
    if is_file_url(value) {
        return Url::parse(value)?
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file URL: {value}"));
    }
    Ok(PathBuf::from(value))
}

pub fn get_text(location: &str) -> Result<String> {
    if is_http(location) {
        let response = http_client()?.get(location).send()?.error_for_status()?;
        Ok(response.text()?)
    } else {
        let path = local_path(location)?;
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }
}

pub fn get_bytes(location: &str) -> Result<Vec<u8>> {
    if is_http(location) {
        let response = http_client()?.get(location).send()?.error_for_status()?;
        Ok(response.bytes()?.to_vec())
    } else {
        let path = local_path(location)?;
        fs::read(&path).with_context(|| format!("reading {}", path.display()))
    }
}

pub fn try_get_text(location: &str) -> Result<Option<String>> {
    if is_http(location) {
        let response = http_client()?.get(location).send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        Ok(Some(response.text()?))
    } else {
        let path = local_path(location)?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
        ))
    }
}

pub fn resolve_artifact_url(repo_location: &str, artifact: &str) -> Result<String> {
    if is_remote(artifact) || Path::new(artifact).is_absolute() {
        return Ok(artifact.to_string());
    }
    if is_http(repo_location) {
        let base = Url::parse(repo_location)?;
        return Ok(base.join(artifact)?.to_string());
    }
    let location = local_path(repo_location)?;
    let base = if location.is_dir() {
        location
    } else {
        location
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    Ok(base.join(artifact).to_string_lossy().to_string())
}

pub fn download_to(url: &str, destination: &Path) -> Result<u64> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if is_http(url) {
        let mut response = http_client()?.get(url).send()?.error_for_status()?;
        let mut file = File::create(destination)?;
        let bytes = io::copy(&mut response, &mut file)?;
        file.flush()?;
        Ok(bytes)
    } else {
        let source = local_path(url)?;
        fs::copy(&source, destination)
            .with_context(|| format!("copying {} to {}", source.display(), destination.display()))
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn cache_blob(cache_dir: &Path, url: &str, expected_sha256: &str) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)?;
    let destination = cache_dir.join(expected_sha256.to_ascii_lowercase());
    if destination.exists() {
        let actual = sha256_file(&destination)?;
        if actual.eq_ignore_ascii_case(expected_sha256) {
            return Ok(destination);
        }
        fs::remove_file(&destination)
            .with_context(|| format!("removing corrupted cached blob {}", destination.display()))?;
    }

    let part = cache_dir.join(format!(
        "{}.{}.part",
        expected_sha256.to_ascii_lowercase(),
        std::process::id()
    ));
    let _ = fs::remove_file(&part);
    download_to(url, &part)?;
    let actual = sha256_file(&part)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(&part);
        bail!("sha256 mismatch: expected {expected_sha256}, got {actual}");
    }
    fs::rename(&part, &destination)?;
    Ok(destination)
}
