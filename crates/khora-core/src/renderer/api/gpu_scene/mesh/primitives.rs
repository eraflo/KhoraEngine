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

//! The primitives the engine generates: a plane, a cube, a sphere.

use super::Mesh;
use crate::math::{Aabb, Vec2, Vec3};
use crate::renderer::api::pipeline::PrimitiveTopology;

/// Builds a flat XZ plane primitive of the given size at height `y`.
pub fn create_plane(size: f32, y: f32) -> Mesh {
    let half = size / 2.0;
    let positions = vec![
        Vec3::new(-half, y, -half),
        Vec3::new(half, y, -half),
        Vec3::new(half, y, half),
        Vec3::new(-half, y, half),
    ];
    let normals = vec![
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let tex_coords = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    // CCW winding viewed from above (+Y), so the front face matches the +Y
    // normal — consistent with the cube and glTF convention (front = CCW =
    // outward), which back-face culling relies on.
    let indices = vec![0u32, 2, 1, 0, 3, 2];
    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(Vec3::new(-half, y, -half), Vec3::new(half, y, half)),
    }
}

/// Builds an axis-aligned cube primitive of the given size, centered at origin.
pub fn create_cube(size: f32) -> Mesh {
    let half = size / 2.0;
    let positions = vec![
        // Front (+Z)
        Vec3::new(-half, -half, half),
        Vec3::new(half, -half, half),
        Vec3::new(half, half, half),
        Vec3::new(-half, half, half),
        // Back (-Z)
        Vec3::new(half, -half, -half),
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, half, -half),
        Vec3::new(half, half, -half),
        // Right (+X)
        Vec3::new(half, -half, half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, half, -half),
        Vec3::new(half, half, half),
        // Left (-X)
        Vec3::new(-half, -half, -half),
        Vec3::new(-half, -half, half),
        Vec3::new(-half, half, half),
        Vec3::new(-half, half, -half),
        // Top (+Y)
        Vec3::new(-half, half, half),
        Vec3::new(half, half, half),
        Vec3::new(half, half, -half),
        Vec3::new(-half, half, -half),
        // Bottom (-Y)
        Vec3::new(-half, -half, -half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, -half, half),
        Vec3::new(-half, -half, half),
    ];
    let normals = vec![
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
    ];
    let tex_coords: Vec<Vec2> = (0..6)
        .flat_map(|_| {
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ]
        })
        .collect();
    let indices = vec![
        0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11, 12, 13, 14, 12, 14, 15, 16, 17,
        18, 16, 18, 19, 20, 21, 22, 20, 22, 23,
    ];
    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(
            Vec3::new(-half, -half, -half),
            Vec3::new(half, half, half),
        ),
    }
}

/// Builds a UV sphere primitive of the given radius, segments and rings.
pub fn create_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tex_coords = Vec::new();

    for ring in 0..=rings {
        let phi = std::f32::consts::PI * (ring as f32 / rings as f32);
        let y = radius * phi.cos();
        let ring_radius = radius * phi.sin();
        for segment in 0..=segments {
            let theta = 2.0 * std::f32::consts::PI * (segment as f32 / segments as f32);
            let x = ring_radius * theta.cos();
            let z = ring_radius * theta.sin();
            positions.push(Vec3::new(x, y, z));
            normals.push(Vec3::new(x / radius, y / radius, z / radius));
            tex_coords.push(Vec2::new(
                segment as f32 / segments as f32,
                ring as f32 / rings as f32,
            ));
        }
    }

    // CCW winding as seen from outside, so front faces point outward —
    // consistent with the cube and glTF convention that back-face culling
    // relies on.
    let mut indices = Vec::new();
    for ring in 0..rings {
        for segment in 0..segments {
            let current = ring * (segments + 1) + segment;
            let next = current + segments + 1;
            indices.push(current);
            indices.push(current + 1);
            indices.push(next);
            indices.push(current + 1);
            indices.push(next + 1);
            indices.push(next);
        }
    }

    Mesh {
        positions,
        normals: Some(normals),
        tex_coords: Some(tex_coords),
        tangents: None,
        colors: None,
        indices: Some(indices),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box: Aabb::from_min_max(
            Vec3::new(-radius, -radius, -radius),
            Vec3::new(radius, radius, radius),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cube has the canonical 24 vertices and 36 indices.
    #[test]
    fn cube_has_expected_vertex_and_index_counts() {
        let mesh = create_cube(2.0);
        assert_eq!(mesh.positions.len(), 24);
        assert_eq!(mesh.indices.as_ref().map_or(0, |i| i.len()), 36);
    }

    /// The plane is four vertices at its height, two triangles.
    #[test]
    fn plane_lies_at_its_height() {
        let mesh = create_plane(2.0, -1.0);
        assert_eq!(mesh.positions.len(), 4);
        assert!(mesh.positions.iter().all(|p| p.y == -1.0));
        assert_eq!(mesh.indices.as_ref().map_or(0, |i| i.len()), 6);
    }

    /// The sphere has a vertex per ring and segment crossing, all at its radius.
    #[test]
    fn sphere_vertices_lie_at_its_radius() {
        let mesh = create_sphere(0.5, 8, 4);
        assert_eq!(mesh.positions.len(), 9 * 5);
        assert!(mesh
            .positions
            .iter()
            .all(|p| (p.length() - 0.5).abs() < 1e-5));
    }
}
