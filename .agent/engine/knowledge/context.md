# Knowledge — Project Context

Stable facts about the project. Update only when these change.

## Project
- **Name**: Khora Engine
- **Language**: Rust (edition 2024)
- **Type**: Experimental game engine (SAA / CLAD)
- **License**: Apache-2.0
- **Repository**: https://github.com/eraflo/KhoraEngine
- **Branches**: `dev` (active development), `main` (stable)

## Architecture (one-liners)
- **SAA**: Symbiotic Adaptive Architecture — subsystems are intelligent agents negotiating budgets.
- **CLAD**: Control → Agent → Lane → Data (per-frame descent), plus a Substrate Pass.
- **GORNA**: per-frame Goal-Oriented Resource Negotiation & Allocation between agents.
- **CRPECS**: archetype-based Column-Row Partitioned ECS with SoA storage + AGDF layout adaptation.
- **Rendering**: wgpu 29.0, WGSL shaders, PBR + shadow mapping (LitForward / Forward+ / StandardPbr).

## Workspace — 16 members
13 `khora-*` workspace members under `crates/`: `khora-core`, `khora-data`, `khora-control`,
`khora-script`, `khora-lanes`, `khora-agents`, `khora-infra`, `khora-io`, `khora-telemetry`,
`khora-sdk`, `khora-tool-ui`, `khora-editor`, `khora-runtime` — plus
`examples/sandbox`, `xtask` and `hub`. `khora-macros` is a fourteenth `khora-*` crate but a path
crate, not a member.

## Build commands
- `cargo build` — full workspace
- `cargo test --workspace` — all tests (primary check)
- `cargo run -p sandbox` — demo app
- `cargo run -p khora-editor` — editor
- `cargo xtask all` — CI pipeline (fmt + clippy + test + doc)
- `mdbook build docs/` — documentation
- `node .agent/engine/installer/bin/khora-ai.mjs sweep [--days N] [--max-gb G] [--dry-run] [--force]` — prune `target/`: incremental caches unused for N days (3), and the `debug`/`release` profiles when together they exceed G GiB (20; `doc/` never counts). Runs at the start of every Claude Code session (SessionStart hook), at most once per 12 h (`target/.khora-sweep-stamp`; `--force` ignores it). A full clean can fail a build running at that moment — rerun it. Tests: `node --test ".agent/engine/installer/lib/*.test.mjs"`.
