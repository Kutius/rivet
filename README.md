# Rivet

Rivet is a fast, portable-first package manager for Windows, built as one native Rust binary. It keeps the useful parts of Scoop's model without the PowerShell startup, bucket scans, and implicit metadata refreshes.

Current release: 0.1.0-beta.2.

Rivet is not Scoop-compatible. Scoop manifests, buckets, and PowerShell hooks are intentionally out of scope.

## Why Rivet

Scoop's common commands pay for:

- PowerShell startup and script parsing on every invocation;
- a full JSON scan of every bucket for search and manifest lookup;
- Git pulls of every bucket when a package is installed after the last-update threshold;
- serial downloads and installs.

Rivet moves those costs out of the hot path:

- one native Rust binary (rv), no runtime scripts;
- a local SQLite index for search and package metadata;
- a generated main index instead of a client-side Git bucket clone;
- immutable version directories plus a current junction;
- native shims for app commands;
- explicit update and upgrade commands, with no implicit update during install.

## Install

From PowerShell:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer downloads the release zip, verifies SHA256SUMS, copies rv.exe and rv-shim.exe to %LOCALAPPDATA%\Rivet\bin, adds that directory to the user PATH once, and fetches the signed main catalog. Open a new terminal and run rv --version.

For a portable install without a PATH change:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

Manual zip installation and uninstall are in [docs/INSTALL.md](docs/INSTALL.md).

## Quick start

~~~powershell
rv update
rv search ripgrep
rv install ripgrep
rv list
rv status
~~~

A local directory or index.json is accepted for development and may be unsigned:

~~~powershell
rv config set main_index C:\src\rivet\repo\main\index.json
rv update
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
| rv update | Refresh the main index and taps. Never changes installed packages. |
| rv search [query] | Local full-text search; omit the query to list packages. |
| rv info PACKAGE | Show metadata and available versions. |
| rv install PACKAGE... | Install packages and dependencies. Supports name, repo/name, owner/repo/name, and name@version. |
| rv list | List installed packages and pin state. |
| rv status [--check] | Show installed versus latest versions. --check refreshes metadata; it falls back to the cached index if the network is unavailable. |
| rv upgrade [PACKAGE...] [--all] [--force] | Upgrade packages. Pinned packages are skipped unless forced. |
| rv remove PACKAGE... [--purge] [--force] | Remove packages. Dependents block removal unless forced. --purge deletes persist data. |
| rv rollback PACKAGE... | Switch back to the previous version. |
| rv pin / rv unpin PACKAGE... | Hold or release a package at its current version. |
| rv verify | Check store paths, current links, shims, and recorded versions. Non-zero on damage. |
| rv doctor [--fix] | Diagnose the installation; --fix repairs broken links and missing shims when the store is intact. |
| rv cleanup | Remove old store versions and stale cache entries. |
| rv tap add/list/remove | Manage third-party Git taps. |
| rv repo build/sign/keygen/public-key | Maintainer tooling for indexes and keys. |
| rv config path/get/set | Inspect or edit config. |
| rv init [--no-path] | Install the binaries, configure the catalog, and optionally update the user PATH. |

Add --json before the subcommand for machine-readable search, list, status, and info:

~~~powershell
rv --json search hello
rv --json list
rv --json status
rv --json info hello
~~~

## Main repository

The main repository is a normal Git repository of package manifests. Clients do not clone it. The official catalog is served at https://kutius.github.io/rivet/index.json with its signature next to it; the source lives in repo/main. A CI job or maintainer command builds index.json:

~~~text
repo/main/
  packages/*.toml
  artifacts/
  index.json
  index.json.sig
~~~

~~~powershell
rv repo build repo\main
rv repo sign repo\main --key C:\secure\rivet-keys\rivet-signing.key
~~~

See [docs/MANIFEST.md](docs/MANIFEST.md) for every field and [docs/SECURITY.md](docs/SECURITY.md) for the trust model.

## Taps

Taps are third-party Git repositories in the Homebrew style. They contain packages/*.toml and may contain a generated index.json.

~~~powershell
rv tap add owner/repo
rv tap add demo C:\path\to\tap
rv tap list
rv tap remove demo
~~~

Packages from a tap are namespaced:

~~~powershell
rv install demo/hello
~~~

If the tap has no index.json, rv update builds one locally from its manifests. Search is always local; network access only occurs during explicit rv update, rv tap add, and artifact downloads. See [docs/TAPS.md](docs/TAPS.md).

## Signing the index

The client verifies an Ed25519 signature over the exact index.json bytes before parsing them. Maintainers generate a key pair, sign the index, and publish index.json.sig next to index.json:

~~~powershell
rv repo keygen --out C:\secure\rivet-keys
rv repo sign repo\main --key C:\secure\rivet-keys\rivet-signing.key
rv repo public-key --key C:\secure\rivet-keys\rivet-signing.key
~~~

rv repo keygen writes rivet-signing.key (private) and rivet-signing.pub (public, hex). It prints only the public key. Never commit the private key.

Clients trust a key explicitly:

~~~powershell
rv config set trusted_key <64-hex public key>
# or for one invocation:
$env:RIVET_TRUSTED_KEY = "<64-hex public key>"
~~~

Rules:

- HTTP indexes must be signed; a missing, untrusted, or invalid signature fails rv update before parsing.
- Local file and directory indexes may be unsigned for development.
- --insecure (or rv config set allow_unsigned_remote true) accepts unsigned remote indexes. It is a deliberate downgrade; use it only for testing.

## Storage layout

~~~text
%LOCALAPPDATA%\Rivet\
  bin\                 # one PATH entry; rv-shim.exe and command shims
  store\<name>\<version>\
  apps\<name>\current   # junction to the active version
  persist\<name>\
  cache\blobs\<sha256>\
  taps\<tap-name>\
  state.db             # SQLite index and installed state
  config.toml
~~~

The store is the source of truth. The SQLite database is a cache and state index; rv doctor reports broken links and missing store paths.

## Concurrency and recovery

Every mutating command (update, install, remove, upgrade, rollback, cleanup, tap add/remove, config set, init, and doctor --fix) takes a single-process lock at <root>\rivet.lock.

The lock is an exclusive open file handle. Windows releases it when the process exits, including a crash, so a leftover rivet.lock file never blocks a later command; the file is intentionally left in place. A second rv command that finds the lock held fails with an actionable message and changes nothing.

Interrupted installs are recovered at the start of the next mutating command: staging directories under store\<name>\.tmp-* are deleted and shim files without a database record are removed. rv doctor --fix repairs a broken current junction and missing shims while the immutable store still contains the version; a missing store is reported as unrepairable.

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

This builds dist\rivet-v0.1.0-beta.2-windows-x64.zip, the binaries, install.ps1, and SHA256SUMS. It does not publish.

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
- No self-update or self-uninstall command.
- No shell completions yet.
- No central verified-tap registry; taps are trusted because the user adds them.

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
