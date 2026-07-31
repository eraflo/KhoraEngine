# Hook — teardown (session end)

Checklist before wrapping up an engine-development session.

1. **Verify** — `cargo test --workspace` passes (report the live count); `cargo build` clean, clippy clean.
2. **Security** — no secrets/keys/personal paths in the diff (the `secret-scan` hook is the backstop);
   invoke the `security-auditor` for anything touching `unsafe` or dependencies.
3. **Knowledge** — update [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md) if the build state, latest work,
   or known issues changed. Record durable findings; don't record ephemeral chatter.
4. **Summary** — list files changed, tests affected, and anything skipped or left failing — honestly.
5. **No auto-push** — never push, open a PR, or modify CI without explicit user permission.
