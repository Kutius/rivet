use crate::store::ShimSpec;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn sidecar_for_current_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating shim executable")?;
    Ok(exe.with_extension("rvshim"))
}

pub fn run_from_current_exe() -> Result<()> {
    let sidecar = sidecar_for_current_exe()?;
    let text = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("reading shim sidecar {}", sidecar.display()))?;
    let spec: ShimSpec = serde_json::from_str(&text)
        .with_context(|| format!("parsing shim sidecar {}", sidecar.display()))?;
    let user_args: Vec<String> = std::env::args().skip(1).collect();
    run_spec(&spec, &user_args)
}

fn run_spec(spec: &ShimSpec, user_args: &[String]) -> Result<()> {
    let target = Path::new(&spec.target);
    if !target.exists() {
        bail!("shim target does not exist: {}", target.display());
    }
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut all_args = spec.args.clone();
    all_args.extend_from_slice(user_args);

    let mut command = if extension == "ps1" {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        cmd.arg(target);
        cmd.args(&all_args);
        cmd
    } else if extension == "py" {
        let mut cmd = Command::new("python.exe");
        cmd.arg(target);
        cmd.args(&all_args);
        cmd
    } else if extension == "jar" {
        let mut cmd = Command::new("java.exe");
        cmd.arg("-jar").arg(target);
        cmd.args(&all_args);
        cmd
    } else {
        let mut cmd = Command::new(target);
        cmd.args(&all_args);
        cmd
    };

    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    let status = command
        .status()
        .with_context(|| format!("launching {}", target.display()))?;
    let code = status.code().unwrap_or(1);
    std::process::exit(code);
}
