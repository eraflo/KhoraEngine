# Documentation — reference

Domain knowledge for Khora documentation. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files).

## Scope
The book (`docs/src/`) and the docs site that builds it (`docs/site/`, Astro Starlight; mdBook is
retired), rustdoc, the docs workflow (`.github/workflows/docs.yml`), SVG figures, and keeping the AI
docs (`.agent/`) and the human docs aligned with the code.

## Key files
- The pages: `docs/src/**/*.md`, in mdBook's format (no frontmatter, `# Title` on line 1, figures by
  `{{#include}}`, links to `x.md` or `x.html`). `docs/src/SUMMARY.md` is the one navigation: the site
  builds its sidebar from it, and every page must be listed there once (xtask
  `docs_pages_are_in_the_summary`, `docs_site_nav_is_the_summary`).
- Docs site, `docs/site/` (Node is confined to it; no JS framework; fonts self-hosted by `@fontsource`):
  - `astro.config.mjs` — Starlight config: sidebar parsed from SUMMARY.md, theme, figure CSS/JS, the
    links validator (`starlight-links-validator`, fails the build on a broken internal link or anchor),
    the edit link (a page's `filePath` is `../src/x.md`, so "Edit page" opens `docs/src/x.md`).
  - `src/book/` — reads the book where it is, in memory, writing nothing back: `loader.mjs` (the
    `docs` collection), `page.mjs` (expands `{{#include}}`, takes the `# Title` as the page title,
    rewrites `x.md`/`x.html` links to routes and other `docs/src` files to their URL, drops
    `rust,ignore`-style fence attributes), `summary.mjs` (SUMMARY.md → sidebar), `assets.mjs` (copies
    the non-Markdown files of `docs/src` into the generated `public/`), `site.mjs` (site URL and base),
    `redirects.mjs` (below).
  - `src/styles/khora.css` — the Khora theme over Starlight (navy and silver; gold only for the frame's
    limit). A page with figures narrows the contents panel from 72rem so its column holds a 680 px
    figure at 1280 px; a desktop column of 560–680 px draws the figure to its width; a phone keeps
    680 px and scrolls the figure sideways.
  - `src/styles/figures.css` (figure frames, the step-through bar) and `src/scripts/steps.js` (the
    step-through script, imported by `src/components/Head.astro`).
  - Old URLs: mdBook served a page `x.md` at `x.html`. `src/book/redirects.mjs` writes, after the build,
    a redirect page at every such path (keeping the `#anchor`), computed from SUMMARY.md, plus mdBook's
    own two (`BOOK_REDIRECTS`: a page's former name, a section's former page); a page `x/index.md` is served at its old
    path already. `redirects.test.mjs` pins that every page has its old URL redirected.
  - `src/pages/index.astro` — the site's index: the landing page (see Landing page below).
  - `scripts/mount-api.mjs` — copies `target/doc` to `dist/api/` with an index of the crates.
- Build the site (Node ≥ 22.23.4), from `docs/site/`:
  - `npm ci` — install exactly the lockfile (never `npm install` to build; add a dependency only pinned
    exactly, after `npm audit` and a `security-auditor` review);
  - `npm run dev` — serve with live reload (a change under `docs/src` reloads the book);
  - `npm run build` — build `dist/`, validate every internal link, index the search;
  - `npm test` — the tests of `src/book/` and `src/landing/` (Node's test runner);
  - `npm run frames` — fetch the landing's frames (below) into `frames/`; without them the build shows
    posters. `-- --if-released` (CI) lets a release not published yet (404) through, nothing else;
  - rustdoc at `/api`: `cargo doc --workspace --no-deps` at the root, then `npm run build && npm run api`;
  - `npm run preview` — serve `dist/` (with `/api`) at `http://localhost:4321/KhoraEngine/`;
  - Astro reports usage to its makers unless told not to: set `ASTRO_TELEMETRY_DISABLED=1` (CI does), or
    run `npx astro telemetry disable` once.
- CI (`.github/workflows/docs.yml`): on a pull request and a push to `dev`, build and test the site
  (`npm ci --ignore-scripts`, `npm test`, `npm run frames -- --if-released`, `cargo doc --workspace
  --no-deps`, `npm run build`, `npm run api`); on a push to `main` (or by hand on `main`), the same,
  then deploy `docs/site/dist` to GitHub Pages (`actions/upload-pages-artifact`,
  `actions/deploy-pages`). `rust.yml` runs `cargo doc … --document-private-items` with
  `RUSTDOCFLAGS=-D warnings`, as `cargo gate` does. The repository's Pages source must be "GitHub
  Actions".
- Landing page — the site's index, one tagline: "Every frame, negotiated.":
  - `src/pages/index.astro` (a Starlight splash page whose `hero` only hides the page title:
    `src/components/Hero.astro` draws nothing), `src/styles/landing.css`, `src/landing/film.js`
    (vanilla: the scroll-scrubbed opening, the reels, the illustrated clock). Its sections are
    `not-content`, so `khora.css`'s prose rules skip them.
  - The film's frames are **not in git**. 138 AVIF stills (4 scenes, `FRAMES` in
    `src/landing/frames.mjs`: counts, format, version, release, SHA-256) travel as one tar archive,
    an asset of the GitHub release `landing-frames-v<N>`. `npm run frames` downloads it, checks its
    SHA-256 and every entry name (`isFrameEntry`), and unpacks it into `docs/site/frames/` (ignored);
    `--from <file>` reads a local archive. The build then copies them to `<base>/landing/frames/`
    (`src/landing/integration.mjs`), or the page loads them from `KHORA_FRAMES_URL` (an `https://` URL
    or a root path) when set. With neither, the page is built with its posters only.
  - The posters — the only stills in git — are `src/assets/landing/*.avif`; Astro's `<Picture>` makes
    their AVIF and WebP sizes. Under reduced motion, Save-Data, or when the first frame does not load,
    the page keeps the poster and asks for nothing more. Frames load after the page has loaded and the
    browser is idle (the opening), or when a reel nears the screen.
  - Re-rendered scenes: `npm run frames:encode -- <renders> --posters` (renders: `<scene>/frame_NNN.jpg`)
    writes `frames/`, the posters, and `release/khora-landing-frames-v<N>.tar` (ignored), and prints
    its SHA-256. Bump `FRAMES.version` and the release tag, set `FRAMES.release.sha256`, then publish
    the archive as that release's asset. The same frames always make the same archive.
- AI docs: this `.agent/engine/` tree (canonical source for the provider wrappers).

## Rules
- **Document in existing files in place — no separate ADR section or ADR files** (project convention).
- Public APIs require rustdoc; `# Examples` compile via `cargo test --doc`. Use intra-doc links (`[Type]`).
- Change a public API → update the matching book page in the same change.
- **Every reader can enter every concept** (owner, 2026-10-19). A page opens with the idea in everyday
  words — a `<p class="kp-lede">`, no jargon, no label above it — then goes down to how it works, what it
  guarantees, its limit, its sources. Decide concept by concept who arrives there knowing what: someone
  who never touched a computer, a game maker from another engine, an engine contributor. A new term is
  defined in a sentence where it first appears.
- **Figures:** explain mechanisms with hand-made **SVG figures** — diagrams and illustrated examples —
  — never Mermaid: the site does not draw it. Files under `docs/src/images/<topic>/`, inlined with
  `{{#include ../images/<topic>/x.svg}}` inside `<div class="kp-figure-frame">` (scrolls on narrow
  screens). The root `<svg>` has `id="fig-<topic>-<name>"`, `role="img"` and an `aria-labelledby`
  naming its `<title>`, and **every selector of its `<style>` starts with that id**
  (`#fig-scripting-turn .kp-label`, inside `@media` too; no at-rule but `@media` and `@keyframes`):
  inlined, an unscoped rule restyles every other figure on the page. Classes prefixed `kp-`; no blank
  line inside the SVG (CommonMark ends the HTML block); no id shared by two figures. The
  `docs_figures` test in `xtask/tests/layout.rs` (run by `cargo gate`; logic in
  `xtask/src/lint/figures.rs`) refuses, with no allowlist, every `.svg` under `docs/src/images/` that
  breaks one of these. Not checked, kept by hand: colors `var(--k-*, #hex)`; Archivo / IBM Plex Mono,
  Archivo italic for station names; gold only for one highlighted thread.
- **Step-through figures:** a mechanism that unfolds over time gets an animation the reader steps
  through — `<figure class="kp-steps">` holding the figure and an `<ol class="kp-steps-text">`, one plain
  caption per step. SVG parts declare the steps they show at (`data-on="1 3"`) and their transform from a
  step on (`data-tf="2:translate(40px, 0)|4:none"`). `docs/site/src/scripts/steps.js` adds Back / Next, numbered
  dots and the arrow keys; `data-autoplay="4000"` plays it once on screen, with Pause, never under
  reduced motion. Explain with a concrete picture first and name the technical term after. Examples:
  `docs/src/images/scripting/flipbook.svg`, `docs/src/images/home/negotiated-frame.svg`.
- Keep each AI-doc file tight (progressive disclosure; aim < 500 lines) and **in English**.
- For the docs **visual language** (`docs/src/design/`), use `/impeccable`.
- Editing `.agent/<profile>/**` auto-triggers the installer's doc-change hook — edit the canonical source,
  never the generated wrappers (`CLAUDE.md`, `.cursor/`, …).

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) — `cargo test --doc` for doc examples; the site build above.
- **`/impeccable`** — for the docs **visual language** (`docs/src/design/`): `audit` / `critique` / `polish`.

Verify the site's tests pass and it builds (`npm test`, `npm run build`, links valid), and `cargo test --doc` passes.
