---
name: research-codebase
description: Phase 1 of the RPI workflow — understand a part of your game (or the SDK surface it uses) before writing code. Dispatches read-only research subagents in parallel and distills their findings into a compacted research artifact under docs/research/. Use at the start of any non-trivial change; it is the highest-leverage review point.
---

# Research a codebase question

The **Research** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Goal: a small, accurate map
of *how things work today* — not a solution. Push noisy searching into subagents to keep context clean.

## Steps
1. **Recall first.** Dispatch `knowledge-locator` for prior research/plans/notes in `docs/research/`,
   `docs/plans/`, and `knowledge/`.
2. **Locate.** Dispatch `codebase-locator` to map *where* the relevant game files / SDK calls live.
3. **Analyze + find precedent, in parallel.** Dispatch `codebase-analyzer` (how it works, `file:line`)
   and `codebase-pattern-finder` (an example to mirror — often the `sandbox`). Consult the matching
   `reference/*.md` (`gameplay`, `scene-design`) and [`../../sdk-guide.md`](../../sdk-guide.md).
4. **Distill.** Synthesize the subagent reports — never paste them raw — into one artifact.

## Output artifact
Write `docs/research/AAAA-MM-JJ_topic.md` (today's date, kebab topic):
- **Question** · **Answer/summary** (3-6 sentences) · **Relevant files** (`path:line`) ·
  **How it works** (flow, cited) · **SDK contracts** it relies on · **Open questions**.

## Rules
- Read-only. Writes **only** the research artifact — no source edits.
- Every non-obvious claim carries a `file:line`. SDK-only — never reference internal `khora-*` crates.
- **Human review gate**: surface the artifact before planning. Plan next via
  [`../create-plan/SKILL.md`](../create-plan/SKILL.md).
