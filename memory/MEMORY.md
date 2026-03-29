# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile)
- **Tests**: ~473 passing, 0 failures
- **Last major work**: Editor Phase 5+ — Theme, Status Bar, Asset Browser, Context Menus, Rename, Duplicate
  - Phase 0: egui overlay infrastructure (custom wgpu 28.0 renderer) ✅
  - Phase 1: Abstract editor framework (CLAD-compliant dock layout) ✅
  - Phase 2: Offscreen 3D viewport, EditorCamera (orbit/pan/zoom), infinite grid shader ✅
  - Phase 3: Scene Tree panel, Name component, EditorState, entity spawn/delete, selection ✅
  - Phase 4: Properties Inspector (Transform, Camera, Light, RigidBody, Collider, AudioSource), undo/redo, PropertyEdit apply-back ✅
  - Phase 5: Console (log capture + filtering), Asset Browser (structural), Status Bar (FPS/entities/memory) ✅
  - Phase 5+: Modern blue/silver/black theme, real Asset Browser (VFS-backed, categorized), right-click context menus, double-click rename, entity duplicate, status bar in shell ✅
- **Agent personas**: 8 specialized agents defined

## Known Issues

- Vulkan semaphore validation errors (`VUID-vkAcquireNextImageKHR-semaphore-01286`) still present at runtime
- Object jittering when moving camera — may be camera matrix precision or shadow-related
- `AudioAgent` violates SAA — does pipeline work directly instead of using `SpatialMixingLane`
- `SerializationAgent` bypasses Lane abstraction
- `GarbageCollectorAgent` does moderate direct work (should be Lane-based)
- egui-wgpu crate incompatible with wgpu 28.0 (0.33 needs wgpu ^27, 0.34 needs wgpu ^29) — custom renderer in khora-infra
- `egui_winit::State` is `!Send` on some platforms — `unsafe impl Send/Sync` on `EguiOverlay` requires desktop-only

## Architecture Decisions

- Single acquire-per-frame: `begin_frame()` acquires swapchain once, all agents encode to the same target, `end_frame()` presents once
- Shadow atlas: 2048×2048 × 4 layers, Depth32Float, texel-snapped ortho projection
- Lane trait is the universal pipeline interface — all hot-path work goes through `Lane::execute()`
- CRPECS uses archetype-based SoA storage for cache-friendly iteration
- GORNA protocol for dynamic agent budget negotiation with thermal/battery multipliers
- Asset pipeline: VFS (UUID → metadata O(1)) → AssetAgent → typed loader lanes → Assets<T> registry
- Serialization: 3 strategies (Definition/Recipe/Archetype) selected by SerializationGoal
- UI: Taffy layout → StandardUiLane → UiScene → UiRenderLane
- **Editor overlay**: `EditorOverlay` trait in khora-core (type-erased via `&dyn Any`), `EguiOverlay` impl in khora-infra, `EguiFrameRenderState` passed through `&mut dyn Any` with owned types (`Arc<Mutex<WgpuGraphicsContext>>`) for `'static` compatibility. `render_overlay()` on `RenderSystem` trait creates encoder, overlay renders, encoder submitted.
