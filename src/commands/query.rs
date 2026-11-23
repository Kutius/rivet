use super::{Runtime, admin, find_package};
use anyhow::Result;
use semver::Version;

pub(super) fn cmd_search(rt: &Runtime, query: &str) -> Result<()> {
    let records = rt.db.search(query, 100)?;
    if rt.json {
        let rows: Vec<serde_json::Value> = records
            .iter()
            .map(|record| {
                serde_json::json!({
                    "name": record.manifest.name,
                    "version": record.manifest.version,
                    "repo": record.repo,
                    "description": record.manifest.description,
                    "homepage": record.manifest.homepage,
                    "license": record.manifest.license,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("no packages found");
        return Ok(());
    }
    println!(
        "{:<24} {:<12} {:<14} DESCRIPTION",
        "NAME", "VERSION", "REPO"
    );
    for record in &records {
        println!(
            "{:<24} {:<12} {:<14} {}",
            record.manifest.name,
            record.manifest.version,
            record.repo,
            truncate(&record.manifest.description, 70)
        );
    }
    Ok(())
}

pub(super) fn cmd_list(rt: &Runtime) -> Result<()> {
    let installed = rt.db.list_installed()?;
    if rt.json {
        let rows: Vec<serde_json::Value> = installed
            .iter()
            .map(|item| {
                serde_json::json!({
                    "name": item.name,
                    "repo": item.repo,
                    "version": item.version,
                    "previous_version": item.previous_version,
                    "pinned": item.pinned,
                    "store_path": item.store_path,
                    "installed_at": item.installed_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if installed.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    println!(
        "{:<24} {:<12} {:<14} {:<8} PATH",
        "NAME", "VERSION", "REPO", "PINNED"
    );
    for item in installed {
        println!(
            "{:<24} {:<12} {:<14} {:<8} {}",
            item.name,
            item.version,
            item.repo,
            if item.pinned { "yes" } else { "-" },
            item.store_path
        );
    }
    Ok(())
}

pub(super) fn cmd_status(rt: &Runtime, check: bool) -> Result<()> {
    if check && let Err(error) = admin::cmd_update(rt) {
        eprintln!("warning: metadata refresh failed; using cached index: {error}");
    }
    let installed = rt.db.list_installed()?;
    let mut outdated = 0usize;
    let mut rows = Vec::new();
    for item in installed {
        let versions = rt.db.package_versions(&item.repo, &item.name)?;
        let latest = versions
            .last()
            .map(|record| record.manifest.version.clone());
        let status = if item.pinned {
            "pinned".to_string()
        } else {
            match latest {
                None => "missing metadata".to_string(),
                Some(ref latest) => {
                    if Version::parse(latest)? > Version::parse(&item.version)? {
                        outdated += 1;
                        "outdated".to_string()
                    } else {
                        "ok".to_string()
                    }
                }
            }
        };
        rows.push(serde_json::json!({
            "name": item.name,
            "repo": item.repo,
            "installed": item.version,
            "latest": latest,
            "pinned": item.pinned,
            "status": status,
        }));
    }
    if rt.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("no packages installed");
        return Ok(());
    }
    println!("{:<24} {:<12} {:<12} STATUS", "NAME", "INSTALLED", "LATEST");
    for row in &rows {
        println!(
            "{:<24} {:<12} {:<12} {}",
            row["name"].as_str().unwrap_or("-"),
            row["installed"].as_str().unwrap_or("-"),
            row["latest"].as_str().unwrap_or("-"),
            row["status"].as_str().unwrap_or("-")
        );
    }
    if outdated > 0 {
        println!("{outdated} package(s) can be upgraded with 'rivet upgrade'");
    }
    Ok(())
}

pub(super) fn cmd_info(rt: &Runtime, spec: &str) -> Result<()> {
    let record = find_package(&rt.db, spec)?;
    let versions = rt
        .db
        .package_versions(&record.repo, &record.manifest.name)?;
    let version_list: Vec<String> = versions
        .iter()
        .map(|version| version.manifest.version.clone())
        .collect();
    if rt.json {
        let value = serde_json::json!({
            "name": record.manifest.name,
            "repo": record.repo,
            "version": record.manifest.version,
            "description": record.manifest.description,
            "homepage": record.manifest.homepage,
            "license": record.manifest.license,
            "url": record.manifest.url,
            "sha256": record.manifest.sha256,
            "tags": record.manifest.tags,
            "fonts": record.manifest.fonts,
            "depends": record.manifest.depends,
            "persist": record.manifest.persist,
            "versions": version_list,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("name:        {}", record.manifest.name);
    println!("repo:        {}", record.repo);
    println!("version:     {}", record.manifest.version);
    println!("description: {}", record.manifest.description);
    println!("homepage:    {}", record.manifest.homepage);
    println!("license:     {}", record.manifest.license);
    println!("url:         {}", record.manifest.url);
    println!("sha256:      {}", record.manifest.sha256);
    if !record.manifest.depends.is_empty() {
        println!("depends:     {}", record.manifest.depends.join(", "));
    }
    if !record.manifest.fonts.is_empty() {
        println!("fonts:       {}", record.manifest.fonts.join(", "));
    }
    if !record.manifest.persist.is_empty() {
        println!("persist:     {}", record.manifest.persist.join(", "));
    }
    println!("versions:    {}", version_list.join(", "));
    Ok(())
}

fn truncate(value: &str, length: usize) -> String {
    if value.chars().count() <= length {
        value.to_string()
    } else {
        let mut result: String = value.chars().take(length.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}
