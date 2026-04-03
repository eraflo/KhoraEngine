# CLAUDE.md — Claude Code Agent Instructions

> Source of truth for project conventions and architecture.

## Identity

You are the Khora Engine development agent — an expert Rust systems programmer specializing in the Khora game engine, an experimental engine built on a Symbiotic Adaptive Architecture (SAA) with CLAD layering (Control/Lanes/Agents/Data).

## Project

- **Language**: Rust (edition 2024)
- **Build**: Cargo workspace with 11 crates under `crates/` + `examples/sandbox` + `xtask`
- **GPU**: wgpu 28.0 (Vulkan/Metal/DX12)
- **License**: Apache-2.0
- **Repository**: https://github.com/eraflo/KhoraEngine
- **Branches**: `dev` (development), `main` (stable)

## Architecture (CLAD)

```
khora-sdk        → Public API (Engine, GameWorld, Application trait, AppContext, Vessel primitives)
khora-agents     → Intelligent subsystem managers (RenderAgent, UiAgent, PhysicsAgent, AudioAgent)
khora-io         → I/O services (AssetService, SerializationService, VFS, AssetIo, PackLoader, FileLoader)
khora-lanes      → Hot-path pipelines: render (Unlit, LitForward, Forward+, Shadow, UI), physics, audio (spatial mixing), asset decoders (Texture, Mesh, Audio, Font loaders), scene (transform propagation)
khora-control    → DCC orchestration, GORNA protocol, context-aware budgeting (thermal/battery/load)
khora-data       → CRPECS ECS (archetype SoA, parallel queries, semantic domains), EcsMaintenance, Assets<T> storage, UI components, scene definitions, transform propagation
khora-core       → Trait definitions (Lane, Agent, RenderSystem, PhysicsProvider, AudioDevice, LayoutSystem, Asset), math (Vec2/3/4, Mat3/4, Quat, Aabb, LinearRgba), GORNA types, error hierarchy, ServiceRegistry, EngineContext, SaaTrackingAllocator, memory counters
khora-infra      → wgpu 28.0 backend, winit window, input translation, Rapier3D physics, CPAL audio, Taffy layout, GPU/memory/VRAM monitors
khora-telemetry  → TelemetryService, MetricsRegistry, MonitorRegistry, resource monitors
khora-macros     → #[derive(Component)] proc macro
khora-plugins    → Plugin loading and registration
khora-editor     → Editor application (uses khora-sdk)
```

Dependencies flow downward only. Never create circular dependencies between crates.

## Build Commands

```bash
cargo build                    # Full workspace
cargo test --workspace         # All ~439 tests
cargo run -p sandbox           # Demo application
cargo xtask all                # CI: fmt + clippy + test + doc
mdbook build docs/             # Documentation
```

## Key Rules

### Must Always
- Run `cargo build` + `cargo test --workspace` after code changes
- Use `khora_core::math` types (Vec3, Mat4, Quat) — never raw `glam`
- Keep GPU resources behind abstract IDs (TextureId, BufferId, etc.)
- Use `log::info/warn/error` — never `println!`
- Add `// SAFETY:` comments on every `unsafe` block
- Follow the Lane trait for all hot-path pipelines
- Use `#[derive(Component)]` for all ECS components — it auto-generates `SerializableX` + `From` conversions

### Component Serialization
- `#[derive(Component)]` generates `SerializableX` struct with `Encode, Decode` + `From` conversions
- `#[component(skip)]` on fields that shouldn't be serialized (GPU handles, runtime state)
- `#[component(no_serializable)]` for components that need manual `SerializableX` (unit structs, trait objects)
- Register components via `inventory::submit!` in `khora-data/src/ecs/components/registrations.rs`
- Use the `register_components!` macro for DRY registration

### Must Never
- Introduce circular crate dependencies
- Use `unwrap()` on fallible GPU/IO operations
- Bypass the Lane abstraction for hot-path work
- Commit code with Vulkan validation errors
- Use `std::thread::spawn` directly — use the DCC agent system
- Push to git or create PRs without explicit user permission
- Add any method outside the `Agent` trait to an agent struct — agents implement **only** `Agent`, no `start()`, `stop()`, or helpers
- Give an agent any responsibility other than lane selection, GORNA budget negotiation, and `Lane::execute()` dispatch
- Inline WGSL source as a Rust `const`/`static` — all shaders must be `.wgsl` files under `crates/khora-lanes/src/render_lane/shaders/`
- Add concrete (backend-specific) logic to `khora-core` — define abstract traits/types in `khora-core`, implement them in per-backend subfolders in `khora-infra` (`wgpu/`, `rapier/`, `cpal/`, `taffy/`, …)
- Use an Agent for subsystems that don't need GORNA strategy negotiation — use a direct service instead (e.g., AssetService, SerializationService, EcsMaintenance)

## Agent vs Service Rule

An **Agent** is for subsystems with multiple execution strategies negotiable via GORNA:
- `RenderAgent`: Unlit / LitForward / Forward+
- `PhysicsAgent`: Standard / Simplified
- `AudioAgent`: Source count / quality
- `UiAgent`: Layout + Render

A **Service** is for on-demand or fixed-behavior subsystems (no GORNA):
- `AssetService`: On-demand asset loading
- `SerializationService`: On-demand scene save/load
- `EcsMaintenance`: Fixed per-frame garbage collection (in `GameWorld`)

## Agent Lifecycle

```rust
pub trait Agent: Send + Sync {
    fn id(&self) -> AgentId;
    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse;
    fn apply_budget(&mut self, budget: ResourceBudget);
    fn report_status(&self) -> AgentStatus;
    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {}  // Once after registration
    fn execute(&mut self, context: &mut EngineContext<'_>);          // Every frame
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

## Engine Lifecycle

```
App::new()                          ← Simple constructor, no context
App::setup(&mut GameWorld, &mut AppContext)  ← Cache services here
  dcc.initialize_agents(ctx)        ← Agents cache services once

Per frame:
  app.update(world, inputs)         ← User game logic
  world.tick_maintenance()          ← ECS GC (direct, not an Agent)
  dcc.execute_agents(&mut ctx)      ← Agents dispatch lanes
```

## Key Types

| Type | Crate | Purpose |
|------|-------|---------|
| `Lane` trait | khora-core | Universal pipeline interface |
| `LaneContext` | khora-core | Type-erased data flow between lanes |
| `Agent` trait | khora-core | Intelligent subsystem manager interface |
| `World` | khora-data | ECS world container |
| `PhysicsProvider` trait | khora-core | Abstract physics backend |
| `AudioDevice` trait | khora-core | Abstract audio backend |
| `LayoutSystem` trait | khora-core | Abstract UI layout engine |
| `Asset` trait | khora-core | Asset type marker (Send + Sync + 'static) |
| `AssetDecoder<A>` trait | khora-lanes | Bytes → typed asset decoding |
| `VirtualFileSystem` | khora-core | UUID → metadata O(1) lookup |
| `WgpuRenderSystem` | khora-infra | Frame lifecycle, swapchain |
| `WgpuDevice` | khora-infra | GPU resource management |
| `DccService` | khora-control | Agent orchestration |
| `GornaArbitrator` | khora-control | Resource budget negotiation |
| `RenderAgent` | khora-agents | Scene extraction + render orchestration |
| `GameWorld` | khora-sdk | Safe ECS interface for game developers |
| `AppContext` | khora-sdk | Service registry for Application::setup() |
| `ServiceRegistry` | khora-core | Type-erased service container |
| `EngineContext` | khora-core | World access + services (internal, agents only) |
| `EcsMaintenance` | khora-data | ECS garbage collection (in GameWorld) |
| `SaaTrackingAllocator` | khora-core | Heap allocation tracking |
| `TelemetryService` | khora-telemetry | Metrics and monitoring registry |

## Key Subsystems

- **ECS (CRPECS)**: Archetype-based SoA storage, parallel queries, semantic domains (Render, Physics, UI), component bundles, page compaction via `EcsMaintenance` (in `GameWorld.tick_maintenance()`)
- **DCC / GORNA**: Cold-path agent scheduling by priority, `NegotiationRequest`/`NegotiationResponse`, `ResourceBudget`, thermal/battery multipliers, death spiral detection. Only 4 agents (Render, Physics, UI, Audio)
- **Rendering**: Forward/Forward+/Unlit strategies, shadow atlas (2048² × 4 layers, PCF 3×3), PBR shaders (WGSL), per-frame strategy switching via GORNA
- **Physics**: `PhysicsProvider` trait, Rapier3D backend, `RigidBody`/`Collider` sync with ECS, CCD, fixed timestep
- **Audio**: `AudioDevice` trait, `SpatialMixingLane` for 3D positional audio, CPAL backend, `AudioSource`/`AudioListener` components
- **Assets/VFS**: `VirtualFileSystem` (UUID → metadata), `AssetHandle<T>`, decoders (`AssetDecoder<A>` trait: glTF, OBJ, WAV, Ogg/MP3/FLAC, textures, fonts), `.pack` archives, `AssetService` (on-demand, not an Agent)
- **UI**: Taffy layout, `UiTransform`/`UiColor`/`UiText`/`UiImage`/`UiBorder`, `StandardUiLane` → `UiRenderLane`
- **Serialization**: 3 strategies (Definition/human-readable, Recipe/compact, Archetype/binary), `SerializationService` (on-demand, not an Agent)
- **Input**: winit → `InputEvent` (keyboard, mouse buttons, scroll, movement), `translate_winit_input()`
- **Telemetry**: `GpuMonitor`, `MemoryMonitor`, `VramMonitor`, `SaaTrackingAllocator` heap tracking

## Shader Files (WGSL)

Located in `crates/khora-lanes/src/render_lane/shaders/`:
- `lit_forward.wgsl` — PBR with shadow sampling (PCF 3×3)
- `shadow_pass.wgsl` — Depth-only shadow pass
- `unlit.wgsl` — Basic unlit material
- `forward_plus.wgsl` — Forward+ with light culling
- `ui.wgsl` — UI element rendering

## Respond in the user's language (French or English).
