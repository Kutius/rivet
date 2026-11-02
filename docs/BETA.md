# Rivet Public Beta Gate

Definition: Rivet is public-beta ready when a Windows user can download a signed release, install it without admin rights, install and manage packages from a signed catalog and a third-party tap, recover from the common broken states, and understand exactly what is supported — with no known P0/P1 defect.

This file is the release gate, not a wish list. Do not check a box without a command, test name, file, or measured result recorded next to it.

## P0 — release blockers

### B1. Trusted catalog

- [x] The official main index can be signed with Ed25519 and the client verifies it before parsing.
- [x] Verification is mandatory for HTTP indexes. rv update fails closed on a missing or invalid signature.
- [x] Local file:// and directory indexes may be unsigned for development; an explicit insecure override exists for remote indexes.
- [x] Maintainer commands: key generation, signing, and public-key reporting are documented and tested.
- [x] Every package artifact still requires a SHA-256 and is verified before extraction.
- [x] Signing-key handling never prints the private key in normal output. Private keys are not committed.

Evidence:

- Code: `src/trust.rs` (Ed25519 key generation, signing, verification through `ring`; `Trust` holds the trusted keys), `src/index.rs::load_index` (verifies the raw index bytes before `parse_index`), `src/config.rs` (`trusted_keys`, `allow_unsigned_remote`, `RIVET_TRUSTED_KEY`), `src/cli.rs` (`--insecure`, `rv repo keygen|sign|public-key`).
- Tests, `cargo test --offline` → `tests/trust.rs`: `signed_local_index_verifies_and_tampering_fails` (trusted key accepted; no key, wrong key, and a one-byte tamper all fail), `signed_remote_index_verifies` (HTTP index + `index.json.sig`), `remote_index_fails_closed_without_signature` (HTTP without a signature fails; `--insecure` accepts it), `unsigned_local_index_stays_usable_for_development`, `keygen_never_prints_the_private_key`, `bad_checksum_is_rejected_before_extraction`. Result: 6 passed, 0 failed.
- The remote tests run against a local HTTP server (`tests/trust.rs::serve`), so the HTTP path itself is exercised, not only `file://`.
- Artifact digest: `src/model.rs::PackageManifest::validate` rejects a missing or malformed SHA-256; `src/download.rs::cache_blob` verifies the digest and deletes the partial file on mismatch. Test: `bad_checksum_is_rejected_before_extraction` (install fails with `sha256 mismatch`, `apps/hello` is never created).
- Key handling: `rv repo keygen` prints only paths and the public key; the private key is PKCS#8 bytes in `rivet-signing.key`. No key file is committed (`.gitignore` ignores `*.key`; `git ls-files` contains no key).
- Documentation: README section "Signing the index" and `docs/SECURITY.md`.

### B2. Mutation safety

- [x] One rv mutation at a time: install, remove, upgrade, rollback, cleanup, tap changes, config writes, and update use a process lock.
- [x] A crashed process cannot leave a permanent lock; stale-lock behavior is documented.
- [x] Removing a package that others depend on is blocked with an actionable message; an explicit force override exists.
- [x] rv doctor --fix repairs broken current links and missing shims when the store still contains the version.
- [x] Index replacement and installed-state writes are transactional.
- [x] Interrupted installs leave no active package, no orphan shim, and no partial version visible in rv list.

Evidence:

- Lock: `src/lock.rs` holds an exclusive file handle for the whole command. `src/commands.rs::is_mutating` covers `update`, `install`, `remove`, `upgrade`, `rollback`, `cleanup`, `tap add/remove`, `config set`, `init`, and `doctor --fix`; read-only commands take no lock. Tests: `tests/safety.rs::mutation_lock_blocks_concurrent_commands` (a held lock makes `rv update` fail with "another rv process"; releasing it succeeds and the lock file is removed) and the unit tests `lock::tests::second_lock_is_refused_until_the_first_is_dropped`, `lock::tests::a_leftover_lock_file_does_not_block`.
- Stale locks: the lock is an OS-held handle with `share_mode(0)`. Windows releases it when the process exits, including a crash, so a leftover file is not a lock. Documented in README "Concurrency and recovery".
- Reverse dependencies: `src/commands.rs::reverse_dependents` blocks `rv remove` with "cannot remove ... depend on it ... pass --force"; `Remove --force` overrides. Test: `tests/safety.rs::remove_blocks_reverse_dependencies_until_forced` (install `app` -> `lib`; `rv remove lib` fails and keeps the shim; `rv remove lib --force` succeeds).
- Doctor repair: `cmd_doctor` now checks every installed store path, `current` junction target, and each manifest bin shim. `--fix` recreates the junction and shims through the existing store/reconcile paths and persists them with `Db::save_install_state`. Tests: `doctor_fix_repairs_broken_link_and_missing_shim` (deleted junction and shim are restored), `doctor_reports_missing_store_as_unrepairable`.
- Transactions: `src/db.rs::replace_repo_packages` and `remove_repo` rebuild FTS inside the same transaction as the package rows; `save_install_state` writes the installed row and all shim rows in one transaction. Test: `db::tests::failed_index_replacement_keeps_the_previous_index` (a duplicate-version replacement fails and the old index is still queryable).
- Interrupted installs: `src/store.rs::recover_interrupted` runs at the start of every mutating command, deleting `store/<name>/.tmp-*` staging directories and shim files without a database record; `rv list` reads only the database, so an uncommitted version is never listed. Test: `tests/safety.rs::interrupted_install_artifacts_are_cleaned`.
- Verification: `cargo test --offline` -> 4 e2e + 6 trust + 5 safety + 3 unit tests, all pass.

### B3. Lifecycle completeness

- [x] rv pin and rv unpin work; rv list and rv status show pin state.
- [x] rv upgrade --all skips pinned packages; upgrading a pinned package requires an explicit override.
- [x] rv verify checks installed store paths, current links, shims, and recorded versions, and exits non-zero on damage.
- [x] rv doctor distinguishes repairable state from missing-store damage.
- [x] Rollback still works after a failed upgrade and after rv doctor --fix.

Evidence:

- Commands: `rv pin <packages...>`, `rv unpin <packages...>`, `rv verify`, and `rv upgrade --force` in `src/cli.rs`; `cmd_pin`, `cmd_verify`, and the pinned checks in `cmd_upgrade` in `src/commands.rs`.
- Visibility: `rv list` prints a PINNED column; `rv status` reports `pinned` and does not count pinned packages as outdated.
- Tests, `cargo test --offline` -> `tests/lifecycle.rs`: `pin_state_is_visible_and_blocks_upgrade_until_forced` (pin, list/status, explicit upgrade fails, --all skips, --force upgrades, unpin), `verify_detects_damage_and_passes_after_doctor_fix` (deleted shim -> verify fails with "shim missing" -> doctor --fix -> verify passes), `rollback_works_after_failed_upgrade_and_doctor_fix` (bad checksum leaves 1.1.0 active, rollback restores 1.0.0, doctor --fix repairs a removed junction, rollback still works).
- Result: 18 tests pass (3 unit + 4 e2e + 3 lifecycle + 5 safety + 6 trust); release build succeeds.

### B4. Install and first run

- [x] rv init is idempotent and does not duplicate PATH entries.
- [x] rv init broadcasts the environment change so newly opened shells see the PATH without logout.
- [x] rv init --no-path installs the binaries without touching PATH.
- [x] rv --version reports the release version, and rv doctor reports missing PATH/bin conditions.
- [x] A release zip contains rv.exe, rv-shim.exe, README, LICENSE, and SHA256SUMS.

Evidence:

- Init: `rv init [--no-path]` in `src/cli.rs`; `cmd_init` installs `rv.exe` and `rv-shim.exe`, checks the existing user PATH case-insensitively with trailing-slash tolerance, and only appends once. `path_has_entry` / `append_path` unit tests in `src/commands.rs` cover case, trailing slash, empty PATH, and trailing separator.
- Broadcast: after `reg.exe` updates `HKCU\\Environment\\Path`, `broadcast_environment_change` sends `WM_SETTINGCHANGE` with `"Environment"` through `SendMessageTimeoutW` (5s, `SMTO_ABORTIFHUNG`). Failure is a warning, not a silent success.
- No-path: `tests/install.rs::init_no_path_installs_binaries_without_touching_path` runs init twice, asserts the "PATH not changed" message and that `rv.exe` exists; it also asserts `rv doctor` reports the bin directory missing from PATH.
- Version: `tests/install.rs::version_is_visible` asserts `rv --version`; release binary prints `rv 0.1.0-beta.1`.
- Release: `scripts/release.ps1` builds `dist/rivet-v0.1.0-beta.1-windows-x64.zip` containing `rv.exe`, `rv-shim.exe`, README, LICENSE, docs, and an internal `SHA256SUMS`; `dist/SHA256SUMS` covers the binaries and the zip. Verified with `tar -tf` and `dist/rv.exe --version`.
- Tests: 26 pass (5 unit + 4 e2e + 3 install + 3 lifecycle + 5 safety + 6 trust).

### B5. Scriptable output and errors

- [ ] rv search, rv list, rv status, and rv info support machine-readable JSON.
- [ ] Commands use non-zero exit codes for failure; errors do not print success text before failing.
- [ ] rv install of an unknown package, failed download, bad checksum, and bad archive each produce a distinct actionable error.
- [ ] rv status --check remains usable when the network is unavailable and reports the cached state honestly.

Evidence:

### B6. Documentation and release hygiene

- [ ] README.md has a stranger-safe install path, quick start, supported platforms, and explicit non-goals.
- [ ] docs/INSTALL.md, docs/TAPS.md, docs/MANIFEST.md, docs/SECURITY.md, and docs/TROUBLESHOOTING.md exist and match the shipped behavior.
- [ ] CHANGELOG.md and SECURITY.md exist at the repository root.
- [ ] No document promises a feature the beta does not ship.
- [ ] dist/ contains only release binaries and release notes; debug artifacts are absent.
- [ ] Version is 0.1.0-beta.1 everywhere it is user-visible.

Evidence:

### B7. Verification and CI

- [ ] cargo fmt --check passes.
- [ ] cargo clippy --all-targets -- -D warnings passes.
- [ ] cargo test --offline passes all tests, including new trust/lock/pin/reverse-dependency/JSON/doctor tests.
- [ ] CI runs fmt, clippy, tests, and a release build on windows-latest.
- [ ] A clean clone builds and tests offline.
- [ ] Release smoke test runs from a fresh RIVET_ROOT with no developer state.

Evidence:

### B8. Performance floor

- [ ] Release rv --version under 30 ms on the development machine.
- [ ] Release rv search under 50 ms on the 10,000-package synthetic index.
- [ ] rv update of the 10,000-package index under 500 ms locally.
- [ ] Numbers are recorded in docs/PERFORMANCE.md with the build and measurement method.

Evidence:

## P1 — should ship in beta if cheap

- [ ] Artifact mirror fallback per manifest.
- [ ] rv cleanup --dry-run.
- [ ] Shell completion generation.
- [ ] rv status --json includes update availability.
- [ ] Basic telemetry-free crash report instructions.

## Explicit non-goals for this beta

- Scoop manifest or bucket compatibility.
- PowerShell pre/post-install hooks.
- MSI/EXE system-installer channel.
- A daemon, background service, or always-on updater.
- A full SAT dependency solver.
- Hosted artifact CDN or code-signing certificate procurement.

## Exit rule

Beta is ready only when every P0 box is checked with evidence, all tests pass from a clean clone, dist/ has been rebuilt, and docs/BETA.md contains no unchecked P0 item. P1 items may remain if documented in CHANGELOG.md.
