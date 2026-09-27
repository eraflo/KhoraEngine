---
name: implement-plan
description: Phase 3 of the RPI workflow — execute an approved plan from docs/plans/ one phase at a time (tests first by the test-writer subagent, then the implementation, then the test-breaker subagent tries to break it), verifying each phase and compacting status back into the plan file. Use once a plan is reviewed.
---

# Implement a plan

The **Implement** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Goal: turn the approved
`docs/plans/descriptive-name.md` into working code, phase by phase, without letting the context balloon.

## Steps (loop per phase)
1. **Read the current phase** from the plan file — only that phase, not the whole history.
2. **Tests first.** Dispatch the [`test-writer`](../../agents/test-writer.md) subagent with the plan path
   and the phase. It writes the phase's tests (plus `todo!()` signature stubs so they compile) and reports
   each failure it observed. Check every row of the phase's *Tests* has its test before going on.
3. **Implement** exactly what the phase specifies, until those tests pass. Match surrounding idioms,
   naming, and comment density. Honor Khora's hard rules (`RULES.md`, relevant `reference/*.md`):
   `khora_core::math`, `log::*` not `println!`, no `unwrap()` on GPU/IO, no `std::thread::spawn`, shaders
   as `.wgsl` composed by the `PipelineSystem` backend.
4. **Try to break it.** Dispatch the [`test-breaker`](../../agents/test-breaker.md) subagent with the plan
   path and the diff. It returns failing tests for real defects and missing cases.
5. **Fix** until the breaker's tests pass. If the fixes were substantial, dispatch the breaker once more
   (at most two rounds); list anything left open in the plan's Status and in the report.
6. **Verify** with the phase's check (via [`../build-and-test/SKILL.md`](../build-and-test/SKILL.md));
   `cargo gate`, once, when the phase is done; `cargo t -p <crate> [filter]` while iterating. For GPU work, one clean
   `cargo run -p sandbox`.
7. **Compact** — update the plan's **Status** checklist in place: mark the phase done, record any
   deviation from the plan and why, note the live test count. Then drop the phase's working detail from
   context and move to the next phase.

## Rules
- The plan is the source of truth. If reality contradicts it, **stop and amend the plan** (and surface it),
  don't silently improvise a different design.
- Never mark a phase done while its tests fail or a step was skipped — report it honestly instead.
- **Never edit or delete a test the `test-writer` or `test-breaker` wrote.** If one looks wrong, stop,
  say why, and have it corrected (by that subagent or the human); record the change in the plan's Status.
  Tests for private helpers you add yourself are yours to write.
- Trivial single-file changes that skip the RPI loop still get a test that fails before the change.
- Comments must be self-contained — never reference plan step labels ("phase 2", "D1") in code.
- When all phases are green, run [`../release-checklist/SKILL.md`](../release-checklist/SKILL.md) before any
  commit/push. Never push without explicit permission.
- Update [`../../knowledge/MEMORY.md`](../../knowledge/MEMORY.md) "Latest work" when the change lands.
