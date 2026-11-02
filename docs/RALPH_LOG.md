# Ralph Log

## Round 0 — gate setup

- Defined the public beta gate in docs/BETA.md.
- No product code changed yet.
- Next: audit the current tree against B1–B8 and start with the highest-risk P0.

## Round 1 — B1 trusted catalog

- Baseline before changes: `cargo test --offline` green (4 e2e tests).
- Added Ed25519 index signing and verification using `ring` 0.17, which was already in `Cargo.lock` through rustls, so no new crate download was needed.
- New `src/trust.rs`: `keygen` (PKCS#8 private key + hex public key), `sign`, `load_private_key`, `verify_signature`, `verify_index`, `Trust`.
- `index::load_index(location, trust)` now verifies the exact index bytes before parsing. HTTP indexes fetch `<url>.sig` and fail closed when it is missing, signed by an untrusted key, or invalid. Local file/directory indexes may stay unsigned; a local `.sig` is verified when present.
- Commands: `rv repo keygen [--out DIR]`, `rv repo sign PATH --key KEY`, `rv repo public-key --key KEY`. Config: `trusted_key`/`trusted_keys`, `allow_unsigned_remote`; env `RIVET_TRUSTED_KEY`; global `--insecure` override.
- Tests: `tests/trust.rs` adds 6 tests (signed local index + tamper/wrong-key/no-key failures, signed remote index, unsigned remote fail-closed + `--insecure`, unsigned local index, keygen never prints the private key, bad checksum rejected). Suite: 4 e2e + 6 trust, all pass.
- Docs: README "Signing the index", new `docs/SECURITY.md`, `docs/BETA.md` B1 evidence, `.gitignore` ignores `*.key`.
- Remaining P0: B2–B8. Clippy still reports pre-existing style warnings in `src/commands.rs`, `src/repo.rs`, and `src/store.rs` (B7).
- Next: B2 mutation safety — process lock with stale-lock recovery, reverse-dependency protection with `--force`, and `rv doctor --fix`.

## Round 2 — B2 mutation safety

- Added `src/lock.rs`: `LockGuard` owns the open file handle (exclusive `share_mode(0)`), so the OS releases the lock on process exit and a crashed process cannot leave a permanent lock. `rv` acquires it for every mutating command (`update`, `install`, `remove`, `upgrade`, `rollback`, `cleanup`, `tap add/remove`, `config set`, `init`, `doctor --fix`); read-only commands stay lock-free.
- `rv remove` now refuses to remove a package that another installed package depends on, with an actionable message and a `--force` override.
- `rv doctor --fix` repairs a broken `current` junction and missing shims from the intact store; a missing store is reported as unrepairable. `rv doctor` (without `--fix`) labels repairable problems.
- Index replacement (package rows + FTS) and installed-state writes (installed row + shim rows) are now single transactions. `Db::save_install_state` replaced the separate `save_installed`/`save_shim` calls.
- `store::recover_interrupted` runs at the start of every mutating command: removes `store/<name>/.tmp-*` staging directories and shim files that have no database record.
- Tests: new `tests/safety.rs` (5 tests) and unit tests in `src/lock.rs` (2) and `src/db.rs` (1). Suite: 4 e2e + 6 trust + 5 safety + 3 unit, all pass.
- Docs: README "Concurrency and recovery", docs/BETA.md B2 evidence.
- Remaining P0: B3–B8 (pin/verify, init PATH broadcast, JSON output, docs set, clippy, performance). Clippy still has pre-existing style warnings.
- Next: B3 lifecycle completeness — `rv pin`/`rv unpin`, `rv verify`, and rollback coverage after failed upgrade and doctor repair.

## Round 3 — B3 lifecycle completeness

- Added `rv pin` / `rv unpin`, a PINNED column in `rv list`, and a pinned state in `rv status`. Pinned packages are skipped by `rv upgrade --all`; explicitly upgrading one fails unless `--force` is passed.
- Added `rv verify`: checks the installed store path, manifest presence and version, current junction target, shim executable and sidecar, and recorded shim targets; exits non-zero on damage.
- `rv doctor` continues to distinguish repairable link/shim damage from a missing store, and `--fix` restores the repairable cases.
- Tests: new `tests/lifecycle.rs` with 3 tests (pin lifecycle, verify/doctor repair, rollback after failed upgrade and doctor fix). Suite: 18 tests pass.
- Docs: BETA B3 evidence.
- Remaining P0: B4–B8.
- Next: B4 install/first run — `rv init --no-path`, environment broadcast, release zip contents.

## Round 4 — B4 install and first run

- Added `rv init --no-path`; normal init still installs `rv.exe`/`rv-shim.exe`, checks PATH case-insensitively, appends once, and sends `WM_SETTINGCHANGE` through `SendMessageTimeoutW` after changing `HKCU\\Environment\\Path`.
- Bumped the package version to `0.1.0-beta.1`; `rv --version` now reports it.
- Added `scripts/release.ps1`: builds release binaries, copies them to `dist`, creates `dist/rivet-v0.1.0-beta.1-windows-x64.zip`, an internal `SHA256SUMS`, and `dist/SHA256SUMS`.
- Tests: new `tests/install.rs` (3 tests: no-path install, version, no leftover lock) and PATH helper unit tests. Suite: 26 tests pass.
- Release zip verified: `rv.exe`, `rv-shim.exe`, README, LICENSE, docs, internal SHA256SUMS.
- Remaining P0: B5–B8.
- Next: B5 JSON output, status --check offline behavior, error/exit-code coverage.
