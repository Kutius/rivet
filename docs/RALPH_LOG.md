# Ralph Log

## Round 0 — gate setup

- Defined the public beta gate in docs/BETA.md.
- No product code changed yet.
- Next: audit the current tree against B1–B8 and start with the highest-risk P0.

## Round 1 — B1 trusted catalog

- Baseline before changes: `cargo test --offline` green (4 e2e tests).
- Added Ed25519 index signing and verification using `ring` 0.17, which was already in `Cargo.lock` through rustls, so no new crate download was needed.
- New `src/trust.rs`: `keygen` (PKCS#8 private key + hex public key), `sign`, `load_private_key`, `verify_signature`, `verify_index`, `Trust`.
- `index::load_index(location, trust)` now verifies the exact index bytes before parsing. HTTP indexes fetch `<url>.sig` and fail closed when it is missing, signed by an untrusted key, or invalid. Local file/directory indexes may stay unsigned; a local `.sig` is verified when present.
- Commands: `rv repo keygen [--out DIR]`, `rv repo sign PATH --key KEY`, `rv repo public-key --key KEY`. Config: `trusted_key`/`trusted_keys`, `allow_unsigned_remote`; env `RIVET_TRUSTED_KEY`; global `--insecure` override.
- Tests: `tests/trust.rs` adds 6 tests (signed local index + tamper/wrong-key/no-key failures, signed remote index, unsigned remote fail-closed + `--insecure`, unsigned local index, keygen never prints the private key, bad checksum rejected). Suite: 4 e2e + 6 trust, all pass.
- Docs: README "Signing the index", new `docs/SECURITY.md`, `docs/BETA.md` B1 evidence, `.gitignore` ignores `*.key`.
- Remaining P0: B2–B8. Clippy still reports pre-existing style warnings in `src/commands.rs`, `src/repo.rs`, and `src/store.rs` (B7).
- Next: B2 mutation safety — process lock with stale-lock recovery, reverse-dependency protection with `--force`, and `rv doctor --fix`.
