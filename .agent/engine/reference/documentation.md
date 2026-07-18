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
- Keep each AI-doc file tight (progressive disclosure; aim < 500 lines) and **in English**.
- For the docs **visual language** (`docs/src/design/`), use `/impeccable`.
- Editing `.agent/<profile>/**` auto-triggers the installer's doc-change hook — edit the canonical source,
  never the generated wrappers (`CLAUDE.md`, `.cursor/`, …).

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) — `cargo test --doc` for doc examples + mdBook build.
- **`/impeccable`** — for the docs **visual language** (`docs/src/design/`): `audit` / `critique` / `polish`.

Verify mdBook builds and `cargo test --doc` passes.
