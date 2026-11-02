use crate::config::Paths;
use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

pub struct LockGuard {
    _file: File,
}

pub fn lock_path(paths: &Paths) -> std::path::PathBuf {
    paths.root.join("rivet.lock")
}

pub fn acquire(paths: &Paths) -> Result<LockGuard> {
    acquire_at(&lock_path(paths))
}

/// Acquire the single-process mutation lock.
///
/// The lock is an open file handle with exclusive sharing on Windows. The
/// operating system releases it when the process exits, including a crash, so
/// a leftover rivet.lock file never blocks a later run: the next process
/// simply opens it again. The file is intentionally left in place; the open
/// handle, not the file's existence, is the lock.
pub fn acquire_at(path: &Path) -> Result<LockGuard> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating lock directory {}", parent.display()))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    options.share_mode(0);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if is_locked(&error) => bail!(
            "another rv process is running (lock file {}).\n  Wait for it to finish and retry. A crashed process releases the lock automatically.",
            path.display()
        ),
        Err(error) => {
            return Err(error).with_context(|| format!("opening lock file {}", path.display()));
        }
    };
    let _ = writeln!(file, "pid={}", std::process::id());
    let _ = file.flush();
    Ok(LockGuard { _file: file })
}

fn is_locked(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32) | Some(5))
    }
    #[cfg(not(windows))]
    {
        matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rivet.lock");
        let first = acquire_at(&path).unwrap();
        let error = acquire_at(&path).err().expect("second lock must fail");
        assert!(error.to_string().contains("another rv process"));
        drop(first);
        let second = acquire_at(&path).expect("lock must be reusable after drop");
        drop(second);
        assert!(path.exists(), "the lock file is allowed to remain");
    }

    #[test]
    fn a_leftover_lock_file_does_not_block() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rivet.lock");
        fs::write(&path, "pid=999999\n").unwrap();
        let guard = acquire_at(&path).expect("stale lock file must be reusable");
        drop(guard);
    }
}
