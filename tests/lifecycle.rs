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

fn write_manifest(repo: &Path, version: &str, hash: &str) -> Result<()> {
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join(format!("hello-{version}.toml")),
        format!(
            "name = \"hello\"\nversion = \"{version}\"\ndescription = \"lifecycle test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/hello-{version}.zip\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"hello\" }}]\n"
        ),
    )?;
    Ok(())
}

fn build_index(repo: &Path) -> Result<()> {
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(repo)
        .env("RIVET_ROOT", repo)
        .output()?;
    assert_success(&output);
    Ok(())
}

fn prepare_repo() -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    for version in ["1.0.0", "1.1.0"] {
        let zip = repo.join("artifacts").join(format!("hello-{version}.zip"));
        make_zip(&zip, &format!("@echo off\r\necho hello {version}\r\n"))?;
        let hash = sha256_file(&zip)?;
        write_manifest(&repo, version, &hash)?;
    }
    build_index(&repo)?;
    Ok((dir, repo.join("index.json")))
}

fn add_broken_version(repo: &Path) -> Result<()> {
    let zip = repo.join("artifacts").join("hello-1.2.0.zip");
    make_zip(&zip, "@echo off\r\necho broken\r\n")?;
    write_manifest(repo, "1.2.0", &"0".repeat(64))?;
    build_index(repo)
}

#[test]
fn pin_state_is_visible_and_blocks_upgrade_until_forced() -> Result<()> {
    let (_repo_dir, index) = prepare_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "hello@1.0.0"]));

    assert_success(&run_rv(root_path, &index, &["pin", "hello"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    assert!(stdout(&output).contains("yes"), "list: {}", stdout(&output));
    let output = run_rv(root_path, &index, &["status"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("pinned"),
        "status: {}",
        stdout(&output)
    );

    let output = run_rv(root_path, &index, &["upgrade", "hello"]);
    assert!(!output.status.success(), "pinned upgrade must fail");
    assert!(
        stderr(&output).contains("pinned"),
        "stderr: {}",
        stderr(&output)
    );

    let output = run_rv(root_path, &index, &["upgrade", "--all"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("skipping"),
        "stdout: {}",
        stdout(&output)
    );
    let output = run_rv(root_path, &index, &["list"]);
    assert!(stdout(&output).contains("1.0.0"));

    let output = run_rv(root_path, &index, &["upgrade", "hello", "--force"]);
    assert_success(&output);
    let output = run_rv(root_path, &index, &["list"]);
    assert!(
        stdout(&output).contains("1.1.0"),
        "stdout: {}",
        stdout(&output)
    );

    assert_success(&run_rv(root_path, &index, &["unpin", "hello"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert!(
        !stdout(&output).contains("yes"),
        "list: {}",
        stdout(&output)
    );
    Ok(())
}

#[test]
fn verify_detects_damage_and_passes_after_doctor_fix() -> Result<()> {
    let (_repo_dir, index) = prepare_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "hello@1.0.0"]));
    assert_success(&run_rv(root_path, &index, &["verify"]));

    fs::remove_file(root_path.join("bin").join("hello.exe"))?;
    let output = run_rv(root_path, &index, &["verify"]);
    assert!(!output.status.success(), "verify must fail on damage");
    assert!(
        stdout(&output).contains("shim missing"),
        "stdout: {}",
        stdout(&output)
    );

    assert_success(&run_rv(root_path, &index, &["doctor", "--fix"]));
    assert_success(&run_rv(root_path, &index, &["verify"]));
    Ok(())
}

#[test]
fn rollback_works_after_failed_upgrade_and_doctor_fix() -> Result<()> {
    let (repo_dir, index) = prepare_repo()?;
    let repo = repo_dir.path().join("main");
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "hello@1.0.0"]));
    assert_success(&run_rv(root_path, &index, &["upgrade", "hello"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert!(stdout(&output).contains("1.1.0"));

    add_broken_version(&repo)?;
    assert_success(&run_rv(root_path, &index, &["update"]));
    let output = run_rv(root_path, &index, &["upgrade", "hello"]);
    assert!(
        !output.status.success(),
        "broken artifact must fail the upgrade"
    );
    let output = run_rv(root_path, &index, &["list"]);
    assert!(
        stdout(&output).contains("1.1.0"),
        "failed upgrade must keep 1.1.0: {}",
        stdout(&output)
    );

    assert_success(&run_rv(root_path, &index, &["rollback", "hello"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert!(
        stdout(&output).contains("1.0.0"),
        "stdout: {}",
        stdout(&output)
    );

    assert_success(&run_rv(root_path, &index, &["install", "hello@1.1.0"]));
    let link = root_path.join("apps").join("hello").join("current");
    fs::remove_dir(&link)?;
    assert_success(&run_rv(root_path, &index, &["doctor", "--fix"]));
    assert!(link.exists(), "doctor --fix did not restore the link");
    assert_success(&run_rv(root_path, &index, &["rollback", "hello"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert!(
        stdout(&output).contains("1.0.0"),
        "rollback after doctor --fix: {}",
        stdout(&output)
    );
    Ok(())
}
