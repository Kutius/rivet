# Rivet

Rivet is a fast, portable-first package manager for Windows, built as a clean redesign of the ideas that make Scoop useful without carrying Scoop's slow paths.

It is a working MVP, not a Scoop-compatible implementation. Scoop manifests, PowerShell hooks, and buckets are intentionally not supported.

## Why Rivet

Scoop's common commands pay for:

- PowerShell startup and script parsing on every invocation;
- a full JSON scan of every bucket for search and manifest lookup;
- Git pulls of every bucket when a package is installed after the last-update threshold;
- serial downloads and installs.

Rivet moves those costs out of the hot path:

- one native Rust binary (`rv`), no runtime scripts;
- a local SQLite index for search and package metadata;
- a small generated main index instead of a client-side Git bucket clone;
- immutable version directories plus a `current` junction;
- native shims for app commands;
- explicit `update` and `upgrade` commands, with no implicit update during install.

## Status

Implemented:

- `rv update`: refresh the main index and configured taps
- `rv search`: local full-text search
- `rv list`, `rv status`, `rv info`
- `rv install`, `rv remove`, `rv upgrade`, `rv rollback`, `rv cleanup`
- `rv doctor`
- `rv tap add`, `rv tap list`, `rv tap remove`
- `rv repo build`: generate `index.json` from `packages/*.toml`
- `rv config path|get|set`
- `rv init`: install a copy of `rv.exe`/`rv-shim.exe` into the Rivet bin directory and add it to the user PATH

Not implemented in this MVP:

- Scoop compatibility and PowerShell hooks
- MSI/EXE system installers
- code signing and repository signatures
- artifact mirroring and a hosted CDN
- complex dependency solving (dependencies are installed recursively, latest version, no backtracking)

## Build

~~~powershell
cargo build --release --bins
cargo test
~~~

The project was developed with Rust 1.99 on Windows. If crates.io is slow or TLS-interrupted, use a local mirror such as rsproxy.cn and set `CARGO_REGISTRIES_CRATES_IO_INDEX=sparse+https://rsproxy.cn/index/`.

## Quick start

From the repository root:

~~~powershell
$env:RIVET_ROOT = Join-Path $env:LOCALAPPDATA "Rivet"
$env:RIVET_MAIN_INDEX = (Resolve-Path "repo\main\index.json").Path

.\target\release\rv.exe update
.\target\release\rv.exe search hello
.\target\release\rv.exe install hello
.\target\release\rv.exe list
~~~

The demo package installs a native shim at `$env:RIVET_ROOT\bin\hello.exe`.

To add the Rivet bin directory to the user PATH:

~~~powershell
.\target\release\rv.exe init
# open a new terminal
~~~

For a disposable demo, run `scripts\demo.ps1`.

## Main repository

The main repository is a normal Git repository of package manifests. Clients do not clone it. A CI job or a local maintainer command builds `index.json`:

~~~text
repo/main/
  packages/*.toml
  artifacts/
  index.json
~~~

Build the index:

~~~powershell
rv repo build repo\main
~~~

A minimal package manifest:

~~~toml
name = "hello"
version = "1.0.0"
description = "Rivet demo package"
homepage = "https://example.invalid/rivet/hello"
license = "MIT"
tags = ["demo"]
url = "artifacts/hello-1.0.0.zip"
sha256 = "f61c5c9c48e6b681a11543043dc730571d7d9e1501615f7139b3cdda0e73d5d0"
extract = "zip"
bin = [{ path = "hello.cmd", name = "hello" }]
~~~

Fields:

- `url`: absolute URL or a path relative to `index.json`
- `sha256`: lowercase or uppercase 64-character hex digest
- `extract`: `zip`, `tar.gz`, `tar`, `7z`, or `none`
- `bin`: one or more `{ path, name, args }` entries
- `depends`: package names, optionally `repo/name` or `name@version`
- `persist`: directory paths preserved across versions

## Taps

Taps are third-party Git repositories in the Homebrew style. They contain `packages/*.toml` and may contain a generated `index.json`.

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

If the tap has no `index.json`, `rv update` builds one locally from its manifests. Search is always local; network access only occurs during explicit `rv update`, `rv tap add`, and artifact downloads.

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

The store is the source of truth. The SQLite database is a cache and state index; `rv doctor` reports broken links and missing store paths.

## Design notes

The full rationale is in [docs/DESIGN.md](docs/DESIGN.md). Measured numbers are in [docs/PERFORMANCE.md](docs/PERFORMANCE.md).

The short version:

- one main index, optionally several official repos later;
- third-party taps, explicitly trusted by the user;
- generated index files instead of client-side bucket scans;
- portable-first packages, no arbitrary install scripts;
- immutable store plus pointer switch, so upgrades do not uninstall the old version first;
- native shims instead of PATH entries per app;
- no implicit `scoop update` equivalent during install.

## Limitations and next steps

- Add minisign or Ed25519 signatures before public distribution.
- Add an optional artifact mirror for packages whose upstream downloads are slow.
- Add a proper version solver if dependency complexity grows.
- Add code signing for `rv.exe` and `rv-shim.exe`.
- Add an optional MSI/EXE vendor channel as a separate, explicitly trusted mode.
