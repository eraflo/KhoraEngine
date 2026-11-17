---
name: add-a-shader
description: Adds a new WGSL shader and render pipeline to the engine. Use when implementing a render technique that needs a new pipeline, while respecting the 4-bind-group budget and the naga_oil composition flow owned by the PipelineSystem backend.
---

# Add a shader / pipeline

Shaders are `.wgsl` files, embedded with `include_str!` and composed by the `PipelineSystem` backend
(`WgpuPipelineSystem`, `crates/khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`) through `naga_oil`
`#import`. **Never** inline WGSL as a Rust string; the engine's own shaders are never read from disk at run
time (a trusted project's `.wgsl` may overlay a module; a restricted one's never does). Book:
`docs/src/how-to/add-a-shader.md`.

## Layout
All under `crates/khora-infra/src/graphics/shader/shaders/` (xtask `wgsl_only_in_shader_tree`):
- Entry points: `pipelines/<name>.wgsl`, one pass each, named `khora::pipelines::<name>`.
- Reusable modules: `lib/` (`std/`, `lighting/`, `shadow/`), each declaring a `#define_import_path`
  (e.g. `khora::shadow::sample_2d`).
- `standalone/egui.wgsl`, exposed as `EGUI_WGSL` (`crates/khora-infra/src/graphics/shader/mod.rs`)
  because the egui overlay takes source rather than a pipeline. **No new `_WGSL` constant.**

## Steps
1. Write the entry point under `shaders/pipelines/`; `#import` lib modules by their path instead of
   copying them.
2. Add `("khora::pipelines::<name>", include_str!("../../shader/shaders/pipelines/<name>.wgsl"))` to
   `PIPELINE_MODULES` in `pipeline_system/mod.rs`. A new lib module goes in `LIB_MODULES` there, with a
   matching `#define_import_path`.
3. In the lane, **name** the pipeline in its spec — `shader: "khora::pipelines::<name>"` — and build it
   per `TargetKey`. A lane never handles source; the backend resolves the `#import` graph, validates with
   `naga`, compiles and caches.
4. **Bind-group budget — exactly 4 groups:** 0 Frame (camera), 1 Object (model/normal), 2 Material,
   3 **Lighting** (all lights + shadow). Shadow bindings are fixed at group-3 indices 1/2/3; pack the
   lighting buffers into the other indices (0, 4, 5, …). A new lighting feature adds bindings to
   **group 3**, never a 5th group. Compute pipelines own their layout and are exempt. Mesh pipelines bind
   `Mesh::vertex_buffer_layout()`.
5. Tests: `composes_every_pipeline` and `every_watched_pipeline_file_is_a_registered_pipeline`
   (`pipeline_system/tests.rs`) cover a registered pipeline; add one for any variant (`#ifdef` flags) it
   composes under.

## Verify
`cargo t -p khora-infra pipeline_system`; `cargo t -p xtask`; `cargo gate`; then `cargo run -p sandbox`
(the scene renders, no Vulkan validation error). For visual style choices, use `/impeccable`. Technique
detail: [`../../reference/graphics-rendering.md`](../../reference/graphics-rendering.md).
