# Performance

Measurements were taken on the development Windows machine with the release build:

~~~text
cargo build --release --bins
~~~

Times include process startup and are measured with PowerShell `Measure-Command`. The artifact download is excluded from the search/list/status numbers.

## Small index

The demo main repository has two package versions.

| Command | Average |
|---|---:|
| `rv --version` | 10.1 ms |
| `rv search hello` | 11.9 ms |
| `rv list` | 11.6 ms |
| `rv status` | 11.7 ms |
| `rv info hello` | 12.8 ms |

## 10,000-package synthetic index

A synthetic index with 10,000 package versions was generated to check indexing and search scaling.

| Operation | Result |
|---|---:|
| `rv update`: parse index and rebuild SQLite FTS | 122 ms |
| `rv search pkg00042`: average of 10 runs | 12.2 ms |
| search minimum / maximum | 11.3 / 13.9 ms |

The search path reads only SQLite; it does not scan manifest files. The update path parses the complete index once and writes it in one SQLite transaction.

## What dominates

- For `rv search`, `rv list`, `rv status`, and `rv info`, the dominant cost is process startup and SQLite open, not package count.
- For `rv install`, the dominant cost is the artifact download, extraction, and the application installer, not package lookup.
- For `rv update`, the dominant cost is network transfer of the index and local SQLite insertion, not JSON scanning in PowerShell.

## Not measured here

- Cold NTFS cache with Defender scanning every extracted file.
- Large real artifacts.
- HTTPS index downloads from a public host.
- Concurrent `rv` processes.

Those belong in a release benchmark on representative hardware.
