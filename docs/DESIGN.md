# Rivet Design

This is the lean, individual-developer version of Rivet. It deliberately keeps only the parts that improve the actual command hot path.

## Principles

1. The client is a native Rust CLI (`rivet.exe`, with `rv.exe` kept as a compatibility name) plus a tiny launcher shim. No PowerShell, no script bootstrapping.
2. The client never clones the main repository. It downloads a generated `index.json`.
3. Third-party sources are Homebrew-style taps: normal Git repositories, explicitly added by the user.
4. Search and package lookup use a local SQLite database. The filesystem remains the source of truth for installed packages.
5. Installed versions are immutable directories. The active version is a `current` junction.
6. App commands are direct symlinks when Developer Mode allows it, otherwise a lightweight `rv-shim.exe` launcher. There is one PATH entry, not one PATH entry per app.
7. `rivet update` refreshes metadata. `rivet upgrade` changes installed packages. `rivet install` never runs a full metadata update implicitly.
8. Packages are portable archives by default. There is no arbitrary install hook and no MSI/EXE system-installer channel in this beta.

## Components

~~~text
main repo (Git) --CI--> index.json / artifacts --> GitHub Pages or Release
                                                       |
                                                rivet CLI
                                                       |
                         +-----------------------------+-----------------------------+
                         |                             |                             |
                    SQLite index                  blob cache                  immutable store
                         |                             |                             |
                   search/list                  downloader                  junction + symlink/shim
~~~

## Command semantics

| Command | Behaviour |
|---|---|
| `rivet update` | Refresh main index and taps. Never changes installed packages. |
| `rivet upgrade` | Upgrade selected or all installed packages. Uses cached metadata. |
| `rivet install` | Install packages and dependencies. Never refreshes metadata implicitly. |
| `rivet status` | Show installed versions and available updates. `--check` refreshes metadata and falls back to the cached index on failure. |
| `rivet pin` / `rivet unpin` | Hold or release a package at its current version. |
| `rivet verify` | Check store paths, current links, shims, and recorded versions. |
| `rivet doctor --fix` | Repair broken current links and missing shims when the store is intact. |
| `rivet tap add` | Clone a third-party Git tap and build its local index if needed. |
| `rivet self-update` | Fetch, verify, and stage the latest binaries; replace them after the current process exits. |
| `rivet self-uninstall --yes` | Remove packages, the PATH entry, and the Rivet root after the current process exits. |
| `rivet cleanup --dry-run` | List removable store versions and cache entries without deleting them. |
| `rivet repo add` / `rivet repo update` | Generate or refresh GitHub-release manifests and rebuild the index. |

## Repository and index

The main repository has this shape:

~~~text
packages/<name>-<version>.toml
artifacts/<file>
index.json
~~~

`rivet repo build` reads every TOML manifest, validates it, sorts packages, and writes a single `index.json`. The client downloads that file with an HTTP client and loads it into SQLite. Maintainers use `rivet repo add owner/repo` to generate a manifest from the latest GitHub release and `rivet repo update` to bump every GitHub-hosted package. The daily catalog workflow signs the result with the encrypted `RIVET_SIGNING_KEY_B64` Actions secret; local signing remains available.

The index is deliberately a single file. For a personal repository with tens to hundreds of packages, this keeps implementation and publishing simple. If the repository grows past a few thousand packages, split the catalog and package manifests into a two-level index.

## Tap model

A tap is a Git repository with `packages/*.toml` and optionally a checked-in `index.json`. When no index exists, `rivet update` builds one in the tap directory. Tap packages use the namespace `owner/repo/package` in commands and `owner/repo` in metadata.

Taps are trusted explicitly. There is no central verification tier. Tap indexes may be signed with the same Ed25519 mechanism as the main index, but an unsigned tap remains usable; adding the tap is the trust decision.

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
8. Create or update native symlinks, or shims when symlinks are unavailable.
9. Record the installed version and previous version in SQLite.

Upgrade installs the new version separately and switches the junction. The old version is retained for `rivet rollback` and removed by `rivet cleanup` when it is no longer current or previous.

## Tradeoffs

| Decision | Why | Cost |
|---|---|---|
| Rust, not C# | Native startup, low memory, one binary, good file/HTTP concurrency | More Windows API plumbing than C# |
| Generated index, not Git on client | No bucket scans, no Git pulls in the hot path | Depends on index hosting |
| Upstream artifact URLs | No storage or legal burden for a solo developer | Download speed depends on the vendor |
| Optional Ed25519 index signatures | The maintainer can pin a key; HTTP indexes fail closed | Key distribution and rotation are manual |
| Symlink-first, shim fallback | No PATH bloat, no launcher process for native exes, scripts still work | Symlinks need Developer Mode; script commands pay one small process launch |
| Portable-only | Clean uninstall and predictable behavior | Many Windows GUI apps are out of scope |
| Single index file | Simple to build and cache | Does not scale to tens of thousands of packages |
| Simple dependency resolver | Small codebase | Complex version conflicts are not handled |

## Failure modes

- Index unreachable: cached index remains usable; `rivet update` reports the error.
- Artifact unreachable: install fails before activation; existing current version is untouched.
- Bad archive: extraction fails into a temp directory; temp directory is removed.
- Failed upgrade: old `current` junction remains until the new version is fully staged; the switch is the last step.
- Corrupt SQLite: `rivet doctor` reports filesystem state; the store remains authoritative.
- Shim binary missing: script shims cannot be created; install fails with an actionable message. Run `cargo build --bins` or `rivet init` from a release zip.
- Interrupted mutation: the next mutating command clears staging directories and orphan shims; `rivet doctor --fix` repairs the rest.

## Roadmap

- `rivet lock`/`rivet sync` for reproducible environment snapshots.
- Optional artifact mirror per package.
- A proper dependency solver only when real packages need it.
- Optional code signing for the binaries; deferred.
