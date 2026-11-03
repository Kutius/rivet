# Rivet

Rivet is a fast, portable-first package manager for Windows, built as a native Rust CLI plus a tiny launcher shim. It keeps the useful parts of Scoop's model without the PowerShell startup, bucket scans, and implicit metadata refreshes.

Current release: 0.1.0-beta.6.

Rivet is not Scoop-compatible. Scoop manifests, buckets, and PowerShell hooks are intentionally out of scope.

## Why Rivet

Scoop's common commands pay for:

- PowerShell startup and script parsing on every invocation;
- a full JSON scan of every bucket for search and manifest lookup;
- Git pulls of every bucket when a package is installed after the last-update threshold;
- serial downloads and installs.

Rivet moves those costs out of the hot path:

- one native Rust CLI (`rivet`, also installed as `rv.exe`), no runtime scripts; a 280 KB launcher shim only for script commands or systems without symlink permission;
- a local SQLite index for search and package metadata;
- a generated main index instead of a client-side Git bucket clone;
- immutable version directories plus a current junction;
- native symlinks for app commands where Windows allows them, with a lightweight shim fallback;
- explicit update and upgrade commands, with no implicit update during install.

## Install

From PowerShell:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer downloads the release zip, verifies SHA256SUMS, installs rv.exe, rivet.exe, and rv-shim.exe into %LOCALAPPDATA%\Rivet\bin, adds that directory to the user PATH once, and fetches the signed main catalog. Open a new terminal and run `rivet --version`.

For a portable install without a PATH change:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

Manual zip installation and uninstall are in [docs/INSTALL.md](docs/INSTALL.md).

### PowerShell note: `rivet` and `rv`

PowerShell ships a read-only alias named `rv` for `Remove-Variable`. Alias resolution happens before PATH lookup, so `rv --version` in PowerShell invokes the built-in alias, not Rivet.

Rivet therefore installs both `rivet.exe` and `rv.exe`. Use `rivet` in PowerShell. `rv` still works in `cmd.exe`; if you specifically need it in PowerShell, remove the alias or add this line to `$PROFILE`:

~~~powershell
if (Test-Path Alias:rv) { Remove-Item Alias:rv -Force -ErrorAction SilentlyContinue }
~~~

## Quick start

~~~powershell
rivet update
rivet search ripgrep
rivet install ripgrep
rivet list
rivet status
~~~

A local directory or index.json is accepted for development and may be unsigned:

~~~powershell
rivet config set main_index C:\src\rivet\repo\main\index.json
rivet update
~~~

For a disposable demo from this repository:

~~~powershell
$env:RIVET_ROOT = Join-Path $env:TEMP "rivet-demo"
$env:RIVET_MAIN_INDEX = (Resolve-Path "repo\main\index.json").Path
.\target\release\rv.exe update
.\target\release\rv.exe install hello
~~~

The demo package installs a native shim at %RIVET_ROOT%\bin\hello.exe.

## Commands

| Command | Purpose |
|---|---|
| rivet update | Refresh the main index and taps. Never changes installed packages. |
| rivet search [query] | Local full-text search; omit the query to list packages. |
| rivet info PACKAGE | Show metadata and available versions. |
| rivet install PACKAGE... | Install packages and dependencies. Supports name, repo/name, owner/repo/name, and name@version. |
| rivet list | List installed packages and pin state. |
| rivet status [--check] | Show installed versus latest versions. --check refreshes metadata; it falls back to the cached index if the network is unavailable. |
| rivet upgrade [PACKAGE...] [--all] [--force] | Upgrade packages. Pinned packages are skipped unless forced. |
| rivet remove PACKAGE... [--purge] [--force] | Remove packages. Dependents block removal unless forced. --purge deletes persist data. |
| rivet rollback PACKAGE... | Switch back to the previous version. |
| rivet pin / rivet unpin PACKAGE... | Hold or release a package at its current version. |
| rivet verify | Check store paths, current links, shims, and recorded versions. Non-zero on damage. |
| rivet doctor [--fix] | Diagnose the installation; --fix repairs broken links and missing shims, and restores the `rivet.exe` alias. |
| rivet cleanup [--dry-run] | Remove old store versions and stale cache entries; --dry-run only lists them. |
| rivet tap add/list/remove | Manage third-party Git taps. |
| .\scripts\repo.ps1 add/update/build/sign | Maintainer tooling for catalog packages, indexes, and keys; not part of the installed client. |
| rivet config path/get/set | Inspect or edit config. |
| rivet init [--no-path] | Install the binaries, configure the catalog, and optionally update the user PATH. |
| rivet self-update [--force] | Replace the installed binaries with the latest release. |
| rivet self-uninstall --yes | Remove packages, PATH entry, and the Rivet root. |

Install and remove are idempotent: installing an already-installed package is a no-op, and removing an absent package succeeds without changing anything.

Add --json before the subcommand for machine-readable search, list, status, and info:

~~~powershell
rivet --json search hello
rivet --json list
rivet --json status
rivet --json info hello
~~~

## Main repository

The main repository is a normal Git repository of package manifests. Clients do not clone it. The official catalog is served at https://kutius.github.io/rivet/index.json with its signature next to it; the source lives in repo/main. The daily catalog workflow builds and signs index.json:

~~~text
repo/main/
  packages/*.toml
  sources.toml
  artifacts/
  index.json
  index.json.sig
~~~

Maintainers add GitHub-hosted tools without hand-writing metadata:

~~~powershell
.\scripts\repo.ps1 add BurntSushi/ripgrep --path repo\main
.\scripts\repo.ps1 sync repo\main --limit 10
.\scripts\repo.ps1 update repo\main --check
.\scripts\repo.ps1 update repo\main
.\scripts\repo.ps1 sign repo\main --key C:\secure\rivet-keys\rivet-signing.key
~~~

The maintainer script builds the separate `rv-repo` tool. `add` imports one repository. `sync` reads the curated `repo/main/sources.toml`, imports every missing repository in batches, and skips existing manifests. `update` does the same version bump for every GitHub-hosted package. Set `GITHUB_TOKEN` or `GH_TOKEN` to avoid the unauthenticated GitHub API rate limit. The daily catalog workflow runs `rv-repo update`, signs with the `RIVET_SIGNING_KEY_B64` Actions secret, smoke-installs changed packages, and pushes the result. After a batch import, `scripts/verify-catalog.ps1` installs every package into a temporary root and smoke-tests each command; the weekly Catalog verify workflow runs the same check.

See [docs/MANIFEST.md](docs/MANIFEST.md) for every field, [docs/SECURITY.md](docs/SECURITY.md) for the trust model, and [repo/main/README.md](repo/main/README.md) for the catalog workflow.

## Taps

Taps are third-party Git repositories in the Homebrew style. They contain packages/*.toml and may contain a generated index.json.

~~~powershell
rivet tap add owner/repo
rivet tap add demo C:\path\to\tap
rivet tap list
rivet tap remove demo
~~~

Packages from a tap are namespaced:

~~~powershell
rivet install demo/hello
~~~

If the tap has no index.json, `rivet update` builds one locally from its manifests. Search is always local; network access only occurs during explicit `rivet update`, `rivet tap add`, and artifact downloads. See [docs/TAPS.md](docs/TAPS.md).

## Signing the index

The client verifies an Ed25519 signature over the exact index.json bytes before parsing them. The main catalog is signed by the daily catalog workflow using the `RIVET_SIGNING_KEY_B64` GitHub Actions secret. Local signing remains available:

~~~powershell
.\scripts\repo.ps1 keygen --out C:\secure\rivet-keys
.\scripts\repo.ps1 sign repo\main --key C:\secure\rivet-keys\rivet-signing.key
.\scripts\repo.ps1 public-key --key C:\secure\rivet-keys\rivet-signing.key
~~~

`.\scripts\repo.ps1 keygen` writes rivet-signing.key (private) and rivet-signing.pub (public, hex). It prints only the public key. Never commit the private key. The CI secret is base64-encoded PKCS#8 bytes and is decoded only inside the workflow.

Clients trust a key explicitly:

~~~powershell
rivet config set trusted_key <64-hex public key>
# or for one invocation:
$env:RIVET_TRUSTED_KEY = "<64-hex public key>"
~~~

Rules:

- HTTP indexes must be signed; a missing, untrusted, or invalid signature fails `rivet update` before parsing.
- Local file and directory indexes may be unsigned for development.
- --insecure (or `rivet config set allow_unsigned_remote true`) accepts unsigned remote indexes. It is a deliberate downgrade; use it only for testing.

## Storage layout

~~~text
%LOCALAPPDATA%\Rivet\
  bin\                 # one PATH entry; rv.exe, rivet.exe, rv-shim.exe, command shims
  store\<name>\<version>\
  apps\<name>\current   # junction to the active version
  persist\<name>\
  cache\blobs\<sha256>\
  taps\<tap-name>\
  state.db             # SQLite index and installed state
  config.toml
~~~

The store is the source of truth. The SQLite database is a cache and state index; `rivet doctor` reports broken links and missing store paths.

## Concurrency and recovery

Every mutating command (update, install, remove, upgrade, rollback, cleanup, tap add/remove, config set, init, and doctor --fix) takes a single-process lock at <root>\rivet.lock.

The lock is an exclusive open file handle. Windows releases it when the process exits, including a crash, so a leftover rivet.lock file never blocks a later command; the file is intentionally left in place. A second `rivet` command that finds the lock held fails with an actionable message and changes nothing.

Interrupted installs are recovered at the start of the next mutating command: staging directories under store\<name>\.tmp-* are deleted and shim files without a database record are removed. `rivet doctor --fix` repairs a broken current junction and missing shims while the immutable store still contains the version; a missing store is reported as unrepairable.

## Build and test

~~~powershell
cargo build --release --bins
cargo test --offline
cargo fmt --check
cargo clippy --all-targets -- -D warnings
~~~

The project targets Rust 1.99 on Windows with the MSVC toolchain. If crates.io is slow or TLS-interrupted, use a local mirror such as rsproxy.cn and set CARGO_REGISTRIES_CRATES_IO_INDEX=sparse+https://rsproxy.cn/index/.

Release packaging:

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release.ps1
~~~

This builds dist\rivet-v0.1.0-beta.6-windows-x64.zip, the binaries, install.ps1, and SHA256SUMS. It does not publish.

## Supported

- Windows 10 and Windows 11, x64.
- No administrator rights for normal installs.
- Git only for third-party taps.

## Beta limitations

- No Scoop compatibility and no PowerShell hooks.
- No MSI/EXE system-installer channel.
- No artifact mirror or hosted CDN; artifacts come from the URLs in manifests.
- No code-signing certificate for rv.exe or rv-shim.exe.
- No full dependency solver; dependencies are recursive and latest-version by default, with explicit name@version respected.
- No shell completions yet.
- No central verified-tap registry; taps are trusted because the user adds them.
- The maintainer `rv-repo` tool automates GitHub releases; packages distributed outside GitHub still need a manifest written or imported manually.

The next work is artifact mirroring and a proper dependency solver only if real packages need one; code signing is deferred.

## Documentation

- [Install](docs/INSTALL.md)
- [Taps](docs/TAPS.md)
- [Manifest reference](docs/MANIFEST.md)
- [Security](docs/SECURITY.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Design](docs/DESIGN.md)
- [Performance](docs/PERFORMANCE.md)
- [Changelog](CHANGELOG.md)
