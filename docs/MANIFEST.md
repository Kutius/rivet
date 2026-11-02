# Manifest reference

A package is one TOML file under packages/. rv repo build validates every manifest and writes a single index.json.

## Example

~~~toml
name = "hello"
version = "1.1.0"
description = "Rivet demo package"
homepage = "https://example.invalid/rivet/hello"
license = "MIT"
tags = ["demo", "cli"]
arch = "x64"
url = "artifacts/hello-1.1.0.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
extract = "zip"
strip_components = 0
bin = [{ path = "hello.exe", name = "hello" }]
depends = ["runtime@1.0.0"]
persist = ["data"]
~~~

## Fields

| Field | Required | Meaning |
|---|---|---|
| name | yes | Lowercase package name; letters, digits, -, _, . |
| version | yes | Semantic version. The highest version is the default install target. |
| description | no | One-line description shown by search and info. |
| homepage | no | Project URL. |
| license | no | SPDX-style license identifier. |
| tags | no | Search keywords. |
| arch | no | Advisory architecture string, for example x64. The beta does not enforce it. |
| url | yes | Absolute URL, or a path relative to index.json. |
| sha256 | yes | 64 hexadecimal characters. Required and verified before extraction. |
| extract | no | zip, tar.gz, tar, 7z, or none. Inferred from the URL when omitted. |
| strip_components | no | Number of leading path components to remove after extraction. |
| bin | no | Commands to expose through rv-shim.exe. |
| depends | no | Package names, repo/name, or name@version. |
| persist | no | Paths preserved across versions under persist/<package>/. |

## bin forms

Simple form uses the file stem as the command name:

~~~toml
bin = ["hello.exe"]
~~~

Detailed form:

~~~toml
bin = [
  { path = "bin/hello.exe", name = "hello" },
  { path = "scripts/hello.cmd", name = "hello-legacy", args = ["--compat"] },
]
~~~

The path is relative to the extracted package root. Every declared bin must exist after extraction, or the install fails before activation. A command name cannot be empty, contain Windows path or wildcard characters, end in a dot or space, or be a reserved device name such as CON or NUL.

## depends

~~~toml
depends = ["runtime", "demo/tools", "runtime@1.0.0"]
~~~

Dependencies are installed recursively. An explicit @version is respected; without one, the latest version in the same repo is selected. The beta does not backtrack or solve version conflicts.

## persist

~~~toml
persist = ["data", "config/settings.json"]
~~~

On first install, files shipped inside the package at these paths are moved into persist/<package>/ and replaced with junctions. Later upgrades and rollbacks keep the persist data.

## Build and sign

~~~powershell
rv repo build repo\main
rv repo keygen --out C:\keys\rivet
rv repo sign repo\main --key C:\keys\rivet\rivet-signing.key
~~~

Sign after every build; the signature covers the exact index.json bytes. Keep the private key outside the repository.
