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
| [`sdk-guide.md`](./sdk-guide.md) | on demand | Concrete API: `EngineApp`, `Vessel`, input, scene, backends. |
| [`conventions.md`](./conventions.md) | on demand | Game project structure and patterns. |
| [`security-privacy.md`](./security-privacy.md) | on demand | No dangerous code, no secrets in builds. |

## Knowledge

| File | Purpose |
|---|---|
| [`knowledge/context.md`](./knowledge/context.md) | SDK surface + commands. |
| [`knowledge/MEMORY.md`](./knowledge/MEMORY.md) | Notes about *your* game (update as it grows). |

## Sub-agents — [`agents/`](./agents/)

| Agent | Use when |
|---|---|
| [`orchestrator`](./agents/orchestrator.md) | Default front door / routing. |
| [`gameplay-expert`](./agents/gameplay-expert.md) | Entities via `Vessel`, the `EngineApp` loop, input, components. |
| [`scene-design-expert`](./agents/scene-design-expert.md) | Scene setup, spawning, lighting, game UI — design via `/impeccable`. |
| [`security-auditor`](./agents/security-auditor.md) | No secrets in builds; safe SDK usage. |

## Skills — [`skills/`](./skills/)

| Skill | Use when |
|---|---|
| [`start-a-game`](./skills/start-a-game/SKILL.md) | Scaffold a new game on the SDK. |
| [`spawn-entity`](./skills/spawn-entity/SKILL.md) | Spawn entities via `Vessel`. |
| [`setup-input`](./skills/setup-input/SKILL.md) | Bind input actions with `InputMap`. |
| [`load-scene`](./skills/load-scene/SKILL.md) | Load / serialize a scene. |
| [`pack-and-ship`](./skills/pack-and-ship/SKILL.md) | Build, pack assets, ship a runtime. |

## Reference

The **`sandbox`** example (`examples/sandbox/src/main.rs` in the engine repo) is the canonical game using
only the SDK — read it when in doubt.

---

*Load the one doc, skill, or agent the task needs. The orchestrator routes; the index maps.*
