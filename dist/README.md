# Rivet binaries

These are release binaries built from this repository:

- `rv.exe`: the Rivet CLI.
- `rv-shim.exe`: the native shim used for executable and script targets. Keep it next to `rv.exe` when running `rv` directly, or let `rv install` copy it into the Rivet bin directory.

Quick run from this directory:

~~~powershell
$env:RIVET_ROOT = Join-Path $env:TEMP "rivet-demo"
$env:RIVET_MAIN_INDEX = (Resolve-Path "..\repo\main\index.json").Path
.\rv.exe update
.\rv.exe search hello
.\rv.exe install hello
.\rv.exe list
~~~
