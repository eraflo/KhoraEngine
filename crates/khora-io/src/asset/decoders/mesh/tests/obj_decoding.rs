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

//! An OBJ file decodes to the engine mesh whatever attributes it carries.

use super::*;

/// A unit quad in the XY plane, counter-clockwise seen from +Z, positions only.
const QUAD_POSITIONS_ONLY: &str = "\
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
f 1 2 3 4
";

/// A triangle with normals but no texture coordinates.
const TRIANGLE_WITHOUT_UV: &str = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 1
f 1//1 2//1 3//1
";

#[test]
fn obj_without_normals_gets_flat_normals() {
    let mesh = decode_obj(QUAD_POSITIONS_ONLY);

    let normals = mesh.normals.clone().expect("flat normals generated");
    assert_eq!(normals.len(), mesh.positions.len());
    assert_eq!(mesh.positions.len(), 6, "each triangle owns its vertices");
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::Z, &format!("normal {i}"));
    }
    for (t, corners) in triangles(&mesh).into_iter().enumerate() {
        assert!(
            face_normal(corners).dot(Vec3::Z) > 0.0,
            "triangle {t} lost its counter-clockwise winding"
        );
    }
}

#[test]
fn obj_without_uv_still_builds_the_full_mesh_vertex() {
    let mesh = decode_obj(TRIANGLE_WITHOUT_UV);

    assert!(mesh.tex_coords.is_none());
    let buffer = mesh.create_vertex_buffer();
    assert_eq!(
        buffer.len(),
        mesh.positions.len() * Mesh::VERTEX_STRIDE as usize
    );
    for v in 0..mesh.positions.len() {
        assert_eq!(
            buffer_floats(&buffer, v, 12, 3),
            vec![0.0, 0.0, 1.0],
            "normal {v}"
        );
        assert_eq!(buffer_floats(&buffer, v, 24, 2), vec![0.0, 0.0], "uv {v}");
        assert_eq!(
            buffer_floats(&buffer, v, COLOUR_OFFSET, 4),
            vec![1.0, 1.0, 1.0, 1.0],
            "colour {v}"
        );
    }
}
