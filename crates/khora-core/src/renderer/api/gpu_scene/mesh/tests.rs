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

//! The engine mesh vertex: the attribute table, the interleaved buffer built
//! from it, and flat-normal generation for meshes that ship without normals.

use super::*;
use crate::math::{Aabb, Vec2, Vec3, Vec4};
use crate::renderer::api::pipeline::{PrimitiveTopology, VertexFormat, VertexStepMode};

const EPS: f32 = 1e-5;

fn mesh(positions: Vec<Vec3>, indices: Option<Vec<u32>>, topology: PrimitiveTopology) -> Mesh {
    Mesh {
        positions,
        normals: None,
        tex_coords: None,
        tangents: None,
        colors: None,
        indices,
        primitive_type: topology,
        bounding_box: Aabb::INVALID,
    }
}

/// The `n` floats stored at byte `offset` inside vertex `vertex` of `buffer`.
fn floats(buffer: &[u8], vertex: usize, offset: u64, n: usize) -> Vec<f32> {
    let start = vertex * Mesh::VERTEX_STRIDE as usize + offset as usize;
    (0..n)
        .map(|i| {
            let at = start + i * 4;
            f32::from_ne_bytes([buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]])
        })
        .collect()
}

fn offset_of(location: u32) -> u64 {
    Mesh::VERTEX_ATTRIBUTES
        .iter()
        .find(|a| a.shader_location == location)
        .unwrap_or_else(|| panic!("no attribute at location {location}"))
        .offset
}

fn assert_close(actual: &[f32], expected: &[f32], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() < EPS, "{what}: {actual:?} != {expected:?}");
    }
}

fn assert_vec3_close(actual: Vec3, expected: Vec3, what: &str) {
    assert!(
        (actual - expected).length() < 1e-4,
        "{what}: {actual:?} != {expected:?}"
    );
}

/// The three corner positions of every triangle, in index order.
fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
    let indices: Vec<u32> = match &mesh.indices {
        Some(i) => i.clone(),
        None => (0..mesh.positions.len() as u32).collect(),
    };
    indices
        .chunks(3)
        .map(|t| {
            [
                mesh.positions[t[0] as usize],
                mesh.positions[t[1] as usize],
                mesh.positions[t[2] as usize],
            ]
        })
        .collect()
}

/// Two triangles are the same, same winding, when one is a rotation of the other.
fn same_winding(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    let eq = |p: Vec3, q: Vec3| (p - q).length() < 1e-5;
    (0..3).any(|r| (0..3).all(|i| eq(a[i], b[(i + r) % 3])))
}

/// A unit quad in the XY plane, counter-clockwise seen from +Z, as two
/// indexed triangles sharing the diagonal.
fn quad() -> Mesh {
    mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        Some(vec![0, 1, 2, 0, 2, 3]),
        PrimitiveTopology::TriangleList,
    )
}

// ─── The attribute table ────────────────────────────────────────────────

#[test]
fn vertex_attributes_are_locations_zero_to_three_in_order() {
    let locations: Vec<u32> = Mesh::VERTEX_ATTRIBUTES
        .iter()
        .map(|a| a.shader_location)
        .collect();
    assert_eq!(locations, vec![0, 1, 2, 3]);
}

#[test]
fn vertex_attributes_are_position_normal_uv_colour() {
    let formats: Vec<VertexFormat> = Mesh::VERTEX_ATTRIBUTES.iter().map(|a| a.format).collect();
    assert_eq!(
        formats,
        vec![
            VertexFormat::Float32x3,
            VertexFormat::Float32x3,
            VertexFormat::Float32x2,
            VertexFormat::Float32x4,
        ]
    );
}

#[test]
fn vertex_attribute_offsets_are_the_running_sum_of_their_sizes() {
    let mut running = 0u64;
    for attribute in Mesh::VERTEX_ATTRIBUTES {
        assert_eq!(
            attribute.offset, running,
            "location {} is not packed after the previous attribute",
            attribute.shader_location
        );
        running += attribute.format.size() as u64;
    }
    assert_eq!(
        running,
        Mesh::VERTEX_STRIDE,
        "attributes must fill the stride"
    );
}

#[test]
fn vertex_stride_is_48_bytes() {
    assert_eq!(Mesh::VERTEX_STRIDE, 48);
}

#[test]
fn vertex_buffer_layout_declares_the_attribute_table() {
    let layout = Mesh::vertex_buffer_layout();
    assert_eq!(layout.array_stride, Mesh::VERTEX_STRIDE);
    assert_eq!(layout.step_mode, VertexStepMode::Vertex);
    assert_eq!(layout.attributes.as_ref(), Mesh::VERTEX_ATTRIBUTES);
}

// ─── The interleaved vertex buffer ──────────────────────────────────────

#[test]
fn vertex_buffer_is_one_stride_per_position() {
    let m = quad();
    let buffer = m.create_vertex_buffer();
    assert_eq!(buffer.len(), 4 * Mesh::VERTEX_STRIDE as usize);
}

#[test]
fn vertex_buffer_writes_each_attribute_at_its_offset() {
    let mut m = mesh(
        vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)],
        None,
        PrimitiveTopology::TriangleList,
    );
    m.normals = Some(vec![Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)]);
    m.tex_coords = Some(vec![Vec2::new(0.25, 0.5), Vec2::new(0.75, 1.0)]);
    m.colors = Some(vec![
        Vec4::new(0.1, 0.2, 0.3, 0.4),
        Vec4::new(0.5, 0.6, 0.7, 0.8),
    ]);
    // Tangents are CPU data only: they must not shift the GPU vertex.
    m.tangents = Some(vec![Vec4::new(9.0, 9.0, 9.0, 9.0); 2]);

    let buffer = m.create_vertex_buffer();
    assert_eq!(buffer.len(), 2 * Mesh::VERTEX_STRIDE as usize);

    assert_close(
        &floats(&buffer, 0, offset_of(0), 3),
        &[1.0, 2.0, 3.0],
        "position 0",
    );
    assert_close(
        &floats(&buffer, 1, offset_of(0), 3),
        &[4.0, 5.0, 6.0],
        "position 1",
    );
    assert_close(
        &floats(&buffer, 0, offset_of(1), 3),
        &[0.0, 0.0, 1.0],
        "normal 0",
    );
    assert_close(
        &floats(&buffer, 1, offset_of(1), 3),
        &[1.0, 0.0, 0.0],
        "normal 1",
    );
    assert_close(&floats(&buffer, 0, offset_of(2), 2), &[0.25, 0.5], "uv 0");
    assert_close(&floats(&buffer, 1, offset_of(2), 2), &[0.75, 1.0], "uv 1");
    assert_close(
        &floats(&buffer, 0, offset_of(3), 4),
        &[0.1, 0.2, 0.3, 0.4],
        "colour 0",
    );
    assert_close(
        &floats(&buffer, 1, offset_of(3), 4),
        &[0.5, 0.6, 0.7, 0.8],
        "colour 1",
    );
}

#[test]
fn vertex_buffer_defaults_absent_attributes() {
    let m = mesh(
        vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, 0.0, 0.5)],
        None,
        PrimitiveTopology::TriangleList,
    );
    let buffer = m.create_vertex_buffer();
    assert_eq!(buffer.len(), 2 * Mesh::VERTEX_STRIDE as usize);
    for v in 0..2 {
        assert_close(
            &floats(&buffer, v, offset_of(1), 3),
            &[0.0, 1.0, 0.0],
            "default normal",
        );
        assert_close(
            &floats(&buffer, v, offset_of(2), 2),
            &[0.0, 0.0],
            "default uv",
        );
        assert_close(
            &floats(&buffer, v, offset_of(3), 4),
            &[1.0, 1.0, 1.0, 1.0],
            "default colour",
        );
    }
    assert_close(
        &floats(&buffer, 1, offset_of(0), 3),
        &[-1.0, 0.0, 0.5],
        "position",
    );
}

#[test]
fn colour_without_uv_lands_at_the_colour_offset() {
    let mut m = mesh(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        Some(vec![0, 1, 2]),
        PrimitiveTopology::TriangleList,
    );
    m.normals = Some(vec![Vec3::Z; 3]);
    m.colors = Some(vec![
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, 0.5),
    ]);
    let buffer = m.create_vertex_buffer();
    assert_eq!(buffer.len(), 3 * Mesh::VERTEX_STRIDE as usize);
    assert_close(
        &floats(&buffer, 0, 32, 4),
        &[1.0, 0.0, 0.0, 1.0],
        "colour 0",
    );
    assert_close(
        &floats(&buffer, 1, 32, 4),
        &[0.0, 1.0, 0.0, 1.0],
        "colour 1",
    );
    assert_close(
        &floats(&buffer, 2, 32, 4),
        &[0.0, 0.0, 1.0, 0.5],
        "colour 2",
    );
    for v in 0..3 {
        assert_close(&floats(&buffer, v, 24, 2), &[0.0, 0.0], "uv defaulted");
    }
}

#[test]
fn vertex_buffer_fills_defaults_past_a_short_attribute() {
    let mut m = mesh(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        None,
        PrimitiveTopology::TriangleList,
    );
    m.normals = Some(vec![Vec3::Z]);
    m.tex_coords = Some(vec![Vec2::new(0.5, 0.5), Vec2::new(1.0, 0.0)]);
    m.colors = Some(vec![Vec4::new(0.2, 0.4, 0.6, 0.8)]);

    let buffer = m.create_vertex_buffer();
    assert_eq!(buffer.len(), 3 * Mesh::VERTEX_STRIDE as usize);

    assert_close(
        &floats(&buffer, 0, offset_of(1), 3),
        &[0.0, 0.0, 1.0],
        "normal 0 given",
    );
    assert_close(
        &floats(&buffer, 1, offset_of(1), 3),
        &[0.0, 1.0, 0.0],
        "normal 1 default",
    );
    assert_close(
        &floats(&buffer, 2, offset_of(1), 3),
        &[0.0, 1.0, 0.0],
        "normal 2 default",
    );
    assert_close(
        &floats(&buffer, 1, offset_of(2), 2),
        &[1.0, 0.0],
        "uv 1 given",
    );
    assert_close(
        &floats(&buffer, 2, offset_of(2), 2),
        &[0.0, 0.0],
        "uv 2 default",
    );
    assert_close(
        &floats(&buffer, 0, offset_of(3), 4),
        &[0.2, 0.4, 0.6, 0.8],
        "colour 0 given",
    );
    assert_close(
        &floats(&buffer, 2, offset_of(3), 4),
        &[1.0, 1.0, 1.0, 1.0],
        "colour 2 default",
    );
}

#[test]
fn empty_mesh_has_an_empty_vertex_buffer() {
    let m = mesh(Vec::new(), None, PrimitiveTopology::TriangleList);
    assert!(m.create_vertex_buffer().is_empty());
}

// ─── Flat normals ───────────────────────────────────────────────────────

#[test]
fn flat_normals_unweld_an_indexed_quad_into_six_vertices() {
    let mut m = quad();
    m.generate_flat_normals();
    assert_eq!(m.positions.len(), 6);
    assert_eq!(m.normals.as_ref().map(Vec::len), Some(6));
    assert_eq!(m.indices, Some(vec![0, 1, 2, 3, 4, 5]));
    assert_eq!(m.primitive_type, PrimitiveTopology::TriangleList);
}

#[test]
fn flat_normals_are_the_face_normal() {
    let mut m = quad();
    m.generate_flat_normals();
    let normals = m.normals.clone().expect("normals generated");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3_close(*n, Vec3::Z, &format!("normal {i}"));
    }
}

#[test]
fn flat_normals_follow_each_triangle_of_a_bent_mesh() {
    // Two triangles sharing an edge: one in the XY plane (faces +Z), one in
    // the XZ plane folded down (faces +Y when wound counter-clockwise).
    let mut m = mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
        ],
        Some(vec![0, 1, 2, 0, 3, 1]),
        PrimitiveTopology::TriangleList,
    );
    m.generate_flat_normals();
    let normals = m.normals.clone().expect("normals generated");
    assert_eq!(normals.len(), 6);
    for (i, &normal) in normals.iter().enumerate().take(3) {
        assert_vec3_close(normal, Vec3::Z, &format!("first triangle, vertex {i}"));
    }
    // (0,0,-1)-(0,0,0) x (1,0,0)-(0,0,0) = (0,0,-1) x (1,0,0) = (0,-1,0)
    for (i, &normal) in normals.iter().enumerate().skip(3).take(3) {
        assert_vec3_close(
            normal,
            Vec3::new(0.0, -1.0, 0.0),
            &format!("second triangle, vertex {i}"),
        );
    }
}

#[test]
fn flat_normals_keep_counter_clockwise_winding() {
    let source = quad();
    let before = triangles(&source);
    let mut m = quad();
    m.generate_flat_normals();
    let after = triangles(&m);
    assert_eq!(after.len(), before.len());
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        assert!(
            same_winding(*b, *a),
            "triangle {i} changed winding or corners: {b:?} -> {a:?}"
        );
    }
    let normals = m.normals.clone().expect("normals generated");
    for (t, corners) in after.iter().enumerate() {
        let face = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        for k in 0..3 {
            assert!(
                face.dot(normals[t * 3 + k]) > 0.0,
                "triangle {t}: stored normal points away from the CCW front"
            );
        }
    }
}

#[test]
fn flat_normals_carry_uv_colour_and_tangent() {
    let mut m = quad();
    let uvs = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    let colours = [
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, 1.0),
        Vec4::new(1.0, 1.0, 0.0, 0.5),
    ];
    let tangents = [
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, -1.0),
        Vec4::new(1.0, 0.0, 0.0, -1.0),
    ];
    let source_positions = m.positions.clone();
    m.tex_coords = Some(uvs.to_vec());
    m.colors = Some(colours.to_vec());
    m.tangents = Some(tangents.to_vec());

    m.generate_flat_normals();

    let out_uvs = m.tex_coords.clone().expect("uv kept");
    let out_colours = m.colors.clone().expect("colour kept");
    let out_tangents = m.tangents.clone().expect("tangent kept");
    assert_eq!(out_uvs.len(), m.positions.len());
    assert_eq!(out_colours.len(), m.positions.len());
    assert_eq!(out_tangents.len(), m.positions.len());

    // Every source position is distinct, so it names the source vertex each
    // output vertex was copied from.
    for (i, p) in m.positions.iter().enumerate() {
        let src = source_positions
            .iter()
            .position(|s| (*s - *p).length() < EPS)
            .unwrap_or_else(|| panic!("output vertex {i} at {p:?} is not a source vertex"));
        assert_eq!(out_uvs[i], uvs[src], "uv of output vertex {i}");
        assert_eq!(out_colours[i], colours[src], "colour of output vertex {i}");
        assert_eq!(
            out_tangents[i], tangents[src],
            "tangent of output vertex {i}"
        );
    }
}

#[test]
fn flat_normals_on_a_non_indexed_list_take_vertices_three_by_three() {
    let mut m = mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        ],
        None,
        PrimitiveTopology::TriangleList,
    );
    m.generate_flat_normals();
    assert_eq!(m.positions.len(), 6);
    assert_eq!(m.indices, Some(vec![0, 1, 2, 3, 4, 5]));
    let normals = m.normals.clone().expect("normals generated");
    assert_eq!(normals.len(), 6);
    for (i, &normal) in normals.iter().enumerate().take(3) {
        assert_vec3_close(normal, Vec3::Z, &format!("vertex {i}"));
    }
    // (0,0,1) x (1,0,0) = (0,1,0)
    for (i, &normal) in normals.iter().enumerate().skip(3).take(3) {
        assert_vec3_close(normal, Vec3::Y, &format!("vertex {i}"));
    }
}

#[test]
fn flat_normals_leave_a_mesh_with_normals_untouched() {
    let mut m = quad();
    let given = vec![Vec3::X; 4];
    m.normals = Some(given.clone());
    m.generate_flat_normals();
    assert_eq!(m.positions, quad().positions);
    assert_eq!(m.indices, Some(vec![0, 1, 2, 0, 2, 3]));
    assert_eq!(m.normals, Some(given));
}

#[test]
fn flat_normals_leave_a_line_list_untouched() {
    let mut m = mesh(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z],
        Some(vec![0, 1, 2, 3]),
        PrimitiveTopology::LineList,
    );
    m.generate_flat_normals();
    assert_eq!(m.positions, vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z]);
    assert_eq!(m.indices, Some(vec![0, 1, 2, 3]));
    assert!(m.normals.is_none());
    assert_eq!(m.primitive_type, PrimitiveTopology::LineList);
}

#[test]
fn degenerate_triangle_gets_the_up_normal() {
    let mut m = mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(5.0, 5.0, 5.0),
            Vec3::new(5.0, 5.0, 5.0),
            Vec3::new(5.0, 5.0, 5.0),
        ],
        Some(vec![0, 1, 2, 3, 4, 5]),
        PrimitiveTopology::TriangleList,
    );
    m.generate_flat_normals();
    let normals = m.normals.clone().expect("normals generated");
    assert_eq!(normals.len(), 6);
    for (i, n) in normals.iter().enumerate() {
        assert!(
            n.x.is_finite() && n.y.is_finite() && n.z.is_finite(),
            "normal {i} is not finite: {n:?}"
        );
        assert_vec3_close(*n, Vec3::Y, &format!("normal {i}"));
    }
}

/// A valid triangle only a few millimetres across (a detailed model in
/// metres, or one under a 0.01 node scale) is not degenerate: it gets its
/// face normal, not the `(0, 1, 0)` fallback.
#[test]
fn flat_normals_of_a_millimetre_triangle_are_its_face_normal() {
    let mut m = mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.002, 0.0, 0.0),
            Vec3::new(0.0, 0.002, 0.0),
        ],
        Some(vec![0, 1, 2]),
        PrimitiveTopology::TriangleList,
    );
    m.generate_flat_normals();
    let normals = m.normals.clone().expect("normals generated");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3_close(*n, Vec3::Z, &format!("normal {i}"));
    }
}

/// A triangle naming a vertex that does not exist is dropped whole; the
/// triangles after it keep their own three corners, and every attribute
/// stays one per vertex.
#[test]
fn flat_normals_drop_a_triangle_with_an_out_of_range_corner_whole() {
    let mut m = mesh(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        Some(vec![0, 1, 9, 0, 1, 2]),
        PrimitiveTopology::TriangleList,
    );
    m.generate_flat_normals();
    let normals = m.normals.clone().expect("normals generated");
    let indices = m.indices.clone().expect("indexed");
    assert_eq!(
        normals.len(),
        m.positions.len(),
        "one normal per vertex: positions {:?}, normals {normals:?}",
        m.positions
    );
    assert_eq!(indices.len() % 3, 0, "whole triangles only: {indices:?}");
    assert_eq!(m.positions.len(), 3, "only the valid triangle survives");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3_close(*n, Vec3::Z, &format!("normal {i}"));
    }
}
