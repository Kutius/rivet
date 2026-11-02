#![cfg(windows)]

use anyhow::Result;
use rivet::download::sha256_file;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

fn rv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv"))
}

fn run_rv(root: &Path, index: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rv_bin())
        .args(args)
        .env("RIVET_ROOT", root)
        .env("RIVET_MAIN_INDEX", index)
        .output()
        .expect("running rv")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "rv failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
    writer.start_file("data/default.txt", SimpleFileOptions::default())?;
    writer.write_all(b"shipped default")?;
    writer.finish()?;
    Ok(())
}

fn write_manifest(
    repo: &Path,
    name: &str,
    version: &str,
    zip_name: &str,
    hash: &str,
    bin_name: &str,
) -> Result<()> {
    let content = format!(
        "name = \"{name}\"\nversion = \"{version}\"\ndescription = \"test package\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\ntags = [\"test\"]\nurl = \"artifacts/{zip_name}\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"{bin_name}\" }}]\npersist = [\"data\"]\n"
    );
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join(format!("{name}-{version}.toml")),
        content,
    )?;
    Ok(())
}

fn prepare_main_repo() -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    for (version, text) in [
        (
            "1.0.0",
            "@echo off\r\necho Hello from Rivet 1.0!\r\necho args: %*\r\n",
        ),
        (
            "1.1.0",
            "@echo off\r\necho Hello from Rivet 1.1!\r\necho args: %*\r\n",
        ),
    ] {
        let zip_path = repo.join("artifacts").join(format!("hello-{version}.zip"));
        make_zip(&zip_path, text)?;
        let hash = sha256_file(&zip_path)?;
        write_manifest(
            &repo,
            "hello",
            version,
            &format!("hello-{version}.zip"),
            &hash,
            if version == "1.1.0" {
                "hello2"
            } else {
                "hello"
            },
        )?;
    }
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    let index = repo.join("index.json");
    Ok((dir, index))
}

#[test]
fn install_upgrade_rollback_remove() -> Result<()> {
    let (_repo_dir, index) = prepare_main_repo()?;
    let root = TempDir::new()?;
    let root_path = root.path();

    let output = run_rv(root_path, &index, &["update"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("main:"));

    let output = run_rv(root_path, &index, &["search", "hello"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));

    let output = run_rv(root_path, &index, &["install", "hello@1.0.0"]);
    assert_success(&output);
    let shim = root_path.join("bin").join("hello.exe");
    assert!(shim.exists(), "shim missing at {}", shim.display());
    let shipped_persist = root_path
        .join("persist")
        .join("hello")
        .join("data")
        .join("default.txt");
    assert_eq!(
        fs::read_to_string(&shipped_persist)?,
        "shipped default",
        "persist seed file was not moved"
    );

    let output = run_rv(root_path, &index, &["status"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1.0.0"));

    let persist_dir = root_path.join("persist").join("hello").join("data");
    fs::create_dir_all(&persist_dir)?;
    fs::write(persist_dir.join("marker.txt"), "keep me")?;

    let output = Command::new(&shim).args(["a", "b"]).output()?;
    assert!(
        output.status.success(),
        "shim failed: status={:?} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Hello from Rivet 1.0!"), "stdout: {stdout}");
    assert!(
        stdout.contains("args: \"a\" \"b\"") || stdout.contains("args: a b"),
        "stdout: {stdout}"
    );

    let output = run_rv(root_path, &index, &["upgrade", "hello"]);
    assert_success(&output);
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1.1.0"));
    assert!(root_path.join("bin").join("hello2.exe").exists());
    assert!(!root_path.join("bin").join("hello.exe").exists());
    assert_eq!(
        fs::read_to_string(persist_dir.join("marker.txt"))?,
        "keep me"
    );

    let output = run_rv(root_path, &index, &["rollback", "hello"]);
    assert_success(&output);
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1.0.0"));
    assert!(root_path.join("bin").join("hello.exe").exists());
    assert!(!root_path.join("bin").join("hello2.exe").exists());

    let output = run_rv(root_path, &index, &["remove", "hello"]);
    assert_success(&output);
    assert!(!root_path.join("store").join("hello").exists());
    Ok(())
}

#[test]
fn third_party_tap_install() -> Result<()> {
    let (_repo_dir, index) = prepare_main_repo()?;
    let tap_root = TempDir::new()?;
    let tap = tap_root.path().join("tap");
    fs::create_dir_all(tap.join("artifacts"))?;
    let zip_path = tap.join("artifacts").join("hello-2.0.0.zip");
    make_zip(
        &zip_path,
        "@echo off\r\necho Hello from the tap!\r\necho args: %*\r\n",
    )?;
    let hash = sha256_file(&zip_path)?;
    write_manifest(&tap, "hello", "2.0.0", "hello-2.0.0.zip", &hash, "hello")?;
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&tap)
        .env("RIVET_ROOT", tap_root.path())
        .output()?;
    assert_success(&output);
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.email=demo@example.invalid",
            "-c",
            "user.name=RivetDemo",
            "commit",
            "-q",
            "-m",
            "tap",
        ],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(&tap)
            .args(args)
            .status()?;
        assert!(status.success());
    }

    let root = TempDir::new()?;
    let root_path = root.path();
    let output = run_rv(root_path, &index, &["update"]);
    assert_success(&output);
    let tap_url = tap.to_string_lossy().to_string();
    let output = run_rv(root_path, &index, &["tap", "add", "demo", &tap_url]);
    assert_success(&output);
    let traversal = run_rv(root_path, &index, &["tap", "remove", ".."]);
    assert!(
        !traversal.status.success(),
        "tap traversal should be rejected"
    );
    assert!(root_path.exists());
    let output = run_rv(root_path, &index, &["install", "demo/hello"]);
    assert_success(&output);
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("demo"));
    Ok(())
}

#[test]
fn raw_artifact_and_missing_bin() -> Result<()> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let raw_artifact = repo.join("artifacts").join("tool.cmd");
    fs::write(&raw_artifact, "@echo off\r\necho raw tool\r\n")?;
    let raw_hash = sha256_file(&raw_artifact)?;
    let raw_manifest = format!(
        "name = \"raw\"\nversion = \"1.0.0\"\ndescription = \"raw test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/tool.cmd\"\nsha256 = \"{raw_hash}\"\nextract = \"none\"\nbin = [{{ path = \"tool.cmd\", name = \"raw\" }}]\n"
    );
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(repo.join("packages").join("raw-1.0.0.toml"), raw_manifest)?;

    let broken_zip = repo.join("artifacts").join("broken.zip");
    make_zip(&broken_zip, "@echo off\r\necho broken\r\n")?;
    let broken_hash = sha256_file(&broken_zip)?;
    let broken_manifest = format!(
        "name = \"broken\"\nversion = \"1.0.0\"\ndescription = \"missing bin test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/broken.zip\"\nsha256 = \"{broken_hash}\"\nextract = \"zip\"\nbin = [{{ path = \"missing.cmd\", name = \"broken\" }}]\n"
    );
    fs::write(
        repo.join("packages").join("broken-1.0.0.toml"),
        broken_manifest,
    )?;

    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    let index = repo.join("index.json");
    let root = TempDir::new()?;
    let root_path = root.path();
    let output = run_rv(root_path, &index, &["update"]);
    assert_success(&output);
    let output = run_rv(root_path, &index, &["install", "raw"]);
    assert_success(&output);
    assert!(root_path.join("bin").join("raw.exe").exists());

    let output = run_rv(root_path, &index, &["install", "broken"]);
    assert!(!output.status.success(), "missing bin should fail install");
    assert!(!root_path.join("apps").join("broken").exists());
    Ok(())
}
#[test]
fn dependency_version_is_respected() -> Result<()> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    for version in ["1.0.0", "1.1.0"] {
        let zip_path = repo.join("artifacts").join(format!("lib-{version}.zip"));
        make_zip(&zip_path, "@echo off\r\necho lib\r\n")?;
        let hash = sha256_file(&zip_path)?;
        write_manifest(
            &repo,
            "lib",
            version,
            &format!("lib-{version}.zip"),
            &hash,
            "lib",
        )?;
    }
    let app_zip = repo.join("artifacts").join("app-1.0.0.zip");
    make_zip(&app_zip, "@echo off\r\necho app\r\n")?;
    let app_hash = sha256_file(&app_zip)?;
    write_manifest(&repo, "app", "1.0.0", "app-1.0.0.zip", &app_hash, "app")?;
    {
        let manifest_path = repo.join("packages").join("app-1.0.0.toml");
        let mut text = fs::read_to_string(&manifest_path)?;
        text.push_str("depends = [\"lib@1.0.0\"]\n");
        fs::write(&manifest_path, text)?;
    }
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    let index = repo.join("index.json");
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, &index, &["update"]));
    assert_success(&run_rv(root_path, &index, &["install", "app"]));
    let output = run_rv(root_path, &index, &["list"]);
    assert_success(&output);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("lib"), "list missing lib: {text}");
    assert!(text.contains("1.0.0"), "list should have lib 1.0.0: {text}");
    assert!(
        !text.contains("1.1.0"),
        "dependency should not float to latest: {text}"
    );
    Ok(())
}
