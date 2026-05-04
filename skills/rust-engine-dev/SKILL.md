---
name: rust-engine-dev
description: "General Rust game engine development — build systems, workspace management, dependency resolution, proc macros, unsafe auditing, and performance profiling. Use for any Rust compilation, testing, or workspace-level task."
license: Apache-2.0
allowed-tools: cargo-build cargo-test cargo-clippy
metadata:
  author: eraflo
  version: "1.0.0"
  category: engine-development
---

# Rust Engine Development

## Instructions

When working on Rust engine code:

1. **Workspace structure** — KhoraEngine is a Cargo workspace with 11 crates under `crates/` plus `examples/sandbox` and `xtask`. The root `Cargo.toml` lists all members.

2. **Build commands**:
   - `cargo build` — full workspace build (dev profile with opt-level=1)
   - `cargo test --workspace` — run all ~470 tests
   - `cargo nextest run --workspace` — preferred test runner
   - `cargo clippy --workspace` — lint check
   - `cargo xtask all` — full CI: fmt + clippy + test + doc

3. **Crate dependency order** (bottom to top):
   - `khora-core` — trait definitions, math, platform abstractions
   - `khora-macros` — procedural macros (Component derive)
   - `khora-data` — ECS (CRPECS), allocators, resources
   - `khora-control` — DCC service, GORNA protocol
   - `khora-telemetry` — metrics, monitoring
   - `khora-lanes` — hot-path pipelines (render, physics, audio, asset)
   - `khora-infra` — wgpu backend, platform implementations
   - `khora-agents` — RenderAgent, AudioAgent, etc.
   - `khora-plugins` — plugin system
   - `khora-sdk` — public API for game developers
   - `khora-editor` — future editor (stub)

4. **Rust edition**: 2024 (set in `rust-toolchain.toml`)

5. **Key patterns**:
   - `Arc<dyn Trait>` for cross-crate shared resources
   - `Mutex<T>` for interior mutability of shared state
   - `Result<T, SpecificError>` — never panic in library code
   - Procedural macro `#[derive(Component)]` in `khora-macros`

## Common Issues

- **Circular deps**: If adding a dependency between crates, verify the DAG in root `Cargo.toml`
- **Feature flags**: `deny.toml` blocks known-vulnerable crates
- **Build times**: Dev profile uses `opt-level = 1`; release uses LTO + single codegen unit
