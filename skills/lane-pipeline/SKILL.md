---
name: lane-pipeline
description: "Lane pipeline architecture — unified Lane trait, LaneContext type-map, Slot<T>/Ref<T> ephemeral access, LaneRegistry, and all concrete lane implementations (render, shadow, physics, audio, asset, ECS). Use when working on lane execution, context passing, or pipeline orchestration."
license: Apache-2.0
metadata:
  author: eraflo
  version: "1.0.0"
  category: engine-architecture
---

# Lane Pipeline Architecture

## Instructions

When working on the Lane system:

1. **Core trait** (`crates/khora-core/src/lane/`):
   ```rust
   pub trait Lane: Send + Sync {
       fn strategy_name(&self) -> &'static str;
       fn kind(&self) -> LaneKind;
       fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError>;
   }
   ```

2. **LaneContext** — a type-erased container for passing data through lanes:
   - `ctx.insert(value)` — store a value by its type
   - `ctx.get::<T>()` → `Option<&T>` — retrieve by type
   - `ctx.get_mut::<T>()` → `Option<&mut T>` — mutable access
   - `Slot<T>` — ephemeral mutable access to borrowed data (e.g., command encoder)
   - `Ref<T>` — shared reference wrapper for context insertion

3. **LaneKind** enum:
   - `Render` — main render passes (lit, unlit, forward+)
   - `Shadow` — shadow depth passes
   - `Physics` — physics simulation
   - `Audio` — audio mixing
   - `Asset` — asset loading
   - `Scene` — serialization, transform propagation
   - `Ecs` — ECS maintenance (compaction)
   - `Ui` — UI layout

4. **LaneRegistry** — stores and retrieves lanes by name/kind:
   - `registry.register(lane)` — add a lane
   - `registry.get(name)` → lane reference
   - `registry.find_by_kind(kind)` → iterator over matching lanes

5. **Concrete lanes** (`crates/khora-lanes/src/`):
   - `render_lane/` — `SimpleUnlitLane`, `LitForwardLane`, `ForwardPlusLane`, `UiRenderLane`, `ExtractLane`
   - `render_lane/shadow_pass_lane.rs` — `ShadowPassLane`
   - `physics_lane/` — `StandardPhysicsLane`, `PhysicsDebugLane`
   - `audio_lane/mixing/` — `SpatialMixingLane`
   - `asset_lane/loading/` — `GltfLoaderLane`, `ObjLoaderLane`, `WavLoaderLane`, `SymphoniaLoaderLane`, `TextureLoaderLane`, `FontLoaderLane`
   - `asset_lane/pack_loader.rs` — `PackLoadingLane`
   - `ecs_lane/` — `CompactionLane`
   - `scene_lane/` — `DefinitionSerializationLane`, `RecipeSerializationLane`, `ArchetypeSerializationLane`, `TransformPropagationLane`
   - `ui_lane/` — `StandardUiLane`

6. **Context keys** (`crates/khora-core/src/lane/context_keys.rs`):
   - `ColorTarget(TextureViewId)` — render target
   - `DepthTarget(TextureViewId)` — depth buffer
   - `ClearColor(LinearRgba)` — clear color
   - `ShadowAtlasView(TextureViewId)` — shadow atlas
   - `ShadowComparisonSampler(SamplerId)` — shadow sampler

7. **Execution flow** (per frame):
   - RenderAgent builds `LaneContext` with device, meshes, encoder, targets
   - Shadow lanes execute first → insert shadow atlas into context
   - Selected render lane executes → draws scene with shadows
   - UiAgent encodes UI on top via separate `render_with_encoder()` call

## Rules

- Every hot-path pipeline MUST implement the `Lane` trait
- Lanes must be stateless or use interior mutability (`Mutex`/`RwLock`)
- Context data flows through `LaneContext` — no global mutable state
