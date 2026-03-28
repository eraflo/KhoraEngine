# Khora Engine — GitHub Copilot Instructions

## Project Overview

Khora Engine is an experimental Rust game engine built on a **Symbiotic Adaptive Architecture (SAA)** with **CLAD layering** (Control/Lanes/Agents/Data). It uses a Cargo workspace with 11 crates, wgpu 28.0 for GPU rendering, and a custom ECS called CRPECS.

## Language & Tooling

- **Rust** edition 2024 — idiomatic, safe, zero-cost abstractions
- **Build**: `cargo build`, `cargo test --workspace` (~470 tests), `cargo xtask all`
- **GPU**: wgpu 28.0, WGSL shaders, Vulkan/Metal/DX12 backends
- **Run**: `cargo run -p sandbox` for the demo app
- **Docs**: `mdbook build docs/`

## Workspace Structure

```
crates/
  khora-core       # Foundation: traits, math, Lane trait, LaneContext
  khora-macros     # Proc macros: #[derive(Component)]
  khora-data       # [D]ata: CRPECS ECS, allocators
  khora-control    # [C]ontrol: DCC service, GORNA protocol
  khora-telemetry  # Metrics, monitoring
  khora-lanes      # [L]anes: render, physics, audio, asset, ECS pipelines
  khora-infra      # wgpu backend, platform implementations
  khora-agents     # [A]gents: RenderAgent, UiAgent, etc.
  khora-plugins    # Plugin system
  khora-sdk        # Public SDK (Engine::run, GameWorld, Application trait)
  khora-editor     # Future editor (stub)
examples/sandbox   # Demo application
xtask              # Build automation
```

## Coding Conventions

- Use `khora_core::math::{Vec3, Mat4, Quat, LinearRgba}` — never raw `glam`
- GPU resources through typed IDs: `TextureId`, `BufferId`, `PipelineId`, etc.
- Logging: `log::info!`, `log::warn!`, `log::error!` — never `println!`
- Error handling: `Result<T, Error>` with `?` — never `unwrap()` on GPU/IO ops
- All `unsafe` blocks require `// SAFETY:` comments
- Hot-path pipelines must implement the `Lane` trait
- Crate dependencies flow downward: SDK → Agents → Lanes → Data/Core (never upward)

## Architecture Key Concepts

### Lane Trait (universal pipeline interface)
```rust
pub trait Lane: Send + Sync {
    fn strategy_name(&self) -> &'static str;
    fn kind(&self) -> LaneKind;
    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError>;
}
```

### LaneContext (type-erased data flow)
```rust
ctx.insert(value);           // Store by type
ctx.get::<T>()               // Retrieve by type
ctx.insert(Slot::new(data)); // Ephemeral mutable access
```

### Frame Lifecycle
```
begin_frame()               // Acquire swapchain (once)
  N × render_with_encoder() // Agents encode to shared target
end_frame()                 // Present (once)
```

### ECS (CRPECS)
```rust
let entity = world.spawn(bundle.with(Transform::default()).with(Light::default()));
for (transform, light) in world.query::<(&Transform, &Light)>() { /* ... */ }
```

### Key Subsystems

- **ECS (CRPECS)**: Archetype-based SoA storage, parallel queries, semantic domains (Render, Physics, UI), component bundles, page compaction
- **DCC / GORNA**: Cold-path agent scheduling by priority, resource budget negotiation, thermal/battery multipliers, death spiral detection
- **Rendering**: Forward/Forward+/Unlit strategies, shadow atlas (2048² × 4 layers, PCF 3×3), PBR shaders (WGSL), per-frame strategy switching
- **Physics**: `PhysicsProvider` trait, Rapier3D backend, RigidBody/Collider sync with ECS, CCD, fixed timestep
- **Audio**: `AudioDevice` trait, `SpatialMixingLane` for 3D positional audio, CPAL backend, `AudioSource`/`AudioListener` components
- **Assets/VFS**: `VirtualFileSystem` (UUID → metadata O(1)), `AssetHandle<T>`, loaders (glTF, OBJ, WAV, Ogg/MP3/FLAC, textures, fonts), `.pack` archives
- **UI**: Taffy layout, `UiTransform`/`UiColor`/`UiText`/`UiImage`/`UiBorder`, `StandardUiLane` → `UiRenderLane`
- **Serialization**: 3 strategies (Definition/human-readable, Recipe/compact, Archetype/binary), `SerializationGoal` enum
- **Input**: winit → `InputEvent` translation (keyboard, mouse buttons, scroll, movement)
- **Telemetry**: `GpuMonitor`, `MemoryMonitor`, `VramMonitor`, `SaaTrackingAllocator` for heap tracking

## Important Constraints

- NEVER introduce circular dependencies between crates
- NEVER bypass the Lane abstraction for hot-path work
- NEVER commit code with Vulkan validation errors
- NEVER use `std::thread::spawn` — use the DCC agent system
- ALWAYS test after changes: `cargo test --workspace`

## Respond in the user's language (French or English).
