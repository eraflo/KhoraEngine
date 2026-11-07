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

//! The shared reader decodes a project's real files to the values the
//! service's `load` returns: a texture, a mesh, a glTF decoded whole and its
//! parts. It names the files a glTF reads beside its own.

use super::gltf_builder::{png, primitive, with_material, GltfBuilder};
use crate::asset::{AssetIdRegistry, AssetService, Decoded};
use khora_core::asset::{Asset, AssetUUID, Material, StandardMaterial};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_telemetry::MetricsRegistry;
use serde_json::json;
use std::any::TypeId;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const PIXEL: [u8; 4] = [10, 20, 30, 255];

const TRIANGLE_OBJ: &str = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 1
f 1//1 2//1 3//1
";

/// A project on disk: `<tmp>/project/assets/…`.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    assets: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().join("project");
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).expect("the assets folder");
        Self {
            _dir: dir,
            root,
            assets,
        }
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.assets.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent folder");
        }
        std::fs::write(&path, bytes).expect("the file is written");
    }

    fn open(&self) -> AssetService {
        AssetService::open_loose_files(
            &self.root,
            &AssetIdRegistry::load(&self.root),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens")
    }
}

fn file_uuid(rel: &str) -> AssetUUID {
    AssetUUID::new_v5(rel)
}

fn sub(rel: &str, label: &str) -> AssetUUID {
    AssetUUID::sub_asset(file_uuid(rel), label)
}

/// The value `decoded` holds, as an `A`; panics naming `what` otherwise.
fn decoded_as<A: Asset>(decoded: Decoded, what: &str) -> Box<A> {
    decoded
        .value
        .unwrap_or_else(|e| panic!("{what} decodes through the reader: {e}"))
        .downcast::<A>()
        .unwrap_or_else(|_| panic!("{what} decodes as a {}", std::any::type_name::<A>()))
}

/// Decodes `uuid` as an `A` through the service's reader, then loads it, and
/// returns both values' debug forms: equal when the two paths agree.
fn through_both<A: Asset + std::fmt::Debug>(
    svc: &mut AssetService,
    uuid: AssetUUID,
    what: &str,
) -> (String, String) {
    let decoded = decoded_as::<A>(svc.reader().decode(&uuid, TypeId::of::<A>()), what);
    let loaded = svc
        .load::<A>(&uuid)
        .unwrap_or_else(|e| panic!("{what} loads: {e:#}"));
    (format!("{:?}", *decoded), format!("{:?}", *loaded))
}

/// One triangle in a red material whose base colour is a 3×2 PNG held in
/// the file's buffer.
fn textured_triangle_file() -> GltfBuilder {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let nrm = b.vec3(&[[0.0, 0.0, 1.0]; 3]);
    let idx = b.indices(&[0, 1, 2]);
    let tri = primitive(&[("POSITION", pos), ("NORMAL", nrm)], Some(idx));
    let image = b.image_in_buffer(&png(3, 2, PIXEL), "image/png");
    let texture = b.texture(image, None);
    let red = b.material(json!({
        "name": "Red",
        "pbrMetallicRoughness": {
            "baseColorFactor": [0.8, 0.1, 0.1, 1.0],
            "baseColorTexture": { "index": texture },
        },
    }));
    let m = b.mesh(vec![with_material(tri, red)]);
    let n = b.node(json!({ "mesh": m }));
    let s = b.scene(&[n]);
    b.default_scene(s);
    b
}

#[test]
fn a_reader_decodes_a_loose_texture_to_the_value_load_returns() {
    let project = Project::new();
    project.write("textures/a.png", &png(3, 2, PIXEL));
    let mut svc = project.open();

    let (decoded, loaded) =
        through_both::<CpuTexture>(&mut svc, file_uuid("textures/a.png"), "the PNG");
    assert_eq!(decoded, loaded);
}

#[test]
fn a_reader_decodes_a_loose_mesh_to_the_value_load_returns() {
    let project = Project::new();
    project.write("models/tri.obj", TRIANGLE_OBJ.as_bytes());
    project.write("models/tri.gltf", &textured_triangle_file().build());
    let mut svc = project.open();

    let (decoded, loaded) = through_both::<Mesh>(&mut svc, file_uuid("models/tri.obj"), "the OBJ");
    assert_eq!(decoded, loaded);
    let (decoded, loaded) =
        through_both::<Mesh>(&mut svc, file_uuid("models/tri.gltf"), "the glTF whole");
    assert_eq!(decoded, loaded);
}

#[test]
fn a_reader_decodes_the_parts_of_a_gltf_to_the_values_load_returns() {
    const FILE: &str = "models/tri.glb";
    let project = Project::new();
    project.write(FILE, &textured_triangle_file().build_glb());
    let mut svc = project.open();

    let (decoded, loaded) = through_both::<Mesh>(&mut svc, sub(FILE, "mesh0/prim0"), "the mesh");
    assert_eq!(decoded, loaded);
    let (decoded, loaded) =
        through_both::<CpuTexture>(&mut svc, sub(FILE, "texture0"), "the texture");
    assert_eq!(decoded, loaded);

    let material = decoded_as::<Box<dyn Material>>(
        svc.reader()
            .decode(&sub(FILE, "material0"), TypeId::of::<Box<dyn Material>>()),
        "the material",
    );
    let loaded = svc
        .load::<Box<dyn Material>>(&sub(FILE, "material0"))
        .expect("the material loads");
    let standard = |material: &dyn Material| {
        format!(
            "{:?}",
            material
                .as_any()
                .downcast_ref::<StandardMaterial>()
                .expect("a glTF material is a StandardMaterial")
        )
    };
    assert_eq!(standard(&**material), standard(&**loaded));
}

#[test]
fn a_reader_names_the_files_a_gltf_reads_beside_its_own() {
    const FILE: &str = "models/thing.gltf";
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI);
    let idx = b.indices(&[0, 1, 2]);
    let tri = primitive(&[("POSITION", pos)], Some(idx));
    let m = b.mesh(vec![tri]);
    let n = b.node(json!({ "mesh": m }));
    let s = b.scene(&[n]);
    b.default_scene(s);
    let (json, bin) = b.build_with_buffer_uri("thing.bin");
    let project = Project::new();
    project.write(FILE, &json);
    project.write("models/thing.bin", &bin);
    let svc = project.open();
    let reader = svc.reader();
    let beside = vec![(file_uuid(FILE), vec!["models/thing.bin".to_string()])];

    let part = reader.decode(&sub(FILE, "mesh0/prim0"), TypeId::of::<Mesh>());
    assert_eq!(
        part.external_files, beside,
        "a part names its container's files"
    );
    decoded_as::<Mesh>(part, "the part");

    let whole = reader.decode(&file_uuid(FILE), TypeId::of::<Mesh>());
    assert_eq!(
        whole.external_files, beside,
        "the file decoded whole names them too"
    );
    decoded_as::<Mesh>(whole, "the whole file");

    project.write("models/plain.obj", TRIANGLE_OBJ.as_bytes());
    let svc = project.open();
    let plain = svc
        .reader()
        .decode(&file_uuid("models/plain.obj"), TypeId::of::<Mesh>());
    assert!(
        plain.external_files.is_empty(),
        "an OBJ reads nothing beside it: {:?}",
        plain.external_files
    );
    decoded_as::<Mesh>(plain, "the OBJ");
}
