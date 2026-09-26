# RPI workflow — Research → Plan → Implement

The default loop for any non-trivial change on Khora, adapted from HumanLayer's *Advanced Context
Engineering* (ACE-FCA). It exists to keep the working context small and accurate, and to move human
review to where it has the most leverage.

## Why

LLMs degrade as the context window fills (the "dumb zone", roughly past ~40-60% utilization). So we
**offload noisy work to subagents** and **compact progress into artifact files** instead of holding
everything in one context. Reviewing a 200-line plan beats reviewing a 2000-line diff.

## The loop

```
Research ──► Plan ──► Implement ──► compact back into the plan
   │           │           │
 review #1   review #2    tests first ─► implement ─► try to break it ─► fix ─► gate
 (highest leverage)
```

1. **Research** — [`skills/research-codebase`](./skills/research-codebase/SKILL.md). Dispatch the read-only
   research subagents (`knowledge-locator`, `codebase-locator`, `codebase-analyzer`,
   `codebase-pattern-finder`) to understand *how it works today*. Output → `docs/research/AAAA-MM-JJ_topic.md`.
   **Human reviews the research** (cheapest place to catch a wrong mental model).
2. **Plan** — [`skills/create-plan`](./skills/create-plan/SKILL.md). Turn the research into a precise,
   phase-by-phase plan. Output → `docs/plans/descriptive-name.md`. **Human reviews the plan.**
3. **Implement** — [`skills/implement-plan`](./skills/implement-plan/SKILL.md), per phase:
   1. **Tests first** — the [`test-writer`](./agents/test-writer.md) subagent writes the phase's tests from
      the plan alone, in a fresh context, and shows each one failing for the right reason.
   2. **Implement** — the main agent, until those tests pass. It **never edits or deletes** them; a test
      it believes wrong is raised with the human, not rewritten.
   3. **Try to break it** — the [`test-breaker`](./agents/test-breaker.md) subagent reads the plan and the
      diff in a fresh context and writes a failing test for each real defect or missing case.
   4. **Fix** — the main agent, until the breaker's tests pass. At most two breaker rounds; what remains
      is reported to the human, never dropped.
   5. **Gate** (`cargo test --workspace`, see [`skills/build-and-test`](./skills/build-and-test/SKILL.md)),
      then **compact status back into the plan file**, and move to the next phase.

## Context discipline (the whole point)

- **Subagents are for context control, not role-play.** The research subagents run the noisy searches in
  a separate window and return a distilled `file:line` summary; they never edit. The two verification
  subagents, `test-writer` and `test-breaker`, write **tests only** — never implementation. The main
  agent implements.
- **Why tests come from a separate context.** Tests written by the implementer test the implementation it
  had in mind; tests written from the plan test the plan. A fresh reviewer is not biased toward code it
  just wrote. And an agent under pressure to go green will weaken or delete a test — which is why the
  implementer may not touch the tests it was given.
- **Domain knowledge lives in [`reference/`](./reference/)**, loaded on demand — not in a fleet of
  "expert" agents.
- **Compact deliberately.** When a phase is verified, distill its outcome into the plan's Status section
  and drop the working detail. Don't carry raw build logs or full-file reads forward.
- **Aim to stay well under ~50% context.** If it climbs, stop and compact.

## Artifacts

- `docs/research/AAAA-MM-JJ_topic.md` — research findings (**scratch, never committed**).
- `docs/plans/descriptive-name.md` — implementation plan + live status (**scratch, never committed**).

Both are working files: they describe a moment, not the system. Delete them once the work lands —
a kept artefact becomes a second, stale description of the code that a later reader takes for
current intent. Anything durable belongs in the code's own comments or in
[`knowledge/`](./knowledge/MEMORY.md).

`knowledge/MEMORY.md` tracks the current state and points at whichever plan is still live.

## When to skip

Trivial, mechanical, single-file edits don't need the full loop — just do them and verify. Everything
that spans subsystems, touches CLAD boundaries, or is hard to hold in one head goes through RPI.
