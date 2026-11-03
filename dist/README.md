# Rivet 0.1.0-beta.6

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

- The client no longer exposes the maintainer `repo` command family. Maintainer tooling now lives in the separate `rv-repo` binary, built from source with `.\scripts\repo.ps1`.
- The daily catalog workflow now builds `rv-repo`, updates GitHub-hosted packages, signs the index with the `RIVET_SIGNING_KEY_B64` Actions secret, smoke-installs changed packages, and pushes the result.
- The main catalog now includes hyperfine, dust, bottom, just, starship, xh, sd, procs, eza, lsd, watchexec, and typst.

## Release artifacts

- rivet-v0.1.0-beta.6-windows-x64.zip: portable release with rv.exe, rv-shim.exe, docs, LICENSE, and an internal SHA256SUMS. `rivet.exe` is created by `rivet init`.
- rv.exe: the Rivet CLI.
- rv-shim.exe: the lightweight launcher used for script commands and systems without symlink permission.
- install.ps1: the one-command installer.
- SHA256SUMS: checksums for rv.exe, rv-shim.exe, install.ps1, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.6-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See docs/INSTALL.md for the full install, upgrade, and uninstall flow.
