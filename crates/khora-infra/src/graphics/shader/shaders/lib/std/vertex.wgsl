#define_import_path khora::std::vertex

// The engine's mesh vertex (position, normal, UV, linear RGBA colour) and
// the interpolated outputs lit shaders consume. Mirrors
// `Mesh::VERTEX_ATTRIBUTES` in khora-core; every mesh is uploaded in it,
// a mesh without vertex colours as opaque white.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};
