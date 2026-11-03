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
| url | yes | Absolute URL, or a path relative to index.json. |
| sha256 | yes | 64 hexadecimal characters. Required and verified before extraction. |
| extract | no | zip, tar.gz, tar, 7z, or none. Inferred from the URL when omitted. |
| strip_components | no | Number of leading path components to remove after extraction. |
| bin | no | Commands to expose through a symlink or rv-shim.exe. |
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

## Build and sign

~~~powershell
.\scripts\repo.ps1 build repo\main
.\scripts\repo.ps1 keygen --out C:\keys\rivet
.\scripts\repo.ps1 sign repo\main --key C:\keys\rivet\rivet-signing.key
~~~

Sign after every build; the signature covers the exact index.json bytes. The main catalog is signed by the daily catalog workflow using the `RIVET_SIGNING_KEY_B64` GitHub Actions secret. Keep the private key outside the repository and do not commit it.
