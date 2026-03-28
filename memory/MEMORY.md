# Memory

## Current State

- **Branch**: `dev`
- **Build**: Clean (all crates compile)
- **Tests**: ~470 passing, 0 failures
- **Last major work**: GitAgent setup (29 files), frame lifecycle refactor (begin_frame/end_frame), shadow texel snapping
- **Agent personas**: 7 specialized agents defined (security-auditor, deprecation-cleaner, editor-ui-ux, graphics-rendering-expert, physics-expert, math-expert, api-ux-expert)

## Known Issues

- Vulkan semaphore validation errors (`VUID-vkAcquireNextImageKHR-semaphore-01286`) still present at runtime
- Object jittering when moving camera — may be camera matrix precision or shadow-related
- `AudioAgent` violates SAA — does pipeline work directly instead of using `SpatialMixingLane`
- `SerializationAgent` bypasses Lane abstraction
- `GarbageCollectorAgent` does moderate direct work (should be Lane-based)
- `khora-editor` is a stub (prints "Coming Soon!")

## Architecture Decisions

- Single acquire-per-frame: `begin_frame()` acquires swapchain once, all agents encode to the same target, `end_frame()` presents once
- Shadow atlas: 2048×2048 × 4 layers, Depth32Float, texel-snapped ortho projection
- Lane trait is the universal pipeline interface — all hot-path work goes through `Lane::execute()`
- CRPECS uses archetype-based SoA storage for cache-friendly iteration
- GORNA protocol for dynamic agent budget negotiation with thermal/battery multipliers
- Asset pipeline: VFS (UUID → metadata O(1)) → AssetAgent → typed loader lanes → Assets<T> registry
- Serialization: 3 strategies (Definition/Recipe/Archetype) selected by SerializationGoal
- UI: Taffy layout → StandardUiLane → UiScene → UiRenderLane
