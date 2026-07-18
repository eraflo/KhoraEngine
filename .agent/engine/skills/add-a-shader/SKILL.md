---
name: add-a-shader
description: Adds a new WGSL shader and render pipeline to the engine. Use when implementing a render technique that needs a new pipeline, while respecting the 4-bind-group budget and the ShaderRegistry composition flow.
---

# Add a shader / pipeline

Shaders are `.wgsl` files composed at runtime by `ShaderRegistry` via `naga_oil #import`. **Never** inline
WGSL as a Rust string; **never** read shader files at runtime.

## Layout
- Entry points: `crates/khora-lanes/src/render_lane/shaders/pipelines/<name>.wgsl`.
- Reusable modules: `crates/khora-lanes/src/render_lane/shaders/lib/` (`std/`, `lighting/`, `shadow/`),
  each with a `#define_import_path` (e.g. `khora::shadow::sample_2d`).

## Steps
1. Write the entry-point `.wgsl` under `pipelines/`; `#import` lib modules by their path instead of copying.
2. Register the module name in `PIPELINE_MODULES` in `shader_registry.rs`.
3. In the lane's `on_initialize`, call `ShaderRegistry::create_module("khora::pipelines::<name>")` — the
   registry resolves imports, validates with `naga`, and emits final WGSL.
4. **Bind-group budget — exactly 4 groups:** 0 Frame (camera), 1 Object (model/normal), 2 Material,
   3 **Lighting** (all lights + shadow). Shadow bindings are fixed at group-3 indices 1/2/3; pack your
   lighting buffers into the remaining indices (0, 4, 5, …). A new lighting feature adds bindings to
   **group 3**, never a 5th group. (Compute pipelines own their own layout and are exempt.)
5. Add a test that the module compiles through the registry.

## Verify
`cargo test --workspace` and `cargo run -p sandbox` (clean frame, scene renders). For visual style choices,
use `/impeccable`. For technique detail, consult [`../../reference/graphics-rendering.md`](../../reference/graphics-rendering.md).
