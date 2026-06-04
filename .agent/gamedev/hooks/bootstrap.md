# Hook — bootstrap (session start)

Checklist the orchestrator runs when starting a game-development session.

1. **Identity** — load [`../SOUL.md`](../SOUL.md) and [`../RULES.md`](../RULES.md).
2. **State** — read [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md) for your game's current state.
3. **API** — keep [`../sdk-guide.md`](../sdk-guide.md) handy; the `sandbox` example is the reference.
4. **Tooling** — confirm headroom (context compression), rtk (token-optimized commands), and `/impeccable`
   (design) are available; the installer sets them up. Warn if missing, don't block.
5. **Build sanity** (optional) — `cargo build` to confirm a clean baseline.

Keep context small: load docs/skills on demand via [`../index.md`](../index.md).
