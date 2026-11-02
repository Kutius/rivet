# Rivet Public Beta Gate

Definition: Rivet is public-beta ready when a Windows user can download a signed release, install it without admin rights, install and manage packages from a signed catalog and a third-party tap, recover from the common broken states, and understand exactly what is supported — with no known P0/P1 defect.

This file is the release gate, not a wish list. Do not check a box without a command, test name, file, or measured result recorded next to it.

## P0 — release blockers

### B1. Trusted catalog

- [ ] The official main index can be signed with Ed25519 and the client verifies it before parsing.
- [ ] Verification is mandatory for HTTP indexes. rv update fails closed on a missing or invalid signature.
- [ ] Local file:// and directory indexes may be unsigned for development; an explicit insecure override exists for remote indexes.
- [ ] Maintainer commands: key generation, signing, and public-key reporting are documented and tested.
- [ ] Every package artifact still requires a SHA-256 and is verified before extraction.
- [ ] Signing-key handling never prints the private key in normal output. Private keys are not committed.

Evidence:

### B2. Mutation safety

- [ ] One rv mutation at a time: install, remove, upgrade, rollback, cleanup, tap changes, config writes, and update use a process lock.
- [ ] A crashed process cannot leave a permanent lock; stale-lock behavior is documented.
- [ ] Removing a package that others depend on is blocked with an actionable message; an explicit force override exists.
- [ ] rv doctor --fix repairs broken current links and missing shims when the store still contains the version.
- [ ] Index replacement and installed-state writes are transactional.
- [ ] Interrupted installs leave no active package, no orphan shim, and no partial version visible in rv list.

Evidence:

### B3. Lifecycle completeness

- [ ] rv pin and rv unpin work; rv list and rv status show pin state.
- [ ] rv upgrade --all skips pinned packages; upgrading a pinned package requires an explicit override.
- [ ] rv verify checks installed store paths, current links, shims, and recorded versions, and exits non-zero on damage.
- [ ] rv doctor distinguishes repairable state from missing-store damage.
- [ ] Rollback still works after a failed upgrade and after rv doctor --fix.

Evidence:

### B4. Install and first run

- [ ] rv init is idempotent and does not duplicate PATH entries.
- [ ] rv init broadcasts the environment change so newly opened shells see the PATH without logout.
- [ ] rv init --no-path installs the binaries without touching PATH.
- [ ] rv --version reports the release version, and rv doctor reports missing PATH/bin conditions.
- [ ] A release zip contains rv.exe, rv-shim.exe, README, LICENSE, and SHA256SUMS.

Evidence:

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
