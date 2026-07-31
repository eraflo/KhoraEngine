---
name: implement-plan
description: Phase 3 of the RPI workflow — execute an approved plan from docs/plans/ one phase at a time, verifying each phase and compacting status back into the plan file (intentional compaction). Use once a plan is reviewed; keeps the working context small on long implementations.
---

# Implement a plan

The **Implement** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Turn the approved
`docs/plans/descriptive-name.md` into working game code, phase by phase, without ballooning context.

## Steps (loop per phase)
1. **Read the current phase** from the plan file — only that phase.
2. **Implement** exactly what it specifies. Match surrounding idioms. Honor the SDK-only rules: public
   `khora_sdk` surface only, math via `prelude::math`, `log::*` not `println!`, no `unwrap()` on IO,
   cache handles in `setup`, `sync_global_transform` after moving entities, no secrets in the build.
3. **Verify** with the phase's check: `cargo build`, then `cargo run` — window opens, behavior correct.
4. **Compact** — update the plan's **Status** checklist in place: mark the phase done, record any
   deviation and why. Drop the phase's working detail and move on.

## Rules
- The plan is the source of truth. If reality contradicts it, **stop and amend the plan** (surface it).
- Never mark a phase done while it doesn't build/run or a step was skipped — report it honestly.
- Comments must be self-contained — never reference plan step labels in code.
- Before shipping, run [`../pack-and-ship/SKILL.md`](../pack-and-ship/SKILL.md) (security gate included).
  Never push without explicit permission.
- Update [`../../knowledge/MEMORY.md`](../../knowledge/MEMORY.md) when the change lands.
