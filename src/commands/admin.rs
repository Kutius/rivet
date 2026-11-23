use super::{Runtime, lifecycle};
use crate::cli::{ConfigCommands, TapCommands};
use crate::config::{Config, Paths};
use crate::repo;
use crate::selfmanage;
use anyhow::{Result, bail};

pub(super) fn cmd_update(rt: &Runtime) -> Result<()> {
    let trust = rt.config.trust();
    let has_main = rt.config.main_index_location().is_some() || rt.db.get_repo("main")?.is_some();
    if has_main {
        rt.log(format!(
            "syncing main index from {:?}",
            rt.config.main_index_location()
        ));
        repo::sync_main(&rt.db, &rt.paths, &rt.config, &trust)?;
    } else {
        println!("main index is not configured; updating taps only");
    }
    repo::sync_taps(&rt.db, &rt.paths, &trust)?;
    Ok(())
}

pub(super) fn cmd_tap(rt: &Runtime, command: TapCommands) -> Result<()> {
    let trust = rt.config.trust();
    match command {
        TapCommands::Add { name, url } => {
            repo::add_tap(&rt.db, &rt.paths, &name, url.as_deref(), &trust)
        }
        TapCommands::List => {
            let repos = rt.db.repos()?;
            if repos.iter().all(|repo| repo.kind != "tap") {
                println!("no taps configured");
            } else {
                for repo in repos.into_iter().filter(|repo| repo.kind == "tap") {
                    println!(
                        "{}\t{}\t{}",
                        repo.name,
                        repo.location,
                        repo.updated_at.unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        TapCommands::Remove { name } => repo::remove_tap(&rt.db, &rt.paths, &name),
    }
}

fn format_config_value(config: &Config, key: &str) -> Result<String> {
    match key {
        "main_index" => Ok(config.main_index.clone()),
        "concurrency" => Ok(config.concurrency.to_string()),
        "keep_versions" => Ok(config.keep_versions.to_string()),
        "trusted_key" | "trusted_keys" => Ok(config.trusted_keys.join(",")),
        "allow_unsigned_remote" => Ok(config.allow_unsigned_remote.to_string()),
        _ => bail!("unknown config key '{key}'"),
    }
}

pub(super) fn cmd_config(paths: &Paths, command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Path => {
            println!("{}", paths.config_file.display());
            Ok(())
        }
        ConfigCommands::Get { key } => {
            let config = Config::load(paths)?;
            println!("{}", format_config_value(&config, &key)?);
            Ok(())
        }
        ConfigCommands::Set { key, value } => {
            let mut config = Config::load(paths)?;
            match key.as_str() {
                "main_index" => config.main_index = value,
                "concurrency" => {
                    let parsed: usize = value.parse()?;
                    if parsed == 0 {
                        bail!("concurrency must be at least 1");
                    }
                    config.concurrency = parsed;
                }
                "keep_versions" => config.keep_versions = value.parse()?,
                "trusted_key" | "trusted_keys" => {
                    for entry in value.split(',') {
                        let entry = entry.trim().to_ascii_lowercase();
                        if entry.is_empty() {
                            continue;
                        }
                        if entry.len() != 64 || !entry.chars().all(|c| c.is_ascii_hexdigit()) {
                            bail!("invalid trusted key '{entry}': expected 64 hex characters");
                        }
                        if !config.trusted_keys.contains(&entry) {
                            config.trusted_keys.push(entry);
                        }
                    }
                }
                "allow_unsigned_remote" => config.allow_unsigned_remote = value.parse()?,
                _ => bail!("unknown config key '{key}'"),
            }
            config.save(paths)?;
            println!("{} = {}", key, format_config_value(&config, &key)?);
            Ok(())
        }
    }
}

pub(super) fn cmd_self_uninstall(rt: &Runtime, yes: bool) -> Result<()> {
    if !yes {
        bail!("refusing to uninstall without --yes");
    }
    let names: Vec<String> = rt
        .db
        .list_installed()?
        .into_iter()
        .map(|item| item.name)
        .collect();
    if !names.is_empty() {
        lifecycle::cmd_remove(rt, &names, true, true)?;
    }
    selfmanage::remove_bin_from_user_path(&rt.paths)?;
    let log = selfmanage::schedule_uninstall(&rt.paths)?;
    println!("removed Rivet packages and PATH entry");
    println!(
        "the remaining {} will be deleted after this process exits",
        rt.paths.root.display()
    );
    println!("log: {}", log.display());
    Ok(())
}
