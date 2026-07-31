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
| [`workflow-rpi.md`](./workflow-rpi.md) | always | The Research → Plan → Implement loop + context discipline. |
| [`conventions.md`](./conventions.md) | on demand | Naming, code patterns, file layout, bind-group budget. |
| [`architecture.md`](./architecture.md) | on demand | CLAD map, 16 crates, trait map, components, file locations, codegraph. |
| [`security-privacy.md`](./security-privacy.md) | on demand | No dangerous code, never push secrets. |

## Knowledge (persistent memory)

| File | Purpose |
|---|---|
| [`knowledge/MEMORY.md`](./knowledge/MEMORY.md) | Current state, latest work, known issues. Update when state changes. |
| [`knowledge/decisions.md`](./knowledge/decisions.md) | Architecture decisions + rationale. |
| [`knowledge/context.md`](./knowledge/context.md) | Project facts, workspace, build commands. |

## Research subagents — [`agents/`](./agents/)

Read-only workers for **context control**: they run noisy searches in a separate context and return a
distilled `file:line` summary. Dispatch them during the Research phase; the main agent implements.

| Agent | Use when |
|---|---|
| [`orchestrator`](./agents/orchestrator.md) | Default front door / routing / drive the RPI loop. |
| [`codebase-locator`](./agents/codebase-locator.md) | Find **where** files/symbols/modules live (categorized map). |
| [`codebase-analyzer`](./agents/codebase-analyzer.md) | Understand **how** a mechanism works (`file:line` + CLAD flow). |
| [`codebase-pattern-finder`](./agents/codebase-pattern-finder.md) | Find an existing example/pattern to mirror. |
| [`knowledge-locator`](./agents/knowledge-locator.md) | Find prior research/plans/decisions in `docs/*` + `knowledge/`. |
| [`security-auditor`](./agents/security-auditor.md) | unsafe audit, supply chain, secrets, pre-push gate. |

## Domain reference — [`reference/`](./reference/)

On-demand knowledge (scope, key files, hard rules) per domain. Load the one the task touches; consult it
during Research and Implement. Replaces the former per-domain "expert" agents.

| Reference | Domain |
|---|---|
| [`graphics-rendering`](./reference/graphics-rendering.md) | wgpu, WGSL, render pipelines, PBR, shadows, bind-group budget. |
| [`physics`](./reference/physics.md) | Rapier3D, rigid bodies, colliders, CCD, physics lanes/flow. |
| [`audio`](./reference/audio.md) | CPAL devices, spatial mixing lanes, audio flow. |
| [`math`](./reference/math.md) | `khora_core::math`, explicit SIMD, numerical correctness. |
| [`ecs-data`](./reference/ecs-data.md) | CRPECS, storage, queries, SoA/AGDF layout, component registration. |
| [`control-gorna`](./reference/control-gorna.md) | DCC, GORNA negotiation/replay, cost model, PID budget, adaptation modes. |
| [`editor-ui-ux`](./reference/editor-ui-ux.md) | khora-editor panels, gizmos, dock — design via `/impeccable`. |
| [`api-ux`](./reference/api-ux.md) | khora-sdk public ergonomics, builder/type-state. |
| [`documentation`](./reference/documentation.md) | mdBook, rustdoc, diagrams. |
| [`deprecation`](./reference/deprecation.md) | API modernization, dead-code removal. |

## Task skills — [`skills/`](./skills/)

| Skill | Use when |
|---|---|
| [`research-codebase`](./skills/research-codebase/SKILL.md) | RPI phase 1 — understand a problem → `docs/research/`. |
| [`create-plan`](./skills/create-plan/SKILL.md) | RPI phase 2 — write a phase-by-phase plan → `docs/plans/`. |
| [`implement-plan`](./skills/implement-plan/SKILL.md) | RPI phase 3 — execute a plan, compact status per phase. |
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
