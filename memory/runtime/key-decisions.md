# Key Decisions

1. **Single-acquire frame lifecycle** — `begin_frame()` acquires the swapchain texture once; all agents encode to that same target via `render_with_encoder()`; `end_frame()` presents. This prevents Vulkan semaphore errors and enables correct `LoadOp::Load` compositing.

2. **Lane is the universal pipeline** — every hot-path execution (render, physics, audio, asset, ECS) must go through `Lane::execute()`. Agents orchestrate lanes but do not do pipeline work themselves.

3. **LaneContext as typed data-flow** — lanes communicate through `LaneContext` (a type-erased map). Shadow lanes insert `ShadowAtlasView` for render lanes to consume. No global mutable state.

4. **CRPECS for ECS** — archetype-based Column-Row Partitioned ECS with SoA storage. Components must be `'static + Send + Sync`. Queries are the primary data access pattern.

5. **Abstract GPU resource IDs** — all wgpu resources accessed via typed IDs (`TextureId`, `BufferId`, etc.). Raw handles never leak into public APIs.
