#![cfg(windows)]

use anyhow::Result;
use rivet::download::sha256_file;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

fn rv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv"))
}

fn repo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv-repo"))
}

fn run_rv(root: &Path, index: &Path, args: &[&str]) -> Output {
    Command::new(rv_bin())
        .args(args)
        .env("RIVET_ROOT", root)
        .env("RIVET_MAIN_INDEX", index)
        .output()
        .expect("running rv")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "rv failed\nstdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
}

fn make_zip(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = zip::ZipWriter::new(file);
    writer.start_file("hello.cmd", SimpleFileOptions::default())?;
    writer.write_all(content.as_bytes())?;
    writer.finish()?;
    Ok(())
}

fn write_manifest(repo: &Path, name: &str, version: &str, file: &str, hash: &str) -> Result<()> {
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join(format!("{name}-{version}.toml")),
        format!(
            "name = \"{name}\"\nversion = \"{version}\"\ndescription = \"output test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/{file}\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"{name}\" }}]\n"
        ),
    )?;
    Ok(())
}

fn prepare_repo() -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let zip = repo.join("artifacts").join("hello-1.0.0.zip");
    make_zip(&zip, "@echo off\r\necho hello\r\n")?;
    let hash = sha256_file(&zip)?;
    write_manifest(&repo, "hello", "1.0.0", "hello-1.0.0.zip", &hash)?;
    let output = Command::new(repo_bin())
        .args(["build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    Ok((dir, repo.join("index.json")))
}

#[test]
fn json_views_are_machine_readable() -> Result<()> {
    let (_repo_dir, index) = prepare_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "hello"]));

    let output = run_rv(root_path, &index, &["--json", "search", "hello"]);
    assert_success(&output);
    let search: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(search[0]["name"], "hello");
    assert_eq!(search[0]["version"], "1.0.0");

    let output = run_rv(root_path, &index, &["--json", "list"]);
    assert_success(&output);
    let list: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(list[0]["name"], "hello");
    assert_eq!(list[0]["pinned"], false);

    let output = run_rv(root_path, &index, &["--json", "status"]);
    assert_success(&output);
    let status: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(status[0]["installed"], "1.0.0");
    assert_eq!(status[0]["latest"], "1.0.0");
    assert_eq!(status[0]["status"], "ok");

    let output = run_rv(root_path, &index, &["--json", "info", "hello"]);
    assert_success(&output);
    let info: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(info["name"], "hello");
    assert_eq!(info["versions"][0], "1.0.0");
    Ok(())
}

#[test]
fn status_check_falls_back_to_cached_metadata() -> Result<()> {
    let (_repo_dir, index) = prepare_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "hello"]));

    let bad = "http://127.0.0.1:9/index.json";
    let output = run_rv(root_path, Path::new(bad), &["status", "--check"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("1.0.0"),
        "cached status missing: {}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains("metadata refresh failed"),
        "stderr: {}",
        stderr(&output)
    );
    Ok(())
}

#[test]
fn missing_artifact_reports_a_download_error() -> Result<()> {
    let repo = TempDir::new()?;
    let repo_path = repo.path().join("main");
    write_manifest(
        &repo_path,
        "gone",
        "1.0.0",
        "missing-1.0.0.zip",
        &"0".repeat(64),
    )?;
    let output = Command::new(repo_bin())
        .args(["build"])
        .arg(&repo_path)
        .env("RIVET_ROOT", repo.path())
        .output()?;
    assert_success(&output);
    let index = repo_path.join("index.json");
    let root = TempDir::new()?;
    assert_success(&run_rv(root.path(), &index, &["update"]));
    let output = run_rv(root.path(), &index, &["install", "gone"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("downloading"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("missing-1.0.0.zip"),
        "stderr: {}",
        stderr(&output)
    );
    Ok(())
}

#[test]
fn unknown_package_and_bad_archive_fail_with_actionable_errors() -> Result<()> {
    let (_repo_dir, index) = prepare_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));

    let output = run_rv(root_path, &index, &["install", "missing"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("not found"),
        "stderr: {}",
        stderr(&output)
    );

    let repo = TempDir::new()?;
    let repo_path = repo.path().join("main");
    fs::create_dir_all(repo_path.join("artifacts"))?;
    let bad = repo_path.join("artifacts").join("bad-1.0.0.zip");
    fs::write(&bad, b"this is not a zip")?;
    let hash = sha256_file(&bad)?;
    write_manifest(&repo_path, "bad", "1.0.0", "bad-1.0.0.zip", &hash)?;
    let output = Command::new(repo_bin())
        .args(["build"])
        .arg(&repo_path)
        .env("RIVET_ROOT", repo.path())
        .output()?;
    assert_success(&output);
    let bad_index = repo_path.join("index.json");
    let root = TempDir::new()?;
    assert_success(&run_rv(root.path(), &bad_index, &["update"]));
    let output = run_rv(root.path(), &bad_index, &["install", "bad"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("extracting"),
        "stderr: {}",
        stderr(&output)
    );
    Ok(())
}
