#define_import_path khora::std::material

// Material properties uniform — bound at @group(2) @binding(0). Layout
// matches `khora_core::renderer::api::scene::MaterialUniforms`
// (`#[repr(C)]`); changing the field order or types here MUST be
// mirrored Rust-side.

struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive: vec3<f32>,
    specular_power: f32,
    ambient: vec3<f32>,
    _padding: f32,
};

@group(2) @binding(0)
var<uniform> material: MaterialUniforms;
