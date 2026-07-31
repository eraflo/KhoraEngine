# Hook — bootstrap (session start)

Checklist the orchestrator runs at the start of an engine-development session.

1. **Identity** — load [`../SOUL.md`](../SOUL.md) and [`../RULES.md`](../RULES.md).
2. **State** — read [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md) for current branch, build status, and
   latest work; note any known issues.
3. **Git** — current branch (`dev` for active work; branch off `main` before changes). Note uncommitted changes.
4. **Tooling** — confirm the bootstrap tools are available (codegraph MCP for symbol lookup, rtk for
   token-optimized commands, `/impeccable` for design). The installer sets these up; warn if missing,
   don't block.
5. **Build sanity** (optional, on demand) — `cargo build` to confirm a clean baseline before editing.

Keep context small: load docs/agents on demand via [`../index.md`](../index.md), not all up front.
