# Rivet 0.1.0-beta.1

Release artifacts:

- rivet-v0.1.0-beta.1-windows-x64.zip: portable release with rv.exe, rv-shim.exe, docs, LICENSE, and an internal SHA256SUMS.
- rv.exe: the Rivet CLI.
- rv-shim.exe: the native shim used for executable and script targets. Keep it next to rv.exe.
- SHA256SUMS: checksums for rv.exe, rv-shim.exe, and the zip.

Verify before running:

~~~powershell
Get-Content .\SHA256SUMS
(Get-FileHash .\rivet-v0.1.0-beta.1-windows-x64.zip -Algorithm SHA256).Hash.ToLower()
~~~

Install:

~~~powershell
Expand-Archive .\rivet-v0.1.0-beta.1-windows-x64.zip -DestinationPath .\rivet
.\rivet\rv.exe init
# open a new terminal
rv --version
~~~

No administrator rights are required. See docs/INSTALL.md for the full install, upgrade, and uninstall flow.
