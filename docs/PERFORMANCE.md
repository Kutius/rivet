# Performance

Measurements were taken on the development Windows machine with the 0.1.0-beta.1 release build; the measured code paths are unchanged in 0.1.0-beta.2:

~~~powershell
cargo build --release --bins
~~~

Times include process startup. The 10,000-package index is a synthetic index.json with valid manifests; update timing includes reading the file, validating 10,000 packages, replacing the SQLite rows, and rebuilding the FTS index. Search timing uses the same root after update. Each row is the average of 10 runs, except update (5 runs), measured with PowerShell Measure-Command.

## 10,000-package synthetic index

| Operation | Average | Min | Max |
|---|---:|---:|---:|
| rv --version | 8.8 ms | 8.2 ms | 11.7 ms |
| rv search pkg00042 | 12.0 ms | 11.4 ms | 14.5 ms |
| rv update (10k index) | 153.1 ms | 150.0 ms | 156.9 ms |

The index file is 2.99 MB. The search path reads only SQLite; it does not scan manifest files or the index. Update parses the complete index once and writes it in one SQLite transaction.

## What dominates

- For rv search, rv list, rv status, and rv info, the dominant cost is process startup and SQLite open, not package count.
- For rv install, the dominant cost is the artifact download, extraction, and the application itself, not package lookup.
- For rv update, the dominant cost is network transfer of the index and local SQLite insertion, not JSON scanning in PowerShell.

## Not measured here

- Cold NTFS cache with Defender scanning every extracted file.
- Large real artifacts.
- HTTPS index downloads from a public host.
- Concurrent rv processes.

Those belong in a release benchmark on representative hardware.
