# Documentation — reference

Domain knowledge for Khora documentation. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files).

## Scope
mdBook narrative (`docs/src/`), rustdoc, Mermaid diagrams, and keeping the AI docs (`.agent/`) and the
human docs aligned with the code.

## Key files
- mdBook: `docs/src/*.md` (chapters), `docs/src/SUMMARY.md`, theme `docs/theme/custom.css` (book),
  `figures.css` + `steps.js` (figures and step-throughs), `home.css` (landing page); Mermaid through
  `docs/mermaid-init.js` (CI's `mermaid.min.js`, else a pinned CDN build).
- Build: `mdbook build docs/` / `mdbook serve docs/ --open`.
- Landing page: sources in `docs/landing/` (`page.html` with `{root}`/`{img}`, `landing.css`, `landing.js`,
  rendered frames under `images/<scene>/`). `cargo xtask landing docs/book` writes it as the site's
  `index.html` after `mdbook build`; the book's own first page stays `home.html`. `docs.yml` runs both.
- AI docs: this `.agent/engine/` tree (canonical source for the provider wrappers).

## Rules
- **Document in existing files in place — no separate ADR section or ADR files** (project convention).
- Public APIs require rustdoc; `# Examples` compile via `cargo test --doc`. Use intra-doc links (`[Type]`).
- Change a public API → update the matching mdBook chapter in the same change.
- **Every reader can enter every concept** (owner, 2026-10-19). A page opens with the idea in everyday
  words — a `<p class="kp-lede">`, no jargon, no label above it — then goes down to how it works, what it
  guarantees, its limit, its sources. Decide concept by concept who arrives there knowing what: someone
  who never touched a computer, a game maker from another engine, an engine contributor. A new term is
  defined in a sentence where it first appears.
- **Figures:** explain mechanisms with hand-made **SVG figures** — diagrams and illustrated examples —
  beside Mermaid (keep Mermaid for flowcharts). Files under `docs/src/images/<topic>/`, inlined with
  `{{#include ../images/<topic>/x.svg}}` inside `<div class="kp-figure-frame">` (scrolls on narrow
  screens). The root `<svg>` has `id="fig-<topic>-<name>"`, `role="img"` and an `aria-labelledby`
  title, and **every rule of its `<style>` is scoped to that id** (`#fig-scripting-turn .kp-label`):
  inlined, an unscoped rule restyles every other figure on the page. Classes prefixed `kp-`; colors
  `var(--k-*, #hex)`; Archivo / IBM Plex Mono, Archivo italic for station names; gold only for one
  highlighted thread; no blank lines inside the SVG (CommonMark ends the HTML block). The
  `docs_figures` test in `xtask` (run by `cargo gate`) refuses a figure that breaks any of these.
- **Step-through figures:** a mechanism that unfolds over time gets an animation the reader steps
  through — `<figure class="kp-steps">` holding the figure and an `<ol class="kp-steps-text">`, one plain
  caption per step. SVG parts declare the steps they show at (`data-on="1 3"`) and their transform from a
  step on (`data-tf="2:translate(40px, 0)|4:none"`). `docs/theme/steps.js` adds Back / Next, numbered
  dots and the arrow keys; `data-autoplay="4000"` plays it once on screen, with Pause, never under
  reduced motion. Explain with a concrete picture first and name the technical term after. Examples:
  `docs/src/images/scripting/flipbook.svg`, `docs/src/images/home/negotiated-frame.svg`.
- Keep each AI-doc file tight (progressive disclosure; aim < 500 lines) and **in English**.
- For the docs **visual language** (`docs/src/design/`), use `/impeccable`.
- Editing `.agent/<profile>/**` auto-triggers the installer's doc-change hook — edit the canonical source,
  never the generated wrappers (`CLAUDE.md`, `.cursor/`, …).

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) — `cargo test --doc` for doc examples + mdBook build.
- **`/impeccable`** — for the docs **visual language** (`docs/src/design/`): `audit` / `critique` / `polish`.

Verify mdBook builds and `cargo test --doc` passes.
