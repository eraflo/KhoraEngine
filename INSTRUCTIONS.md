# Instructions

## Quick Reference

| Command | Purpose |
|---------|---------|
| `cargo build` | Build all crates |
| `cargo test --workspace` | Run ~470 tests |
| `cargo run -p sandbox` | Launch demo app |
| `cargo xtask all` | Full CI pipeline |
| `cargo clippy --workspace` | Lint check |
| `mdbook build docs/` | Build documentation |

## Workspace Crates (dependency order)

1. `khora-core` — traits, math, Lane trait, LaneContext
2. `khora-macros` — `#[derive(Component)]`
3. `khora-data` — CRPECS ECS, allocators
4. `khora-control` — DCC service, GORNA protocol
5. `khora-telemetry` — metrics, monitoring
6. `khora-lanes` — render, physics, audio, asset, ECS lanes
7. `khora-infra` — wgpu 28.0 backend, platform I/O
8. `khora-agents` — RenderAgent, UiAgent, AudioAgent, etc.
9. `khora-plugins` — plugin loading
10. `khora-sdk` — public API (Engine::run, GameWorld, Application)
11. `khora-editor` — future editor (stub)

## Critical File Locations

| Area | Path |
|------|------|
| Lane trait | `crates/khora-core/src/lane/` |
| Agent trait | `crates/khora-core/src/agent/` |
| Math types | `crates/khora-core/src/math/` |
| Physics trait | `crates/khora-core/src/physics/` |
| Audio trait | `crates/khora-core/src/audio/` |
| Asset trait + VFS | `crates/khora-core/src/asset/`, `crates/khora-core/src/vfs/` |
| UI layout trait | `crates/khora-core/src/ui/` |
| Scene/Serialization | `crates/khora-core/src/scene/` |
| GORNA types | `crates/khora-core/src/control/gorna/` |
| ServiceRegistry | `crates/khora-core/src/service_registry.rs` |
| EngineContext | `crates/khora-core/src/context.rs` |
| Error hierarchy | `crates/khora-core/src/renderer/error.rs` |
| ECS (CRPECS) | `crates/khora-data/src/ecs/` |
| Components | `crates/khora-data/src/ecs/components/` |
| UI components | `crates/khora-data/src/ui/` |
| Asset storage | `crates/khora-data/src/assets/` |
| Tracking allocator | `crates/khora-data/src/allocators/` |
| DCC service | `crates/khora-control/src/service.rs` |
| GORNA arbitrator | `crates/khora-control/src/gorna/` |
| Analysis/heuristics | `crates/khora-control/src/analysis.rs` |
| Render lanes | `crates/khora-lanes/src/render_lane/` |
| Physics lanes | `crates/khora-lanes/src/physics_lane/` |
| Audio lanes | `crates/khora-lanes/src/audio_lane/` |
| Asset loaders | `crates/khora-lanes/src/asset_lane/loading/` |
| Scene lanes | `crates/khora-lanes/src/scene_lane/` |
| UI lane | `crates/khora-lanes/src/ui_lane/` |
| ECS lane | `crates/khora-lanes/src/ecs_lane/` |
| Shaders (WGSL) | `crates/khora-lanes/src/render_lane/shaders/` |
| Render system | `crates/khora-infra/src/graphics/wgpu/system.rs` |
| GPU device | `crates/khora-infra/src/graphics/wgpu/device.rs` |
| Window (winit) | `crates/khora-infra/src/platform/window/` |
| Input system | `crates/khora-infra/src/platform/input.rs` |
| Physics backend | `crates/khora-infra/src/physics/` |
| Audio backend | `crates/khora-infra/src/audio/` |
| Taffy layout | `crates/khora-infra/src/ui/taffy/` |
| Resource monitors | `crates/khora-infra/src/telemetry/` |
| Render agent | `crates/khora-agents/src/render_agent/` |
| UI agent | `crates/khora-agents/src/ui_agent/` |
| Physics agent | `crates/khora-agents/src/physics_agent/` |
| Audio agent | `crates/khora-agents/src/audio_agent/` |
| Asset agent | `crates/khora-agents/src/asset_agent/` |
| Serialization agent | `crates/khora-agents/src/serialization_agent/` |
| GC agent | `crates/khora-agents/src/ecs_agent/` |
| SDK entry | `crates/khora-sdk/src/lib.rs` |
| GameWorld | `crates/khora-sdk/src/game_world.rs` |
| Vessel primitives | `crates/khora-sdk/src/vessel.rs` |
| Telemetry service | `crates/khora-telemetry/src/service.rs` |
| Component macro | `crates/khora-macros/src/` |
| Sandbox app | `examples/sandbox/src/main.rs` |

## Checklist Before Completing Work

- [ ] `cargo build` succeeds with no errors
- [ ] `cargo test --workspace` — all tests pass
- [ ] No Vulkan validation errors in `cargo run -p sandbox`
- [ ] No `unwrap()` on GPU/IO fallible paths
- [ ] `// SAFETY:` comment on any `unsafe` block
- [ ] No circular crate dependencies introduced
