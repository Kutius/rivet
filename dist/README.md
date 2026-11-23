# Rivet 0.1.0-beta.8

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

- Concurrent artifact prefetching in `rivet install` and `rivet upgrade` (`config.concurrency`) plus process-wide HTTP connection pooling and `--verbose` diagnostics.
- Native Win32 Registry API for `PATH` management (replacing `reg.exe`), hardened tar extraction (ignoring symlinks/hardlinks), unified `index.json` schema, and stricter install/config/lock safety checks.
- Scalable catalog automation: concurrent `rv-repo update` with partial-success Issue reporting, 4-shard parallel `Catalog verify`, `rv-repo import-scoop`, and automatic `sources.toml` sync.
- The main catalog now contains 81 manifests (80 packages), adding actionlint, age, ast-grep, buf, caddy, cloudflared, dbmate, difftastic, dive, dotenvx, duf, git-cliff, gitleaks, glow, grpcurl, gum, hadolint, k9s, micro, mise, nushell, scc, sops, sqlc, tealdeer, tree-sitter, typos, and yt-dlp.

## Release artifacts

- `rivet-v0.1.0-beta.8-windows-x64.zip`: portable release with `rv.exe`, `rv-shim.exe`, docs, LICENSE, and an internal `SHA256SUMS`. `rivet.exe` is created by `rivet init`.
- `rv.exe`: the Rivet CLI.
- `rv-shim.exe`: the lightweight launcher used for script commands and systems without symlink permission.
- `install.ps1`: the one-command installer.
- `SHA256SUMS`: checksums for `rv.exe`, `rv-shim.exe`, `install.ps1`, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.8-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

No administrator rights are required. See `docs/INSTALL.md` for the full install, upgrade, and uninstall flow.
