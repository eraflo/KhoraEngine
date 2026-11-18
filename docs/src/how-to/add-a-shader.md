# Add a shader

A shader is the small program the graphics card runs for every vertex and every
pixel of a draw. This guide shows you how to **add a `.wgsl` shader** to the engine
and draw with it from a lane: write the file, register it with the `PipelineSystem`
backend, which composes it with the engine's shared modules through `naga_oil`
`#import`, and name it from the lane's pipeline spec, within the four-bind-group
budget.

**Prerequisites:** you have read the shader section of
[Rendering](../concepts/rendering.md) and [Add a lane](./add-a-lane.md).

> **Shaders are files, never strings.** Never inline WGSL as a Rust `const` or
> `static`. Every shader is a `.wgsl` file under the shader tree, embedded with
> `include_str!` and composed by the `PipelineSystem` backend (`WgpuPipelineSystem`,
> `crates/khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`). A lane never
> handles source: it names a pipeline. The one exception is the egui overlay's
> `EGUI_WGSL`, whose consumer takes source; no other `_WGSL` constant may appear.

## Step 1 — Add the `.wgsl` file

Everything lives under `crates/khora-infra/src/graphics/shader/shaders/` (the xtask
check `wgsl_only_in_shader_tree` refuses a `.wgsl` file anywhere else):

- an **entry point** — a pipeline, with its `@vertex` and `@fragment` (or
  `@compute`) functions — goes in `pipelines/<name>.wgsl`, one pass each, and is
  named `khora::pipelines::<name>`;
- a **reusable module** — shared structs and functions — goes in `lib/` (`std/`,
  `lighting/`, `shadow/`), and declares its own `#define_import_path`, for example
  `khora::shadow::sample_2d`.

Import the shared modules you need instead of copying them. The standard library
lives under `khora::std` (`camera`, `model`, `material`, `vertex`), next to the
`khora::lighting::*` and `khora::shadow::*` modules:

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

Read the engine's limits by name, never as numbers of your own. The backend composes
every module with a few **shader defs**, `naga_oil` `#define`s whose values live once,
in Rust, in `ShaderDefs` (`khora_core::renderer::api::shader`):

| Def | Value |
|---|---|
| `MAX_DIRECTIONAL_LIGHTS` | 4 |
| `MAX_POINT_LIGHTS` | 16 |
| `MAX_SPOT_LIGHTS` | 8 |
| `MAX_LIGHTS_PER_TILE` | 128 |

A WGSL module writes `#{MAX_POINT_LIGHTS}` and never declares its own copy, so Rust
and WGSL cannot disagree on a limit. A pipeline's **variant** (`ShaderVariantKey`) adds
its own defs on top — boolean flags for `#ifdef`, or integers; never a float, which a
`#define` cannot hold.

## Step 2 — Register the module with the backend

Add the file to its table in
`crates/khora-infra/src/graphics/wgpu/pipeline_system/mod.rs`. An entry point goes
in `PIPELINE_MODULES`, keyed by its logical name:

```rust
const PIPELINE_MODULES: &[(&str, &str)] = &[
    // … existing …
    (
        "khora::pipelines::my_pipeline",
        include_str!("../../shader/shaders/pipelines/my_pipeline.wgsl"),
    ),
];
```

A reusable module goes in `LIB_MODULES` the same way, keyed by the path its
`#define_import_path` declares.

The sources are embedded at compile time: the engine's own shaders are never read
from disk while it runs. The backend resolves the `#import` graph, validates the
result with `naga`, and compiles each variant the first time it is asked for, then
caches it. During development, the shader hot reload can overlay a module with a
project's `.wgsl` file (`PipelineSystem::set_overlay_source`) — only in a
[trusted](../concepts/trust.md) project; a restricted project's shaders never
replace the engine's. It overlays only a module the backend registers
(`PipelineSystem::composes`): a file of no lib nor pipeline in the tables, such as
a test probe under `shaders/tests/`, is never handed over. A lib overlay must
declare the lib's own `#define_import_path`; one that does not compose is dropped,
and every other module stays composed. In the frame's `PreExtract` phase every cached
pipeline that uses an overlaid module is rebuilt under the same key: the cache then
hands out the new pipeline and the old one is destroyed, so a lane that asks for its
pipeline each frame draws with the new one. A rebuild that fails keeps the last working
pipeline. Clearing the overlays rebuilds the composer from the engine's own sources and
every cached pipeline with it.

## Step 3 — Respect the four-bind-group budget

A render pipeline binds **at most four** bind groups, and the engine gives each a
role:

<div class="kp-figure-frame">

{{#include ../images/shaders/bind-groups.svg}}

</div>

| Group | `LayoutKey` | Holds |
|---|---|---|
| 0 | `Camera` | the view-projection and the camera's position |
| 1 | `Model` | the model matrices, at a dynamic offset per draw |
| 2 | `Material` | the material's uniforms, and the textures its variant holds |
| 3 | `Lighting` | the lighting uniforms at binding 0; the shadow atlas, its sampler and the cube atlas at 1, 2 and 3; image-based lighting from 4 |

A new lighting feature adds bindings to **group 3**, at a free index, never a
fifth group: four is a hard limit on many devices. The shadow part (1 to 3) is the
same in every lit pipeline; the image-based lighting block (four bindings) sits at a
base the lane picks — 4 in lit forward, 8 in Forward+, whose own group 3 holds its
light lists at 0 and 4 to 7 (see
[Rendering — Group 3 of the lit lanes](../concepts/rendering.md#group-3-of-the-lit-lanes)).
The layout entries, the bind group and the WGSL must use the same base. A mesh pipeline binds
`Mesh::vertex_buffer_layout()`. Compute pipelines own their layout and are exempt.
A pipeline that does not draw meshes (the sky, the grid, the interface) may give
inline layouts of its own (`LayoutSpec::Inline`, cached by a unique label), still
within four.

## Step 4 — Name the pipeline from the lane

A lane asks for its pipeline in `prepare`, with a `PipelineSpec` naming the shader,
for the formats of the frame's target (`TargetKey`), and calls
`PipelineSystem::pipeline(device, &spec)`, which returns a cached
`RenderPipelineId`. The lit forward lane's spec, from
`crates/khora-lanes/src/render/lit_forward/gpu.rs`:

```rust
PipelineSpec {
    label: "LitForward Pipeline",
    shader: "khora::pipelines::lit_forward",
    variant: surface.variant.clone(),
    bind_group_layouts: vec![
        LayoutSpec::Named(LayoutKey::Camera),
        LayoutSpec::Named(LayoutKey::Model),
        LayoutSpec::Named(LayoutKey::Material),
        LayoutSpec::Named(LayoutKey::Lighting),
    ],
    vertex_buffers: vec![Mesh::vertex_buffer_layout()],
    vs_entry: "vs_main",
    fs_entry: Some("fs_main"),
    primitive: PrimitiveStateDescriptor { /* … */ ..Default::default() },
    depth_stencil: scene_depth_state(key, surface.blend),
    color_targets: scene_color_targets(key, surface.blend),
    multisample: scene_multisample(key),
}
```

Two pipelines of one shader differ only by what `PipelineSpec::key` holds: the
variant, the target's colour and depth formats, the sample count, the cull mode and
the blending. The lane builds its pipelines again when the target's formats change
(`prepare` is called with the new `TargetKey`).

## Step 5 — Verify it composes

The backend's tests compose, validate and emit every registered pipeline at the
empty variant, without a GPU, and check that every file in `pipelines/` is
registered:

```bash
cargo t -p khora-infra composes_every_pipeline
cargo t -p khora-infra every_watched_pipeline_file_is_a_registered_pipeline
cargo t -p xtask
cargo gate
cargo run -p sandbox   # the scene renders, no wgpu/Vulkan validation error
```

Add a test of your own for each variant (`#ifdef` flags) the pipeline composes
under. If composition fails, the error names the module and the `naga_oil`
diagnostic — usually a wrong `#import` path, or a binding outside the four groups.

## Related

- [Rendering](../concepts/rendering.md) — the render family and its lanes.
- [Add a lane](./add-a-lane.md) — the lane that asks for and draws with this pipeline.
- [Trust](../concepts/trust.md) — what a restricted project's shaders may not do.
