#![cfg(windows)]

use anyhow::Result;
use rivet::download::sha256_file;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

fn rv_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rv"))
}

fn run_rv(root: &Path, index: Option<&str>, trusted: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(rv_bin());
    command.args(args).env("RIVET_ROOT", root);
    if let Some(index) = index {
        command.env("RIVET_MAIN_INDEX", index);
    }
    if let Some(key) = trusted {
        command.env("RIVET_TRUSTED_KEY", key);
    }
    command.output().expect("running rv")
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

fn prepare_repo() -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let zip_path = repo.join("artifacts").join("hello-1.0.0.zip");
    make_zip(&zip_path, "@echo off\r\necho signed hello\r\n")?;
    let hash = sha256_file(&zip_path)?;
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join("hello-1.0.0.toml"),
        format!(
            "name = \"hello\"\nversion = \"1.0.0\"\ndescription = \"signed package\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/hello-1.0.0.zip\"\nsha256 = \"{hash}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"hello\" }}]\n"
        ),
    )?;
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    Ok((dir, repo))
}

fn keygen(dir: &Path) -> Result<(PathBuf, String)> {
    let output = Command::new(rv_bin())
        .args(["repo", "keygen", "--out"])
        .arg(dir)
        .output()?;
    assert_success(&output);
    let private = dir.join("rivet-signing.key");
    let public = dir.join("rivet-signing.pub");
    let key = fs::read_to_string(&public)?.trim().to_string();
    Ok((private, key))
}

fn sign_repo(repo: &Path, private: &Path) -> Output {
    Command::new(rv_bin())
        .args(["repo", "sign"])
        .arg(repo)
        .arg("--key")
        .arg(private)
        .output()
        .expect("signing")
}

fn serve(files: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binding test server");
    let address = listener.local_addr().expect("test server address");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .trim_start_matches('/')
                .to_string();
            match files.get(&path) {
                Some(body) => {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        }
    });
    format!("http://{address}/index.json")
}

#[test]
fn signed_local_index_verifies_and_tampering_fails() -> Result<()> {
    let (_repo_dir, repo) = prepare_repo()?;
    let keys = TempDir::new()?;
    let (private, key) = keygen(keys.path())?;
    assert_success(&sign_repo(&repo, &private));
    let index = repo.join("index.json");
    let index_text = index.to_string_lossy().to_string();
    let root = TempDir::new()?;
    let root_path = root.path();

    let output = run_rv(root_path, Some(&index_text), Some(&key), &["update"]);
    assert_success(&output);

    let output = run_rv(root_path, Some(&index_text), None, &["update"]);
    assert!(!output.status.success(), "signed index needs a trusted key");
    assert!(
        stderr(&output).contains("no trusted signing key"),
        "stderr: {}",
        stderr(&output)
    );

    let other_keys = TempDir::new()?;
    let (_other_private, other_key) = keygen(other_keys.path())?;
    let output = run_rv(root_path, Some(&index_text), Some(&other_key), &["update"]);
    assert!(!output.status.success(), "wrong trusted key must fail");
    assert!(
        stderr(&output).contains("untrusted key"),
        "stderr: {}",
        stderr(&output)
    );

    let mut tampered = fs::read_to_string(&index)?;
    tampered.push(' ');
    fs::write(&index, tampered)?;
    let output = run_rv(root_path, Some(&index_text), Some(&key), &["update"]);
    assert!(!output.status.success(), "tampered index must fail");
    assert!(
        stderr(&output).contains("signature verification failed"),
        "stderr: {}",
        stderr(&output)
    );
    Ok(())
}

#[test]
fn unsigned_local_index_stays_usable_for_development() -> Result<()> {
    let (_repo_dir, repo) = prepare_repo()?;
    let index = repo.join("index.json");
    let root = TempDir::new()?;
    let output = run_rv(
        root.path(),
        Some(&index.to_string_lossy()),
        None,
        &["update"],
    );
    assert_success(&output);
    Ok(())
}

#[test]
fn remote_index_fails_closed_without_signature() -> Result<()> {
    let (_repo_dir, repo) = prepare_repo()?;
    let index_bytes = fs::read(repo.join("index.json"))?;
    let mut files = HashMap::new();
    files.insert("index.json".to_string(), index_bytes);
    let url = serve(files);

    let root = TempDir::new()?;
    let root_path = root.path();
    let output = run_rv(root_path, Some(&url), None, &["update"]);
    assert!(!output.status.success(), "unsigned remote index must fail");
    assert!(
        stderr(&output).contains("refusing unsigned remote index"),
        "stderr: {}",
        stderr(&output)
    );

    let output = run_rv(root_path, Some(&url), None, &["update", "--insecure"]);
    assert_success(&output);
    Ok(())
}

#[test]
fn signed_remote_index_verifies() -> Result<()> {
    let (_repo_dir, repo) = prepare_repo()?;
    let keys = TempDir::new()?;
    let (private, key) = keygen(keys.path())?;
    assert_success(&sign_repo(&repo, &private));
    let mut files = HashMap::new();
    files.insert("index.json".to_string(), fs::read(repo.join("index.json"))?);
    files.insert(
        "index.json.sig".to_string(),
        fs::read(repo.join("index.json.sig"))?,
    );
    let url = serve(files);
    let root = TempDir::new()?;
    let output = run_rv(root.path(), Some(&url), Some(&key), &["update"]);
    assert_success(&output);
    Ok(())
}

#[test]
fn bad_checksum_is_rejected_before_extraction() -> Result<()> {
    let dir = TempDir::new()?;
    let repo = dir.path().join("main");
    fs::create_dir_all(repo.join("artifacts"))?;
    let zip_path = repo.join("artifacts").join("hello-1.0.0.zip");
    make_zip(&zip_path, "@echo off\r\necho hello\r\n")?;
    fs::create_dir_all(repo.join("packages"))?;
    fs::write(
        repo.join("packages").join("hello-1.0.0.toml"),
        format!(
            "name = \"hello\"\nversion = \"1.0.0\"\ndescription = \"bad checksum\"\nhomepage = \"https://example.invalid\"\nlicense = \"MIT\"\nurl = \"artifacts/hello-1.0.0.zip\"\nsha256 = \"{}\"\nextract = \"zip\"\nbin = [{{ path = \"hello.cmd\", name = \"hello\" }}]\n",
            "0".repeat(64)
        ),
    )?;
    let output = Command::new(rv_bin())
        .args(["repo", "build"])
        .arg(&repo)
        .env("RIVET_ROOT", dir.path())
        .output()?;
    assert_success(&output);
    let index = repo.join("index.json").to_string_lossy().to_string();
    let root = TempDir::new()?;
    let root_path = root.path();
    assert_success(&run_rv(root_path, Some(&index), None, &["update"]));
    let output = run_rv(root_path, Some(&index), None, &["install", "hello"]);
    assert!(
        !output.status.success(),
        "bad checksum must fail the install"
    );
    assert!(
        stderr(&output).contains("sha256 mismatch"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!root_path.join("apps").join("hello").exists());
    Ok(())
}

#[test]
fn keygen_never_prints_the_private_key() -> Result<()> {
    let keys = TempDir::new()?;
    let output = Command::new(rv_bin())
        .args(["repo", "keygen", "--out"])
        .arg(keys.path())
        .output()?;
    assert_success(&output);
    let private = fs::read(keys.path().join("rivet-signing.key"))?;
    let printed = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        !printed.contains(&hex::encode(&private)),
        "keygen printed the private key"
    );
    assert!(!printed.contains("BEGIN PRIVATE KEY"), "keygen printed PEM");

    let output = Command::new(rv_bin())
        .args(["repo", "public-key", "--key"])
        .arg(keys.path().join("rivet-signing.key"))
        .output()?;
    assert_success(&output);
    let public = stdout(&output).trim().to_string();
    assert_eq!(public.len(), 64, "public key hex: {public}");
    Ok(())
}
