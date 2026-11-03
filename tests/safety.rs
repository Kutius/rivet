#![cfg(windows)]

use anyhow::Result;
use rivet::download::sha256_file;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
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

fn write_manifest(
    repo: &Path,
    name: &str,
    version: &str,
    zip_name: &str,
    hash: &str,
    depends: &str,
) -> Result<()> {
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join(format!("{name}-{version}.toml")),
        format!(
            "name = \"{name}\"\nversion = \"{version}\"\ndescription = \"safety test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/{zip_name}\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"{name}\" }}]\n{depends}"
        ),
    )?;
    Ok(())
}

fn prepare_repo(with_app: bool) -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let lib_zip = repo.join("artifacts").join("lib-1.0.0.zip");
    make_zip(&lib_zip, "@echo off\r\necho lib\r\n")?;
    let lib_hash = sha256_file(&lib_zip)?;
    write_manifest(&repo, "lib", "1.0.0", "lib-1.0.0.zip", &lib_hash, "")?;
    if with_app {
        let app_zip = repo.join("artifacts").join("app-1.0.0.zip");
        make_zip(&app_zip, "@echo off\r\necho app\r\n")?;
        let app_hash = sha256_file(&app_zip)?;
        write_manifest(
            &repo,
            "app",
            "1.0.0",
            "app-1.0.0.zip",
            &app_hash,
            "depends = [\"lib\"]\n",
        )?;
    }
    let output = Command::new(repo_bin())
        .args(["build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    Ok((dir, repo.join("index.json")))
}

#[test]
fn mutation_lock_blocks_concurrent_commands() -> Result<()> {
    let (_repo_dir, index) = prepare_repo(false)?;
    let root = TempDir::new()?;
    let root_path = root.path();
    fs::create_dir_all(root_path)?;
    let held = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root_path.join("rivet.lock"))?;

    let output = run_rv(root_path, &index, &["update"]);
    assert!(!output.status.success(), "update must wait for the lock");
    assert!(
        stderr(&output).contains("another rv process"),
        "stderr: {}",
        stderr(&output)
    );
    drop(held);
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert!(
        root_path.join("rivet.lock").exists(),
        "the lock file is allowed to remain; the OS handle is the lock"
    );
    Ok(())
}

#[test]
fn remove_blocks_reverse_dependencies_until_forced() -> Result<()> {
    let (_repo_dir, index) = prepare_repo(true)?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "app"]));

    let output = run_rv(root_path, &index, &["remove", "lib"]);
    assert!(!output.status.success(), "removing a dependency must fail");
    assert!(
        stderr(&output).contains("depend on it"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(root_path.join("bin").join("lib.exe").exists());

    let output = run_rv(root_path, &index, &["remove", "lib", "--force"]);
    assert_success(&output);
    assert!(!root_path.join("bin").join("lib.exe").exists());
    Ok(())
}

#[test]
fn doctor_fix_repairs_broken_link_and_missing_shim() -> Result<()> {
    let (_repo_dir, index) = prepare_repo(false)?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "lib"]));
    let link = root_path.join("apps").join("lib").join("current");
    assert!(link.exists());
    fs::remove_dir(&link)?;
    assert!(!link.exists());
    fs::remove_file(root_path.join("bin").join("lib.exe"))?;
    fs::remove_file(root_path.join("bin").join("lib.rvshim"))?;

    let output = run_rv(root_path, &index, &["doctor"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("repairable"),
        "stdout: {}",
        stdout(&output)
    );

    let output = run_rv(root_path, &index, &["doctor", "--fix"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("fixed:"),
        "stdout: {}",
        stdout(&output)
    );
    assert!(link.exists(), "current link was not repaired");
    assert!(
        root_path.join("bin").join("lib.exe").exists(),
        "shim was not repaired"
    );
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    assert!(stdout(&output).contains("lib"));
    Ok(())
}

#[test]
fn doctor_reports_missing_store_as_unrepairable() -> Result<()> {
    let (_repo_dir, index) = prepare_repo(false)?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "lib"]));
    fs::remove_dir_all(root_path.join("store").join("lib"))?;
    let output = run_rv(root_path, &index, &["doctor", "--fix"]);
    assert_success(&output);
    assert!(
        stdout(&output).contains("cannot repair"),
        "stdout: {}",
        stdout(&output)
    );
    Ok(())
}

#[test]
fn interrupted_install_artifacts_are_cleaned() -> Result<()> {
    let (_repo_dir, index) = prepare_repo(false)?;
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));

    let staging = root_path.join("store").join("lib").join(".tmp-1.0.0-99999");
    fs::create_dir_all(&staging)?;
    fs::write(staging.join("partial.txt"), "partial")?;
    let bin = root_path.join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("ghost.exe"), "stale")?;
    fs::write(bin.join("ghost.rvshim"), "{}")?;

    let output = run_rv(root_path, &index, &["update"]);
    assert_success(&output);
    assert!(!staging.exists(), "staging directory was not cleaned");
    assert!(
        !bin.join("ghost.exe").exists(),
        "orphan shim was not cleaned"
    );
    assert!(
        !bin.join("ghost.rvshim").exists(),
        "orphan sidecar was not cleaned"
    );
    Ok(())
}
