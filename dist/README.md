# Rivet 0.1.0-beta.4

## Install

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer downloads the zip, verifies SHA256SUMS, runs rv init, and refreshes the signed main catalog. For a portable install:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

## What changed

- `rv self-update` downloads and verifies the latest release, then replaces the binaries after the current process exits.
- `rv self-uninstall --yes` removes packages, the PATH entry, and the Rivet root.
- `rv cleanup --dry-run` lists old store versions and cache entries without deleting them.
- Native .exe commands are direct symlinks when Developer Mode allows it; otherwise Rivet falls back to a 280 KB launcher shim.

## Release artifacts

- rivet-v0.1.0-beta.4-windows-x64.zip: portable release with rv.exe, rv-shim.exe, docs, LICENSE, and an internal SHA256SUMS.
- rv.exe: the Rivet CLI.
- rv-shim.exe: the lightweight launcher used for script commands and systems without symlink permission.
- install.ps1: the one-command installer.
- SHA256SUMS: checksums for rv.exe, rv-shim.exe, install.ps1, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.4-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See docs/INSTALL.md for the full install, upgrade, and uninstall flow.
