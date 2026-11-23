use super::{Runtime, manifest_font_sources, reconcile_fonts, reconcile_shims};
use crate::font;
use crate::model::PackageManifest;
use crate::selfmanage;
use crate::store;
use anyhow::{Result, bail};
use std::fs;
use std::path::Path;

pub(super) fn cmd_verify(rt: &Runtime) -> Result<()> {
    let db = &rt.db;
    let paths = &rt.paths;
    let mut problems = 0usize;
    for item in db.list_installed()? {
        let store_path = Path::new(&item.store_path);
        let manifest_path = store_path.join("rivet-manifest.json");
        if !store_path.is_dir() {
            println!(
                "damage:    {} store path missing: {}",
                item.name, item.store_path
            );
            problems += 1;
            continue;
        }
        if !manifest_path.is_file() {
            println!(
                "damage:    {} manifest missing: {}",
                item.name,
                manifest_path.display()
            );
            problems += 1;
            continue;
        }
        let manifest: PackageManifest =
            match serde_json::from_str(&fs::read_to_string(&manifest_path)?) {
                Ok(manifest) => manifest,
                Err(error) => {
                    println!("damage:    {} manifest invalid: {error}", item.name);
                    problems += 1;
                    continue;
                }
            };
        if manifest.version != item.version {
            println!(
                "damage:    {} recorded version {} but store manifest is {}",
                item.name, item.version, manifest.version
            );
            problems += 1;
        }
        let font_sources = match manifest_font_sources(store_path, &manifest) {
            Ok(sources) => sources,
            Err(error) => {
                println!("damage:    {} font source missing: {error:#}", item.name);
                problems += 1;
                Vec::new()
            }
        };
        let font_records = db.fonts_for_package(&item.name)?;
        if !font_sources.is_empty() && font_records.len() != font_sources.len() {
            println!(
                "damage:    {} expected {} font file(s), registered {}",
                item.name,
                font_sources.len(),
                font_records.len()
            );
            problems += 1;
        }
        if font_sources.is_empty() && !font_records.is_empty() {
            println!("damage:    {} has stale font registrations", item.name);
            problems += 1;
        }
        for problem in font::verify(&font_records, &font::registry_key()) {
            println!("damage:    {} {problem}", item.name);
            problems += 1;
        }
        let link = store::current_link(paths, &item.name);
        let link_ok = match (fs::canonicalize(&link), fs::canonicalize(store_path)) {
            (Ok(link_target), Ok(store_target)) => link_target == store_target,
            _ => false,
        };
        if !link_ok {
            println!(
                "damage:    {} current link missing or wrong: {}",
                item.name,
                link.display()
            );
            problems += 1;
        }
        let records = db.shims_for_package(&item.name)?;
        for bin in &manifest.bin {
            let command = bin.command_name();
            let shim_path = paths.bin.join(format!("{command}.exe"));
            if fs::symlink_metadata(&shim_path).is_err() {
                println!(
                    "damage:    {} shim missing: {}",
                    item.name,
                    shim_path.display()
                );
                problems += 1;
                continue;
            }
            let Some(record) = records
                .iter()
                .find(|shim| shim.command.eq_ignore_ascii_case(&command))
            else {
                println!(
                    "damage:    {} shim record missing: {}",
                    item.name,
                    shim_path.display()
                );
                problems += 1;
                continue;
            };
            if record.sidecar.is_empty() {
                let metadata = fs::symlink_metadata(&shim_path)?;
                if !metadata.file_type().is_symlink() {
                    println!(
                        "damage:    {} shim is not a symlink: {}",
                        item.name,
                        shim_path.display()
                    );
                    problems += 1;
                }
            } else {
                let sidecar_path = Path::new(&record.sidecar);
                match fs::read_to_string(sidecar_path) {
                    Ok(text) => {
                        if serde_json::from_str::<crate::shim::ShimSpec>(&text).is_err() {
                            println!(
                                "damage:    {} shim sidecar invalid: {}",
                                item.name,
                                sidecar_path.display()
                            );
                            problems += 1;
                        }
                    }
                    Err(_) => {
                        println!(
                            "damage:    {} shim sidecar missing: {}",
                            item.name,
                            sidecar_path.display()
                        );
                        problems += 1;
                    }
                }
            }
            if !Path::new(&record.target).exists() {
                println!(
                    "damage:    {} shim target missing: {} -> {}",
                    item.name, record.command, record.target
                );
                problems += 1;
            }
        }
    }
    if problems == 0 {
        println!("verify:    ok");
        Ok(())
    } else {
        bail!("verify found {problems} problem(s)")
    }
}

pub(super) fn cmd_doctor(rt: &Runtime, fix: bool) -> Result<()> {
    let db = &rt.db;
    let paths = &rt.paths;
    println!("root:      {}", paths.root.display());
    println!("database:  {}", paths.db_file.display());
    let mut ok = true;
    let mut repairable = 0usize;
    let rivet_missing = !paths.bin.join("rivet.exe").exists();
    let shim_missing = !paths.bin.join("rv-shim.exe").exists();
    if rivet_missing || shim_missing {
        if fix {
            selfmanage::install_self(paths)?;
            if rivet_missing && paths.bin.join("rivet.exe").exists() {
                println!("fixed:     rivet.exe");
            }
            if shim_missing && paths.bin.join("rv-shim.exe").exists() {
                println!("fixed:     rv-shim.exe");
            }
        } else {
            if rivet_missing {
                println!(
                    "bin:       missing {} (repairable: rivet doctor --fix)",
                    paths.bin.join("rivet.exe").display()
                );
                ok = false;
                repairable += 1;
            }
            if shim_missing {
                println!(
                    "bin:       missing {} (repairable: rivet doctor --fix)",
                    paths.bin.join("rv-shim.exe").display()
                );
                ok = false;
                repairable += 1;
            }
        }
    }
    if paths.bin.join("rv-shim.exe").exists() {
        println!("bin:       ok");
    } else if fix {
        println!(
            "bin:       missing {} (run 'rivet init' from a release zip)",
            paths.bin.join("rv-shim.exe").display()
        );
        ok = false;
    }
    if store::path_contains(paths) {
        println!("PATH:      ok");
    } else {
        println!(
            "PATH:      {} is not in PATH (run 'rivet init')",
            paths.bin.display()
        );
        ok = false;
    }
    for item in db.list_installed()? {
        let store_path = Path::new(&item.store_path);
        let manifest_path = store_path.join("rivet-manifest.json");
        if !store_path.is_dir() || !manifest_path.is_file() {
            println!(
                "install:   missing store for {} at {} (cannot repair; reinstall it)",
                item.name, item.store_path
            );
            ok = false;
            continue;
        }
        let manifest: PackageManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        let link = store::current_link(paths, &item.name);
        let link_ok = match (fs::canonicalize(&link), fs::canonicalize(store_path)) {
            (Ok(link_target), Ok(store_target)) => link_target == store_target,
            _ => false,
        };
        if !link_ok {
            if fix {
                store::switch_current(paths, &item.name, &item.version)?;
                println!("fixed:     current link {} -> {}", item.name, item.version);
            } else {
                println!(
                    "current:   broken link for {} (repairable: rivet doctor --fix)",
                    item.name
                );
                repairable += 1;
                ok = false;
            }
        }
        let missing: Vec<String> = manifest
            .bin
            .iter()
            .map(|bin| bin.command_name())
            .filter(|command| {
                fs::symlink_metadata(paths.bin.join(format!("{command}.exe"))).is_err()
            })
            .collect();
        let font_sources = manifest_font_sources(store_path, &manifest);
        if fix {
            // Reconcile every package so doctor --fix also switches between
            // symlink and launcher modes when the environment changes.
            let shims = reconcile_shims(db, paths, &item.name, &manifest)?;
            let fonts = match &font_sources {
                Ok(sources) => reconcile_fonts(db, &item.name, sources)?,
                Err(error) => {
                    println!(
                        "font:      {} source missing: {error:#} (cannot repair; reinstall it)",
                        item.name
                    );
                    ok = false;
                    db.fonts_for_package(&item.name)?
                }
            };
            db.save_install_state(&item, &shims, &fonts)?;
            if !missing.is_empty() {
                println!(
                    "fixed:     shims for {} ({})",
                    item.name,
                    missing.join(", ")
                );
            }
        } else {
            if let Err(error) = &font_sources {
                println!(
                    "font:      {} source missing: {error:#} (cannot repair; reinstall it)",
                    item.name
                );
                ok = false;
            }
            let fonts = db.fonts_for_package(&item.name)?;
            if let Ok(sources) = &font_sources
                && sources.len() != fonts.len()
            {
                println!(
                    "font:      {} expected {} font file(s), registered {} (repairable: rivet doctor --fix)",
                    item.name,
                    sources.len(),
                    fonts.len()
                );
                repairable += 1;
                ok = false;
            }
            if font_sources
                .as_ref()
                .is_ok_and(|sources| sources.is_empty())
                && !fonts.is_empty()
            {
                println!(
                    "font:      {} has stale font registrations (repairable: rivet doctor --fix)",
                    item.name
                );
                repairable += 1;
                ok = false;
            }
            for problem in font::verify(&fonts, &font::registry_key()) {
                println!(
                    "font:      {} {problem} (repairable: rivet doctor --fix)",
                    item.name
                );
                repairable += 1;
                ok = false;
            }
            if !missing.is_empty() {
                println!(
                    "shim:      missing {} (repairable: rivet doctor --fix)",
                    missing.join(", ")
                );
                repairable += 1;
                ok = false;
            }
        }
        for shim in db.shims_for_package(&item.name)? {
            if !Path::new(&shim.target).exists() {
                println!(
                    "shim:      broken {} -> {} (repairable: rivet doctor --fix)",
                    shim.command, shim.target
                );
                repairable += 1;
                ok = false;
            }
            if !shim.sidecar.is_empty() && !Path::new(&shim.sidecar).exists() {
                println!(
                    "shim:      sidecar missing {} (repairable: rivet doctor --fix)",
                    shim.sidecar
                );
                repairable += 1;
                ok = false;
            }
        }
    }
    if ok {
        println!("status:    ok");
    } else if repairable > 0 {
        println!("status:    issues found ({repairable} repairable with 'rivet doctor --fix')");
    } else {
        println!("status:    issues found");
    }
    Ok(())
}
