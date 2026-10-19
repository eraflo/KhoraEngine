# Documentation — reference

Domain knowledge for Khora documentation. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files).

## Scope
mdBook narrative (`docs/src/`), rustdoc, Mermaid diagrams, and keeping the AI docs (`.agent/`) and the
human docs aligned with the code.

## Key files
- mdBook: `docs/src/*.md` (chapters), `docs/src/SUMMARY.md`, theme `docs/theme/custom.css`.
- Build: `mdbook build docs/` / `mdbook serve docs/ --open`.
- AI docs: this `.agent/engine/` tree (canonical source for the provider wrappers).

## Rules
- **Document in existing files in place — no separate ADR section or ADR files** (project convention).
- Public APIs require rustdoc; `# Examples` compile via `cargo test --doc`. Use intra-doc links (`[Type]`).
- Change a public API → update the matching mdBook chapter in the same change.
- **Figures:** explain mechanisms with hand-made **SVG figures** — diagrams and illustrated examples —
  beside Mermaid (keep Mermaid for flowcharts). Files under `docs/src/images/<topic>/`, inlined with
  `{{#include ../images/<topic>/x.svg}}` inside `<div class="kp-figure-frame">` (scrolls on narrow
  screens). Classes prefixed `kp-`; colors `var(--k-*, #hex)` from `docs/theme/custom.css`; Geist /
  Geist Mono, Fraunces italic for station names; gold only for one highlighted thread; no blank lines
  inside the SVG (CommonMark ends the HTML block). Check text geometry with the real fonts. Reference
  set: `docs/src/images/persistence/`.
- **Animated figures (step-through):** a mechanism that unfolds over time — a load, a save, a merge, a
  patch reaching old data — gets a small animation the reader steps through (numbered steps,
  Previous / Next, arrow keys), one or two plain sentences per step under the picture, and beside it a
  deeper explanation: how it works, what it guarantees, its limit, sources. Explain with a concrete
  picture first and name the technical term after. SVG parts declare the steps they show at
  (`data-on="1 3"`) and their transform from a step on (`data-tf="1:none|2:translate(…)"`); a small
  shared script in `docs/theme/` drives them, `prefers-reduced-motion` respected. Reference
  implementation: the « Calques et tampons » explainer (claude.ai artifact `Pgk6ufp3BKnZGb6YJ9G3oi`).
  The full docs pass after Ergon puts SVG figures and these animations throughout the book.
- Keep each AI-doc file tight (progressive disclosure; aim < 500 lines) and **in English**.
- For the docs **visual language** (`docs/src/design/`), use `/impeccable`.
- Editing `.agent/<profile>/**` auto-triggers the installer's doc-change hook — edit the canonical source,
  never the generated wrappers (`CLAUDE.md`, `.cursor/`, …).

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) — `cargo test --doc` for doc examples + mdBook build.
- **`/impeccable`** — for the docs **visual language** (`docs/src/design/`): `audit` / `critique` / `polish`.

Verify mdBook builds and `cargo test --doc` passes.
