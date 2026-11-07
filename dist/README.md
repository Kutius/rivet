# Rivet 0.1.0-beta.7

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

- Per-user font packages: the `fonts` manifest field copies .ttf/.otf/.ttc/.otc files into %LOCALAPPDATA%\Microsoft\Windows\Fonts, registers them under HKCU, and removes them on uninstall. No administrator rights or installer scripts.
- `rivet verify` and `rivet doctor --fix` now check and repair font files and registry values.
- `rv-repo sync` imports every missing GitHub repository from `repo/main/sources.toml`; `scripts/verify-catalog.ps1` installs the whole catalog into a temporary root and smoke-tests every command.
- The main catalog is now 53 package versions, including JetBrains Mono, Fira Code, gh, deno, bun, pandoc, duckdb, rclone, restic, lazygit, and the earlier CLI set.

## Release artifacts

- rivet-v0.1.0-beta.7-windows-x64.zip: portable release with rv.exe, rv-shim.exe, docs, LICENSE, and an internal SHA256SUMS. `rivet.exe` is created by `rivet init`.
- rv.exe: the Rivet CLI.
- rv-shim.exe: the lightweight launcher used for script commands and systems without symlink permission.
- install.ps1: the one-command installer.
- SHA256SUMS: checksums for rv.exe, rv-shim.exe, install.ps1, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.7-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See docs/INSTALL.md for the full install, upgrade, and uninstall flow.
