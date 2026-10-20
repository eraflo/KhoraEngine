// Simple Unlit pipeline — geometry shaded by a fixed-direction
// half-Lambert based on the surface normal, modulated by the
// material's base color and the vertex colour. No real lighting / shadow
// sampling.

#import khora::std::camera::camera
#import khora::std::model::model
#import khora::std::material::material

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let world_pos = model.model_matrix * vec4<f32>(input.position, 1.0);
    output.clip_position = camera.view_projection * world_pos;
    output.normal = normalize((model.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.3));
    let ndotl = max(dot(input.normal, light_dir), 0.0);
    let ambient = 0.15;
    let brightness = ambient + ndotl * 0.85;
    let base = vec3<f32>(brightness, brightness, brightness);
    let tint = material.base_color * input.color;
    return vec4<f32>(base * tint.rgb, tint.a);
}
