# Rivet 0.1.0-beta.9

## Install

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer downloads the zip, verifies SHA256SUMS, runs `rivet init`, creates the `rivet.exe` PowerShell alias, and refreshes the signed main catalog. For a portable install:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

## What changed

- Modern, zero-dependency interactive terminal UI: smooth horizontal download progress bars (`━━━───`) with live speed, Braille spinners for extraction/activation/removal/rollback/catalog refresh, elapsed duration & cache indicators, `→ ready to run` command hints, preserved persist-data notices on uninstall, and multi-package summaries. Automatically degrades to clean plain text when piped or with `NO_COLOR`.
- `Catalog update` now automatically triggers GitHub Pages deployment whenever the signed catalog is updated.

## Release artifacts

- `rivet-v0.1.0-beta.9-windows-x64.zip`: portable release with `rv.exe`, `rv-shim.exe`, docs, LICENSE, and an internal `SHA256SUMS`. `rivet.exe` is created by `rivet init`.
- `rv.exe`: the Rivet CLI.
- `rv-shim.exe`: the lightweight launcher used for script commands and systems without symlink permission.
- `install.ps1`: the one-command installer.
- `SHA256SUMS`: checksums for `rv.exe`, `rv-shim.exe`, `install.ps1`, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.9-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See `docs/INSTALL.md` for the full install, upgrade, and uninstall flow.
