# Rivet 0.1.0-beta.5

## Install

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer downloads the zip, verifies SHA256SUMS, runs rivet init, creates the `rivet.exe` PowerShell alias, and refreshes the signed main catalog. For a portable install:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

## What changed

- `rivet.exe` is installed next to `rv.exe` so PowerShell's built-in `rv` alias cannot shadow the CLI.
- `rivet repo add owner/repo` generates a manifest from the latest GitHub release, picks the Windows x64 asset, infers the executable, verifies SHA-256, and rebuilds the index.
- `rivet repo update [--check] [--package NAME]` refreshes GitHub-hosted packages to their latest releases.
- `GITHUB_TOKEN` / `GH_TOKEN` support raises the GitHub API rate limit for catalog maintenance.
- `rivet doctor --fix` restores a missing `rivet.exe` alias.

## Release artifacts

- rivet-v0.1.0-beta.5-windows-x64.zip: portable release with rv.exe, rivet.exe after init, rv-shim.exe, docs, LICENSE, and an internal SHA256SUMS.
- rv.exe: the Rivet CLI.
- rv-shim.exe: the lightweight launcher used for script commands and systems without symlink permission.
- install.ps1: the one-command installer.
- SHA256SUMS: checksums for rv.exe, rv-shim.exe, install.ps1, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.5-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See docs/INSTALL.md for the full install, upgrade, and uninstall flow.
