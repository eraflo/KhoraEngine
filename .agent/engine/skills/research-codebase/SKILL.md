---
name: research-codebase
description: Phase 1 of the RPI workflow — understand a problem in the codebase before any code is written. Dispatches read-only research subagents in parallel and distills their findings into a compacted research artifact under docs/research/. Use at the start of any non-trivial change; it is the highest-leverage human review point.
---

# Research a codebase question

The **Research** phase of [`../../workflow-rpi.md`](../../workflow-rpi.md). Goal: produce a small,
accurate map of *how things work today* — not a solution. Keep the parent context clean by pushing the
noisy searching into subagents.

## Steps
1. **Recall first.** Dispatch `knowledge-locator` to surface prior research/plans/decisions in
   `docs/research/`, `docs/plans/`, and `knowledge/`. Don't re-research what's already written.
2. **Locate.** Dispatch `codebase-locator` to map *where* the relevant files/symbols live.
3. **Analyze + find precedent, in parallel.** Dispatch `codebase-analyzer` (how the mechanism works,
   `file:line`) and `codebase-pattern-finder` (existing example to mirror) together. Consult the matching
   `reference/*.md` domain doc for the hard rules.
4. **Distill.** Synthesize the subagent reports — never paste them raw — into one artifact.

## Output artifact
Write `docs/research/AAAA-MM-JJ_topic.md` (today's date, kebab topic) containing:
- **Question** — what we set out to understand.
- **Answer / summary** — 3-6 sentences.
- **Relevant files** — `path:line` list with one-line roles.
- **How it works** — the flow, each step cited `file:line`, named by CLAD layer.
- **Constraints** — the hard rules that bound any change (from `RULES.md` / `reference/`).
- **Prior art** — how other engines, formats, libraries or papers solved the problem, from web research,
  with sources.
- **Design proposals** — several options adapted to Khora (what each borrows, what is Khora-native,
  trade-offs) and a recommendation. A research doc that stops at findings is not finished.
- **Open questions** — what's still unknown or needs a human decision.

## Design review page
When the research proposes a design for the owner to validate, present it as a **published artifact
page**, not only the markdown file:
- One small SVG scene per idea, with **simple step-through animations**: numbered steps, Prev/Next and
  the arrow keys, one or two plain sentences per step.
- An everyday picture first (a box of pieces, a stamp, a transparent sheet); the technical words come
  after, in a glossary.
- Beside each scene, a deeper part: how it works, what it guarantees, the limit, prior art.
- Then the bugs found on the way, the phases, and the decisions to make, with the recommendation marked.
- In the owner's language (French). Khora identity: navy / silver, gold reserved for one thread (what the
  user changed by hand).
- Link the page from the research doc and in the report; the owner validates the design on the page.

## Rules
- Read-only. This phase writes **only** the research artifact — no source edits.
- Every non-obvious claim carries a `file:line`. No invented behavior.
- Stop at understanding. The plan comes next via [`../create-plan/SKILL.md`](../create-plan/SKILL.md).
- **Human review gate**: surface the artifact for review before planning — a wrong research doc is
  cheaper to fix here than after implementation. A design goes to review as the design review page above.
- A brief from another session that doesn't mention the page doesn't waive it.
