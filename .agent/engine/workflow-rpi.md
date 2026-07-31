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
Research ──► Plan ──► Implement ──► (verify) ──► compact back into the plan
   │           │           │
 review #1   review #2    per-phase check
 (highest leverage)
```

1. **Research** — [`skills/research-codebase`](./skills/research-codebase/SKILL.md). Dispatch the read-only
   research subagents (`knowledge-locator`, `codebase-locator`, `codebase-analyzer`,
   `codebase-pattern-finder`) to understand *how it works today*. Output → `docs/research/AAAA-MM-JJ_topic.md`.
   **Human reviews the research** (cheapest place to catch a wrong mental model).
2. **Plan** — [`skills/create-plan`](./skills/create-plan/SKILL.md). Turn the research into a precise,
   phase-by-phase plan. Output → `docs/plans/descriptive-name.md`. **Human reviews the plan.**
3. **Implement** — [`skills/implement-plan`](./skills/implement-plan/SKILL.md). Execute one phase, verify
   (`cargo test --workspace`), then **compact status back into the plan file**, and repeat.

## Context discipline (the whole point)

- **Subagents are for context control, not role-play.** They run the noisy searches in a separate window
  and return a distilled `file:line` summary. They never edit — the main agent implements.
- **Domain knowledge lives in [`reference/`](./reference/)**, loaded on demand — not in a fleet of
  "expert" agents.
- **Compact deliberately.** When a phase is verified, distill its outcome into the plan's Status section
  and drop the working detail. Don't carry raw build logs or full-file reads forward.
- **Aim to stay well under ~50% context.** If it climbs, stop and compact.

## Artifacts

- `docs/research/AAAA-MM-JJ_topic.md` — research findings (committed).
- `docs/plans/descriptive-name.md` — implementation plans + live status (committed).

Both are versioned in git so they show up in review and history. `knowledge/MEMORY.md` tracks the
current state and points at the active plan docs.

## When to skip

Trivial, mechanical, single-file edits don't need the full loop — just do them and verify. Everything
that spans subsystems, touches CLAD boundaries, or is hard to hold in one head goes through RPI.
