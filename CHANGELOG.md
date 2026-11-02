# Changelog

## 0.1.0-beta.2 — catalog and installer

### Added

- Hosted, signed main catalog at https://kutius.github.io/rivet/index.json with ripgrep, fd, bat, jq, fzf, delta, yq, and zoxide, in addition to the hello demo.
- `install.ps1`: downloads the release, verifies SHA256SUMS, runs `rv init`, and refreshes the catalog. Supports `-NoPath`, `-NoUpdate`, and `-Version`.
- `rv init` now writes the default catalog URL and trusted Ed25519 public key, so a fresh install can run `rv update` without manual configuration.
- GitHub Pages workflow that publishes repo/main and install.ps1.

### Changed

- Release packaging now ships install.ps1 as a separate asset.
- README and INSTALL describe the one-command install and the default catalog.

## 0.1.0-beta.1 — public beta

Rivet is a portable-first Windows package manager built as one native Rust binary.

### Added

- rv update, search, list, status, info, install, remove, upgrade, rollback, cleanup, doctor, verify, pin, unpin, tap, repo, config, and init.
- Generated main index; the client does not clone the main repository.
- SQLite FTS search and package metadata cache.
- Immutable version store, current junction, persist junctions, native rv-shim.exe shims.
- SHA-256 verification for every artifact and content-addressed blob cache.
- Ed25519 index signatures with trusted-key pinning; remote indexes fail closed unless --insecure is passed.
- Process-wide mutation lock; transactional index and installed-state writes; interrupted-install recovery.
- Reverse-dependency protection on remove with --force.
- rv doctor --fix for broken current links and missing shims.
- JSON output for search, list, status, and info.
- rv init --no-path and WM_SETTINGCHANGE broadcast after a PATH update.
- Release packaging script and checksums.

### Not in this beta

- Scoop manifests, buckets, and PowerShell hooks.
- MSI/EXE system installers.
- Artifact mirroring or a hosted CDN.
- Shell completion generation.
- rv cleanup --dry-run.
- Self-uninstall command.
- A full dependency solver; dependencies are resolved recursively to the latest compatible package unless a version is pinned in the manifest.

## Unreleased

- Nothing yet.
