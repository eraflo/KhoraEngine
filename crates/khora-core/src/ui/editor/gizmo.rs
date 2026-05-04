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

//! Editor gizmo geometry generation.
//!
//! Pure functions that generate wireframe line segments for rendering
//! in the 3D viewport. All math goes through `khora_core::math`.

use super::state::GizmoMode;
use crate::math::{Mat4, Vec3};

/// A single line segment for GPU rendering.
///
/// `#[repr(C)]` layout matches the WGSL `GizmoLine` struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoLineInstance {
    /// Start point in world space.
    pub start: [f32; 4],
    /// End point in world space.
    pub end: [f32; 4],
    /// RGBA color.
    pub color: [f32; 4],
}

impl GizmoLineInstance {
    /// Creates a new line segment.
    pub fn new(start: Vec3, end: Vec3, color: [f32; 4]) -> Self {
        Self {
            start: [start.x, start.y, start.z, 1.0],
            end: [end.x, end.y, end.z, 1.0],
            color,
        }
    }
}

/// Generates a wireframe cube gizmo at the given world-space transform.
pub fn wireframe_cube(
    transform: &Mat4,
    half_extents: Vec3,
    color: [f32; 4],
) -> Vec<GizmoLineInstance> {
    let corners = [
        Vec3::new(-half_extents.x, -half_extents.y, -half_extents.z),
        Vec3::new(half_extents.x, -half_extents.y, -half_extents.z),
        Vec3::new(half_extents.x, half_extents.y, -half_extents.z),
        Vec3::new(-half_extents.x, half_extents.y, -half_extents.z),
        Vec3::new(-half_extents.x, -half_extents.y, half_extents.z),
        Vec3::new(half_extents.x, -half_extents.y, half_extents.z),
        Vec3::new(half_extents.x, half_extents.y, half_extents.z),
        Vec3::new(-half_extents.x, half_extents.y, half_extents.z),
    ];

    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];

    edges
        .iter()
        .map(|&(a, b)| {
            let start = transform.transform_point(corners[a]);
            let end = transform.transform_point(corners[b]);
            GizmoLineInstance::new(start, end, color)
        })
        .collect()
}

/// Generates XYZ axis lines at the given transform.
pub fn transform_axes(transform: &Mat4, length: f32) -> Vec<GizmoLineInstance> {
    let origin = transform.cols[3].truncate();
    let right = transform.transform_vector(Vec3::X);
    let up = transform.transform_vector(Vec3::Y);
    let forward = transform.transform_vector(Vec3::Z);

    vec![
        GizmoLineInstance::new(origin, origin + right * length, [0.95, 0.32, 0.28, 1.0]),
        GizmoLineInstance::new(origin, origin + up * length, [0.34, 0.88, 0.43, 1.0]),
        GizmoLineInstance::new(origin, origin + forward * length, [0.35, 0.63, 0.97, 1.0]),
    ]
}

/// Generates a wireframe camera frustum at the given transform.
pub fn camera_frustum(
    transform: &Mat4,
    fov_y: f32,
    aspect: f32,
    near: f32,
    far: f32,
    color: [f32; 4],
) -> Vec<GizmoLineInstance> {
    let half_h_near = (fov_y * 0.5).tan() * near;
    let half_w_near = half_h_near * aspect;
    let half_h_far = (fov_y * 0.5).tan() * far;
    let half_w_far = half_h_far * aspect;

    let near_corners = [
        Vec3::new(-half_w_near, -half_h_near, -near),
        Vec3::new(half_w_near, -half_h_near, -near),
        Vec3::new(half_w_near, half_h_near, -near),
        Vec3::new(-half_w_near, half_h_near, -near),
    ];
    let far_corners = [
        Vec3::new(-half_w_far, -half_h_far, -far),
        Vec3::new(half_w_far, -half_h_far, -far),
        Vec3::new(half_w_far, half_h_far, -far),
        Vec3::new(-half_w_far, half_h_far, -far),
    ];

    let mut lines = Vec::with_capacity(16);

    for i in 0..4 {
        let j = (i + 1) % 4;
        lines.push(GizmoLineInstance::new(
            transform.transform_point(near_corners[i]),
            transform.transform_point(near_corners[j]),
            color,
        ));
    }

    for i in 0..4 {
        let j = (i + 1) % 4;
        lines.push(GizmoLineInstance::new(
            transform.transform_point(far_corners[i]),
            transform.transform_point(far_corners[j]),
            color,
        ));
    }

    for i in 0..4 {
        lines.push(GizmoLineInstance::new(
            transform.transform_point(near_corners[i]),
            transform.transform_point(far_corners[i]),
            color,
        ));
    }

    lines
}

/// Generates a wireframe sphere for point lights.
pub fn wireframe_sphere(
    transform: &Mat4,
    radius: f32,
    segments: u32,
    color: [f32; 4],
) -> Vec<GizmoLineInstance> {
    let mut lines = Vec::new();

    for ring in 0..3 {
        let phi = std::f32::consts::PI * ((ring as f32 + 0.5) / 3.0 - 0.5);
        let ring_radius = radius * phi.cos();
        let y = radius * phi.sin();
        for seg in 0..segments {
            let theta0 = 2.0 * std::f32::consts::PI * (seg as f32 / segments as f32);
            let theta1 = 2.0 * std::f32::consts::PI * ((seg + 1) as f32 / segments as f32);
            let p0 = Vec3::new(ring_radius * theta0.cos(), y, ring_radius * theta0.sin());
            let p1 = Vec3::new(ring_radius * theta1.cos(), y, ring_radius * theta1.sin());
            lines.push(GizmoLineInstance::new(
                transform.transform_point(p0),
                transform.transform_point(p1),
                color,
            ));
        }
    }

    for seg in 0..segments {
        let theta = 2.0 * std::f32::consts::PI * (seg as f32 / segments as f32);
        let dir = Vec3::new(theta.cos(), 0.0, theta.sin());
        for step in 0..8 {
            let phi0 = std::f32::consts::PI * (step as f32 / 8.0 - 0.5);
            let phi1 = std::f32::consts::PI * ((step + 1) as f32 / 8.0 - 0.5);
            let p0 = Vec3::new(
                dir.x * radius * phi0.cos(),
                radius * phi0.sin(),
                dir.z * radius * phi0.cos(),
            );
            let p1 = Vec3::new(
                dir.x * radius * phi1.cos(),
                radius * phi1.sin(),
                dir.z * radius * phi1.cos(),
            );
            lines.push(GizmoLineInstance::new(
                transform.transform_point(p0),
                transform.transform_point(p1),
                color,
            ));
        }
    }

    lines
}

/// Generates a directional light icon (arrow).
pub fn directional_light_icon(
    transform: &Mat4,
    length: f32,
    color: [f32; 4],
) -> Vec<GizmoLineInstance> {
    let origin = transform.cols[3].truncate();
    let dir = transform
        .transform_vector(Vec3::new(0.0, 0.0, -1.0))
        .normalize();
    let tip = origin + dir * length;

    let right = transform.transform_vector(Vec3::X).normalize();
    let up = transform.transform_vector(Vec3::Y).normalize();
    let head_size = length * 0.3;

    vec![
        GizmoLineInstance::new(origin, tip, color),
        GizmoLineInstance::new(tip, tip - dir * head_size + right * head_size * 0.5, color),
        GizmoLineInstance::new(tip, tip - dir * head_size - right * head_size * 0.5, color),
        GizmoLineInstance::new(tip, tip - dir * head_size + up * head_size * 0.5, color),
        GizmoLineInstance::new(tip, tip - dir * head_size - up * head_size * 0.5, color),
    ]
}

/// Generates all gizmo lines for a set of selected entities.
pub fn generate_selection_gizmos(
    selected_entities: &[(Mat4, GizmoKind)],
    gizmo_mode: GizmoMode,
) -> Vec<GizmoLineInstance> {
    let mut lines = Vec::new();

    for (transform, kind) in selected_entities {
        match kind {
            GizmoKind::Empty => {
                lines.extend(wireframe_cube(
                    transform,
                    Vec3::new(0.5, 0.5, 0.5),
                    [0.5, 0.5, 0.6, 1.0],
                ));
            }
            GizmoKind::Camera {
                fov_y,
                aspect,
                near,
                far,
            } => {
                lines.extend(camera_frustum(
                    transform,
                    *fov_y,
                    *aspect,
                    *near,
                    *far,
                    [0.35, 0.63, 0.97, 1.0],
                ));
            }
            GizmoKind::DirectionalLight => {
                lines.extend(directional_light_icon(
                    transform,
                    1.0,
                    [1.0, 0.95, 0.5, 1.0],
                ));
            }
            GizmoKind::PointLight { radius } => {
                lines.extend(wireframe_sphere(
                    transform,
                    *radius,
                    12,
                    [1.0, 0.7, 0.3, 1.0],
                ));
            }
            GizmoKind::Audio => {
                lines.extend(wireframe_sphere(transform, 0.4, 8, [0.7, 0.3, 1.0, 1.0]));
            }
            GizmoKind::Mesh => {
                lines.extend(wireframe_cube(
                    transform,
                    Vec3::new(0.5, 0.5, 0.5),
                    [0.3, 0.85, 0.6, 1.0],
                ));
            }
        }

        match gizmo_mode {
            GizmoMode::Select | GizmoMode::Move => {
                lines.extend(transform_axes(transform, 0.8));
            }
            _ => {}
        }
    }

    lines
}

/// The kind of gizmo to draw for an entity.
#[derive(Debug, Clone)]
pub enum GizmoKind {
    /// Empty entity — wireframe cube.
    Empty,
    /// Camera entity — frustum wireframe.
    Camera {
        /// Vertical field of view, in radians.
        fov_y: f32,
        /// Width / height ratio of the projection.
        aspect: f32,
        /// Distance to the near clip plane.
        near: f32,
        /// Distance to the far clip plane.
        far: f32,
    },
    /// Directional light — arrow icon.
    DirectionalLight,
    /// Point light — wireframe sphere.
    PointLight {
        /// Influence radius of the light.
        radius: f32,
    },
    /// Audio source — wireframe sphere.
    Audio,
    /// Entity with a mesh — highlighted cube.
    Mesh,
}
