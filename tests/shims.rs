#![cfg(windows)]

use anyhow::Result;
use rivet::download::sha256_file;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

fn rv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv"))
}

fn run_rv(root: &Path, index: &Path, args: &[&str]) -> Output {
    Command::new(rv_bin())
        .args(args)
        .env("RIVET_ROOT", root)
        .env("RIVET_MAIN_INDEX", index)
        .env("RIVET_DISABLE_SYMLINKS", "1")
        .output()
        .expect("running rv")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "rv failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn make_native_repo() -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    fs::create_dir_all(repo.join("packages"))?;

    let zip_path = repo.join("artifacts").join("tool.zip");
    let file = File::create(&zip_path)?;
    let mut writer = zip::ZipWriter::new(file);
    writer.start_file("tool.exe", SimpleFileOptions::default())?;
    writer.write_all(&fs::read(rv_bin())?)?;
    writer.finish()?;

    let hash = sha256_file(&zip_path)?;
    let manifest = format!(
        "name = \"tool\"\nversion = \"1.0.0\"\ndescription = \"native shim test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/tool.zip\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"tool.exe\", name = \"tool\" }}]\n"
    );
    fs::write(repo.join("packages").join("tool.toml"), manifest)?;
    rivet::index::build_index(&repo, None)?;
    Ok((dir, repo.join("index.json")))
}

#[test]
fn native_exe_falls_back_to_a_launcher_when_symlinks_are_disabled() -> Result<()> {
    let (_repo_dir, index) = make_native_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();

    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "tool"]));

    let shim = root_path.join("bin").join("tool.exe");
    let metadata = fs::symlink_metadata(&shim)?;
    assert!(
        !metadata.file_type().is_symlink(),
        "fallback must create a launcher, not a symlink"
    );
    assert!(
        root_path.join("bin").join("tool.rvshim").is_file(),
        "fallback launcher must have a sidecar"
    );

    let output = Command::new(&shim).arg("--version").output()?;
    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("rv "));
    assert_success(&run_rv(root_path, &index, &["verify"]));
    Ok(())
}
