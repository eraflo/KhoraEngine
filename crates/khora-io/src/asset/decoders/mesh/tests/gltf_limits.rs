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

//! A glTF decodes within the limits it is given: its decoded vertices
//! against `max_elements`, in the decoder, the mesh dispatcher and the
//! container alike.

use super::gltf_builder::{primitive, GltfBuilder};
use super::*;
use crate::asset::decoders::mesh::{GltfContainer, MeshDispatcher};
use crate::asset::AssetContainer;
use crate::trust::{FileKind, LimitExceeded, Limits, Trust};
use khora_core::asset::AssetUUID;
use serde_json::json;
use std::error::Error;
use std::path::Path;

/// A right triangle in the XY plane: three decoded vertices.
const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

/// A `.glb` showing one indexed triangle in its default scene.
fn triangle_glb() -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&[[0.0, 0.0, 1.0]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.mesh(vec![primitive(
        &[("POSITION", pos), ("NORMAL", nrm)],
        Some(idx),
    )]);
    let n = b.node(json!({ "mesh": m }));
    let s = b.scene(&[n]);
    b.default_scene(s);
    b.build_glb()
}

/// A trusted project's glTF limits, with at most `elements` decoded
/// vertices.
fn at_most(elements: u64) -> Limits {
    Limits {
        max_elements: elements,
        ..Limits::of(FileKind::Gltf, Trust::Project)
    }
}

/// The `LimitExceeded` an error is, or holds among its sources.
fn exceeded<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a LimitExceeded> {
    let mut next = Some(error);
    while let Some(error) = next {
        if let Some(limit) = error.downcast_ref::<LimitExceeded>() {
            return Some(limit);
        }
        next = error.source();
    }
    None
}

/// **A glTF past its element limit is refused by the decoder and the
/// dispatcher**: a triangle's three vertices against `max_elements` 2 stop
/// the decode with a `LimitExceeded` naming 2; at a trusted project's
/// limits the same file decodes its three vertices.
#[test]
fn a_gltf_past_its_element_limit_is_refused_by_the_decoder() {
    let file = triangle_glb();
    let decoder = GltfDecoder::new(Arc::new(NoOpResourceResolver));
    let dispatcher = MeshDispatcher::default();

    let refusals = [
        (
            "decoder",
            decoder.load_limited(&file, Path::new("models/tri.glb"), &at_most(2)),
        ),
        (
            "dispatcher",
            dispatcher.load_limited(&file, Path::new("models/tri.glb"), &at_most(2)),
        ),
    ];
    for (who, result) in refusals {
        let refused = result.expect_err("three vertices are past a limit of two");
        let limit =
            exceeded(&*refused).unwrap_or_else(|| panic!("{who}: a LimitExceeded, not: {refused}"));
        assert_eq!(limit.limit, 2, "{who}");
    }

    let limits = Limits::of(FileKind::Gltf, Trust::Project);
    for (who, result) in [
        (
            "decoder",
            decoder.load_limited(&file, Path::new("models/tri.glb"), &limits),
        ),
        (
            "dispatcher",
            dispatcher.load_limited(&file, Path::new("models/tri.glb"), &limits),
        ),
    ] {
        let mesh = result
            .unwrap_or_else(|e| panic!("{who}: the triangle decodes at a project's limits: {e}"));
        assert_eq!(mesh.positions.len(), 3, "{who}");
    }
}

/// **A glTF container opened within a limit holds its parts to it**: opened
/// with `max_elements` 2, the triangle's primitive is refused with a
/// `LimitExceeded` — when the container opens or when the primitive is
/// extracted; opened at a trusted project's limits, the primitive extracts
/// its three vertices.
#[test]
fn a_gltf_container_opened_within_a_limit_holds_its_parts_to_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let file = triangle_glb();
    let uuid = AssetUUID::new_v5("models/tri.glb");

    let refused: anyhow::Error =
        match container.open_limited(&file, Path::new("models/tri.glb"), &at_most(2)) {
            Err(error) => error,
            Ok(opened) => container
                .extract(opened.as_ref(), uuid, "mesh0/prim0", "mesh")
                .expect_err("a primitive of three vertices is past a limit of two"),
        };
    let limit = refused
        .chain()
        .find_map(|error| error.downcast_ref::<LimitExceeded>())
        .unwrap_or_else(|| panic!("a LimitExceeded, not: {refused:#}"));
    assert_eq!(limit.limit, 2);

    let opened = container
        .open_limited(
            &file,
            Path::new("models/tri.glb"),
            &Limits::of(FileKind::Gltf, Trust::Project),
        )
        .unwrap_or_else(|e| panic!("the triangle opens at a project's limits: {e:#}"));
    let mesh = container
        .extract(opened.as_ref(), uuid, "mesh0/prim0", "mesh")
        .unwrap_or_else(|e| panic!("its primitive extracts: {e:#}"))
        .downcast::<Mesh>()
        .expect("a primitive is a Mesh");
    assert_eq!(mesh.positions.len(), 3);
}
