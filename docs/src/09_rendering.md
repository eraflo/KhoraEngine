# Rendering

The pipeline that turns ECS components into pixels. Adaptive, strategy-based, shadow-aware.

- Document — Khora Rendering v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. Why this design
2. Frame lifecycle
3. The frame data containers
4. The frame graph
5. Render strategies
6. Shadow system
7. Shader files
8. GPU resource management
9. The default backend — wgpu
10. For game developers
11. For engine contributors
12. Decisions
13. Open questions

---

## 01 — Why this design

A modern engine must render across hardware that varies by orders of magnitude — from a Steam Deck to a workstation. Picking a single render path at compile time means leaving performance on the table everywhere except the developer's machine.

Khora's renderer is a *family* of strategies. The `RenderAgent` chooses between Unlit, LitForward, and Forward+ each tick based on GORNA's budget. The `ShadowAgent` runs in `OBSERVE` and publishes the shadow atlas before the main pass. Shaders are WGSL files on disk — editable, reviewable, never inlined.

The default backend is wgpu 28.0. It can be replaced — a Vulkan-direct or Metal-direct backend would drop in as a new `khora-infra/src/graphics/<backend>/` and implement the `RenderSystem` trait from `khora-core`.

## 02 — Frame lifecycle

```
EngineCore::tick_with_services
  ├─ drain_inputs                                     # Pop queued events
  │
  ├─ run_app_update(&inputs)
  │   ├─ app.update(world, inputs)                    # User logic
  │   ├─ world.tick_maintenance()                     # ECS GC
  │   ├─ GpuCache: upload freshly added meshes        # GPU mesh sync
  │   ├─ extract_scene → RenderWorldStore             # Read ECS, fill render scene
  │   └─ extract_ui_scene → UiSceneStore              # Read ECS, fill UI scene
  │
  ├─ presents = begin_render_frame(&frame_services)
  │   └─ RenderSystem::begin_frame()
  │       ├─ device.poll_device_non_blocking()
  │       ├─ device.wait_for_last_submission()
  │       ├─ get_current_texture()
  │       └─ insert ColorTarget, DepthTarget, ClearColor → FrameContext
  │
  ├─ run_scheduler(&frame_services)
  │   ├─ OBSERVE phase
  │   │   ├─ ShadowAgent.execute()                    # Encode shadow atlas pass
  │   │   │   └─ records into SharedFrameGraph; publishes ShadowAtlasView,
  │   │   │      ShadowComparisonSampler into FrameContext
  │   │   └─ RenderAgent.execute() (Observe)          # Records main pass
  │   │       └─ LitForwardLane reads atlas from FrameContext, encodes draw
  │   ├─ TRANSFORM phase                              # Physics, audio, AI agents
  │   ├─ MUTATE phase
  │   ├─ OUTPUT phase
  │   │   └─ UiAgent.execute()                        # Records UI overlay pass
  │   │       └─ UiRenderLane (LoadOp::Load) into SharedFrameGraph
  │   └─ FINALIZE phase                               # Telemetry, cleanup
  │
  └─ end_render_frame(presents)
      ├─ submit_frame_graph(graph, device)            # Drain passes, topo-order, submit
      └─ RenderSystem::end_frame(presents)            # surface_texture.present()
```

One swapchain acquire, one present, per frame. Lanes do not encode directly to a shared encoder — they record `PassDescriptor`s into the `SharedFrameGraph`, and the engine drains the graph after the scheduler completes.

## 03 — The frame data containers

Several engine-registered services carry data through the frame:

| Service | Crate | Purpose |
|---|---|---|
| `GpuCache` | `khora-data::gpu::cache` | Shared GPU mesh store — handles to uploaded mesh buffers, keyed by ECS handle |
| `ProjectionRegistry` | `khora-data` | Runs `sync_all()` once per frame before agents — uploads new meshes through `GpuCache`, syncs projection state |
| `RenderWorldStore` | `khora-data::render` | `Arc<RwLock<RenderWorld>>` populated each frame by `extract_scene` from the ECS |
| `UiSceneStore` | `khora-data::ui` | `Arc<RwLock<UiScene>>` populated each frame by `extract_ui_scene` |
| `SharedFrameGraph` | `khora-data::render::frame_graph` | `Arc<Mutex<FrameGraph>>` — pass collector; agents append, the engine drains |
| `FrameContext` | `khora-core::renderer::api::core::frame_context` | Per-frame blackboard for cross-agent sync (shadow atlas, color/depth targets, stages) |

Game code never touches these directly; agents and lanes read and write them through the per-frame service registry.

## 04 — The frame graph

`FrameGraph` is Khora's pass collector. It is intentionally simple — a list of `PassDescriptor`s with declared resource reads and writes, plus the matching command buffers, ordered topologically before submission.

```rust
pub struct PassDescriptor {
    pub name: String,
    pub reads:  Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
}

pub enum ResourceId {
    Color,
    Depth,
    ShadowAtlas,
    Custom(u64),
}
```

Lanes call `frame_graph.lock().submit_pass(descriptor, command_buffer)` during `OUTPUT` (or any phase that produces GPU work). After the Scheduler returns, the engine calls `submit_frame_graph(graph, device)` which:

1. Builds the dependency graph from `reads` / `writes` overlap.
2. Topologically orders passes — a `ShadowAtlas`-write pass must precede a `ShadowAtlas`-read pass, even if the lanes were registered in a different order.
3. Submits command buffers in the resolved order.

This is **not** a full render-graph framework. There is no implicit resource allocation, no transient resource pooling, no aliasing analysis. Khora has nine to ten passes per frame today; that does not warrant the complexity. The decision is logged in [Decisions](./decisions.md); the size at which we revisit is in [Open questions](./open_questions.md).

## 05 — Render strategies

Per-frame switching via GORNA. The `RenderAgent` selects based on its current `ResourceBudget`.

| Strategy | Lane | Description |
|---|---|---|
| Unlit | `SimpleUnlitLane` | No lighting, baseline cost |
| Forward | `LitForwardLane` | PBR with per-light passes, shadow sampling (PCF 3×3) |
| Forward+ | `ForwardPlusLane` | Tile-based light culling, many lights |
| Shadow | `ShadowPassLane` | Depth-only shadow map rendering (owned by `ShadowAgent`) |
| UI | `UiRenderLane` | 2D UI primitives (owned by `UiAgent`) |
| Extract | `ExtractLane` | ECS → GPU-ready data transfer |

The transition between strategies is seamless: pipelines for all strategies are pre-compiled at boot; switching is one bind group flip.

## 06 — Shadow system

`ShadowAgent` is the canonical example of agent split. It runs in `OBSERVE`, before `RenderAgent`, and produces:

- A 2048 × 2048 Depth32Float **shadow atlas** with 4 layers.
- A `ShadowAtlasView` and `ShadowComparisonSampler` in `FrameContext`.

`RenderAgent` declares `AgentDependency::Hard(AgentId::ShadowRenderer)`. The Scheduler enforces ordering. The lit forward pass reads the atlas from the per-frame context.

| Detail | Value |
|---|---|
| Atlas size | 2048 × 2048, Depth32Float |
| Layers | 4 (one per cascade) |
| Light type | Directional (orthographic projection from camera frustum AABB in light space) |
| Texel snapping | Ortho bounds rounded to texel-aligned boundaries to prevent shimmer |
| Sampling | PCF 3×3 in `lit_forward.wgsl` with comparison sampler |
| Inter-agent transport | `ShadowAtlasView` + `ShadowComparisonSampler` slots in `FrameContext` |

Shimmer prevention is the subtle bit. A naive ortho projection re-derived per frame jitters by sub-texel amounts as the camera moves, producing crawl on shadow edges. We snap the ortho bounds to texel boundaries — visible artifacts disappear.

## 07 — Shader files

All shaders are WGSL files. Inlining shader source as a Rust string is forbidden by the [rules](./../.agent/rules.md).

| Shader | Purpose |
|---|---|
| `lit_forward.wgsl` | PBR lit material with shadow sampling |
| `shadow_depth.wgsl` | Depth-only shadow pass |
| `simple_unlit.wgsl` | Basic unlit material |
| `standard_pbr.wgsl` | PBR material model |
| `forward_plus.wgsl` | Forward+ light culling |
| `ui.wgsl` | UI rendering |

All under `crates/khora-lanes/src/render_lane/shaders/`.

## 08 — GPU resource management

All GPU resources are accessed through typed IDs:

| Type | Refers to |
|---|---|
| `TextureId` | A managed texture allocation |
| `BufferId` | A managed buffer allocation |
| `PipelineId` | A managed render or compute pipeline |
| `BindGroupId` | A managed bind group |
| `SamplerId` | A managed sampler |

`WgpuDevice` (the default backend implementation of `RenderSystem`) manages creation, destruction, and lifetime tracking. `SubmissionIndex` is stored per submit for GPU sync via `wait_for_last_submission()`.

Public APIs never expose raw wgpu handles. This is the seam that lets us swap the backend.

## 09 — The default backend — wgpu

The current implementation is wgpu 28.0. It targets Vulkan, Metal, DX12 — and WebGPU once the spec stabilizes for our subset.

| File | Purpose |
|---|---|
| `crates/khora-infra/src/graphics/wgpu/system.rs` | `WgpuRenderSystem` — implements `RenderSystem` |
| `crates/khora-infra/src/graphics/wgpu/device.rs` | `WgpuDevice` — manages GPU resources |

To swap to a different backend (Vulkan-direct, Metal-direct, even a software rasterizer for tests): create `crates/khora-infra/src/graphics/<backend>/`, implement `RenderSystem` and the device contract, register it in the SDK's service initialization. Lanes never see the change — they hold `Arc<dyn GraphicsDevice>`, not a concrete type.

---

## For game developers

Most rendering work is component setup:

```rust
// A camera
world.spawn((
    Transform::default(),
    GlobalTransform::identity(),
    Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0/9.0, 0.1, 1000.0),
    Name::new("Main Camera"),
));

// A light with shadows
world.spawn((
    Transform::from_translation(Vec3::new(2.0, 5.0, 3.0)),
    GlobalTransform::identity(),
    Light::directional(LinearRgba::WHITE, 1.0).with_shadows(true),
));

// A mesh entity
world.spawn((
    Transform::default(),
    GlobalTransform::identity(),
    HandleComponent::new(mesh_handle),
    MaterialComponent::pbr(albedo_handle),
));
```

Behind the scenes, `RenderAgent` extracts these every frame, picks the strategy GORNA approved, and renders. You do not call render functions; you describe a scene.

To watch the engine's choices in real time, open the editor and look at the *GORNA Stream* panel.

## For engine contributors

The render pipeline is a stack of lanes orchestrated by two agents (`RenderAgent`, `ShadowAgent`). To add a new render strategy:

1. Create a new lane under `crates/khora-lanes/src/render_lane/` implementing `Lane`.
2. Add its WGSL shader under `crates/khora-lanes/src/render_lane/shaders/`.
3. Wire it into `RenderAgent::negotiate` as a `StrategyOption` with cost estimate.
4. Add a switch in `RenderAgent::apply_budget` to instantiate the new lane.
5. Write a benchmark — add the strategy's cost to `MEMORY.md`.

Cost estimates calibrate themselves over time through telemetry, but the initial value should reflect a measured baseline.

For shadow work specifically: the atlas size, cascade count, and PCF kernel are tunable in `ShadowPassLane`. Texel-snapping logic lives in the same lane — leave it alone unless you can prove a bug.

## Decisions

### We said yes to
- **Strategy-based rendering.** Three strategies cover a wide performance envelope. More can be added without restructuring.
- **Shadow as a separate agent.** Decoupling shadow encoding from the main pass lets us run them in parallel phases and lets `ShadowAgent` negotiate atlas density independently.
- **WGSL files on disk, never strings.** Hot-reload, syntax highlighting, review.
- **GPU IDs over raw handles.** The seam that makes the backend swappable.
- **One acquire, one present per frame.** Anything else fights the swapchain abstraction.

### We said no to
- **A render graph.** Considered, deferred. Today the lane order is small enough that explicit dependency declaration is clearer than a graph. We will revisit when the lane count crosses ~10 per frame.
- **Inline shader source.** Convenient at first, miserable at scale. The rule is absolute.
- **Backend choice exposed in lane code.** Lanes hold `Arc<dyn GraphicsDevice>`. They never know whether wgpu, Vulkan-direct, or anything else is underneath.

## Open questions

1. **Forward+ tile size and light limits.** Tunable in `forward_plus.wgsl`. Defaults work; the optimal is hardware-dependent and deserves a heuristic.
2. **HDR pipeline.** Currently SDR. HDR target format support exists in wgpu 28.0; the tone-mapping pass and editor color-correctness pass are not yet implemented.
3. **Compute-driven culling.** A compute pass for view-frustum culling would let us skip the per-frame extraction cost in `LitForwardLane::prepare`. Designed, not built.

---

*Next: physics. See [Physics](./10_physics.md).*
