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

//! Gizmo geometry: pure functions that turn the selection into wireframe line
//! segments for the viewport's gizmo pass.

use super::interact::{perpendiculars, GizmoAxis, GizmoBasis};
use khora_sdk::editor_ui::gizmo::GizmoLineInstance;
use khora_sdk::editor_ui::GizmoMode;
use khora_sdk::khora_core::math::{Mat4, Vec3, Vec4};
use khora_sdk::khora_core::renderer::api::resource::view::ViewInfo;

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

/// Generates the wireframe of the volume `projection` sees, placed by
/// `transform`: the NDC corners `(±1, ±1, 0)` (near) and `(±1, ±1, 1)` (far)
/// unprojected through `projection`'s inverse — 12 edges (near ring, far ring,
/// four rails). Exact for perspective and orthographic alike. A projection
/// that cannot be inverted draws nothing.
pub fn camera_frustum(
    transform: &Mat4,
    projection: &Mat4,
    color: [f32; 4],
) -> Vec<GizmoLineInstance> {
    let Some(inverse) = projection.inverse() else {
        return Vec::new();
    };
    let corner = |x: f32, y: f32, z: f32| {
        let p = inverse * Vec4::new(x, y, z, 1.0);
        transform.transform_point(p.truncate() / p.w)
    };
    let ring = |z: f32| {
        [
            corner(-1.0, -1.0, z),
            corner(1.0, -1.0, z),
            corner(1.0, 1.0, z),
            corner(-1.0, 1.0, z),
        ]
    };
    let near = ring(0.0);
    let far = ring(1.0);
    // A volume reaching infinity (an infinite far plane) has no edge to draw.
    if near
        .iter()
        .chain(&far)
        .any(|c| !(c.x.is_finite() && c.y.is_finite() && c.z.is_finite()))
    {
        return Vec::new();
    }

    let mut lines = Vec::with_capacity(12);
    for i in 0..4 {
        let j = (i + 1) % 4;
        lines.push(GizmoLineInstance::new(near[i], near[j], color));
        lines.push(GizmoLineInstance::new(far[i], far[j], color));
        lines.push(GizmoLineInstance::new(near[i], far[i], color));
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
            GizmoKind::Camera { projection } => {
                lines.extend(camera_frustum(
                    transform,
                    projection,
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
                let sphere = wireframe_sphere(transform, *radius, 12, [1.0, 0.7, 0.3, 1.0]);
                // A range too large for its entity's scale reaches infinity:
                // no sphere rather than lines to nowhere.
                if sphere
                    .iter()
                    .all(|line| line.start.iter().chain(&line.end).all(|v| v.is_finite()))
                {
                    lines.extend(sphere);
                }
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
        /// The camera's projection (view to clip space), at the aspect of
        /// the image it would fill.
        projection: Mat4,
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

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: [f32; 4] = [0.35, 0.63, 0.97, 1.0];

    fn point(p: [f32; 4]) -> Vec3 {
        Vec3::new(p[0], p[1], p[2])
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    /// The eight corners of a view volume, near ring then far ring, each
    /// paired with its NDC signs `(x, y, near = 0 / far = 1)`.
    fn corners(
        half_near: (f32, f32),
        near: f32,
        half_far: (f32, f32),
        far: f32,
    ) -> Vec<(Vec3, (i8, i8, i8))> {
        let mut out = Vec::new();
        for (depth, (hw, hh), z) in [(0, half_near, -near), (1, half_far, -far)] {
            for sx in [-1_i8, 1] {
                for sy in [-1_i8, 1] {
                    out.push((
                        Vec3::new(hw * sx as f32, hh * sy as f32, z),
                        (sx, sy, depth),
                    ));
                }
            }
        }
        out
    }

    /// The corner `p` stands for, if any.
    fn corner_of(p: Vec3, corners: &[(Vec3, (i8, i8, i8))]) -> Option<(i8, i8, i8)> {
        corners
            .iter()
            .find(|(c, _)| close(*c, p))
            .map(|(_, label)| *label)
    }

    /// `lines` is the box over `corners`: twelve edges, every endpoint one of
    /// the corners, every corner reached, and every edge joining two corners
    /// that differ in exactly one of x, y or depth (a ring side or a rail).
    fn assert_is_the_box_over(lines: &[GizmoLineInstance], corners: &[(Vec3, (i8, i8, i8))]) {
        assert_eq!(lines.len(), 12, "a frustum is twelve edges");
        let mut reached = vec![0_u32; corners.len()];
        for line in lines {
            let (start, end) = (point(line.start), point(line.end));
            let a = corner_of(start, corners)
                .unwrap_or_else(|| panic!("{start:?} is not a corner of the volume"));
            let b = corner_of(end, corners)
                .unwrap_or_else(|| panic!("{end:?} is not a corner of the volume"));
            let differ = (a.0 != b.0) as u32 + (a.1 != b.1) as u32 + (a.2 != b.2) as u32;
            assert_eq!(
                differ, 1,
                "the edge {start:?} → {end:?} is not a ring side or a rail"
            );
            for p in [start, end] {
                let i = corners
                    .iter()
                    .position(|(c, _)| close(*c, p))
                    .expect("a corner");
                reached[i] += 1;
            }
        }
        for ((corner, _), edges) in corners.iter().zip(reached) {
            assert_eq!(edges, 3, "three edges meet at the corner {corner:?}");
        }
    }

    /// Fov 90°, aspect 2, near 1, far 10: the far half-height is
    /// `tan(45°)·10 = 10`, its half-width `20`; the near plane is ten times
    /// smaller. The camera looks down −Z.
    #[test]
    fn a_perspective_frustum_spans_its_near_and_far_planes() {
        let projection = Mat4::perspective_rh_zo(std::f32::consts::FRAC_PI_2, 2.0, 1.0, 10.0);
        let lines = camera_frustum(&Mat4::IDENTITY, &projection, COLOR);
        assert_is_the_box_over(&lines, &corners((2.0, 1.0), 1.0, (20.0, 10.0), 10.0));
    }

    /// An orthographic volume is a box: the near and far rings are the same
    /// size, `(±4, ±2)`, at −near and −far.
    #[test]
    fn an_orthographic_frustum_is_a_box_between_its_clip_planes() {
        let projection = Mat4::orthographic_rh_zo(-4.0, 4.0, -2.0, 2.0, 0.5, 5.0);
        let lines = camera_frustum(&Mat4::IDENTITY, &projection, COLOR);
        assert_is_the_box_over(&lines, &corners((4.0, 2.0), 0.5, (4.0, 2.0), 5.0));
    }

    #[test]
    fn a_translated_frustum_moves_every_endpoint_by_the_translation() {
        let offset = Vec3::new(3.0, -2.0, 7.0);
        let projection = Mat4::perspective_rh_zo(std::f32::consts::FRAC_PI_2, 2.0, 1.0, 10.0);
        let lines = camera_frustum(&Mat4::from_translation(offset), &projection, COLOR);
        let moved: Vec<_> = corners((2.0, 1.0), 1.0, (20.0, 10.0), 10.0)
            .into_iter()
            .map(|(c, label)| (c + offset, label))
            .collect();
        assert_is_the_box_over(&lines, &moved);
    }

    #[test]
    fn every_frustum_edge_has_the_given_color() {
        let projection = Mat4::orthographic_rh_zo(-4.0, 4.0, -2.0, 2.0, 0.5, 5.0);
        let color = [0.1, 0.2, 0.3, 0.4];
        let lines = camera_frustum(&Mat4::IDENTITY, &projection, color);
        assert_eq!(lines.len(), 12);
        assert!(lines.iter().all(|line| line.color == color));
    }

    #[test]
    fn a_projection_that_cannot_be_inverted_draws_no_frustum() {
        assert!(camera_frustum(&Mat4::IDENTITY, &Mat4::ZERO, COLOR).is_empty());
    }

    /// An orthographic camera with no depth (near = far) or no extent
    /// (height 0) projects through infinities: there is no volume to draw,
    /// and no line may reach the GPU with a non-finite endpoint.
    #[test]
    fn a_volume_without_depth_or_extent_draws_no_line_to_infinity() {
        for (what, projection) in [
            (
                "near = far",
                Mat4::orthographic_rh_zo(-4.0, 4.0, -2.0, 2.0, 5.0, 5.0),
            ),
            (
                "zero height",
                Mat4::orthographic_rh_zo(0.0, 0.0, 0.0, 0.0, 0.5, 5.0),
            ),
        ] {
            let lines = camera_frustum(&Mat4::IDENTITY, &projection, COLOR);
            assert!(
                lines
                    .iter()
                    .flat_map(|line| line.start.into_iter().chain(line.end))
                    .all(f32::is_finite),
                "{what}: a frustum line has a non-finite endpoint"
            );
        }
    }
}
