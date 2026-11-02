use crate::model::{IndexFile, PackageManifest};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct RepoRecord {
    pub name: String,
    pub kind: String,
    pub location: String,
    pub priority: i64,
    pub enabled: bool,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PackageRecord {
    pub repo: String,
    pub location: String,
    pub manifest: PackageManifest,
}

#[derive(Clone, Debug)]
pub struct InstalledRecord {
    pub name: String,
    pub repo: String,
    pub version: String,
    pub previous_version: Option<String>,
    pub store_path: String,
    pub installed_at: String,
    pub pinned: bool,
}

#[derive(Clone, Debug)]
pub struct ShimRecord {
    pub command: String,
    pub package: String,
    pub target: String,
    pub sidecar: String,
}

pub struct Db {
    pub conn: Connection,
}

fn rebuild_fts_tx(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM packages_fts", [])?;
    conn.execute(
        "INSERT INTO packages_fts(rowid, name, description, tags, binaries)
         SELECT id, name, description, tags, binaries FROM packages",
        [],
    )?;
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening state database {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS repos (
                name TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                location TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 100,
                enabled INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS packages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                homepage TEXT NOT NULL DEFAULT '',
                license TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '',
                binaries TEXT NOT NULL DEFAULT '',
                manifest_json TEXT NOT NULL,
                UNIQUE(repo, name, version)
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS packages_fts USING fts5(
                name, description, tags, binaries
            );

            CREATE TABLE IF NOT EXISTS installed (
                name TEXT PRIMARY KEY,
                repo TEXT NOT NULL,
                version TEXT NOT NULL,
                previous_version TEXT,
                store_path TEXT NOT NULL,
                installed_at TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS shims (
                command TEXT PRIMARY KEY,
                package TEXT NOT NULL,
                target TEXT NOT NULL,
                sidecar TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_packages_name ON packages(name);
            CREATE INDEX IF NOT EXISTS idx_packages_repo ON packages(repo);
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_repo(&self, repo: &RepoRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO repos(name, kind, location, priority, enabled, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(name) DO UPDATE SET
                kind=excluded.kind,
                location=excluded.location,
                priority=excluded.priority,
                enabled=excluded.enabled,
                updated_at=excluded.updated_at",
            params![
                repo.name,
                repo.kind,
                repo.location,
                repo.priority,
                if repo.enabled { 1 } else { 0 },
                repo.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn set_repo_updated(&self, name: &str, updated_at: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET updated_at = ?1 WHERE name = ?2",
            params![updated_at, name],
        )?;
        Ok(())
    }

    pub fn repos(&self) -> Result<Vec<RepoRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT name, kind, location, priority, enabled, updated_at FROM repos ORDER BY priority, name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RepoRecord {
                name: row.get(0)?,
                kind: row.get(1)?,
                location: row.get(2)?,
                priority: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
                updated_at: row.get(5)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn get_repo(&self, name: &str) -> Result<Option<RepoRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT name, kind, location, priority, enabled, updated_at FROM repos WHERE name = ?1",
        )?;
        let row = statement
            .query_row(params![name], |row| {
                Ok(RepoRecord {
                    name: row.get(0)?,
                    kind: row.get(1)?,
                    location: row.get(2)?,
                    priority: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    updated_at: row.get(5)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn remove_repo(&self, name: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM packages WHERE repo = ?1", params![name])?;
        tx.execute("DELETE FROM repos WHERE name = ?1", params![name])?;
        rebuild_fts_tx(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn replace_repo_packages(&self, repo: &str, index: &IndexFile) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM packages WHERE repo = ?1", params![repo])?;
        {
            let mut statement = tx.prepare(
                "INSERT INTO packages(repo, name, version, description, homepage, license, tags, binaries, manifest_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for package in &index.packages {
                let binaries = package.binary_names().join(" ");
                let tags = package.tags.join(" ");
                let manifest_json = serde_json::to_string(package)?;
                statement.execute(params![
                    repo,
                    package.name,
                    package.version,
                    package.description,
                    package.homepage,
                    package.license,
                    tags,
                    binaries,
                    manifest_json
                ])?;
            }
        }
        rebuild_fts_tx(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn rebuild_fts(&self) -> Result<()> {
        rebuild_fts_tx(&self.conn)?;
        Ok(())
    }

    pub fn package_versions(&self, repo: &str, name: &str) -> Result<Vec<PackageRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT p.repo, r.location, p.manifest_json
             FROM packages p JOIN repos r ON r.name = p.repo
             WHERE p.repo = ?1 AND p.name = ?2 AND r.enabled = 1",
        )?;
        let rows = statement.query_map(params![repo, name], |row| {
            let manifest_json: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                manifest_json,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (repo, location, json) = row?;
            let manifest: PackageManifest = serde_json::from_str(&json)?;
            result.push(PackageRecord {
                repo,
                location,
                manifest,
            });
        }
        result.sort_by(|a, b| crate::model::compare_manifests(&a.manifest, &b.manifest));
        Ok(result)
    }

    pub fn packages_by_name(&self, name: &str) -> Result<Vec<PackageRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT p.repo, r.location, p.manifest_json
             FROM packages p JOIN repos r ON r.name = p.repo
             WHERE p.name = ?1 AND r.enabled = 1
             ORDER BY r.priority, p.repo",
        )?;
        let rows = statement.query_map(params![name], |row| {
            let manifest_json: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                manifest_json,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (repo, location, json) = row?;
            let manifest: PackageManifest = serde_json::from_str(&json)?;
            result.push(PackageRecord {
                repo,
                location,
                manifest,
            });
        }
        result.sort_by(|a, b| crate::model::compare_manifests(&b.manifest, &a.manifest));
        Ok(result)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<PackageRecord>> {
        if query.trim().is_empty() {
            let mut statement = self.conn.prepare(
                "SELECT p.repo, r.location, p.manifest_json
                 FROM packages p JOIN repos r ON r.name = p.repo
                 WHERE r.enabled = 1",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            let mut by_package: std::collections::BTreeMap<(String, String), PackageRecord> =
                std::collections::BTreeMap::new();
            for row in rows {
                let (repo, location, json) = row?;
                let record = PackageRecord {
                    repo: repo.clone(),
                    location,
                    manifest: serde_json::from_str(&json)?,
                };
                let key = (repo, record.manifest.name.clone());
                match by_package.get(&key) {
                    Some(existing)
                        if crate::model::compare_manifests(
                            &record.manifest,
                            &existing.manifest,
                        )
                        .is_le() => {}
                    _ => {
                        by_package.insert(key, record);
                    }
                }
            }
            let mut result: Vec<PackageRecord> = by_package.into_values().collect();
            result.sort_by(|a, b| {
                a.manifest
                    .name
                    .cmp(&b.manifest.name)
                    .then_with(|| a.repo.cmp(&b.repo))
            });
            result.truncate(limit);
            return Ok(result);
        }

        let fts_query = format!("\"{}\"*", query.replace('"', " "));
        let sql = "SELECT p.repo, r.location, p.manifest_json
                   FROM packages_fts f
                   JOIN packages p ON p.id = f.rowid
                   JOIN repos r ON r.name = p.repo
                   WHERE packages_fts MATCH ?1 AND r.enabled = 1
                   ORDER BY rank LIMIT ?2";
        let mut statement = match self.conn.prepare(sql) {
            Ok(statement) => statement,
            Err(_) => {
                return self.search_like(query, limit);
            }
        };
        let rows = match statement.query_map(params![fts_query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            Ok(rows) => rows,
            Err(_) => return self.search_like(query, limit),
        };
        let mut result = Vec::new();
        for row in rows {
            let (repo, location, json) = row?;
            result.push(PackageRecord {
                repo,
                location,
                manifest: serde_json::from_str(&json)?,
            });
        }
        Ok(result)
    }

    fn search_like(&self, query: &str, limit: usize) -> Result<Vec<PackageRecord>> {
        let pattern = format!("%{}%", query);
        let mut statement = self.conn.prepare(
            "SELECT p.repo, r.location, p.manifest_json
             FROM packages p JOIN repos r ON r.name = p.repo
             WHERE r.enabled = 1 AND (p.name LIKE ?1 OR p.description LIKE ?1 OR p.binaries LIKE ?1)
             ORDER BY p.name LIMIT ?2",
        )?;
        let rows = statement.query_map(params![pattern, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (repo, location, json) = row?;
            result.push(PackageRecord {
                repo,
                location,
                manifest: serde_json::from_str(&json)?,
            });
        }
        Ok(result)
    }

    pub fn installed(&self, name: &str) -> Result<Option<InstalledRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT name, repo, version, previous_version, store_path, installed_at, pinned
             FROM installed WHERE name = ?1",
        )?;
        let row = statement
            .query_row(params![name], |row| {
                Ok(InstalledRecord {
                    name: row.get(0)?,
                    repo: row.get(1)?,
                    version: row.get(2)?,
                    previous_version: row.get(3)?,
                    store_path: row.get(4)?,
                    installed_at: row.get(5)?,
                    pinned: row.get::<_, i64>(6)? != 0,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT name, repo, version, previous_version, store_path, installed_at, pinned
             FROM installed ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(InstalledRecord {
                name: row.get(0)?,
                repo: row.get(1)?,
                version: row.get(2)?,
                previous_version: row.get(3)?,
                store_path: row.get(4)?,
                installed_at: row.get(5)?,
                pinned: row.get::<_, i64>(6)? != 0,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn save_installed(&self, record: &InstalledRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO installed(name, repo, version, previous_version, store_path, installed_at, pinned)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET
                repo=excluded.repo,
                version=excluded.version,
                previous_version=excluded.previous_version,
                store_path=excluded.store_path,
                installed_at=excluded.installed_at,
                pinned=excluded.pinned",
            params![
                record.name,
                record.repo,
                record.version,
                record.previous_version,
                record.store_path,
                record.installed_at,
                if record.pinned { 1 } else { 0 }
            ],
        )?;
        Ok(())
    }

    /// Persist the installed row and the package's shim rows in one transaction.
    pub fn save_install_state(&self, record: &InstalledRecord, shims: &[ShimRecord]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO installed(name, repo, version, previous_version, store_path, installed_at, pinned)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET
                repo=excluded.repo,
                version=excluded.version,
                previous_version=excluded.previous_version,
                store_path=excluded.store_path,
                installed_at=excluded.installed_at,
                pinned=excluded.pinned",
            params![
                record.name,
                record.repo,
                record.version,
                record.previous_version,
                record.store_path,
                record.installed_at,
                if record.pinned { 1 } else { 0 }
            ],
        )?;
        tx.execute("DELETE FROM shims WHERE package = ?1", params![record.name])?;
        for shim in shims {
            tx.execute(
                "INSERT INTO shims(command, package, target, sidecar) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(command) DO UPDATE SET
                    package=excluded.package, target=excluded.target, sidecar=excluded.sidecar",
                params![shim.command, shim.package, shim.target, shim.sidecar],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_pinned(&self, name: &str, pinned: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE installed SET pinned = ?1 WHERE name = ?2",
            params![if pinned { 1 } else { 0 }, name],
        )?;
        Ok(())
    }

    pub fn remove_installed(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM installed WHERE name = ?1", params![name])?;
        self.conn
            .execute("DELETE FROM shims WHERE package = ?1", params![name])?;
        Ok(())
    }

    pub fn shims_for_package(&self, package: &str) -> Result<Vec<ShimRecord>> {
        let mut statement = self
            .conn
            .prepare("SELECT command, package, target, sidecar FROM shims WHERE package = ?1")?;
        let rows = statement.query_map(params![package], |row| {
            Ok(ShimRecord {
                command: row.get(0)?,
                package: row.get(1)?,
                target: row.get(2)?,
                sidecar: row.get(3)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn all_shims(&self) -> Result<Vec<ShimRecord>> {
        let mut statement = self
            .conn
            .prepare("SELECT command, package, target, sidecar FROM shims ORDER BY command")?;
        let rows = statement.query_map([], |row| {
            Ok(ShimRecord {
                command: row.get(0)?,
                package: row.get(1)?,
                target: row.get(2)?,
                sidecar: row.get(3)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn save_shim(&self, shim: &ShimRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO shims(command, package, target, sidecar) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(command) DO UPDATE SET
                package=excluded.package, target=excluded.target, sidecar=excluded.sidecar",
            params![shim.command, shim.package, shim.target, shim.sidecar],
        )?;
        Ok(())
    }

    pub fn remove_shim(&self, command: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM shims WHERE command = ?1", params![command])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::INDEX_SCHEMA;

    fn manifest(name: &str, version: &str) -> PackageManifest {
        PackageManifest {
            name: name.to_string(),
            version: version.to_string(),
            description: String::new(),
            homepage: String::new(),
            license: String::new(),
            tags: Vec::new(),
            arch: None,
            url: "artifact.zip".to_string(),
            sha256: "0".repeat(64),
            extract: None,
            strip_components: None,
            bin: vec![crate::model::BinSpec::Simple("tool.exe".to_string())],
            depends: Vec::new(),
            persist: Vec::new(),
        }
    }

    fn index(packages: Vec<PackageManifest>) -> IndexFile {
        IndexFile {
            schema: INDEX_SCHEMA,
            generated_at: String::new(),
            packages,
        }
    }

    #[test]
    fn failed_index_replacement_keeps_the_previous_index() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("state.db")).unwrap();
        db.upsert_repo(&RepoRecord {
            name: "main".to_string(),
            kind: "main".to_string(),
            location: "test".to_string(),
            priority: 0,
            enabled: true,
            updated_at: None,
        })
        .unwrap();
        db.replace_repo_packages("main", &index(vec![manifest("alpha", "1.0.0")]))
            .unwrap();
        assert_eq!(db.search("", 100).unwrap().len(), 1);

        let duplicate = index(vec![manifest("beta", "1.0.0"), manifest("beta", "1.0.0")]);
        assert!(
            db.replace_repo_packages("main", &duplicate).is_err(),
            "duplicate package versions must fail the replacement"
        );
        let packages = db.search("", 100).unwrap();
        assert_eq!(
            packages.len(),
            1,
            "old index must survive a failed replacement"
        );
        assert_eq!(packages[0].manifest.name, "alpha");
    }
}
