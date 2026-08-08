---
name: implement-plan
description: Phase 3 of the RPI workflow — execute an approved plan from docs/plans/ one phase at a time, verifying each phase and compacting status back into the plan file (intentional compaction). Use once a plan is reviewed; keeps the working context small on long implementations.
---

# Implement a plan

The **Implement** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Goal: turn the approved
`docs/plans/descriptive-name.md` into working code, phase by phase, without letting the context balloon.

## Steps (loop per phase)
1. **Read the current phase** from the plan file — only that phase, not the whole history.
2. **Implement** exactly what the phase specifies. Match surrounding idioms, naming, and comment density.
   Honor Khora's hard rules (`RULES.md`, relevant `reference/*.md`): `khora_core::math`, `log::*` not
   `println!`, no `unwrap()` on GPU/IO, no `std::thread::spawn`, shaders as `.wgsl` composed by the `PipelineSystem` backend.
3. **Verify** with the phase's check (via [`../build-and-test/SKILL.md`](../build-and-test/SKILL.md));
   primary gate is `cargo test --workspace`. For GPU work, one clean `cargo run -p sandbox`.
4. **Compact** — update the plan's **Status** checklist in place: mark the phase done, record any
   deviation from the plan and why, note the live test count. Then drop the phase's working detail from
   context and move to the next phase.

## Rules
- The plan is the source of truth. If reality contradicts it, **stop and amend the plan** (and surface it),
  don't silently improvise a different design.
- Never mark a phase done while its tests fail or a step was skipped — report it honestly instead.
- Comments must be self-contained — never reference plan step labels ("phase 2", "D1") in code.
- When all phases are green, run [`../release-checklist/SKILL.md`](../release-checklist/SKILL.md) before any
  commit/push. Never push without explicit permission.
- Update [`../../knowledge/MEMORY.md`](../../knowledge/MEMORY.md) "Latest work" when the change lands.
