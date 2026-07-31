#define_import_path khora::lighting::structs

// Light struct layouts mirroring `khora_core::renderer::api::scene::*LightUniform`
// (`#[repr(C)]`). The bytemuck-derived size on the Rust side and the
// WGSL std140 layout must match exactly — verified by the
// `ShaderRegistry` validation pass at boot.

struct DirectionalLight {
    direction: vec4<f32>,
    color: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    shadow_params: vec4<f32>,
};

struct PointLight {
    position: vec4<f32>,
    color: vec4<f32>,
    shadow_params: vec4<f32>,
};

struct SpotLight {
    position: vec4<f32>,
    direction: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    shadow_params: vec4<f32>,
};
