use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub fn extract(
    blob: &Path,
    destination: &Path,
    kind: Option<&str>,
    strip_components: usize,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    let kind = kind
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| infer_kind(blob));
    match kind.as_str() {
        "zip" => extract_zip(blob, destination, strip_components),
        "tar.gz" | "tgz" | "targz" => extract_tar_gz(blob, destination, strip_components),
        "tar" => extract_tar(blob, destination, strip_components),
        "7z" => extract_7z(blob, destination),
        "none" | "raw" => copy_raw(blob, destination),
        other => bail!("unsupported archive type '{other}'"),
    }
}

fn infer_kind(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        "tar.gz".to_string()
    } else if name.ends_with(".tar") {
        "tar".to_string()
    } else if name.ends_with(".zip") {
        "zip".to_string()
    } else if name.ends_with(".7z") {
        "7z".to_string()
    } else {
        "none".to_string()
    }
}

fn safe_relative(path: &Path, strip_components: usize) -> Option<PathBuf> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if components.len() <= strip_components {
        return None;
    }
    let mut result = PathBuf::new();
    for component in components.into_iter().skip(strip_components) {
        result.push(component);
    }
    Some(result)
}

fn extract_zip(blob: &Path, destination: &Path, strip_components: usize) -> Result<()> {
    let file = File::open(blob)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(relative) = safe_relative(&enclosed, strip_components) else {
            continue;
        };
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&target)?;
            io::copy(&mut entry, &mut output)?;
        }
    }
    Ok(())
}

fn extract_tar_gz(blob: &Path, destination: &Path, strip_components: usize) -> Result<()> {
    let file = File::open(blob)?;
    let decoder = GzDecoder::new(file);
    extract_tar_reader(decoder, destination, strip_components)
}

fn extract_tar(blob: &Path, destination: &Path, strip_components: usize) -> Result<()> {
    let file = File::open(blob)?;
    extract_tar_reader(file, destination, strip_components)
}

fn extract_tar_reader<R: Read>(
    reader: R,
    destination: &Path,
    strip_components: usize,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_type = entry.header().entry_type();
        let path = entry.path()?.to_path_buf();
        let Some(relative) = safe_relative(&path, strip_components) else {
            continue;
        };
        let target = destination.join(relative);
        if entry_type.is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&target)?;
            io::copy(&mut entry, &mut output)?;
        }
    }
    Ok(())
}

fn extract_7z(blob: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let status = std::process::Command::new("7z.exe")
        .arg("x")
        .arg("-y")
        .arg(format!("-o{}", destination.display()))
        .arg(blob)
        .status()
        .context("launching 7z.exe; install 7-Zip or use a zip/tar package")?;
    if !status.success() {
        bail!("7z extraction failed with status {status}");
    }
    Ok(())
}

fn copy_raw(blob: &Path, destination: &Path) -> Result<()> {
    let file_name = blob
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact.bin");
    fs::copy(blob, destination.join(file_name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::extract;
    use std::fs::{self, File};
    use tar::{Builder, EntryType, Header};

    #[test]
    fn tar_extraction_ignores_symlinks_and_extracts_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("sample.tar");
        {
            let file = File::create(&archive_path).unwrap();
            let mut builder = Builder::new(file);

            let mut link = Header::new_gnu();
            link.set_entry_type(EntryType::Symlink);
            link.set_size(0);
            link.set_cksum();
            builder.append_link(&mut link, "escape", "..").unwrap();

            let data = b"hello";
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_size(data.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "bin/tool.exe", &data[..])
                .unwrap();
            builder.finish().unwrap();
        }

        let dest = dir.path().join("out");
        extract(&archive_path, &dest, Some("tar"), 0).unwrap();
        assert!(!dest.join("escape").exists(), "symlink must not be created");
        assert_eq!(
            fs::read(dest.join("bin").join("tool.exe")).unwrap(),
            b"hello"
        );
    }
}
