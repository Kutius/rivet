# Troubleshooting

## rivet: command not found

Run `rivet doctor`. If the bin directory is not in PATH, run `rivet init` and open a new terminal. If PATH still does not update, add %LOCALAPPDATA%\Rivet\bin manually. `rivet init` sends WM_SETTINGCHANGE, but some shells cache their environment.

If `rivet` is missing but `rv.exe` still works, run `rivet doctor --fix`; it restores the `rivet.exe` alias.

## main index is not configured

`rivet init` normally writes the official catalog URL and trusted key. If the config was created by an older build, run:

~~~powershell
rivet config set main_index https://kutius.github.io/rivet/index.json
rivet config set trusted_key <64-hex-public-key>
rivet update
~~~

For local development, a file path is accepted and may be unsigned:

~~~powershell
rivet config set main_index C:\src\rivet\repo\main\index.json
rivet update
~~~

A remote index without a valid signature is refused by design.

## signature verification failed

The index bytes changed after signing, the wrong key is trusted, or the .sig file is missing. Rebuild, re-sign, and confirm the public key:

~~~powershell
.\scripts\repo.ps1 build repo\main
.\scripts\repo.ps1 sign repo\main --key C:\keys\rivet\rivet-signing.key
.\scripts\repo.ps1 public-key --key C:\keys\rivet\rivet-signing.key
rivet config set trusted_key <the 64-hex key>
rivet update
~~~

## sha256 mismatch

The downloaded artifact does not match the manifest. This is a hard failure; the partial file is deleted. Re-download by running the install again. If it persists, the upstream file or the manifest is wrong.

## missing declared bin

The archive does not contain a path declared in bin. The install fails before the current version is switched. Fix the manifest or the archive; the existing installed version is untouched.

## font install fails

`font file ... already exists and is not managed by Rivet` means another tool or a manual install already placed a font with that file name in %LOCALAPPDATA%\Microsoft\Windows\Fonts. Remove the existing file or choose a package that does not collide; Rivet will not overwrite it.

Font packages must be archives. A single raw .ttf download is rejected.

## font remove fails

Windows refuses to delete a font that an application is using. Close editors, terminals, and other applications that loaded the font, then run `rivet remove <package>` again. The package remains installed until the font files can be removed.

Fonts may not appear in an already-running application until it is restarted; Rivet broadcasts WM_FONTCHANGE, but applications are not required to handle it.

## missing rv-shim.exe

`rivet doctor` reports this. Run `rivet init` from a release zip that contains rv.exe, rivet.exe, and rv-shim.exe. A source build must run cargo build --release --bins.

## symlink shims

Rivet uses a direct symlink for native .exe commands when Developer Mode allows it, and falls back to a small rv-shim.exe launcher otherwise. Script commands (.cmd, .bat, .ps1, .py, .jar) always use the launcher.

If symlinks cause trouble with backup or endpoint-security tools, force the launcher mode and repair the existing commands:

~~~powershell
$env:RIVET_DISABLE_SYMLINKS = "1"
rivet doctor --fix
~~~

Run `rivet verify` afterwards.

## broken current link or missing shim

~~~powershell
rivet doctor
rivet doctor --fix
rivet verify
~~~

--fix repairs a current junction and missing shims when the store version still exists. A missing store is not repairable; reinstall the package.

## rivet update fails but installed packages still work

The local SQLite index is a cache. Search, list, status, and upgrade can use the last successfully loaded index. `rivet status --check` reports a metadata refresh warning and prints the cached state instead of failing.

## Remove refuses because another package depends on it

Remove the dependent first. If the dependency is genuinely no longer needed, pass --force and then repair the dependent package.

## rivet self-update

The updater stages the new binaries and finishes after the current process exits. It prints the helper log path. If the version does not change, check that log and confirm the process is not blocked by endpoint security. `rivet self-update --force` re-downloads the current release.

## rivet self-uninstall

`rivet self-uninstall --yes` removes packages, the PATH entry, and the Rivet root after the current process exits. If the directory remains, wait a few seconds and check the log path printed by the command; a process with an open handle can delay deletion.

## Where data lives

~~~text
%LOCALAPPDATA%\Rivet\
  bin\                 rv.exe, rivet.exe, rv-shim.exe, command shims
  store\<name>\<version>\
  apps\<name>\current   junction to the active version
  persist\<name>\
  cache\blobs\<sha256>\
  taps\<name>\
  state.db
  config.toml
~~~

The store is the source of truth. `rivet list` and `rivet verify` read the database; `rivet doctor` checks the filesystem.
