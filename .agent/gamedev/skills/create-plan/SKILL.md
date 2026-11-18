---
name: create-plan
description: Phase 2 of the RPI workflow — turn a research artifact into a precise, phase-by-phase implementation plan under docs/plans/. The plan is the source of truth the implementation follows and the second (high-leverage) review point. Use after research-codebase, before writing game code.
---

# Create an implementation plan

The **Plan** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Goal: a plan precise enough that
implementation is mechanical and a reviewer can catch mistakes by reading ~200 lines instead of a large diff.

## Steps
1. **Consume the research** in `docs/research/AAAA-MM-JJ_topic.md`. If a fact is missing, dispatch a
   research subagent — don't guess.
2. **Design the change** against the rules ([`../../RULES.md`](../../RULES.md), the relevant
   `reference/*.md`, [`../../sdk-guide.md`](../../sdk-guide.md)): gameplay in Ergon, Rust on the SDK. Reuse
   the shapes the research surfaced.
3. **Break into verifiable phases** — each independently buildable and runnable.
4. **Write the plan artifact** and surface it for review.

## Output artifact
Write `docs/plans/descriptive-name.md`:
- **Context** — why; the problem and intended outcome (link the research artifact).
- **Phases** — ordered; for each: exact files to edit, the specific edits, and the **verification**
  (Play in the editor, or `cargo build` + `cargo run` — behavior correct, Problems empty).
- **Out of scope** — what this deliberately doesn't do.
- **Status** — a checklist the implementation phase updates in place.

## Rules
- Writes **only** the plan artifact — no source edits yet.
- Prefer the matching skill (`start-a-game`, `write-a-script`, `spawn-entity`, `setup-input`, `load-scene`,
  `add-a-family`) over inventing a shape.
- **Minimal changes.** **Human review gate** before implementing. Implement via
  [`../implement-plan/SKILL.md`](../implement-plan/SKILL.md).
