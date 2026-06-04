# Hook — teardown (session end)

Checklist before wrapping up a game-development session.

1. **Verify** — `cargo build` clean; `cargo run` confirms the game still launches and renders.
2. **Security** — no secrets/keys/personal paths in the diff or build (the `secret-scan` hook is the
   backstop); run the `security-auditor` before any release.
3. **Notes** — update [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md) with new state, entities, or open tasks.
4. **Summary** — list files changed and anything skipped — honestly.
5. **No auto-push / no auto-ship** — never push or distribute a build without explicit permission.
