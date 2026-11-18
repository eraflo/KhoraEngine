# Khora SDK — Gamedev Profile Index

Entry point for AI agents building a **game with the Khora Engine**. Load what the task needs.

> Start with [`SOUL.md`](./SOUL.md) (identity + SDK map) and [`RULES.md`](./RULES.md) (SDK-only
> constraints). Then route from here.

---

## Core docs

| Doc | Load | Purpose |
|---|---|---|
| [`SOUL.md`](./SOUL.md) | always | Orchestrator identity, values, SDK map, routing. |
| [`RULES.md`](./RULES.md) | always | SDK-only rules, boundaries, permission model. |
| [`workflow-rpi.md`](./workflow-rpi.md) | always | The Research → Plan → Implement loop + context discipline. |
| [`sdk-guide.md`](./sdk-guide.md) | on demand | The Rust side: `EngineApp`, bootstrap, assets, seal, `GameWorld`, input, `Activation`, saves. |
| [`conventions.md`](./conventions.md) | on demand | Project layout, scripts and native code, patterns. |
| [`security-privacy.md`](./security-privacy.md) | on demand | No dangerous code, no secrets in builds. |

## Knowledge

| File | Purpose |
|---|---|
| [`knowledge/context.md`](./knowledge/context.md) | SDK surface + commands. |
| [`knowledge/MEMORY.md`](./knowledge/MEMORY.md) | Notes about *your* game (update as it grows). |

## Research subagents — [`agents/`](./agents/)

Read-only workers for **context control**: they run noisy searches in a separate context and return a
distilled `file:line` summary. Dispatch them during the Research phase; the main agent implements.

| Agent | Use when |
|---|---|
| [`orchestrator`](./agents/orchestrator.md) | Default front door / routing / drive the RPI loop. |
| [`codebase-locator`](./agents/codebase-locator.md) | Find **where** game files / SDK calls live. |
| [`codebase-analyzer`](./agents/codebase-analyzer.md) | Understand **how** a mechanism works (`file:line`). |
| [`codebase-pattern-finder`](./agents/codebase-pattern-finder.md) | Find an existing example to mirror (often the `sandbox`). |
| [`knowledge-locator`](./agents/knowledge-locator.md) | Find prior research/plans/notes in `docs/*` + `knowledge/`. |
| [`security-auditor`](./agents/security-auditor.md) | No secrets in builds; safe SDK usage. |

## Domain reference — [`reference/`](./reference/)

On-demand knowledge per domain. Load the one the task touches; consult it during Research and Implement.

| Reference | Domain |
|---|---|
| [`ergon`](./reference/ergon.md) | Ergon — behaviors, components, systems, events, queries, input actions. |
| [`gameplay`](./reference/gameplay.md) | Gameplay across Ergon and Rust: entities, input, physics, which families run. |
| [`scene-design`](./reference/scene-design.md) | Scenes in the editor, lighting, materials, prefabs, game UI — design via `/impeccable`. |

## Skills — [`skills/`](./skills/)

| Skill | Use when |
|---|---|
| [`research-codebase`](./skills/research-codebase/SKILL.md) | RPI phase 1 — understand a problem → `docs/research/`. |
| [`create-plan`](./skills/create-plan/SKILL.md) | RPI phase 2 — write a phase-by-phase plan → `docs/plans/`. |
| [`implement-plan`](./skills/implement-plan/SKILL.md) | RPI phase 3 — execute a plan, compact status per phase. |
| [`start-a-game`](./skills/start-a-game/SKILL.md) | Create the project; add native code when needed. |
| [`write-a-script`](./skills/write-a-script/SKILL.md) | Gameplay in Ergon: a behavior, a component, a system, an event. |
| [`spawn-entity`](./skills/spawn-entity/SKILL.md) | Spawn entities from a script, or from Rust via `Vessel`. |
| [`setup-input`](./skills/setup-input/SKILL.md) | Bind input actions with `InputMap`; read them in scripts. |
| [`load-scene`](./skills/load-scene/SKILL.md) | Load / save a scene; save and load the game. |
| [`add-a-family`](./skills/add-a-family/SKILL.md) | A game's own subsystem: a family of lanes and one `register_agent!`. |
| [`pack-and-ship`](./skills/pack-and-ship/SKILL.md) | Build Game: signed pack, stamped binary; what refuses a start. |

## Reference

The engine repo's **`sandbox`** (`examples/sandbox/`) is a native game on the SDK with one Ergon
behavior (`assets/scripts/hover.erg`). The engine book (`docs/src/`) holds the full SDK surface
(`reference/sdk.md`), the language (`reference/ergon.md`) and the tutorials — read them when in doubt.

---

*Load the one doc, skill, or agent the task needs. The orchestrator routes; the index maps.*
