# Changelog

## 0.1.0-beta.9 — modern terminal UI and automatic Pages deployment

### Added

- Zero-dependency interactive terminal UI (`src/ui.rs`):
  - Smooth horizontal download progress bars (`━━━───`) with percentage, transferred/total bytes, and live transfer speed for single and concurrent multi-package downloads.
  - Braille spinners (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) during extraction, activation, removal, rollback, and catalog refresh.
  - Human-friendly completion lines with green `✓`, elapsed time, `(cached)` indicator, `→ ready to run: <command>` hints, preserved persist-data notices on uninstall, and multi-package batch summaries.
  - Automatic TTY detection and `NO_COLOR` support so piped/CI output stays clean and deterministic.

### Fixed

- `Catalog update` workflow now triggers `Pages` deployment automatically after pushing updated signed indexes.

## 0.1.0-beta.8 — architecture cleanup, concurrent downloads, and scalable catalog

### Added

- Concurrent artifact prefetching during `rivet install` and `rivet upgrade` honoring `config.concurrency`, with process-wide HTTP connection pooling.
- `--verbose` diagnostic logging to `stderr` for lock acquisition, index sync, artifact URLs, and concurrent prefetching.
- `rv-repo update --concurrency`, `--allow-failures`, and `--failures-out`, plus `{version}` asset templates and automatic versioned-binary path reconciliation (fixing `restic` and similar releases).
- `rv-repo import-scoop`: strict, portable-only importer for Scoop JSON manifests (rejects PowerShell hooks, installer scripts, multi-URLs, and non-SHA256 hashes).
- `test_args` manifest field and 4-shard parallel `Catalog verify` workflow; `Catalog update` now auto-syncs `sources.toml` and manages a `Catalog update failures` GitHub Issue on partial failures.

### Changed

- Unified `index.json` into a single `packages` array (removed the legacy `fonts` top-level compatibility split).
- Replaced `reg.exe` subprocess calls for user `PATH` management with native Win32 Registry APIs shared in `src/winreg.rs` (Unicode-safe and supports arbitrary `PATH` lengths).
- Hardened tar extraction to skip symlinks/hardlinks/special entries and write only validated regular files and directories.
- Modularized `src/commands/` (`mod`, `query`, `lifecycle`, `health`, `admin`) and consolidated self-lifecycle/PATH management in `src/selfmanage.rs` (hard-linking `rivet.exe` to `rv.exe`).
- Fixed `rivet status --check` mutation locking, transactional `remove_installed`, batch removal of dependent+dependency together, preflight font/shim conflict checks before activation, `rivet doctor --fix` for `rv-shim.exe`, and `rivet config set` validation.
- Untracked generated release binaries under `dist/`.

### Catalog

- Expanded the main catalog from 53 to 81 manifests, adding actionlint, age, ast-grep, buf, caddy, cloudflared, dbmate, difftastic, dive, dotenvx, duf, git-cliff, gitleaks, glow, grpcurl, gum, hadolint, k9s, micro, mise, nushell, scc, sops, sqlc, tealdeer, tree-sitter, typos, and yt-dlp.

### Tests

- 51 tests covering versioned binary reconciliation, tar symlink rejection, concurrent blob prefetching, Scoop manifest conversion/filtering, batch dependent removal, config validation, and `status --check` locking.

## 0.1.0-beta.7 — fonts and bulk catalog

### Added

- `rv-repo sync`: import every missing GitHub repository from a curated `sources.toml`, with per-package name/asset/bin overrides, `--dry-run`, `--limit`, and per-source failure isolation.
- `scripts/verify-catalog.ps1` and a weekly Catalog verify workflow that installs every package in a temporary root, verifies hashes, and smoke-tests exposed commands.
- Improved release asset selection for bulk imports: two-part versions, scoped tags, `win32-x64` names, main-package preference, and single-executable assets.
- `fonts` manifest field: install .ttf/.otf/.ttc/.otc files into the per-user font directory, register them under HKCU, and remove them on uninstall. Font packages may omit `bin`.
- `rivet verify` and `rivet doctor --fix` now check and repair font files and registry values.

### Catalog

- Expanded the main catalog from 22 to 53 package versions (52 packages), including air, biome, bun, cargo-binstall, cargo-deny, dasel, deno, dprint, duckdb, fira-code, fq, gh, git-lfs, gofumpt, gojq, golangci-lint, hexyl, jetbrains-mono, lazydocker, lazygit, mdbook, miller, pandoc, pnpm, rclone, restic, ruff, shfmt, task, uv, and xsv.
- Added `repo/main/sources.toml` as the curated source list for future batch imports.

## 0.1.0-beta.6 — maintainer tooling split

### Changed

- Removed the `repo` command family from the client. Package-repository maintenance now lives in the separate `rv-repo` binary, built from source with `.\scripts\repo.ps1`; the release archive still contains only `rv.exe` and `rv-shim.exe`.
- The daily catalog workflow now builds `rv-repo`, updates GitHub-hosted packages, signs the index with the `RIVET_SIGNING_KEY_B64` Actions secret, smoke-installs changed packages, and pushes the result.

### Catalog

- Added hyperfine, dust, bottom, just, starship, xh, sd, procs, eza, lsd, watchexec, and typst to the main catalog.

### Tests

- 38 tests, including the maintainer/client split and the existing install, trust, safety, and shim coverage.

## 0.1.0-beta.5 — PowerShell alias and catalog maintenance

### Added

- `rivet.exe` is installed next to `rv.exe`; PowerShell users get a command that is not shadowed by the built-in `rv` alias for `Remove-Variable`.
- `rivet repo add owner/repo`: generate a manifest from the latest GitHub release, auto-detect the Windows x64 asset and executable, verify SHA-256, and rebuild the index.
- `rivet repo update [--check] [--package NAME]`: refresh every GitHub-hosted package to its latest release.
- `GITHUB_TOKEN` / `GH_TOKEN` support for higher GitHub API rate limits.

### Changed

- User-facing PowerShell examples use `rivet`; `rv` remains available for `cmd.exe` and compatible scripts.
- `rivet doctor --fix` restores a missing `rivet.exe` alias.
- `install.ps1` checks that `rivet.exe` was created and prints `rivet` in its guidance.

### Tests

- 38 tests, including the alias update path, GitHub repository/tag parsing, and command-name normalization.

## 0.1.0-beta.4 — self-management

### Added

- `rv self-update [--force]`: fetch the latest release, verify SHA256SUMS, stage it, and replace the binaries after the current process exits.
- `rv self-uninstall --yes`: remove all packages (including persist data), remove the Rivet PATH entry, and delete the root after the current process exits.
- `rv cleanup --dry-run`: list removable store versions, orphan stores, and cache entries without deleting them.
- `rv install` and `rv remove` are idempotent: installing an already-installed package is a no-op; removing an absent package succeeds without changes.

### Changed

- `rv cleanup` only takes the mutation lock when it will actually delete something.

### Tests

- 34 tests, including PATH entry removal, dry-run lock behavior, idempotent install/remove, and the native-exe shim fallback.

## 0.1.0-beta.3 — symlink-first commands and release automation

### Added

- Native `.exe` commands are installed as direct symlinks when Developer Mode allows unprivileged symlink creation; the installer falls back to a shim when symlink creation fails.
- `RIVET_DISABLE_SYMLINKS=1` forces launcher shims and `rv doctor --fix` repairs existing commands.
- Release workflow creates a draft GitHub release and uploads the zip, binaries, installer, and checksums.

### Changed

- `rv-shim.exe` is now built with a size-optimized release profile: 2.66 MB → 280 KB.
- `rv.exe` is now 3.75 MB (previously 7.67 MB).
- `rv verify` and `rv doctor` understand both symlink and launcher shims.
- Orphan symlink cleanup during interrupted-install recovery.

### Tests

- 31 tests, including a native `.exe` fallback test with symlinks disabled.

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
