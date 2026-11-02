#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn rv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv"))
}

fn run_rv(root: &Path, args: &[&str]) -> Output {
    Command::new(rv_bin())
        .args(args)
        .env("RIVET_ROOT", root)
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

#[test]
fn init_no_path_installs_binaries_without_touching_path() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    let output = run_rv(root_path, &["init", "--no-path"]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(text.contains("PATH not changed"), "stdout: {text}");
    assert!(
        root_path.join("bin").join("rv.exe").exists(),
        "rv.exe was not installed"
    );

    let second = run_rv(root_path, &["init", "--no-path"]);
    assert_success(&second);
    assert!(
        stdout(&second).contains("PATH not changed"),
        "stdout: {}",
        stdout(&second)
    );

    let doctor = run_rv(root_path, &["doctor"]);
    assert_success(&doctor);
    assert!(
        stdout(&doctor).contains("is not in PATH"),
        "doctor: {}",
        stdout(&doctor)
    );
}

#[test]
fn version_is_visible() {
    let root = TempDir::new().unwrap();
    let output = run_rv(root.path(), &["--version"]);
    assert_success(&output);
    assert!(
        stdout(&output).starts_with("rv "),
        "version: {}",
        stdout(&output)
    );
}

#[test]
fn init_no_path_does_not_create_a_user_path_change_marker() {
    let root = TempDir::new().unwrap();
    let output = run_rv(root.path(), &["init", "--no-path"]);
    assert_success(&output);
    assert!(!root.path().join("rivet.lock").exists());
    assert!(fs::metadata(root.path().join("bin").join("rv.exe")).is_ok());
}
