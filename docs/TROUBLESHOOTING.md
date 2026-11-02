# Troubleshooting

## rv: command not found

Run rv doctor. If the bin directory is not in PATH, run rv init and open a new terminal. If PATH still does not update, add %LOCALAPPDATA%\Rivet\bin manually. rv init sends WM_SETTINGCHANGE, but some shells cache their environment.

## main index is not configured

`rv init` normally writes the official catalog URL and trusted key. If the config was created by an older build, run:

~~~powershell
rv config set main_index https://kutius.github.io/rivet/index.json
rv config set trusted_key <64-hex-public-key>
rv update
~~~

For local development, a file path is accepted and may be unsigned:

~~~powershell
rv config set main_index C:\src\rivet\repo\main\index.json
rv update
~~~

A remote index without a valid signature is refused by design.

## signature verification failed

The index bytes changed after signing, the wrong key is trusted, or the .sig file is missing. Rebuild, re-sign, and confirm the public key:

~~~powershell
rv repo build repo\main
rv repo sign repo\main --key C:\keys\rivet\rivet-signing.key
rv repo public-key --key C:\keys\rivet\rivet-signing.key
rv config set trusted_key <the 64-hex key>
rv update
~~~

## sha256 mismatch

The downloaded artifact does not match the manifest. This is a hard failure; the partial file is deleted. Re-download by running the install again. If it persists, the upstream file or the manifest is wrong.

## missing declared bin

The archive does not contain a path declared in bin. The install fails before the current version is switched. Fix the manifest or the archive; the existing installed version is untouched.

## missing rv-shim.exe

rv doctor reports this. Run rv init from a release zip that contains both rv.exe and rv-shim.exe. A source build must run cargo build --release --bins.

## broken current link or missing shim

~~~powershell
rv doctor
rv doctor --fix
rv verify
~~~

--fix repairs a current junction and missing shims when the store version still exists. A missing store is not repairable; reinstall the package.

## rv update fails but installed packages still work

The local SQLite index is a cache. Search, list, status, and upgrade can use the last successfully loaded index. rv status --check reports a metadata refresh warning and prints the cached state instead of failing.

## Remove refuses because another package depends on it

Remove the dependent first. If the dependency is genuinely no longer needed, pass --force and then repair the dependent package.

## Where data lives

~~~text
%LOCALAPPDATA%\Rivet\
  bin\                 rv.exe, rv-shim.exe, command shims
  store\<name>\<version>\
  apps\<name>\current   junction to the active version
  persist\<name>\
  cache\blobs\<sha256>\
  taps\<name>\
  state.db
  config.toml
~~~

The store is the source of truth. rv list and rv verify read the database; rv doctor checks the filesystem.
