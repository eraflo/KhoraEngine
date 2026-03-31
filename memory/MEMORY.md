# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile)
- **Tests**: ~439 passing, 0 failures
- **Last major work**: SAA Architecture Refactoring (3 phases)
  - **Phase 1 — Agent trait cleanup**: Removed `update()`, added `on_initialize()`, `execute()` now receives `&mut EngineContext`. All 7 agents refactored (Render, UI, Physics, GC, Asset, Audio, Serialization). Agent lifecycle: `on_initialize` (once) → `execute` (per frame) → `negotiate`/`apply_budget` (GORNA) ✅
  - **Phase 2 — Data layer cleanup**: 
    - `SaaTrackingAllocator` moved from `khora-data::allocators` → `khora-core::memory` ✅
    - `EcsMaintenance` added to `khora-data/src/ecs/maintenance.rs`. `GameWorld.tick_maintenance()` replaces GarbageCollectorAgent ✅
    - `AssetService` replaces `AssetAgent` (on-demand service, not an Agent) ✅
    - `SerializationService` replaces `SerializationAgent` (on-demand service) ✅
    - `AssetLoaderLane` trait renamed to `AssetDecoder` ✅
    - `GarbageCollectorAgent` and `ecs_agent` module removed ✅
    - `asset_agent` and `serialization_agent` modules removed ✅
  - **Phase 3 — SDK cleanup**:
    - `AppContext` replaces `EngineContext` in SDK public API ✅
    - `Application::new()` takes nothing, `setup()` receives `&mut AppContext` ✅
    - SDK prelude cleaned: removed ~30 renderer types, editor types, `shaders::*` glob. Only game dev types remain ✅
    - `EngineContext` returns to internal-only (agents only) ✅
- **Previous work**: Editor cleanup & feature completion (8 phases) ✅

## Known Issues

- Vulkan semaphore validation errors (`VUID-vkAcquireNextImageKHR-semaphore-01286`) still present at runtime
- Object jittering when moving camera — may be camera matrix precision or shadow-related
- egui-wgpu crate incompatible with wgpu 28.0 (0.33 needs wgpu ^27, 0.34 needs wgpu ^29) — custom renderer in khora-infra
- `egui_winit::State` is `!Send` on some platforms — `unsafe impl Send/Sync` on `EguiOverlay` requires desktop-only
- Editor unused import warnings after prelude cleanup (cosmetic, not errors)

## Architecture Decisions

- Single acquire-per-frame: `begin_frame()` acquires swapchain once, all agents encode to the same target, `end_frame()` presents once
- Shadow atlas: 2048×2048 × 4 layers, Depth32Float, texel-snapped ortho projection
- Lane trait is the universal pipeline interface — all hot-path work goes through `Lane::execute()`
- CRPECS uses archetype-based SoA storage for cache-friendly iteration
- GORNA protocol for dynamic agent budget negotiation with thermal/battery multipliers
- **Agent vs Service rule**: Only 4 agents (Render, Physics, UI, Audio) — subsystems without GORNA strategy use direct services
- Asset pipeline: VFS (UUID → metadata O(1)) → `AssetService` → `AssetDecoder<A>` → `Assets<T>` registry
- Serialization: 3 strategies (Definition/Recipe/Archetype) selected by `SerializationGoal`, via `SerializationService`
- ECS maintenance: `EcsMaintenance` in `GameWorld.tick_maintenance()` — direct, not an Agent
- UI: Taffy layout → StandardUiLane → UiScene → UiRenderLane
- Editor overlay: `EditorOverlay` trait in khora-core (type-erased via `&dyn Any`), `EguiOverlay` impl in khora-infra
