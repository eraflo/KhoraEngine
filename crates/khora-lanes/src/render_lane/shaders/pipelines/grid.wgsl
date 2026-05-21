// Khora Engine — Infinite editor grid shader.
//
// Renders an infinite XZ plane grid using a fullscreen triangle.
// The fragment shader reconstructs world position from depth=0 on
// the near plane and the camera inverse VP matrix, then draws
// antialiased grid lines with fade-out by distance.
//
// Bind group 0: Camera uniforms (view_projection, camera_position).

struct CameraUniforms {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) near_point: vec3<f32>,
    @location(1) far_point: vec3<f32>,
};

// Fullscreen triangle positions (CCW).
var<private> POSITIONS: array<vec3<f32>, 6> = array(
    vec3(-1.0, -1.0, 0.0),
    vec3( 1.0, -1.0, 0.0),
    vec3( 1.0,  1.0, 0.0),
    vec3(-1.0, -1.0, 0.0),
    vec3( 1.0,  1.0, 0.0),
    vec3(-1.0,  1.0, 0.0),
);

fn unproject(pos: vec3<f32>, inv_vp: mat4x4<f32>) -> vec3<f32> {
    let h = inv_vp * vec4(pos, 1.0);
    return h.xyz / h.w;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    let p = POSITIONS[idx];

    // We need the inverse of the view-projection matrix.
    // Computing a 4×4 inverse in the shader is acceptable for 6 vertices.
    let inv_vp = inverse_mat4(camera.view_projection);

    var out: VertexOutput;
    out.clip_position = vec4(p.xy, 0.0, 1.0);
    out.near_point = unproject(vec3(p.xy, 0.0), inv_vp); // near plane
    out.far_point  = unproject(vec3(p.xy, 1.0), inv_vp); // far plane
    return out;
}

// Manual 4×4 matrix inverse (cofactor method).
fn inverse_mat4(m: mat4x4<f32>) -> mat4x4<f32> {
    let a00 = m[0][0]; let a01 = m[0][1]; let a02 = m[0][2]; let a03 = m[0][3];
    let a10 = m[1][0]; let a11 = m[1][1]; let a12 = m[1][2]; let a13 = m[1][3];
    let a20 = m[2][0]; let a21 = m[2][1]; let a22 = m[2][2]; let a23 = m[2][3];
    let a30 = m[3][0]; let a31 = m[3][1]; let a32 = m[3][2]; let a33 = m[3][3];

    let b00 = a00 * a11 - a01 * a10;
    let b01 = a00 * a12 - a02 * a10;
    let b02 = a00 * a13 - a03 * a10;
    let b03 = a01 * a12 - a02 * a11;
    let b04 = a01 * a13 - a03 * a11;
    let b05 = a02 * a13 - a03 * a12;
    let b06 = a20 * a31 - a21 * a30;
    let b07 = a20 * a32 - a22 * a30;
    let b08 = a20 * a33 - a23 * a30;
    let b09 = a21 * a32 - a22 * a31;
    let b10 = a21 * a33 - a23 * a31;
    let b11 = a22 * a33 - a23 * a32;

    let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    let inv_det = 1.0 / det;

    return mat4x4<f32>(
        vec4( (a11 * b11 - a12 * b10 + a13 * b09) * inv_det,
              (-a01 * b11 + a02 * b10 - a03 * b09) * inv_det,
              (a31 * b05 - a32 * b04 + a33 * b03) * inv_det,
              (-a21 * b05 + a22 * b04 - a23 * b03) * inv_det),
        vec4( (-a10 * b11 + a12 * b08 - a13 * b07) * inv_det,
              (a00 * b11 - a02 * b08 + a03 * b07) * inv_det,
              (-a30 * b05 + a32 * b02 - a33 * b01) * inv_det,
              (a20 * b05 - a22 * b02 + a23 * b01) * inv_det),
        vec4( (a10 * b10 - a11 * b08 + a13 * b06) * inv_det,
              (-a00 * b10 + a01 * b08 - a03 * b06) * inv_det,
              (a30 * b04 - a31 * b02 + a33 * b00) * inv_det,
              (-a20 * b04 + a21 * b02 - a23 * b00) * inv_det),
        vec4( (-a10 * b09 + a11 * b07 - a12 * b06) * inv_det,
              (a00 * b09 - a01 * b07 + a02 * b06) * inv_det,
              (-a30 * b03 + a31 * b01 - a32 * b00) * inv_det,
              (a20 * b03 - a21 * b01 + a22 * b00) * inv_det),
    );
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

fn grid(frag_pos: vec3<f32>, scale: f32) -> vec4<f32> {
    let coord = frag_pos.xz * scale;
    let derivative = fwidth(coord);
    let grid_val = abs(fract(coord - 0.5) - 0.5) / derivative;
    let line = min(grid_val.x, grid_val.y);
    let min_z = min(derivative.y, 1.0);
    let min_x = min(derivative.x, 1.0);
    var color = vec4(0.35, 0.35, 0.35, 1.0 - min(line, 1.0));

    // X axis (red)
    if frag_pos.z > -0.1 * (1.0 / scale) && frag_pos.z < 0.1 * (1.0 / scale) {
        color = vec4(0.8, 0.2, 0.2, color.a);
    }
    // Z axis (blue)
    if frag_pos.x > -0.1 * (1.0 / scale) && frag_pos.x < 0.1 * (1.0 / scale) {
        color = vec4(0.2, 0.2, 0.8, color.a);
    }

    return color;
}

fn compute_depth(pos: vec3<f32>) -> f32 {
    let clip = camera.view_projection * vec4(pos, 1.0);
    return clip.z / clip.w;
}

fn compute_fade(pos: vec3<f32>) -> f32 {
    let dist = length(pos - camera.camera_position.xyz);
    // Fade from 80% visible at 20 units to 0% at 150 units.
    return 1.0 - smoothstep(20.0, 150.0, dist);
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    // Intersect the ray from near_point→far_point with the Y=0 plane.
    let t = -in.near_point.y / (in.far_point.y - in.near_point.y);

    // Discard fragments above/below the plane (ray doesn't hit Y=0).
    if t < 0.0 {
        discard;
    }

    let frag_pos = in.near_point + t * (in.far_point - in.near_point);

    // Two grid scales: 1m and 10m.
    let small = grid(frag_pos, 1.0);
    let large = grid(frag_pos, 0.1);

    let fade = compute_fade(frag_pos);

    // Blend the two scales, large on top for emphasis.
    var color = small;
    color = mix(color, large, clamp(large.a, 0.0, 1.0));
    color.a *= fade;

    if color.a < 0.01 {
        discard;
    }

    var out: FragmentOutput;
    out.color = color;
    out.depth = compute_depth(frag_pos);
    return out;
}
