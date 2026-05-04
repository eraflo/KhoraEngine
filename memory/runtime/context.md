# Context

## Project
- **Name**: Khora Engine
- **Language**: Rust (edition 2024)
- **Type**: Experimental game engine
- **License**: Apache-2.0
- **Repository**: https://github.com/eraflo/KhoraEngine
- **Branch**: `dev` (active development), `main` (stable)

## Architecture
- **SAA**: Symbiotic Adaptive Architecture — subsystems as intelligent negotiating agents
- **CLAD**: Control → Lanes → Agents → Data layering
- **ECS**: CRPECS (Column-Row Partitioned Entity Component System)
- **Rendering**: wgpu 28.0 backend, WGSL shaders, PBR with shadow mapping

## Workspace (11 crates)
- `khora-core` — trait definitions, math, interfaces
- `khora-macros` — proc macros (#[derive(Component)])
- `khora-data` — ECS, allocators, resources
- `khora-control` — DCC, GORNA protocol
- `khora-telemetry` — metrics, monitoring
- `khora-lanes` — hot-path pipelines
- `khora-infra` — wgpu backend, platform
- `khora-agents` — RenderAgent, UiAgent, etc.
- `khora-plugins` — plugin system
- `khora-sdk` — public SDK for game developers
- `khora-editor` — future editor (stub)

## Build Commands
- `cargo build` — full workspace
- `cargo test --workspace` — all tests
- `cargo run -p sandbox` — demo app
- `cargo xtask all` — CI pipeline
- `mdbook build docs/` — documentation
