# Manifest reference

A package is one TOML file under packages/. `.\scripts\repo.ps1 build` validates every manifest and writes a single index.json.

## Example

~~~toml
name = "hello"
version = "1.1.0"
description = "Rivet demo package"
homepage = "https://example.invalid/rivet/hello"
license = "MIT"
tags = ["demo", "cli"]
arch = "x64"
url = "artifacts/hello-1.1.0.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
extract = "zip"
strip_components = 0
bin = [{ path = "hello.exe", name = "hello" }]
depends = ["runtime@1.0.0"]
persist = ["data"]
~~~

## Fields

| Field | Required | Meaning |
|---|---|---|
| name | yes | Lowercase package name; letters, digits, -, _, . |
| version | yes | Semantic version. The highest version is the default install target. |
| description | no | One-line description shown by search and info. |
| homepage | no | Project URL. |
| license | no | SPDX-style license identifier. |
| tags | no | Search keywords. |
| arch | no | Advisory architecture string, for example x64. The beta does not enforce it. |
| asset | no | Release asset name or substring used by `rv-repo update` when auto-detection needs help. |
| url | yes | Absolute URL, or a path relative to index.json. |
| sha256 | yes | 64 hexadecimal characters. Required and verified before extraction. |
| extract | no | zip, tar.gz, tar, 7z, or none. Inferred from the URL when omitted. |
| strip_components | no | Number of leading path components to remove after extraction. |
| bin | no | Commands to expose through a symlink or rv-shim.exe. |
| fonts | no | Font files or directories inside the archive. Installs per-user. |
| depends | no | Package names, repo/name, or name@version. |
| persist | no | Paths preserved across versions under persist/<package>/. |

## bin forms

Simple form uses the file stem as the command name:

~~~toml
bin = ["hello.exe"]
~~~

Detailed form:

~~~toml
bin = [
  { path = "bin/hello.exe", name = "hello" },
  { path = "scripts/hello.cmd", name = "hello-legacy", args = ["--compat"] },
]
~~~

The path is relative to the extracted package root. Every declared bin must exist after extraction, or the install fails before activation. A command name cannot be empty, contain Windows path or wildcard characters, end in a dot or space, or be a reserved device name such as CON or NUL.

## fonts

Font-only packages omit `bin` and declare `fonts`:

~~~toml
name = "jetbrains-mono"
version = "2.304.0"
url = "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip"
sha256 = "6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf"
extract = "zip"
fonts = ["fonts/ttf"]
~~~

Each entry is a file or a directory relative to the extracted package root. Directories are scanned recursively for .ttf, .otf, .ttc, and .otc files. Wildcards are not supported. A package may declare both `bin` and `fonts`; it must declare at least one.

Fonts are installed per-user without elevation:

1. copy each font to %LOCALAPPDATA%\Microsoft\Windows\Fonts;
2. write a value under HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts;
3. broadcast WM_FONTCHANGE.

Uninstall removes the registry values and font files. If a font is loaded by another application, Windows may refuse the delete; close the application and run `rivet remove` again. Existing files with the same name that Rivet does not manage are never overwritten.

Per-user font installation requires Windows 10 version 1809 (build 17763) or newer. Machine-wide fonts in %WINDIR%\Fonts require administrator rights and are out of scope. Rivet does not execute installer scripts, so font packages must be portable archives.

## depends

~~~toml
depends = ["runtime", "demo/tools", "runtime@1.0.0"]
~~~

Dependencies are installed recursively. An explicit @version is respected; without one, the latest version in the same repo is selected. The beta does not backtrack or solve version conflicts.

## persist

~~~toml
persist = ["data", "config/settings.json"]
~~~

On first install, files shipped inside the package at these paths are moved into persist/<package>/ and replaced with junctions. Later upgrades and rollbacks keep the persist data.

## Add and update packages

Generate a manifest from the latest GitHub release and rebuild the index:

~~~powershell
.\scripts\repo.ps1 add BurntSushi/ripgrep --path repo\main
~~~

Override auto-detection when needed:

~~~powershell
.\scripts\repo.ps1 add owner/repo --path repo\main --name package-name
.\scripts\repo.ps1 add owner/repo --path repo\main --asset windows-x64.zip
.\scripts\repo.ps1 add owner/repo --path repo\main --bin bin\tool.exe
~~~

Refresh all GitHub-hosted packages:

~~~powershell
.\scripts\repo.ps1 update repo\main --check
.\scripts\repo.ps1 update repo\main
~~~

Set `GITHUB_TOKEN` or `GH_TOKEN` to raise the GitHub API rate limit. Packages with non-GitHub URLs or an installer instead of a portable archive still need a hand-written manifest.

## Bulk import

Keep the curated candidate list in `repo/main/sources.toml`. It supports a compact repository list and per-package overrides:

~~~toml
repositories = [
  "BurntSushi/ripgrep",
  "sharkdp/fd",
]

[[source]]
repository = "cli/cli"
name = "gh"

[[source]]
repository = "owner/repo"
asset = "tool-windows-amd64.zip"
bin = "bin/tool.exe"
~~~

Add every missing package:

~~~powershell
.\scripts\repo.ps1 sync repo\main --dry-run
.\scripts\repo.ps1 sync repo\main --limit 10
.\scripts\repo.ps1 sync repo\main
~~~

`sync` matches existing manifests by their GitHub release URL, generates only missing packages, continues after individual failures, and rebuilds the index once at the end. Use `--allow-failures` for an exploratory run; without it, any failed source makes the command exit non-zero after the successful additions are written. Run in batches and verify before committing:

~~~powershell
cargo build --release --bin rv --bin rv-repo
.\scripts\verify-catalog.ps1 -Root repo\main
~~~

The verification script installs every package into a temporary `RIVET_ROOT`, runs `rivet verify`, and smoke-tests each command with a version argument. The weekly Catalog verify workflow runs the same check.

Font-only packages are not generated by `add` or `sync`; write the manifest by hand and declare `fonts`. Use `asset` when the release asset name does not contain a Windows architecture marker, so `rv-repo update` can still find it.

## Build and sign

~~~powershell
.\scripts\repo.ps1 build repo\main
.\scripts\repo.ps1 keygen --out C:\keys\rivet
.\scripts\repo.ps1 sign repo\main --key C:\keys\rivet\rivet-signing.key
~~~

Sign after every build; the signature covers the exact index.json bytes. The main catalog is signed by the daily catalog workflow using the `RIVET_SIGNING_KEY_B64` GitHub Actions secret. Keep the private key outside the repository and do not commit it.
