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

//! Mesh decoders normalise every file to the engine mesh: the glTF default
//! scene baked into one mesh, defaults filled, normals generated when absent.

mod gltf_builder;
mod gltf_decoding;
mod obj_decoding;

use super::{GltfDecoder, NoOpResourceResolver, ObjDecoder};
use crate::asset::AssetDecoder;
use khora_core::math::{Vec3, Vec4};
use khora_core::renderer::api::gpu_scene::Mesh;
use std::sync::Arc;

const EPS: f32 = 1e-4;

fn decode_gltf(bytes: &[u8]) -> Mesh {
    GltfDecoder::new(Arc::new(NoOpResourceResolver))
        .load(bytes)
        .unwrap_or_else(|e| panic!("glTF decode failed: {e}"))
}

fn decode_obj(text: &str) -> Mesh {
    ObjDecoder
        .load(text.as_bytes())
        .unwrap_or_else(|e| panic!("OBJ decode failed: {e}"))
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn close3(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < EPS
}

fn assert_vec3(actual: Vec3, expected: Vec3, what: &str) {
    assert!(
        close3(actual, expected),
        "{what}: {actual:?} != {expected:?}"
    );
}

fn assert_vec4(actual: Vec4, expected: Vec4, what: &str) {
    let d = [
        actual.x - expected.x,
        actual.y - expected.y,
        actual.z - expected.z,
        actual.w - expected.w,
    ];
    assert!(
        d.iter().all(|c| c.abs() < EPS),
        "{what}: {actual:?} != {expected:?}"
    );
}

fn assert_positions(mesh: &Mesh, expected: &[Vec3]) {
    assert_eq!(
        mesh.positions.len(),
        expected.len(),
        "vertex count: {:?}",
        mesh.positions
    );
    for (i, (a, e)) in mesh.positions.iter().zip(expected).enumerate() {
        assert_vec3(*a, *e, &format!("position {i}"));
    }
}

/// The corner positions of every triangle, in index order. The mesh must be
/// an indexed triangle list.
fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
    let indices = mesh
        .indices
        .as_ref()
        .expect("a decoded triangle mesh is indexed");
    assert_eq!(
        indices.len() % 3,
        0,
        "index count is not a whole number of triangles"
    );
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

/// Same corners, same winding: one triangle is a rotation of the other.
fn same_winding(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    (0..3).any(|r| (0..3).all(|i| close3(a[i], b[(i + r) % 3])))
}

fn face_normal(t: [Vec3; 3]) -> Vec3 {
    (t[1] - t[0]).cross(t[2] - t[0])
}

/// The `n` floats stored at byte `offset` of vertex `vertex` in a mesh buffer.
fn buffer_floats(buffer: &[u8], vertex: usize, offset: usize, n: usize) -> Vec<f32> {
    let start = vertex * Mesh::VERTEX_STRIDE as usize + offset;
    (0..n)
        .map(|i| {
            let at = start + i * 4;
            f32::from_ne_bytes([buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]])
        })
        .collect()
}

/// Byte offset of the vertex colour in the engine mesh vertex.
const COLOUR_OFFSET: usize = 32;
