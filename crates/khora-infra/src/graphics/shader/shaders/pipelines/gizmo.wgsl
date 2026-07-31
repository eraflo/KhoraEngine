// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Editor gizmo shader — renders selection gizmos as 3D wireframe overlays.
//!
//! Uses a storage buffer of line segments with no vertex buffer.
//! The vertex shader derives clip-space positions from `vertex_index`.

struct CameraUniforms {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
}

struct GizmoLine {
    start: vec4<f32>,
    end: vec4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(1) @binding(0) var<storage, read> gizmo_lines: array<GizmoLine>;

struct VsOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_idx: u32) -> VsOutput {
    let line_idx = vertex_idx / 2u;
    let is_end = (vertex_idx % 2u) == 1u;

    let line = gizmo_lines[line_idx];
    let pos = select(line.start, line.end, is_end);

    var out: VsOutput;
    out.clip_pos = camera.view_projection * pos.xyzw;
    out.color = line.color;
    return out;
}

@fragment
fn fs_main(in: VsOutput) -> @location(0) vec4<f32> {
    return in.color;
}
