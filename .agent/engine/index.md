# Khora Engine — Engine Profile Index

Entry point for AI agents working **on the engine** (contributors). Everything is one click away —
load what the task needs, not the whole tree.

> Start with [`SOUL.md`](./SOUL.md) (who you are + global map) and [`RULES.md`](./RULES.md) (hard
> constraints). Then come here to route.

---

## Core docs

| Doc | Load | Purpose |
|---|---|---|
| [`SOUL.md`](./SOUL.md) | always | Orchestrator identity, values, global engine map, routing rules. |
| [`RULES.md`](./RULES.md) | always | Must always / never, boundaries, permission model. |
| [`conventions.md`](./conventions.md) | on demand | Naming, code patterns, file layout, bind-group budget. |
| [`architecture.md`](./architecture.md) | on demand | CLAD map, 16 crates, trait map, components, file locations, codegraph. |
| [`security-privacy.md`](./security-privacy.md) | on demand | No dangerous code, never push secrets. |

## Knowledge (persistent memory)

| File | Purpose |
|---|---|
| [`knowledge/MEMORY.md`](./knowledge/MEMORY.md) | Current state, latest work, known issues. Update when state changes. |
| [`knowledge/decisions.md`](./knowledge/decisions.md) | Architecture decisions + rationale. |
| [`knowledge/context.md`](./knowledge/context.md) | Project facts, workspace, build commands. |

## Specialist sub-agents — [`agents/`](./agents/)

Invoke one when the task is squarely in its domain; otherwise work as the orchestrator.

| Agent | Use when |
|---|---|
| [`orchestrator`](./agents/orchestrator.md) | Default front door / routing / unclear scope. |
| [`graphics-rendering-expert`](./agents/graphics-rendering-expert.md) | wgpu, WGSL, render pipelines, PBR, shadows, bind-group budget. |
| [`physics-expert`](./agents/physics-expert.md) | Rapier3D, rigid bodies, colliders, CCD, physics lanes/flow. |
| [`audio-expert`](./agents/audio-expert.md) | CPAL devices, spatial mixing lanes, audio flow. |
| [`math-expert`](./agents/math-expert.md) | `khora_core::math`, explicit SIMD, numerical correctness. |
| [`ecs-data-expert`](./agents/ecs-data-expert.md) | CRPECS, storage, queries, SoA/AGDF layout, component registration. |
| [`control-gorna-expert`](./agents/control-gorna-expert.md) | DCC, GORNA negotiation/replay, cost model, PID budget, adaptation modes. |
| [`editor-ui-ux`](./agents/editor-ui-ux.md) | khora-editor panels, gizmos, dock — design via `/impeccable`. |
| [`api-ux-expert`](./agents/api-ux-expert.md) | khora-sdk public ergonomics, builder/type-state. |
| [`documentation-expert`](./agents/documentation-expert.md) | mdBook, rustdoc, diagrams. |
| [`security-auditor`](./agents/security-auditor.md) | unsafe audit, supply chain, secrets, pre-push gate. |
| [`deprecation-cleaner`](./agents/deprecation-cleaner.md) | API modernization, dead-code removal. |

## Task skills — [`skills/`](./skills/)

| Skill | Use when |
|---|---|
| [`build-and-test`](./skills/build-and-test/SKILL.md) | Validate a change (primary: `cargo test --workspace`). |
| [`add-a-lane`](./skills/add-a-lane/SKILL.md) | Add a hot-path `Lane`. |
| [`add-an-agent`](./skills/add-an-agent/SKILL.md) | Add a strategist `Agent`. |
| [`add-a-component`](./skills/add-a-component/SKILL.md) | Define + register an ECS component. |
| [`add-a-shader`](./skills/add-a-shader/SKILL.md) | Add a `.wgsl` shader + pipeline within the 4-group budget. |
| [`run-the-engine`](./skills/run-the-engine/SKILL.md) | Launch the editor or sandbox. |
| [`debug-frame`](./skills/debug-frame/SKILL.md) | Investigate a frame / GORNA decision. |
| [`release-checklist`](./skills/release-checklist/SKILL.md) | Pre-push quality + security gate. |

## External references

| Path | Purpose |
|---|---|
| [`../README.md`](../README.md) | Profiles overview + installer commands. |
| [`../../docs/src/`](../../docs/src/) | Full mdBook documentation (human narrative). |
| codegraph MCP | Symbol/edge graph — query before grepping (see [`architecture.md`](./architecture.md)). |

---

*Load the one doc or agent the task needs. The orchestrator routes; the index maps.*
