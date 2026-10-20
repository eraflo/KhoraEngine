# Graphics & Rendering — reference

Domain knowledge for Khora rendering. Consulted during Research (dispatch the `codebase-*` subagents
to apply it to concrete files). Follow [`../RULES.md`](../RULES.md) and [`../conventions.md`](../conventions.md)
§10 (bind-group budget).

## Scope
PBR/BRDF (Cook-Torrance GGX), shadows (CSM, PCF 2D, point-light cube), Forward+ tiled light culling,
HDR/tone mapping, post-processing, GPU occupancy and synchronization (no Vulkan validation errors).

## Key files
- Render lanes: `crates/khora-lanes/src/render_lane/` (Unlit, LitForward, ForwardPlus, StandardPbr, Shadow, Overlay).
- Shaders: `crates/khora-infra/src/graphics/shader/shaders/` — `pipelines/` (entry points), `lib/` (`std`/`lighting`/`shadow`), `standalone/` (`text`/`egui`, raw strings). `graphics/wgpu/pipeline_system/mod.rs` (the `PipelineSystem` backend) composes them via `naga_oil #import`.
- Backend: `crates/khora-infra/src/graphics/wgpu/` (`WgpuRenderSystem`, `WgpuDevice`).
- Render API traits: `crates/khora-core/src/renderer/`.

## Hard rules
- Every render technique is a `Lane` selected via GORNA. The backend owns **no** render pipeline.
- **Exactly 4 bind groups** (Frame/Object/Material/Lighting). New lighting features add bindings to group 3, never a 5th group. Shadow bindings fixed at group-3 indices 1/2/3.
- WGSL only, as `.wgsl` files under `khora-infra/src/graphics/shader/shaders/`, composed by the `PipelineSystem` backend — never inline shader strings, never runtime FS reads.
- GPU resources via typed IDs (`TextureId`, `BufferId`, `PipelineId`) — never raw wgpu handles in public APIs. No `unwrap()` on GPU paths.
- **One mesh vertex.** Every mesh is uploaded as `Mesh::VERTEX_ATTRIBUTES` (`khora-core/src/renderer/api/gpu_scene/mesh.rs`): position, normal, UV, linear RGBA colour — 48 bytes, absent attributes filled (normal +Y, UV 0, colour white). Every mesh pipeline binds `Mesh::vertex_buffer_layout()`; never write a stride or an attribute list by hand. The WGSL mirror is `lib/std/vertex.wgsl`. The vertex colour multiplies the base colour (glTF). A future attribute that only some passes need (skin joints/weights, UV1) goes in a second vertex buffer, not in this one.
- Decoders normalise: a glTF decoded as a `Mesh` is its default scene as one mesh (node transforms baked, strips/fans → lists, flat normals when NORMAL is absent).
- For any visual-design decision (editor look, debug viz style), use `/impeccable`.

Use codegraph to trace lane↔shader↔backend wiring before editing.

## Skills
- [`add-a-shader`](../skills/add-a-shader/SKILL.md) — add a `.wgsl` + pipeline (4-group budget).
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add a render lane / strategy.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — diagnose a render glitch / GORNA pick.
- [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md) — verify.
- **`/impeccable`** (`audit`/`critique`/`polish`) — for any visual-style decision (debug viz, editor look).
