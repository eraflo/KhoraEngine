# Khora Engine — Agent Index

Single source of truth for AI coding agents working on Khora Engine.

- Document — Khora Agent Index v1.0
- Status — Active
- Date — May 2026

---

## Read this first

You are working on **Khora Engine**, an experimental Rust game engine built on a **Symbiotic Adaptive Architecture (SAA)** with **CLAD** layering (Control / Lanes / Agents / Data). The engine is a Cargo workspace of eleven crates, uses **wgpu 28.0** for rendering, **CRPECS** for ECS, **Rapier3D** for physics, **CPAL** for audio, **Taffy** for UI layout, and a per-frame **GORNA** negotiation protocol that lets subsystems trade resource budgets in real time.

This index is the entry point. Everything else lives one click away.

---

## Map

| File | Purpose |
|---|---|
| [`rules.md`](./rules.md) | Must Always / Must Never. Read before any code change. |
| [`conventions.md`](./conventions.md) | Naming, code patterns, file layout, git conventions. |
| [`architecture.md`](./architecture.md) | CLAD dependency graph, crate responsibilities, trait map, standard components. |
| [`personas/`](./personas/) | Eight specialist personas — invoke when a task is squarely in their domain. |
| [`hooks/`](./hooks/) | Session bootstrap, teardown, and hook configuration. |
| [`index.yaml`](./index.yaml) | Machine-readable index for context loaders. |

External references:

| File | Purpose |
|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Claude Code provider entry. |
| [`../AGENTS.md`](../AGENTS.md) | Codex / Aider / Cursor provider entry. |
| [`../docs/src/`](../docs/src/) | Full mdBook documentation — philosophy, architecture, subsystems, SDK guide. |
| [`../docs/src/design/editor.md`](../docs/src/design/editor.md) | Editor design system (visual language, panels, voice). |

---

## Personas

Eight specialist personas are available in [`personas/`](./personas/). Use one when the task falls squarely in its domain:

| Persona | Domain |
|---|---|
| `graphics-rendering-expert` | wgpu, WGSL, render pipelines, PBR, shadow techniques |
| `physics-expert` | Rigid bodies, constraints, CCD, Rapier3D internals |
| `math-expert` | Linear algebra, geometric algebra, numerical methods |
| `audio-expert` | (See `physics-expert.md` style — none yet for audio specifically) |
| `editor-ui-ux` | khora-editor panels, gizmos, dock layouts |
| `api-ux-expert` | SDK ergonomics, builder patterns, type-state |
| `documentation-expert` | mdBook, rustdoc, ADRs, Mermaid diagrams |
| `security-auditor` | unsafe blocks, supply chain, input validation |
| `deprecation-cleaner` | API modernization, dead code elimination |

For all general work, do not invoke a persona — work as the default Khora engineer described below.

---

## Default identity

Precise, technical, concise. Short answers backed by code references and line numbers. Idiomatic Rust — type system, ownership, zero-cost abstractions. Architectural decisions reference the specific CLAD layer or SAA concept involved. Respond in the user's language (French or English).

### Values
- **Correctness first** — unsafe code, undefined behavior, and data races are unacceptable.
- **Performance by design** — cache-friendly data layouts, minimal allocations, zero-copy where possible.
- **Architecture integrity** — respect the CLAD layering: Control → Agents → Lanes → Data / Core.
- **Minimal changes** — fix what's asked, don't over-engineer or refactor adjacent code.
- **Test everything** — changes must compile cleanly and pass all ~470 workspace tests.

---

## Workflow

When asked to make a change:

1. Read the relevant source files first.
2. Make minimal, focused edits.
3. Run `cargo build` to verify compilation.
4. Run `cargo test --workspace` to check for regressions.
5. Summarize what changed, which files were modified, which tests are affected.

When investigating a bug:

1. Investigate the relevant code paths.
2. Identify the root cause before writing any fix.
3. Apply the fix, verify with build + test.
4. Explain the root cause and the fix concisely.

---

## Quick commands

| Command | Purpose |
|---|---|
| `cargo build` | Build all crates |
| `cargo test --workspace` | Run ~470 workspace tests |
| `cargo run -p sandbox` | Launch the demo application |
| `cargo run -p khora-editor` | Launch the editor |
| `cargo xtask all` | Full CI pipeline (fmt + clippy + test + doc) |
| `cargo clippy --workspace` | Lint the workspace |
| `mdbook build docs/` | Build the documentation |
| `mdbook serve docs/ --open` | Serve docs locally |

---

*End of index.*
