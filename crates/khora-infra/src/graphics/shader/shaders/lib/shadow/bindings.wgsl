#define_import_path khora::shadow::bindings

// Shadow bindings — slots 1, 2, 3 of the lit lane's group(3) bind
// group. Slot 0 is the lighting uniform buffer (declared by the lit
// shader itself). The Rust mirror lives in
// `khora_core::renderer::api::shadow::bindings`.

@group(3) @binding(1)
var shadow_atlas: texture_depth_2d_array;

@group(3) @binding(2)
var shadow_sampler: sampler_comparison;

@group(3) @binding(3)
var shadow_cube_atlas: texture_depth_cube_array;
