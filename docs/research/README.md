# Research artifacts

Compacted findings from the **Research** phase of the RPI workflow
(`.agent/engine/workflow-rpi.md`). Each file captures *how the codebase works today* for a given
question, so a plan can be built on accurate ground and a human can review the understanding before any
code is written.

## Convention

- **One file per research question**, named `AAAA-MM-JJ_topic.md` (ISO date + kebab-case topic),
  e.g. `2026-07-18_lanebus-view-publishing.md`.
- Produced by the [`research-codebase`](../../.agent/engine/skills/research-codebase/SKILL.md) skill,
  which distills the read-only research subagents' reports.
- **Not committed.** These are working scratch: they describe a moment, not the system. Once the
  work lands, the code and its comments are the truth, and a committed artefact becomes a second,
  stale description that a later reader — or agent — mistakes for current intent. Delete the file
  when the chantier it served is done.
- A finding that outlives the work belongs in the code's own comments, or in
  [`knowledge/`](../../.agent/engine/knowledge/MEMORY.md) — not in a file under this directory.
- Structure: Question · Answer/summary · Relevant files (`path:line`) · How it works · Constraints ·
  Open questions.

Plans that consume this research live in [`../plans/`](../plans/).
