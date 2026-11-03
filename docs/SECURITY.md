# Security

Rivet trusts two things explicitly: the signing key the user configures, and the SHA-256 recorded in every package manifest.

## Index signatures

- The main index can be signed with Ed25519 (via `ring`). Tap indexes may also be signed.
- The signature covers the exact bytes of `index.json` and is checked before the JSON is parsed or written to SQLite.
- `index.json.sig` is a small JSON document:

  ~~~json
  { "schema": 1, "algorithm": "ed25519", "public_key": "<64 hex>", "signature": "<128 hex>" }
  ~~~

- Remote (HTTP) indexes fail closed. A missing signature, an untrusted key, or a bad signature aborts `rv update` before parsing.
- Local `file://` paths and directories may be unsigned, because they are the development path.
- `--insecure` and `rv config set allow_unsigned_remote true` are the only ways to accept an unsigned remote index. Neither is enabled by default.

## Keys

- `rv repo keygen --out <dir>` writes `rivet-signing.key` (PKCS#8, binary) and `rivet-signing.pub` (64 hex characters).
- No command prints the private key. `rv repo public-key` prints only the public half.
- The private key must never be committed. Keep the key directory outside the repository; `.gitignore` also ignores `*.key`.
- `rv repo sign <repo> --key <private key>` writes `<index>.sig` next to the index. Sign after `rv repo build`, because the signature covers the generated file.
- The main catalog workflow stores the PKCS#8 key as the encrypted GitHub Actions secret `RIVET_SIGNING_KEY_B64`. The workflow decodes it into a temporary file, signs, and deletes the file. It is never written to the repository or workflow logs.
- The secret is only as strong as repository write access. Protect `master` and require review for workflow changes; anyone who can change a workflow can read the secret.
- Key rotation: generate a new pair, replace the Actions secret, sign the index with it, publish the new public key, and have clients run `rv config set trusted_key <hex>`. Remove stale keys from `config.toml`.

## Artifacts

- Every manifest requires a 64-hex SHA-256. `PackageManifest::validate` rejects anything else at index build and at load.
- Downloads are verified against the manifest digest before extraction. A mismatch deletes the partial file and aborts the install.
- Cached blobs are re-verified whenever they are reused.

## Scope

Rivet does not run install scripts, PowerShell hooks, or MSI/EXE system installers, so package installation has no arbitrary code execution step beyond launching the packaged binaries the user chooses to run.

## Reporting

Report a vulnerability to the maintainer privately before opening a public issue. Include the Rivet version (`rv --version`), the command, and the relevant output.
