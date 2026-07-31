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

use super::gizmo_interact::{perpendiculars, GizmoAxis, GizmoBasis};
use super::state::GizmoMode;
use crate::math::{Mat4, Vec3};
use crate::renderer::api::resource::view::ViewInfo;

/// Fraction of the viewport's half-height a manipulator arm spans.
///
/// The manipulator is sized in world units but wants a constant *apparent*
/// size: a fixed world length — what this used to be — swallows the screen when
/// you zoom in and shrinks below the cursor a few metres out, which is exactly
/// when you still need to grab it.
const GIZMO_SCREEN_FRACTION: f32 = 0.32;

/// Segments per rotation ring. Enough that a ring reads as round rather than
/// as a polygon, at three rings per selected entity.
const RING_SEGMENTS: u32 = 48;

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
///
/// Directions are normalised, so a scaled entity gets a square set of axes
/// rather than one stretched along whichever axis it happens to be scaled on.
pub fn transform_axes(transform: &Mat4, length: f32) -> Vec<GizmoLineInstance> {
    let origin = transform.cols[3].truncate();

    [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .zip(GizmoAxis::ALL)
        .map(|(local, axis)| {
            let direction = transform.transform_vector(local).normalize();
            GizmoLineInstance::new(origin, origin + direction * length, axis.color())
        })
        .collect()
}

/// World size a manipulator at `origin` needs to occupy a constant fraction of
/// the viewport, whatever its distance from the camera.
///
/// Derived from the projection matrix rather than from a field of view passed
/// alongside it, so the gizmo cannot drift out of agreement with what is
/// actually being rendered.
pub fn gizmo_world_size(view: &ViewInfo, origin: Vec3) -> f32 {
    // A perspective projection stores `1 / tan(fov_y / 2)` in m11.
    let focal = view.projection_matrix.cols[1].y;
    if focal.abs() < 1e-6 {
        return 1.0;
    }
    let distance = (origin - view.camera_position).length().max(1e-3);
    distance / focal * GIZMO_SCREEN_FRACTION
}

/// The interactive handles for `mode`, drawn at `pivot` along `basis`.
///
/// Empty for [`GizmoMode::Select`]: the pointer tool manipulates nothing, so
/// drawing grabbable-looking handles for it would be a lie.
pub fn manipulator(
    pivot: Vec3,
    basis: &GizmoBasis,
    mode: GizmoMode,
    size: f32,
) -> Vec<GizmoLineInstance> {
    match mode {
        GizmoMode::Select => Vec::new(),
        GizmoMode::Move => move_handles(pivot, basis, size),
        GizmoMode::Rotate => rotate_handles(pivot, basis, size),
        GizmoMode::Scale => scale_handles(pivot, basis, size),
    }
}

/// Three arrows: a shaft out to `size` and a four-barbed head at the tip.
fn move_handles(pivot: Vec3, basis: &GizmoBasis, size: f32) -> Vec<GizmoLineInstance> {
    let head = size * 0.18;
    let mut lines = Vec::with_capacity(GizmoAxis::ALL.len() * 5);

    for axis in GizmoAxis::ALL {
        let direction = basis[axis.index()];
        let color = axis.color();
        let tip = pivot + direction * size;
        let base = tip - direction * head;
        let (u, v) = perpendiculars(direction);

        lines.push(GizmoLineInstance::new(pivot, tip, color));
        for barb in [u, -u, v, -v] {
            lines.push(GizmoLineInstance::new(
                tip,
                base + barb * (head * 0.4),
                color,
            ));
        }
    }
    lines
}

/// Three arms, each capped with a small box — the cap is what says "scale"
/// rather than "move" at a glance.
fn scale_handles(pivot: Vec3, basis: &GizmoBasis, size: f32) -> Vec<GizmoLineInstance> {
    let half = size * 0.09;
    let mut lines = Vec::with_capacity(GizmoAxis::ALL.len() * 13);

    for axis in GizmoAxis::ALL {
        let direction = basis[axis.index()];
        let color = axis.color();
        let tip = pivot + direction * size;
        let (u, v) = perpendiculars(direction);

        lines.push(GizmoLineInstance::new(pivot, tip, color));

        // Eight corners of the cap, indexed so the low bit walks `direction`,
        // the middle bit `u`, and the high bit `v`.
        let corner = |i: usize| {
            let sign = |bit: usize| if i & (1 << bit) == 0 { -1.0 } else { 1.0 };
            tip + direction * (half * sign(0)) + u * (half * sign(1)) + v * (half * sign(2))
        };
        // Every pair of corners differing by exactly one bit is an edge.
        for i in 0..8usize {
            for bit in 0..3 {
                let j = i | (1 << bit);
                if j != i {
                    lines.push(GizmoLineInstance::new(corner(i), corner(j), color));
                }
            }
        }
    }
    lines
}

/// Three rings, one in each plane normal to a handle.
fn rotate_handles(pivot: Vec3, basis: &GizmoBasis, size: f32) -> Vec<GizmoLineInstance> {
    let mut lines = Vec::with_capacity(GizmoAxis::ALL.len() * RING_SEGMENTS as usize);

    for axis in GizmoAxis::ALL {
        let direction = basis[axis.index()];
        let color = axis.color();
        let (u, v) = perpendiculars(direction);

        for segment in 0..RING_SEGMENTS {
            let theta = |s: u32| std::f32::consts::TAU * (s as f32 / RING_SEGMENTS as f32);
            let point = |s: u32| {
                let t = theta(s);
                pivot + u * (size * t.cos()) + v * (size * t.sin())
            };
            lines.push(GizmoLineInstance::new(
                point(segment),
                point(segment + 1),
                color,
            ));
        }
    }
    lines
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

/// One selected entity's gizmo request.
#[derive(Debug, Clone)]
pub struct SelectionGizmo {
    /// World transform of the entity.
    pub transform: Mat4,
    /// Which outline to draw around it.
    pub kind: GizmoKind,
    /// World size of the entity's own axis cross — see [`gizmo_world_size`].
    pub size: f32,
}

/// Generates all gizmo lines for a set of selected entities.
pub fn generate_selection_gizmos(
    selected_entities: &[SelectionGizmo],
    gizmo_mode: GizmoMode,
) -> Vec<GizmoLineInstance> {
    let mut lines = Vec::new();

    for SelectionGizmo {
        transform,
        kind,
        size,
    } in selected_entities
    {
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

        // The pointer tool manipulates nothing, so it shows each entity's own
        // orientation instead. The three real tools get a single [`manipulator`]
        // at the selection's shared pivot — drawn by the caller, because one
        // per entity would stack five identical sets of handles on a
        // five-entity selection, none of them where a drag actually pivots.
        if gizmo_mode == GizmoMode::Select {
            lines.extend(transform_axes(transform, *size));
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
