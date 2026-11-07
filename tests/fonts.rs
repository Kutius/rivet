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

fn repo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv-repo"))
}

fn run_rv(
    root: &Path,
    index: &Path,
    font_dir: &Path,
    registry: &str,
    args: &[&str],
) -> std::process::Output {
    Command::new(rv_bin())
        .args(args)
        .env("RIVET_ROOT", root)
        .env("RIVET_MAIN_INDEX", index)
        .env("RIVET_FONT_DIR", font_dir)
        .env("RIVET_FONT_REGISTRY", registry)
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

#[test]
fn installs_and_removes_user_fonts() -> Result<()> {
    let repo_dir = TempDir::new()?;
    let repo = repo_dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let zip_path = repo.join("artifacts").join("font-test.zip");
    let file = File::create(&zip_path)?;
    let mut writer = zip::ZipWriter::new(file);
    writer.start_file("DemoFont-Regular.ttf", SimpleFileOptions::default())?;
    writer.write_all(b"not a real font, but a valid file")?;
    writer.start_file("README.txt", SimpleFileOptions::default())?;
    writer.write_all(b"font package")?;
    writer.finish()?;
    let hash = sha256_file(&zip_path)?;
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join("font-test.toml"),
        format!(
            "name = \"font-test\"\nversion = \"1.0.0\"\ndescription = \"font test\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/font-test.zip\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nfonts = [\".\"]\n"
        ),
    )?;
    let output = Command::new(repo_bin())
        .args(["build"])
        .arg(&repo)
        .output()?;
    assert_success(&output);
    let index = repo.join("index.json");

    let root = TempDir::new()?;
    let font_dir = root.path().join("fonts");
    let registry = format!(
        "Software\\Rivet\\Tests\\Fonts-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );

    let output = run_rv(root.path(), &index, &font_dir, &registry, &["update"]);
    assert_success(&output);
    let output = run_rv(
        root.path(),
        &index,
        &font_dir,
        &registry,
        &["install", "font-test"],
    );
    assert_success(&output);

    let font_file = font_dir.join("DemoFont-Regular.ttf");
    assert!(font_file.is_file(), "font was not copied");
    let query = Command::new("reg")
        .args([
            "query",
            &format!("HKCU\\{registry}"),
            "/v",
            "DemoFont-Regular (TrueType)",
        ])
        .output()?;
    assert!(
        query.status.success(),
        "font registry value missing\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&query.stdout),
        String::from_utf8_lossy(&query.stderr)
    );

    let output = run_rv(root.path(), &index, &font_dir, &registry, &["verify"]);
    assert_success(&output);
    let output = run_rv(
        root.path(),
        &index,
        &font_dir,
        &registry,
        &["remove", "font-test"],
    );
    assert_success(&output);
    assert!(!font_file.exists(), "font file was not removed");
    let query = Command::new("reg")
        .args([
            "query",
            &format!("HKCU\\{registry}"),
            "/v",
            "DemoFont-Regular (TrueType)",
        ])
        .output()?;
    assert!(
        !query.status.success(),
        "font registry value was not removed"
    );

    let _ = Command::new("reg")
        .args(["delete", &format!("HKCU\\{registry}"), "/f"])
        .status();
    Ok(())
}
