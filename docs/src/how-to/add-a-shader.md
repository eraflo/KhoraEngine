# Add a shader

This guide shows you how to **add a `.wgsl` shader** and wire a render pipeline for
it through the `PipelineSystem` backend, composing reusable modules with `naga_oil`
`#import` and staying within the four-bind-group budget.

**Prerequisites:** you have read the shader section of
[Rendering](../concepts/rendering.md).

> **Shaders are files, never strings.** Never inline WGSL as a Rust `const`/`static`.
> Every shader is a `.wgsl` file composed by the `PipelineSystem` backend /
> `PipelineSystem` so it is reviewable and hot-reloadable.

## Step 1 — Add the `.wgsl` file

Place the source under the backend's shader tree:

- A **pipeline** entry point (has `@vertex` / `@fragment` / `@compute`):
  `crates/khora-infra/src/graphics/shader/shaders/pipelines/`.
- A **reusable** library module (shared structs / functions):
  `crates/khora-infra/src/graphics/shader/shaders/lib/`.

Import the shared modules you need with `naga_oil` `#import`. The standard library
modules use the `khora::…` namespace (`khora::std::camera`, `khora::std::model`,
`khora::std::material`, the `khora::lighting::*` and `khora::shadow::*` families):

```wgsl
// shaders/pipelines/my_pipeline.wgsl
#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::material::material

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    out.clip_position = camera.view_projection * world_pos;
    out.normal = normalize((model.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return material.base_color;
}
```

## Step 2 — Register the module with the backend

Add the file to the relevant table in
`crates/khora-infra/src/graphics/shader/system.rs` so the composer knows it. A
reusable module goes in `LIB_MODULES` (with a `khora::…` import path); an entry-point
pipeline goes in `PIPELINE_MODULES` (keyed by its logical name):

```rust
const PIPELINE_MODULES: &[(&str, &str)] = &[
    // … existing …
    (
        "khora::pipelines::my_pipeline",
        include_str!("shaders/pipelines/my_pipeline.wgsl"),
    ),
];
```

The sources are embedded at compile time. The backend composes, validates with
`naga`, and emits final WGSL per shader-def variant on first use, then caches it.

## Step 3 — Respect the four-bind-group budget

A render pipeline may bind **at most four** bind groups. The engine reserves them by
role — `Camera`, `Model`, `Material`, `Lighting` — resolved by the `PipelineSystem`
from a `LayoutKey`. Build your pipeline by listing the bind-group layouts it needs in
that fixed order. If your shader needs a fifth distinct resource set, fold it into an
existing group rather than adding a fifth — exceeding four is a hard GPU limit on many
targets.

A lane requests its pipeline by building a `PipelineSpec` (shader name, entry points,
bind-group layouts, vertex buffers, targets) and calling
`PipelineSystem::pipeline(device, &spec)`, which returns a cached `RenderPipelineId`.
Mirror an existing render lane for the exact spec shape.

## Step 4 — Verify it composes

The backend has a boot smoke test that composes, validates, and emits every
registered pipeline at the empty variant — it catches import-graph and binding drift
without a GPU:

```bash
cargo test -p khora-infra composes_every_pipeline
cargo test --workspace
cargo run -p sandbox   # confirm no wgpu/Vulkan validation errors
```

If composition fails, the error names the offending module and the `naga_oil`
diagnostic — usually a wrong `#import` path or a binding outside the four-group budget.

## Related

- [Rendering](../concepts/rendering.md) — the render lane / pipeline architecture.
- [Add a lane](./add-a-lane.md) — the lane that requests and draws with this pipeline.
