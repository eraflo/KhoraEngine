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
- **Rendering**: wgpu 28.0, WGSL shaders, PBR + shadow mapping (LitForward / Forward+ / StandardPbr).

## Workspace — 17 members
14 `khora-*` workspace members under `crates/`: `khora-core`, `khora-data`, `khora-control`,
`khora-script`, `khora-lanes`, `khora-agents`, `khora-infra`, `khora-io`, `khora-telemetry`,
`khora-plugins`, `khora-sdk`, `khora-tool-ui`, `khora-editor`, `khora-runtime` — plus
`examples/sandbox`, `xtask` and `hub`. `khora-macros` is a fifteenth `khora-*` crate but a path
crate, not a member.

> Drift note: older docs said "11" or "12 crates" and both omitted `khora-runtime`. The number is **16**.

## Build commands
- `cargo build` — full workspace
- `cargo test --workspace` — all tests (primary check)
- `cargo run -p sandbox` — demo app
- `cargo run -p khora-editor` — editor
- `cargo xtask all` — CI pipeline (fmt + clippy + test + doc)
- `mdbook build docs/` — documentation
