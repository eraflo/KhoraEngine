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

//! A glTF `Mesh` asset is the file's default scene baked into one mesh.

use super::gltf_builder::{
    primitive, primitive_with_mode, GltfBuilder, MODE_LINES, MODE_POINTS, MODE_TRIANGLE_FAN,
    MODE_TRIANGLE_STRIP,
};
use super::*;
use khora_core::math::Vec2;
use khora_core::renderer::api::pipeline::PrimitiveTopology;
use serde_json::json;
use std::f32::consts::FRAC_1_SQRT_2;

/// A right triangle in the XY plane, counter-clockwise seen from +Z.
const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const UP_Z: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];

/// A unit quad in the XY plane, counter-clockwise seen from +Z.
const QUAD: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];
const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

/// A second triangle, away from [`TRI`], so the two are told apart.
const OTHER_TRI: [[f32; 3]; 3] = [[5.0, 5.0, 5.0], [6.0, 5.0, 5.0], [5.0, 6.0, 5.0]];

fn translated(points: &[[f32; 3]], by: [f32; 3]) -> Vec<Vec3> {
    points
        .iter()
        .map(|p| Vec3::new(p[0] + by[0], p[1] + by[1], p[2] + by[2]))
        .collect()
}

/// A builder with one mesh: [`TRI`] with +Z normals, indexed.
fn with_triangle_mesh(b: &mut GltfBuilder, corners: &[[f32; 3]; 3]) -> usize {
    let pos = b.positions(corners);
    let nrm = b.vec3(&UP_Z);
    let idx = b.indices(&[0, 1, 2]);
    b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm)],
        Some(idx),
    )])
}

fn single_node_scene(b: &mut GltfBuilder, node: serde_json::Value) {
    let n = b.node(node);
    let s = b.scene(&[n]);
    b.default_scene(s);
}

// ─── Attributes ─────────────────────────────────────────────────────────

#[test]
fn colour_without_uv_is_read_and_lands_at_the_colour_offset() {
    let colours = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.5],
    ];
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let col = b.vec4(&colours);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm), ("COLOR_0", col)],
        Some(idx),
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert!(mesh.tex_coords.is_none(), "no TEXCOORD_0 in the file");
    let read = mesh.colors.clone().expect("COLOR_0 read");
    assert_eq!(read.len(), 3);
    for (i, c) in colours.iter().enumerate() {
        assert_vec4(
            read[i],
            Vec4::new(c[0], c[1], c[2], c[3]),
            &format!("colour {i}"),
        );
    }

    let buffer = mesh.create_vertex_buffer();
    assert_eq!(buffer.len(), 3 * Mesh::VERTEX_STRIDE as usize);
    for (i, c) in colours.iter().enumerate() {
        assert_eq!(
            buffer_floats(&buffer, i, COLOUR_OFFSET, 4),
            c.to_vec(),
            "vertex {i} colour in the GPU buffer"
        );
    }
}

#[test]
fn vec3_colour_is_opaque() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let col = b.vec3(&[[1.0, 0.5, 0.25]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm), ("COLOR_0", col)],
        Some(idx),
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    let read = mesh.colors.clone().expect("COLOR_0 read");
    for (i, c) in read.iter().enumerate() {
        assert_vec4(*c, Vec4::new(1.0, 0.5, 0.25, 1.0), &format!("colour {i}"));
    }
    let buffer = mesh.create_vertex_buffer();
    for i in 0..3 {
        assert_eq!(
            buffer_floats(&buffer, i, COLOUR_OFFSET, 4),
            vec![1.0, 0.5, 0.25, 1.0],
            "vertex {i} colour in the GPU buffer"
        );
    }
}

#[test]
fn normalised_integer_colours_are_decoded_to_unit_floats() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let bytes = b.unorm8x4(&[[255, 0, 51, 255]; 3]);
    let shorts = b.unorm16x4(&[[0, 65535, 0, 65535]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let m8 = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm), ("COLOR_0", bytes)],
        Some(idx),
    )]);
    let m16 = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm), ("COLOR_0", shorts)],
        Some(idx),
    )]);
    let n8 = b.node(json!({ "mesh": m8 }));
    let n16 = b.node(json!({ "mesh": m16, "translation": [0.0, 0.0, -1.0] }));
    let s = b.scene(&[n8, n16]);
    b.default_scene(s);

    let mesh = decode_gltf(&b.build());

    let read = mesh.colors.clone().expect("COLOR_0 read");
    assert_eq!(read.len(), 6, "both nodes contribute their vertices");
    for (i, c) in read.iter().take(3).enumerate() {
        assert_vec4(*c, Vec4::new(1.0, 0.0, 0.2, 1.0), &format!("u8 colour {i}"));
    }
    for (i, c) in read.iter().skip(3).enumerate() {
        assert_vec4(
            *c,
            Vec4::new(0.0, 1.0, 0.0, 1.0),
            &format!("u16 colour {i}"),
        );
    }
    let buffer = mesh.create_vertex_buffer();
    let gpu = buffer_floats(&buffer, 0, COLOUR_OFFSET, 4);
    assert!(
        (gpu[2] - 0.2).abs() < EPS,
        "u8 colour in the GPU buffer: {gpu:?}"
    );
}

#[test]
fn attribute_missing_from_one_primitive_is_filled_with_its_default() {
    let mut b = GltfBuilder::new();
    // First primitive: colour, no UV.
    let pos_a = b.positions(&TRI);
    let nrm_a = b.vec3(&UP_Z);
    let col_a = b.vec4(&[[1.0, 0.0, 0.0, 1.0]; 3]);
    let idx_a = b.indices(&[0, 1, 2]);
    // Second primitive: UV, no colour.
    let pos_b = b.positions(&OTHER_TRI);
    let nrm_b = b.vec3(&UP_Z);
    let uv_b = b.vec2(&[[0.25, 0.5], [0.75, 0.5], [0.25, 1.0]]);
    let idx_b = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![
        primitive(
            &[("POSITION", pos_a), ("NORMAL", nrm_a), ("COLOR_0", col_a)],
            Some(idx_a),
        ),
        primitive(
            &[("POSITION", pos_b), ("NORMAL", nrm_b), ("TEXCOORD_0", uv_b)],
            Some(idx_b),
        ),
    ]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.positions.len(), 6, "both primitives are merged");
    let colours = mesh
        .colors
        .clone()
        .expect("colour present in one primitive");
    assert_eq!(colours.len(), 6, "colours stay aligned with positions");
    for (i, c) in colours.iter().enumerate().take(3) {
        assert_vec4(
            *c,
            Vec4::new(1.0, 0.0, 0.0, 1.0),
            &format!("authored colour {i}"),
        );
    }
    for (i, c) in colours.iter().enumerate().skip(3) {
        assert_vec4(
            *c,
            Vec4::new(1.0, 1.0, 1.0, 1.0),
            &format!("filled colour {i}"),
        );
    }
    let uvs = mesh
        .tex_coords
        .clone()
        .expect("UV present in one primitive");
    assert_eq!(uvs.len(), 6, "UVs stay aligned with positions");
    for (i, uv) in uvs.iter().enumerate().take(3) {
        assert_eq!(*uv, Vec2::new(0.0, 0.0), "filled UV {i}");
    }
    assert_eq!(uvs[3], Vec2::new(0.25, 0.5));
    assert_eq!(uvs[5], Vec2::new(0.25, 1.0));
}

#[test]
fn primitives_of_one_mesh_merge_with_offset_indices() {
    let mut b = GltfBuilder::new();
    let pos_a = b.positions(&TRI);
    let nrm_a = b.vec3(&UP_Z);
    let idx_a = b.indices(&[0, 1, 2]);
    let pos_b = b.positions(&QUAD);
    let nrm_b = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    let idx_b = b.indices(&QUAD_INDICES);
    let m = b.mesh(vec![
        primitive(&[("POSITION", pos_a), ("NORMAL", nrm_a)], Some(idx_a)),
        primitive(&[("POSITION", pos_b), ("NORMAL", nrm_b)], Some(idx_b)),
    ]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    let mut expected: Vec<Vec3> = TRI.iter().copied().map(v3).collect();
    expected.extend(QUAD.iter().copied().map(v3));
    assert_positions(&mesh, &expected);
    assert_eq!(mesh.indices, Some(vec![0, 1, 2, 3, 4, 5, 3, 5, 6]));
    assert_eq!(mesh.primitive_type, PrimitiveTopology::TriangleList);
}

/// glTF requires every attribute accessor of a primitive to hold as many
/// elements as `POSITION`. A file breaking that is refused, or its normals
/// stay one per vertex — never shifted onto the next primitive's vertices.
#[test]
fn normal_count_unlike_the_position_count_does_not_shift_the_next_primitive() {
    let mut b = GltfBuilder::new();
    let pos_a = b.positions(&TRI);
    // Two normals for three positions.
    let nrm_a = b.vec3(&[[0.0, 0.0, 1.0]; 2]);
    let idx_a = b.indices(&[0, 1, 2]);
    let pos_b = b.positions(&OTHER_TRI);
    let nrm_b = b.vec3(&[[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let idx_b = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![
        primitive(&[("POSITION", pos_a), ("NORMAL", nrm_a)], Some(idx_a)),
        primitive(&[("POSITION", pos_b), ("NORMAL", nrm_b)], Some(idx_b)),
    ]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let Ok(mesh) = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(&b.build()) else {
        return; // refused: fine
    };

    let normals = mesh.normals.clone().expect("normals read");
    assert_eq!(
        normals.len(),
        mesh.positions.len(),
        "one normal per vertex: {normals:?}"
    );
    assert_vec3(
        normals[3],
        Vec3::new(0.0, 0.0, -1.0),
        "second primitive, normal 0",
    );
    assert_vec3(normals[4], Vec3::X, "second primitive, normal 1");
    assert_vec3(normals[5], Vec3::Y, "second primitive, normal 2");
}

// ─── Scene and node transforms ──────────────────────────────────────────

#[test]
fn two_nodes_sharing_a_mesh_place_two_translated_copies() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    let first = b.node(json!({ "mesh": m, "translation": [10.0, 0.0, 0.0] }));
    let second = b.node(json!({ "mesh": m, "translation": [0.0, 0.0, -5.0] }));
    let s = b.scene(&[first, second]);
    b.default_scene(s);

    let mesh = decode_gltf(&b.build());

    let mut expected = translated(&TRI, [10.0, 0.0, 0.0]);
    expected.extend(translated(&TRI, [0.0, 0.0, -5.0]));
    assert_positions(&mesh, &expected);
    assert_eq!(mesh.indices, Some(vec![0, 1, 2, 3, 4, 5]));
    let normals = mesh.normals.clone().expect("normals read");
    assert_eq!(normals.len(), 6);
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(
            *n,
            Vec3::Z,
            &format!("normal {i} is unchanged by a translation"),
        );
    }
}

#[test]
fn nested_node_applies_the_child_transform_then_the_parent() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    let child = b.node(json!({ "mesh": m, "translation": [1.0, 0.0, 0.0] }));
    let parent = b.node(json!({ "children": [child], "scale": [2.0, 2.0, 2.0] }));
    let s = b.scene(&[parent]);
    b.default_scene(s);

    let mesh = decode_gltf(&b.build());

    // world = parent * child: translate by +1 X, then scale by 2.
    assert_positions(
        &mesh,
        &[
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
        ],
    );
}

#[test]
fn node_matrix_is_read_column_major() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    single_node_scene(
        &mut b,
        json!({
            "mesh": m,
            "matrix": [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                3.0, 4.0, 5.0, 1.0
            ]
        }),
    );

    let mesh = decode_gltf(&b.build());

    assert_positions(&mesh, &translated(&TRI, [3.0, 4.0, 5.0]));
}

#[test]
fn rotated_node_rotates_positions_and_normals() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&[[1.0, 0.0, 0.0]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm)],
        Some(idx),
    )]);
    // 90 degrees about +Z: X -> Y, Y -> -X.
    single_node_scene(
        &mut b,
        json!({ "mesh": m, "rotation": [0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2] }),
    );

    let mesh = decode_gltf(&b.build());

    assert_positions(
        &mesh,
        &[
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ],
    );
    let normals = mesh.normals.clone().expect("normals read");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::Y, &format!("normal {i}"));
    }
}

#[test]
fn non_uniform_scale_transforms_normals_by_the_inverse_transpose() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&[[FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm)],
        Some(idx),
    )]);
    single_node_scene(&mut b, json!({ "mesh": m, "scale": [2.0, 1.0, 1.0] }));

    let mesh = decode_gltf(&b.build());

    // inverse-transpose of diag(2,1,1) is diag(0.5,1,1); renormalised.
    let expected = Vec3::new(0.5, 1.0, 0.0).normalize();
    let normals = mesh.normals.clone().expect("normals read");
    for (i, n) in normals.iter().enumerate() {
        assert!(
            (n.length() - 1.0).abs() < EPS,
            "normal {i} is not unit: {n:?}"
        );
        assert_vec3(*n, expected, &format!("normal {i}"));
    }
    assert_vec3(
        mesh.positions[1],
        Vec3::new(2.0, 0.0, 0.0),
        "scaled position",
    );
}

/// A centimetre-to-metre root (scale 0.01) is common in exported files; the
/// node's rotation must still reach the normals however small the scale.
#[test]
fn small_uniform_scale_still_rotates_normals() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    // 90 degrees about +X: +Z -> -Y; scaled to 1/100.
    single_node_scene(
        &mut b,
        json!({
            "mesh": m,
            "rotation": [FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2],
            "scale": [0.01, 0.01, 0.01],
        }),
    );

    let mesh = decode_gltf(&b.build());

    let normals = mesh.normals.clone().expect("normals read");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::new(0.0, -1.0, 0.0), &format!("normal {i}"));
    }
}

#[test]
fn mirroring_node_flips_the_winding() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    single_node_scene(&mut b, json!({ "mesh": m, "scale": [-1.0, 1.0, 1.0] }));

    let mesh = decode_gltf(&b.build());

    assert!(
        mesh.positions
            .iter()
            .any(|p| close3(*p, Vec3::new(-1.0, 0.0, 0.0))),
        "the mirror is applied to the positions: {:?}",
        mesh.positions
    );
    let normals = mesh.normals.clone().expect("normals read");
    let indices = mesh.indices.clone().expect("indexed");
    for (t, corners) in triangles(&mesh).into_iter().enumerate() {
        let face = face_normal(corners);
        for k in 0..3 {
            let vertex_normal = normals[indices[t * 3 + k] as usize];
            assert!(
                face.dot(vertex_normal) > 0.0,
                "triangle {t}: front face {face:?} disagrees with vertex normal {vertex_normal:?}"
            );
        }
    }
}

#[test]
fn flagged_default_scene_is_the_one_decoded() {
    let mut b = GltfBuilder::new();
    let m0 = with_triangle_mesh(&mut b, &TRI);
    let m1 = with_triangle_mesh(&mut b, &OTHER_TRI);
    let n0 = b.node(json!({ "mesh": m0 }));
    let n1 = b.node(json!({ "mesh": m1 }));
    b.scene(&[n0]);
    let flagged = b.scene(&[n1]);
    b.default_scene(flagged);

    let mesh = decode_gltf(&b.build());

    let expected: Vec<Vec3> = OTHER_TRI.iter().copied().map(v3).collect();
    assert_positions(&mesh, &expected);
}

#[test]
fn first_scene_is_decoded_when_none_is_flagged() {
    let mut b = GltfBuilder::new();
    let m0 = with_triangle_mesh(&mut b, &TRI);
    let m1 = with_triangle_mesh(&mut b, &OTHER_TRI);
    let n0 = b.node(json!({ "mesh": m0 }));
    let n1 = b.node(json!({ "mesh": m1 }));
    b.scene(&[n1]);
    b.scene(&[n0]);

    let mesh = decode_gltf(&b.build());

    let expected: Vec<Vec3> = OTHER_TRI.iter().copied().map(v3).collect();
    assert_positions(&mesh, &expected);
}

#[test]
fn file_without_scenes_places_every_mesh_once_at_identity() {
    let mut b = GltfBuilder::new();
    with_triangle_mesh(&mut b, &TRI);
    with_triangle_mesh(&mut b, &OTHER_TRI);

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.positions.len(), 6, "every mesh contributes once");
    let found = triangles(&mesh);
    assert_eq!(found.len(), 2);
    for wanted in [TRI, OTHER_TRI] {
        let wanted = [v3(wanted[0]), v3(wanted[1]), v3(wanted[2])];
        assert!(
            found.iter().any(|t| same_winding(*t, wanted)),
            "triangle {wanted:?} missing or moved: {found:?}"
        );
    }
}

#[test]
fn bounding_box_encloses_the_transformed_positions() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    let first = b.node(json!({ "mesh": m, "translation": [10.0, 0.0, 0.0] }));
    let second = b.node(json!({ "mesh": m, "translation": [0.0, 0.0, -5.0] }));
    let s = b.scene(&[first, second]);
    b.default_scene(s);

    let mesh = decode_gltf(&b.build());

    assert_vec3(mesh.bounding_box.min, Vec3::new(0.0, 0.0, -5.0), "aabb min");
    assert_vec3(mesh.bounding_box.max, Vec3::new(11.0, 1.0, 0.0), "aabb max");
}

/// A node that is its own descendant (glTF forbids it, a corrupt file can
/// still say it) is a decode error, not a stack overflow that aborts the
/// process. The overflow cannot be caught in-process, so the decode runs in a
/// child test process and the parent checks that it exited normally.
#[test]
fn node_that_is_its_own_descendant_is_refused() {
    const CHILD: &str = "KHORA_GLTF_CYCLE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut b = GltfBuilder::new();
        let m = with_triangle_mesh(&mut b, &TRI);
        // node 0 -> node 1 -> node 0
        b.node(json!({ "mesh": m, "children": [1] }));
        b.node(json!({ "children": [0] }));
        let s = b.scene(&[0]);
        b.default_scene(s);
        let result = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(&b.build());
        assert!(result.is_err(), "a cyclic node graph decodes");
        return;
    }
    let name = concat!(
        module_path!(),
        "::node_that_is_its_own_descendant_is_refused"
    );
    let name = name.split_once("::").map_or(name, |(_, rest)| rest);
    let status = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([name, "--exact", "--test-threads=1"])
        .env(CHILD, "1")
        .status()
        .expect("child test process starts");
    assert!(
        status.success(),
        "decoding a cyclic node graph did not return an error (child exit: {status})"
    );
}

// ─── Topology, indices, normals ─────────────────────────────────────────

#[test]
fn non_indexed_primitive_gets_sequential_indices() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let m = b.mesh(vec![primitive(&[("POSITION", pos), ("NORMAL", nrm)], None)]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.indices, Some(vec![0, 1, 2]));
}

#[test]
fn primitive_without_normals_gets_flat_normals() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&QUAD);
    let idx = b.indices(&QUAD_INDICES);
    let m = b.mesh(vec![primitive(&[("POSITION", pos)], Some(idx))]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    let normals = mesh.normals.clone().expect("flat normals generated");
    assert_eq!(mesh.positions.len(), 6, "each triangle owns its vertices");
    assert_eq!(normals.len(), 6);
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::Z, &format!("normal {i}"));
    }
}

#[test]
fn flat_normals_are_computed_after_the_node_transform() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&QUAD);
    let idx = b.indices(&QUAD_INDICES);
    let m = b.mesh(vec![primitive(&[("POSITION", pos)], Some(idx))]);
    // 90 degrees about +X: +Z -> -Y.
    single_node_scene(
        &mut b,
        json!({ "mesh": m, "rotation": [FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2] }),
    );

    let mesh = decode_gltf(&b.build());

    let normals = mesh.normals.clone().expect("flat normals generated");
    assert_eq!(normals.len(), mesh.positions.len());
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::new(0.0, -1.0, 0.0), &format!("normal {i}"));
    }
}

#[test]
fn triangle_strip_becomes_a_triangle_list() {
    let strip = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let mut b = GltfBuilder::new();
    let pos = b.positions(&strip);
    let nrm = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    let m = b.mesh(vec![primitive_with_mode(
        &[("POSITION", pos), ("NORMAL", nrm)],
        None,
        MODE_TRIANGLE_STRIP,
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.primitive_type, PrimitiveTopology::TriangleList);
    let found = triangles(&mesh);
    let v = |i: usize| v3(strip[i]);
    let expected = [[v(0), v(1), v(2)], [v(1), v(3), v(2)]];
    assert_eq!(found.len(), 2, "a 4-vertex strip is two triangles");
    for (k, (f, e)) in found.iter().zip(&expected).enumerate() {
        assert!(same_winding(*f, *e), "triangle {k}: {f:?} != {e:?}");
    }
}

#[test]
fn triangle_fan_becomes_a_triangle_list() {
    let fan = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut b = GltfBuilder::new();
    let pos = b.positions(&fan);
    let nrm = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    let m = b.mesh(vec![primitive_with_mode(
        &[("POSITION", pos), ("NORMAL", nrm)],
        None,
        MODE_TRIANGLE_FAN,
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.primitive_type, PrimitiveTopology::TriangleList);
    let found = triangles(&mesh);
    let v = |i: usize| v3(fan[i]);
    let expected = [[v(1), v(2), v(0)], [v(2), v(3), v(0)]];
    assert_eq!(found.len(), 2, "a 4-vertex fan is two triangles");
    for (k, (f, e)) in found.iter().zip(&expected).enumerate() {
        assert!(same_winding(*f, *e), "triangle {k}: {f:?} != {e:?}");
    }
}

#[test]
fn scene_without_triangles_keeps_the_first_primitive_and_its_topology() {
    let mut b = GltfBuilder::new();
    let line = b.positions(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let point = b.positions(&[[7.0, 7.0, 7.0]]);
    let m = b.mesh(vec![
        primitive_with_mode(&[("POSITION", line)], None, MODE_LINES),
        primitive_with_mode(&[("POSITION", point)], None, MODE_POINTS),
    ]);
    single_node_scene(&mut b, json!({ "mesh": m, "translation": [0.0, 2.0, 0.0] }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.primitive_type, PrimitiveTopology::LineList);
    assert_positions(&mesh, &[Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 2.0, 0.0)]);
}

#[test]
fn lines_beside_triangles_are_skipped() {
    let mut b = GltfBuilder::new();
    let line = b.positions(&[[9.0, 9.0, 9.0], [8.0, 9.0, 9.0]]);
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![
        primitive_with_mode(&[("POSITION", line)], None, MODE_LINES),
        primitive(&[("POSITION", pos), ("NORMAL", nrm)], Some(idx)),
    ]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.primitive_type, PrimitiveTopology::TriangleList);
    let expected: Vec<Vec3> = TRI.iter().copied().map(v3).collect();
    assert_positions(&mesh, &expected);
    assert_eq!(mesh.indices, Some(vec![0, 1, 2]));
}

#[test]
fn index_past_the_vertex_count_is_refused() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&UP_Z);
    let idx = b.indices(&[0, 1, 3]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm)],
        Some(idx),
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let result = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(&b.build());

    assert!(result.is_err(), "index 3 of a 3-vertex primitive decodes");
}

/// The flat normals of a mirrored primitive are computed after the winding
/// flip, so they face the same way as the (mirrored) front face.
#[test]
fn mirroring_node_without_normals_gets_flat_normals_on_the_front_face() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(&[("POSITION", pos)], Some(idx))]);
    single_node_scene(&mut b, json!({ "mesh": m, "scale": [-1.0, 1.0, 1.0] }));

    let mesh = decode_gltf(&b.build());

    let normals = mesh.normals.clone().expect("flat normals generated");
    let indices = mesh.indices.clone().expect("indexed");
    for (t, corners) in triangles(&mesh).into_iter().enumerate() {
        let face = face_normal(corners);
        assert!(
            face.z > 0.0,
            "a mirror in X keeps the face seen from +Z: {face:?}"
        );
        for k in 0..3 {
            let n = normals[indices[t * 3 + k] as usize];
            assert_vec3(n, Vec3::Z, &format!("triangle {t} corner {k}"));
        }
    }
}

/// A mirror under a mirror is no mirror: the winding is left as authored.
#[test]
fn two_nested_mirrors_keep_the_winding() {
    let mut b = GltfBuilder::new();
    let m = with_triangle_mesh(&mut b, &TRI);
    let child = b.node(json!({ "mesh": m, "scale": [1.0, -1.0, 1.0] }));
    let parent = b.node(json!({ "children": [child], "scale": [-1.0, 1.0, 1.0] }));
    let s = b.scene(&[parent]);
    b.default_scene(s);

    let mesh = decode_gltf(&b.build());

    // diag(-1,-1,1) is a half-turn about Z: the front face still looks along +Z.
    let normals = mesh.normals.clone().expect("normals read");
    for (t, corners) in triangles(&mesh).into_iter().enumerate() {
        let face = face_normal(corners);
        assert!(face.z > 0.0, "triangle {t} turned its back: {face:?}");
    }
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::Z, &format!("normal {i}"));
    }
}

#[test]
fn line_loop_is_a_strip_closed_onto_its_first_vertex() {
    const MODE_LINE_LOOP: u32 = 2;
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let m = b.mesh(vec![primitive_with_mode(
        &[("POSITION", pos)],
        None,
        MODE_LINE_LOOP,
    )]);
    single_node_scene(&mut b, json!({ "mesh": m }));

    let mesh = decode_gltf(&b.build());

    assert_eq!(mesh.primitive_type, PrimitiveTopology::LineStrip);
    assert_eq!(mesh.indices, Some(vec![0, 1, 2, 0]));
}
