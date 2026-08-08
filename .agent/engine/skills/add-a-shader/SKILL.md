---
name: add-a-shader
description: Adds a new WGSL shader and render pipeline to the engine. Use when implementing a render technique that needs a new pipeline, while respecting the 4-bind-group budget and the naga_oil composition flow owned by the PipelineSystem backend.
---

# Add a shader / pipeline

Shaders are `.wgsl` files, embedded with `include_str!` and composed at startup by the `PipelineSystem`
backend via `naga_oil` `#import`. **Never** inline WGSL as a Rust string; **never** read shader files at
runtime.

> There is **no `ShaderRegistry` type** — earlier revisions of these docs named one. The composition
> point is `WgpuPipelineSystem` in `crates/khora-infra/src/graphics/shader/system.rs`.

## Layout
All under `crates/khora-infra/src/graphics/shader/shaders/`:
- Entry points: `pipelines/<name>.wgsl`.
- Reusable modules: `lib/` (`std/`, `lighting/`, `shadow/`), each declaring a `#define_import_path`
  (e.g. `khora::shadow::sample_2d`).

Two `.wgsl` files still live in `khora-lanes/src/render_lane/shaders/` — `text.wgsl` and `egui.wgsl`,
exposed as `TEXT_WGSL` / `EGUI_WGSL` because their consumers take a raw string rather than a pipeline
handle. **No new `_WGSL` constant should appear.**

## Steps
1. Write the entry-point `.wgsl` under `shaders/pipelines/`; `#import` lib modules by their path
   instead of copying.
2. Add `("khora::pipelines::<name>", include_str!("shaders/pipelines/<name>.wgsl"))` to
   `PIPELINE_MODULES` in `system.rs`. A new reusable module goes in `LIB_MODULES` the same way, with a
   matching `#define_import_path`.
3. In the lane, **name** the pipeline in its spec — `shader: "khora::pipelines::<name>"`. A lane never
   handles source and never calls the composer; the backend resolves the `#import` graph, validates
   with `naga`, compiles and caches.
4. **Bind-group budget — exactly 4 groups:** 0 Frame (camera), 1 Object (model/normal), 2 Material,
   3 **Lighting** (all lights + shadow). Shadow bindings are fixed at group-3 indices 1/2/3; pack your
   lighting buffers into the remaining indices (0, 4, 5, …). A new lighting feature adds bindings to
   **group 3**, never a 5th group. (Compute pipelines own their own layout and are exempt.)
5. Add a test that the module composes and validates.

## Verify
`cargo test --workspace` and `cargo run -p sandbox` (clean frame, scene renders, no Vulkan validation
errors). For visual style choices, use `/impeccable`. For technique detail, consult
[`../../reference/graphics-rendering.md`](../../reference/graphics-rendering.md).
