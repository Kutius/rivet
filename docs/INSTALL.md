# Install

Rivet is a portable-first Windows package manager. The beta is a portable zip plus a one-command installer; there is no MSI or system-wide installer.

## Supported

- Windows 10 and Windows 11, x64.
- PowerShell 7 or Windows PowerShell 5.1 for the commands shown here.
- No administrator rights are required.
- Git is required only for third-party taps, not for the main catalog.

## One command

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

The installer:

1. downloads the latest release zip and SHA256SUMS;
2. verifies the zip hash before extracting it;
3. copies rv.exe and rv-shim.exe to %LOCALAPPDATA%\Rivet\bin;
4. appends that directory to the user PATH once and broadcasts the change;
5. writes the default catalog URL and trusted key, then runs rv update.

Open a new terminal and check:

~~~powershell
rv --version
rv doctor
rv search ripgrep
~~~

## Portable install

To install the binaries without changing PATH:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 -OutFile install.ps1
.\install.ps1 -NoPath
~~~

This still installs to %LOCALAPPDATA%\Rivet\bin. Run rv by full path, or add the directory to PATH yourself.

To pin a specific version:

~~~powershell
.\install.ps1 -Version 0.1.0-beta.2
~~~

To skip the catalog refresh:

~~~powershell
.\install.ps1 -NoUpdate
~~~

## Manual zip install

1. Download rivet-v0.1.0-beta.2-windows-x64.zip and SHA256SUMS from the release page.
2. Verify the archive before extracting:

   ~~~powershell
   $expected = (Get-Content .\SHA256SUMS | Select-String 'rivet-v0.1.0-beta.2-windows-x64.zip').Line.Split()[0]
   (Get-FileHash .\rivet-v0.1.0-beta.2-windows-x64.zip -Algorithm SHA256).Hash.ToLower() -eq $expected
   ~~~

3. Extract the zip anywhere, then run:

   ~~~powershell
   .\rv.exe init
   ~~~

4. Open a new terminal and check:

   ~~~powershell
   rv --version
   rv doctor
   ~~~

The current beta is 0.1.0-beta.2.

## Package source

The default catalog is:

~~~text
https://kutius.github.io/rivet/index.json
~~~

rv init writes that URL and the matching Ed25519 public key into config.toml. No manual configuration is needed.

For a custom catalog:

~~~powershell
rv config set main_index https://example.com/rivet/index.json
rv config set trusted_key <64-hex-public-key>
rv update
~~~

For local development, a file path or directory is accepted and may be unsigned:

~~~powershell
rv config set main_index C:\src\rivet\repo\main\index.json
rv update
~~~

For a signed remote index, the client fetches index.json.sig next to index.json. A missing signature or an untrusted key fails closed. --insecure is the explicit override; do not use it for a real catalog.

## Upgrade Rivet

There is no self-update command in the beta. Run the installer again:

~~~powershell
irm https://kutius.github.io/rivet/install.ps1 | iex
~~~

Installed packages and persist data are not touched.

## Uninstall

1. Remove installed packages with rv remove, adding --purge for packages whose persist data should also go.
2. Remove the Rivet bin directory from the user PATH (Settings > System > About > Advanced system settings > Environment Variables).
3. Delete %LOCALAPPDATA%\Rivet.

## Build from source

Requires Rust 1.99 or newer and the MSVC toolchain.

~~~powershell
cargo build --release --bins
cargo test --offline
~~~

The binaries are target\release\rv.exe and target\release\rv-shim.exe.
