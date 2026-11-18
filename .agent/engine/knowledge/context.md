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
- **SAA**: Symbiotic Adaptive Architecture — subsystems are families of lanes, each run by one `LaneAgent` negotiating its budget.
- **CLAD**: Control → Agent → Lane → Data (per-frame descent), plus a Substrate Pass.
- **GORNA**: Goal-Oriented Resource Negotiation & Allocation — a protocol: agents offer priced rows, the DCC lends each an envelope per epoch.
- **CRPECS**: archetype-based Column-Row Partitioned ECS with SoA storage + AGDF layout adaptation.
- **Rendering**: wgpu 29.0, WGSL shaders, PBR + shadow mapping (Unlit / LitForward / Forward+).

## Workspace — 17 members
14 `khora-*` workspace members under `crates/`: `khora-core`, `khora-data`, `khora-control`,
`khora-script`, `khora-lanes`, `khora-agents`, `khora-infra`, `khora-io`, `khora-telemetry`,
`khora-sdk`, `khora-tool-ui`, `khora-editor`, `khora-runtime`, `khora-hub` — plus
`examples/sandbox`, `xtask` and the generated `workspace-hack`. `khora-macros` is a fifteenth `khora-*` crate but a path
crate, not a member.

## Build commands
- `cargo gate` — what CI checks (fmt → clippy → nextest → doc tests); run once before a commit
- `cargo t -p <crate> [filter]` — targeted tests while iterating (nextest, `--all-features`)
- `cargo xtask lint-file <files…>` — the per-file convention checks of `xtask/tests/layout.rs` (in-file order, sizes, comments, crate boundaries) on given `.rs` files, no compile; prints `path:line: check: message`, honours `xtask/tests/layout_allow.txt`, exits 1 on a finding. Run by the Claude Code `PostToolUse` hook on every Rust file written and by the pre-commit hook on the staged ones (`.agent/engine/hooks/lint-file.md`)
- `cargo run -p sandbox` / `cargo run -p khora-editor` — run an app
- `cd docs/site && npm ci && npm run build` — the docs site (Astro Starlight over `docs/src`; mdBook is retired); `npm test` its tests (the book's reading, the old URLs' redirects, the landing); `npm run api` mounts `cargo doc --workspace --no-deps` at `/api`. CI builds it on pull requests and `dev`, and deploys it to GitHub Pages from `main` (`.github/workflows/docs.yml`)
- `node .agent/engine/installer/bin/khora-ai.mjs sweep [--days N] [--max-gb G] [--dry-run] [--force]` — prune `target/`: incremental caches unused for N days (3) and side dirs (miri, a nightly clippy, scratch targets) unused as long; then, when the `debug`/`release` profiles together exceed G GiB (20; `doc/` never counts), every incremental cache first, and the profiles whole only if still over. Runs at the start of every Claude Code session (SessionStart hook), at most once per 12 h (`target/.khora-sweep-stamp`; `--force` ignores it). A full clean can fail a build running at that moment — rerun it. Tests: `node --test ".agent/engine/installer/lib/*.test.mjs"`.
