use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use url::Url;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static PART_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn http_client() -> Result<Client> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client.clone());
    }
    init_crypto();
    let client = Client::builder()
        .user_agent(concat!("rivet/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .context("building HTTP client")?;
    let _ = HTTP_CLIENT.set(client.clone());
    Ok(client)
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
    download_to_with_progress(url, destination, &mut |_, _| {})
}

pub fn download_to_with_progress(
    url: &str,
    destination: &Path,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if is_http(url) {
        let mut response = http_client()?.get(url).send()?.error_for_status()?;
        let total = response.content_length();
        let mut file = File::create(destination)?;
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded = 0u64;
        on_progress(0, total);
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            downloaded += read as u64;
            on_progress(downloaded, total);
        }
        file.flush()?;
        Ok(downloaded)
    } else {
        let source = local_path(url)?;
        let bytes = fs::copy(&source, destination).with_context(|| {
            format!("copying {} to {}", source.display(), destination.display())
        })?;
        on_progress(bytes, Some(bytes));
        Ok(bytes)
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
    cache_blob_with_progress(cache_dir, url, expected_sha256, &mut |_, _| {})
}

pub fn cache_blob_with_progress(
    cache_dir: &Path,
    url: &str,
    expected_sha256: &str,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
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

    let seq = PART_COUNTER.fetch_add(1, Ordering::Relaxed);
    let part = cache_dir.join(format!(
        "{}.{}.{}.part",
        expected_sha256.to_ascii_lowercase(),
        std::process::id(),
        seq
    ));
    let _ = fs::remove_file(&part);
    download_to_with_progress(url, &part, on_progress)?;
    let actual = sha256_file(&part)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(&part);
        bail!("sha256 mismatch: expected {expected_sha256}, got {actual}");
    }
    if let Err(error) = fs::rename(&part, &destination) {
        if destination.exists() {
            let _ = fs::remove_file(&part);
        } else {
            return Err(error.into());
        }
    }
    Ok(destination)
}

/// Download and verify multiple blobs into `cache_dir` using up to
/// `concurrency` worker threads. Deduplicates by SHA-256 and skips blobs that
/// are already cached and valid.
pub fn prefetch_blobs(
    cache_dir: &Path,
    items: &[(String, String)],
    concurrency: usize,
    verbose: bool,
) -> Result<()> {
    let mut seen = HashSet::new();
    let unique: Vec<&(String, String)> = items
        .iter()
        .filter(|(_, sha)| seen.insert(sha.to_ascii_lowercase()))
        .collect();
    if unique.len() <= 1 || concurrency <= 1 {
        return Ok(());
    }
    let total_items = unique.len();
    let workers = concurrency.min(total_items);
    if verbose {
        eprintln!("verbose: prefetching {total_items} artifact(s) with concurrency={workers}");
    }
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let total_bytes = AtomicU64::new(0);
    let first_error: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let progress = Mutex::new(crate::ui::ProgressBar::new(format!(
        "Downloading 0/{total_items} packages"
    )));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if first_error.lock().unwrap().is_some() {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((url, sha256)) = unique.get(index) else {
                        break;
                    };
                    let mut last_bytes = 0u64;
                    let mut on_progress = |downloaded: u64, _total: Option<u64>| {
                        let delta = downloaded.saturating_sub(last_bytes);
                        last_bytes = downloaded;
                        if delta > 0 {
                            let bytes = total_bytes.fetch_add(delta, Ordering::Relaxed) + delta;
                            if let Ok(mut bar) = progress.try_lock() {
                                bar.update(bytes, None);
                            }
                        }
                    };
                    match cache_blob_with_progress(cache_dir, url, sha256, &mut on_progress)
                        .with_context(|| format!("downloading {url}"))
                    {
                        Ok(_) => {
                            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                            let bytes = total_bytes.load(Ordering::Relaxed);
                            if let Ok(mut bar) = progress.lock() {
                                bar.set_label(format!("Downloading {done}/{total_items} packages"));
                                bar.update(bytes, None);
                            }
                        }
                        Err(error) => {
                            let mut slot = first_error.lock().unwrap();
                            if slot.is_none() {
                                *slot = Some(error);
                            }
                            break;
                        }
                    }
                }
            });
        }
    });
    progress.into_inner().unwrap().clear();
    if let Some(error) = first_error.into_inner().unwrap() {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{prefetch_blobs, sha256_file};
    use std::fs;

    #[test]
    fn prefetch_blobs_caches_multiple_artifacts_concurrently() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let cache = dir.path().join("cache");
        fs::create_dir_all(&src).unwrap();

        let mut items = Vec::new();
        for i in 0..4 {
            let file = src.join(format!("pkg-{i}.bin"));
            fs::write(&file, format!("payload-{i}")).unwrap();
            let hash = sha256_file(&file).unwrap();
            items.push((file.to_string_lossy().to_string(), hash));
        }

        prefetch_blobs(&cache, &items, 4, false).unwrap();
        for (_, hash) in &items {
            assert!(cache.join(hash).is_file(), "cached blob {hash} missing");
        }
    }
}
