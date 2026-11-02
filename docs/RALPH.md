# Ralph Loop Contract

Objective: take Rivet from working MVP to public beta ready, as defined by docs/BETA.md.

## Every round

1. Work in C:\Users\kutius\projects\rivet.
2. Read README.md, docs/DESIGN.md, docs/BETA.md, docs/RALPH_LOG.md, and git log --oneline -8 before changing code.
3. Run the current test suite first. Do not build on a broken tree.
4. Pick the highest-priority unchecked P0 item. Prefer one coherent change over many unrelated ones.
5. Implement it, add tests that fail without the change, update the relevant docs, and run:
   - cargo fmt
   - cargo test --offline
   - cargo clippy --all-targets -- -D warnings when available
   - cargo build --offline --release --bins
6. Commit the round with a specific message.
7. Update docs/BETA.md with evidence next to each newly satisfied box. Never check a box without a command result, test name, or file reference.
8. Append a short round entry to docs/RALPH_LOG.md: what changed, evidence, remaining P0s, next step.
9. If a gate is impossible without credentials or a user decision, mark it blocked in the log with the exact missing input; continue with other P0s.

## Rules

- Keep Rivet lean. No Scoop compatibility, no PowerShell hooks, no MSI/EXE channel, no daemon, no speculative enterprise features.
- Do not publish, push, create a GitHub repository, or use network credentials. Local Git only.
- Do not modify the user's real PATH or real %LOCALAPPDATA%\Rivet. All manual tests use a temporary RIVET_ROOT.
- Preserve the existing architecture: one native binary, generated index, SQLite cache, immutable store, junctions, native shims.
- Security fixes must fail closed by default; an insecure escape hatch must be explicit and documented.
- cargo test --offline must stay green at every commit.
- The loop is complete only when every P0 gate has evidence. A worker report saying "done" is not evidence.
