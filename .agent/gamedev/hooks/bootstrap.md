# Hook — bootstrap (session start)

Checklist the orchestrator runs when starting a game-development session.

1. **Identity** — load [`../SOUL.md`](../SOUL.md) and [`../RULES.md`](../RULES.md).
2. **State** — read [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md) for your game's current state.
3. **API** — keep [`../reference/ergon.md`](../reference/ergon.md) and [`../sdk-guide.md`](../sdk-guide.md)
   handy; the `sandbox` example is the reference.
4. **Tooling** — confirm rtk (token-optimized commands) and `/impeccable` (design) are available; the
   installer sets them up. Warn if missing, don't block.
5. **Baseline** (optional) — the project opens and plays in the editor; `cargo build` if it has native code.
   A restricted project neither plays nor builds — say so, never trust it for the user.

Keep context small: load `skills/` and `reference/` docs on demand via [`../index.md`](../index.md).
