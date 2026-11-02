# Rivet Design

This is the lean, individual-developer version of Rivet. It deliberately keeps only the parts that improve the actual command hot path.

## Principles

1. The client is one native Rust binary. No PowerShell, no script bootstrapping.
2. The client never clones the main repository. It downloads a generated `index.json`.
3. Third-party sources are Homebrew-style taps: normal Git repositories, explicitly added by the user.
4. Search and package lookup use a local SQLite database. The filesystem remains the source of truth for installed packages.
5. Installed versions are immutable directories. The active version is a `current` junction.
6. App commands are native `rv-shim.exe` shims. There is one PATH entry, not one PATH entry per app.
7. `rv update` refreshes metadata. `rv upgrade` changes installed packages. `rv install` never runs a full metadata update implicitly.
8. Packages are portable archives by default. There is no arbitrary install hook and no MSI/EXE system-installer channel in this MVP.

## Components

~~~text
main repo (Git) --CI--> index.json / artifacts --> GitHub Pages or Release
                                                       |
                                                  rv CLI
                                                       |
                         +-----------------------------+-----------------------------+
                         |                             |                             |
                    SQLite index                  blob cache                  immutable store
                         |                             |                             |
                   search/list                  downloader                  junction + shim
~~~

## Command semantics

| Command | Behaviour |
|---|---|
| `rv update` | Refresh main index and taps. Never changes installed packages. |
| `rv upgrade` | Upgrade selected or all installed packages. Uses cached metadata. |
| `rv install` | Install packages. Refreshes only when the package is unknown, or with `--refresh` in a future version. |
| `rv status` | Show installed versions and available updates. `--check` refreshes metadata first. |
| `rv tap add` | Clone a third-party Git tap and build its local index if needed. |

## Repository and index

The main repository has this shape:

~~~text
packages/<name>-<version>.toml
artifacts/<file>
index.json
~~~

`rv repo build` reads every TOML manifest, validates it, sorts packages, and writes a single `index.json`. The client downloads that file with an HTTP client and loads it into SQLite.

The index is deliberately a single file. For a personal repository with tens to hundreds of packages, this keeps implementation and publishing simple. If the repository grows past a few thousand packages, split the catalog and package manifests into a two-level index.

## Tap model

A tap is a Git repository with `packages/*.toml` and optionally a checked-in `index.json`. When no index exists, `rv update` builds one in the tap directory. Tap packages use the namespace `owner/repo/package` in commands and `owner/repo` in metadata.

Taps are trusted explicitly. There is no central verification tier in this MVP. A future public release should add minisign/Ed25519 signatures and a verified-repository list; it should not add a daemon.

## Storage and lifecycle

~~~text
%LOCALAPPDATA%\Rivet\
  bin\
  store\<name>\<version>\
  apps\<name>\current
  persist\<name>\
  cache\blobs\<sha256>\
  taps\<name>\
  state.db
  config.toml
~~~

Install flow:

1. Read package metadata from SQLite.
2. Resolve dependencies recursively.
3. Download artifacts into the content-addressed cache, verifying SHA-256.
4. Extract into a temporary directory under `store/<name>`.
5. Rename the temporary directory to the final version directory.
6. Apply persist junctions.
7. Switch `apps/<name>/current` to the new version.
8. Create or update native shims.
9. Record the installed version and previous version in SQLite.

Upgrade installs the new version separately and switches the junction. The old version is retained for `rv rollback` and removed by `rv cleanup` when it is no longer current or previous.

## Tradeoffs

| Decision | Why | Cost |
|---|---|---|
| Rust, not C# | Native startup, low memory, one binary, good file/HTTP concurrency | More Windows API plumbing than C# |
| Generated index, not Git on client | No bucket scans, no Git pulls in the hot path | Depends on index hosting |
| Upstream artifact URLs | No storage or legal burden for a solo developer | Download speed depends on the vendor |
| No repository signatures | Smallest viable trust model for personal use | Repository compromise is not mitigated |
| Native shims | Per-app environment and no PATH bloat | Small process-launch overhead |
| Portable-only | Clean uninstall and predictable behavior | Many Windows GUI apps are out of scope |
| Single index file | Simple to build and cache | Does not scale to tens of thousands of packages |
| Simple dependency resolver | Small codebase | Complex version conflicts are not handled |

## Failure modes

- Index unreachable: cached index remains usable; `rv update` reports the error.
- Artifact unreachable: install fails before activation; existing current version is untouched.
- Bad archive: extraction fails into a temp directory; temp directory is removed.
- Failed upgrade: old `current` junction remains until the new version is fully staged; the switch is the last step.
- Corrupt SQLite: `rv doctor` should be used to inspect; the store remains authoritative.
- Shim binary missing: install fails with an actionable message; run `cargo build --bins`.

## Roadmap

- `rv status` and `rv lock`/`rv sync`.
- Optional minisign signatures.
- Optional artifact mirror per package.
- A proper dependency solver only when real packages need it.
- Code signing and a polished bootstrap installer.
