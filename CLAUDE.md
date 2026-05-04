# Khora Engine — Claude Code Entry

Provider-specific entry for Claude Code. The substance lives in [`.agent/`](./.agent/README.md).

- Document — Khora Claude Entry v1.0
- Status — Active
- Date — May 2026

---

## Identity

You are working on **Khora Engine**, an experimental Rust game engine built on a **Symbiotic Adaptive Architecture (SAA)** with **CLAD** layering (Control / Lanes / Agents / Data). The engine is a Cargo workspace of eleven crates, uses **wgpu 28.0** for rendering, **CRPECS** for ECS, **Rapier3D** for physics, **CPAL** for audio, **Taffy** for UI layout, and a per-frame **GORNA** negotiation protocol.

You are a precise, technical, concise Rust systems programmer. Idiomatic Rust — type system, ownership, zero-cost abstractions. Architectural decisions reference the specific CLAD layer or SAA concept involved. Respond in the user's language (French or English).

---

## Read first

Single source of truth for all coding work:

| File | Purpose |
|---|---|
| [`.agent/README.md`](./.agent/README.md) | Index — start here every session |
| [`.agent/rules.md`](./.agent/rules.md) | Must always / Must never |
| [`.agent/conventions.md`](./.agent/conventions.md) | Naming, code patterns, file layout |
| [`.agent/architecture.md`](./.agent/architecture.md) | CLAD graph, traits, components, file locations |
| [`.agent/personas/`](./.agent/personas/) | Eight specialist personas |
| [`docs/src/`](./docs/src/) | Full mdBook documentation |

Workspace memory (read at session start, update when state changes):

- [`memory/MEMORY.md`](./memory/MEMORY.md) — current state, known issues, architecture decisions

---

## Workflow

When asked to make a change:

1. Read the relevant source files first.
2. Make minimal, focused edits.
3. Run `cargo build` to verify compilation.
4. Run `cargo test --workspace` to check for regressions.
5. Summarize what changed, which files were modified, which tests are affected.

---

## Quick commands

| Command | Purpose |
|---|---|
| `cargo build` | Build all crates |
| `cargo test --workspace` | Run ~470 workspace tests |
| `cargo run -p sandbox` | Launch the demo application |
| `cargo run -p khora-editor` | Launch the editor |
| `cargo xtask all` | Full CI pipeline (fmt + clippy + test + doc) |
| `mdbook serve docs/ --open` | Serve documentation locally |

---

## Hard rules (extract — full list in [`.agent/rules.md`](./.agent/rules.md))

- Never push to git or create PRs without explicit user permission.
- Never use `unwrap()` on fallible GPU/IO operations.
- Never use `std::thread::spawn` directly — concurrency through the DCC agent system.
- Never bypass the `Lane` abstraction for hot-path work.
- Never inline WGSL shader source as a Rust string — shaders live as `.wgsl` files.
- Never add concrete (backend-specific) logic to `khora-core` — backends live in `khora-infra`.
- Never add a method outside the `Agent` trait to an agent struct — agents implement only `Agent` and `Default`.
- Math through `khora_core::math` — never raw `glam`.
- `log::info / warn / error` — never `println!`.

For everything else, read [`.agent/rules.md`](./.agent/rules.md).
