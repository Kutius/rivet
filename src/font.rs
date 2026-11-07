use crate::db::FontRecord;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const DEFAULT_REGISTRY_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

pub fn font_dir() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("RIVET_FONT_DIR")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    Ok(dirs::data_local_dir()
        .context("could not determine LOCALAPPDATA")?
        .join("Microsoft")
        .join("Windows")
        .join("Fonts"))
}

pub fn registry_key() -> String {
    std::env::var("RIVET_FONT_REGISTRY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_KEY.to_string())
}

pub fn is_font(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
}

/// Resolve manifest font entries (files or directories) to files inside the
/// extracted package root.
pub fn resolve(root: &Path, entries: &[String]) -> Result<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolving package root {}", root.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = root.join(entry);
        let path = path
            .canonicalize()
            .with_context(|| format!("font path '{entry}' was not found"))?;
        if !path.starts_with(&root) {
            bail!("font path '{entry}' escapes the package root");
        }
        if path.is_dir() {
            for item in WalkDir::new(&path).min_depth(1) {
                let item = item?;
                if item.file_type().is_file() && is_font(item.path()) {
                    files.push(item.path().to_path_buf());
                }
            }
        } else if path.is_file() && is_font(&path) {
            files.push(path);
        } else {
            bail!("'{entry}' is not a supported font file or directory");
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        bail!("no .ttf, .otf, .ttc, or .otc files found");
    }
    Ok(files)
}

pub fn registry_name(file_name: &str) -> String {
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name);
    let suffix = match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("otf") | Some("otc") => "OpenType",
        _ => "TrueType",
    };
    format!("{stem} ({suffix})")
}

pub fn replace(
    package: &str,
    sources: &[PathBuf],
    old: &[FontRecord],
    all: &[FontRecord],
    font_dir: &Path,
    registry_key: &str,
) -> Result<Vec<FontRecord>> {
    fs::create_dir_all(font_dir)
        .with_context(|| format!("creating font directory {}", font_dir.display()))?;
    let mut desired = Vec::new();
    let mut names = HashSet::new();
    for source in sources {
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .with_context(|| format!("font file has no valid name: {}", source.display()))?;
        if !names.insert(file_name.to_ascii_lowercase()) {
            bail!("package '{package}' contains duplicate font file name '{file_name}'");
        }
        let destination = font_dir.join(file_name);
        if let Some(record) = all
            .iter()
            .find(|record| record.package != package && same_path(&record.path, &destination))
        {
            bail!(
                "font file {} is already managed by package '{}'",
                destination.display(),
                record.package
            );
        }
        let replacing = old
            .iter()
            .any(|record| same_path(&record.path, &destination));
        if destination.exists() && !replacing {
            bail!(
                "font file {} already exists and is not managed by Rivet",
                destination.display()
            );
        }
        desired.push((source.clone(), destination, registry_name(file_name)));
    }

    if !old.is_empty()
        && let Err(error) = remove(old, font_dir, registry_key)
    {
        let _ = restore(old, font_dir, registry_key);
        return Err(error).context("removing previous fonts");
    }

    let mut installed = Vec::new();
    for (source, destination, registry_name) in desired {
        if let Err(error) =
            copy_and_register(package, &source, &destination, &registry_name, registry_key)
        {
            let _ = remove(&installed, font_dir, registry_key);
            let _ = restore(old, font_dir, registry_key);
            return Err(error);
        }
        installed.push(FontRecord {
            path: destination.to_string_lossy().to_string(),
            package: package.to_string(),
            registry_name,
            source: source.to_string_lossy().to_string(),
        });
    }
    broadcast_best_effort();
    Ok(installed)
}

fn copy_and_register(
    package: &str,
    source: &Path,
    destination: &Path,
    registry_name: &str,
    registry_key: &str,
) -> Result<()> {
    fs::copy(source, destination).with_context(|| {
        format!(
            "copying font {} to {} for {}",
            source.display(),
            destination.display(),
            package
        )
    })?;
    if let Err(error) = set_registry_value(registry_key, registry_name, destination) {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn restore(records: &[FontRecord], _font_dir: &Path, registry_key: &str) -> Result<()> {
    for record in records {
        let source = Path::new(&record.source);
        let destination = Path::new(&record.path);
        if !source.is_file() || destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
        set_registry_value(registry_key, &record.registry_name, destination)?;
    }
    broadcast_best_effort();
    Ok(())
}

pub fn remove(records: &[FontRecord], font_dir: &Path, registry_key: &str) -> Result<()> {
    let _ = font_dir;
    let mut errors = Vec::new();
    for record in records {
        if let Err(error) = delete_registry_value(registry_key, &record.registry_name) {
            errors.push(format!(
                "removing registry value '{}': {error:#}",
                record.registry_name
            ));
        }
        let path = Path::new(&record.path);
        if path.exists()
            && let Err(error) = fs::remove_file(path)
        {
            errors.push(format!(
                "removing {}: {error}. Close applications that use this font and retry",
                path.display()
            ));
        }
    }
    if !errors.is_empty() {
        bail!("could not remove font(s):\n  {}", errors.join("\n  "));
    }
    if !records.is_empty() {
        broadcast_best_effort();
    }
    Ok(())
}

pub fn verify(records: &[FontRecord], registry_key: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for record in records {
        let path = Path::new(&record.path);
        if !path.is_file() {
            problems.push(format!("font file missing: {}", record.path));
        }
        match registry_value_matches(registry_key, &record.registry_name, path) {
            Ok(true) => {}
            Ok(false) => problems.push(format!(
                "font registry value missing or stale: {}",
                record.registry_name
            )),
            Err(error) => problems.push(format!(
                "font registry value '{}' cannot be read: {error:#}",
                record.registry_name
            )),
        }
    }
    problems
}

fn same_path(left: &str, right: &Path) -> bool {
    left.eq_ignore_ascii_case(&right.to_string_lossy())
}

fn broadcast_best_effort() {
    if let Err(error) = broadcast_change() {
        eprintln!("warning: font change broadcast failed: {error:#}");
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
fn set_registry_value(key: &str, name: &str, value: &Path) -> Result<()> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW,
    };

    let key_wide = wide(key);
    let name_wide = wide(name);
    let value_wide = wide(&value.to_string_lossy());
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        bail!("opening font registry key '{key}' failed with Windows error {status}");
    }
    let status = unsafe {
        RegSetValueExW(
            handle,
            name_wide.as_ptr(),
            0,
            REG_SZ,
            value_wide.as_ptr().cast(),
            (value_wide.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if status != ERROR_SUCCESS {
        bail!("writing font registry value '{name}' failed with Windows error {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn delete_registry_value(key: &str, name: &str) -> Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    };

    let key_wide = wide(key);
    let name_wide = wide(name);
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut handle,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if status != ERROR_SUCCESS {
        bail!("opening font registry key '{key}' failed with Windows error {status}");
    }
    let status = unsafe { RegDeleteValueW(handle, name_wide.as_ptr()) };
    unsafe {
        RegCloseKey(handle);
    }
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        bail!("deleting font registry value '{name}' failed with Windows error {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn registry_value_matches(key: &str, name: &str, expected: &Path) -> Result<bool> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    const ERROR_MORE_DATA: u32 = 234;
    let key_wide = wide(key);
    let name_wide = wide(name);
    let mut kind = 0u32;
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        bail!("reading font registry value '{name}' failed with Windows error {status}");
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut actual = size;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut actual,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if status != ERROR_SUCCESS {
        bail!("reading font registry value '{name}' failed with Windows error {status}");
    }
    let value = String::from_utf16_lossy(&buffer[..(actual as usize / 2)]);
    Ok(value
        .trim_end_matches('\0')
        .eq_ignore_ascii_case(&expected.to_string_lossy()))
}

#[cfg(windows)]
pub fn broadcast_change() -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_FONTCHANGE,
    };

    let mut result = 0usize;
    let ok = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_FONTCHANGE,
            0,
            0,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        )
    };
    if ok == 0 {
        bail!("Windows did not acknowledge WM_FONTCHANGE");
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_registry_value(_key: &str, _name: &str, _value: &Path) -> Result<()> {
    bail!("font installation is only supported on Windows")
}

#[cfg(not(windows))]
fn delete_registry_value(_key: &str, _name: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn registry_value_matches(_key: &str, _name: &str, _expected: &Path) -> Result<bool> {
    Ok(true)
}

#[cfg(not(windows))]
pub fn broadcast_change() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{registry_name, resolve};
    use std::fs;

    #[test]
    fn resolves_font_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.ttf"), b"a").unwrap();
        fs::write(dir.path().join("b.otf"), b"b").unwrap();
        fs::write(dir.path().join("readme.txt"), b"c").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("c.ttc"), b"d").unwrap();

        let all = resolve(dir.path(), &[".".to_string()]).unwrap();
        assert_eq!(all.len(), 3);
        let exact = resolve(dir.path(), &["a.ttf".to_string()]).unwrap();
        assert_eq!(exact.len(), 1);
        assert!(resolve(dir.path(), &["readme.txt".to_string()]).is_err());
    }

    #[test]
    fn names_registry_values_by_font_type() {
        assert_eq!(registry_name("Demo.ttf"), "Demo (TrueType)");
        assert_eq!(registry_name("Demo.ttc"), "Demo (TrueType)");
        assert_eq!(registry_name("Demo.otf"), "Demo (OpenType)");
        assert_eq!(registry_name("Demo.otc"), "Demo (OpenType)");
    }
}
