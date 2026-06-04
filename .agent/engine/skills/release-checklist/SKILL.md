---
name: release-checklist
description: Runs the pre-push quality and security gate for the engine. Use before committing or pushing, or when the user asks to prepare a change for review or release.
---

# Release checklist (pre-push gate)

Run in order; stop and fix on the first failure.

1. **Full CI gate** — `cargo xtask all` (fmt + clippy + test + doc). Must be green.
   - Equivalent manual: `cargo fmt --all`, `cargo clippy --workspace`, `cargo test --workspace`, `cargo test --doc`.
2. **Run once** — `cargo run -p sandbox`: clean frame loop, no Vulkan validation errors, scene renders.
3. **Security & secrets** — invoke the [`security-auditor`](../agents/security-auditor.md) agent: no new
   `unsafe` without `// SAFETY:`, no secrets/keys/`.env`/personal paths in the diff, dependencies justified.
   The `secret-scan` pre-commit hook is the automated backstop.
4. **Docs in sync** — public-API changes have matching rustdoc + mdBook chapter updates (in place, no ADR file).
5. **Summary** — list files changed, tests affected, and anything skipped. Report honestly.

## Hard gate
- **Never** push, create a PR, or modify CI without explicit user permission (see [`../RULES.md`](../RULES.md) §10).
- Develop on `dev`; if you are on `main`, branch first.
- If a secret was ever committed in history, **rotate it** — deletion in a later commit is not enough.
