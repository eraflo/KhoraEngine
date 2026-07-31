# RPI workflow — Research → Plan → Implement

The default loop for any non-trivial change to your game, adapted from HumanLayer's *Advanced Context
Engineering* (ACE-FCA). It keeps the working context small and accurate, and moves human review to
where it has the most leverage.

## Why

LLMs degrade as the context window fills (the "dumb zone", roughly past ~40-60%). So we **offload noisy
work to subagents** and **compact progress into artifact files** instead of holding everything in one
context. Reviewing a 200-line plan beats reviewing a large diff.

## The loop

```
Research ──► Plan ──► Implement ──► (verify) ──► compact back into the plan
   │           │           │
 review #1   review #2    per-phase check
 (highest leverage)
```

1. **Research** — [`skills/research-codebase`](./skills/research-codebase/SKILL.md). Dispatch the
   read-only research subagents (`knowledge-locator`, `codebase-locator`, `codebase-analyzer`,
   `codebase-pattern-finder`) to understand *how it works today*. Output → `docs/research/AAAA-MM-JJ_topic.md`.
   **Human reviews the research.**
2. **Plan** — [`skills/create-plan`](./skills/create-plan/SKILL.md). Turn research into a phase-by-phase
   plan. Output → `docs/plans/descriptive-name.md`. **Human reviews the plan.**
3. **Implement** — [`skills/implement-plan`](./skills/implement-plan/SKILL.md). Execute one phase, verify
   (`cargo build` + `cargo run`), then **compact status back into the plan file**, and repeat.

## Context discipline

- **Subagents are for context control, not role-play.** They run noisy searches in a separate window and
  return a distilled `file:line` summary. They never edit — the main agent implements.
- **Domain knowledge lives in [`reference/`](./reference/)** (`gameplay`, `scene-design`) plus
  [`sdk-guide.md`](./sdk-guide.md), loaded on demand.
- **Compact deliberately.** When a phase is verified, distill its outcome into the plan's Status section
  and drop the working detail.
- **Stay well under ~50% context.** If it climbs, stop and compact.

## Artifacts

- `docs/research/AAAA-MM-JJ_topic.md` — research findings (committed).
- `docs/plans/descriptive-name.md` — implementation plans + live status (committed).

## When to skip

Trivial, mechanical, single-file edits don't need the full loop — just do them and verify with
`cargo run`. Everything larger goes through RPI.
