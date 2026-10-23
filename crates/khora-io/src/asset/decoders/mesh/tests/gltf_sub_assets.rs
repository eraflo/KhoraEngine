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

//! A glTF file holds sub-assets: its scenes, each primitive as a mesh, its
//! materials and its textures. The lister names them from the JSON alone;
//! the container extracts each one; external files resolve next to the glTF
//! file and never outside the assets root.

use super::gltf_builder::{
    png, primitive, primitive_with_mode, with_material, GltfBuilder, MODE_LINES, MODE_POINTS,
    MODE_TRIANGLE_FAN, MODE_TRIANGLE_STRIP,
};
use super::*;
use crate::asset::decoders::mesh::{
    list_gltf_sub_assets, FileSystemResolver, GltfContainer, MeshDispatcher,
};
use crate::asset::{AssetContainer, AssetIdRegistry, AssetService, IndexBuilder, SubAssetEntry};
use khora_core::asset::{
    AlphaMode, AssetHandle, AssetSource, AssetUUID, Material, StandardMaterial,
};
use khora_core::math::LinearRgba;
use khora_core::renderer::api::pipeline::PrimitiveTopology;
use khora_core::renderer::api::resource::CpuTexture;
use khora_telemetry::MetricsRegistry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ─── Fixtures ───────────────────────────────────────────────────────────

/// A right triangle in the XY plane, counter-clockwise seen from +Z.
const TRI_A: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// A second triangle, away from [`TRI_A`], so the two are told apart.
const TRI_B: [[f32; 3]; 3] = [[5.0, 5.0, 5.0], [6.0, 5.0, 5.0], [5.0, 6.0, 5.0]];
const UP_Z: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];

/// A unit quad in the XY plane, counter-clockwise seen from +Z.
const QUAD: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];
const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

const RED: [f32; 4] = [0.8, 0.1, 0.1, 1.0];
const BLUE: [f32; 4] = [0.1, 0.2, 0.9, 0.5];

/// The colour of the fixture images, and of the decoys beside them.
const PIXEL: [u8; 4] = [10, 20, 30, 255];
const DECOY_PIXEL: [u8; 4] = [200, 0, 200, 255];

const KHR_EMISSIVE_STRENGTH: &str = "KHR_materials_emissive_strength";
const KHR_TEXTURE_TRANSFORM: &str = "KHR_texture_transform";
const KHR_SPECULAR: &str = "KHR_materials_specular";

fn points(corners: &[[f32; 3]]) -> Vec<Vec3> {
    corners.iter().copied().map(v3).collect()
}

/// An indexed triangle primitive with +Z normals, no material.
fn triangle_primitive(b: &mut GltfBuilder, corners: &[[f32; 3]; 3]) -> Value {
    let pos = b.positions(corners);
    let nrm = b.vec3(&UP_Z);
    let idx = b.indices(&[0, 1, 2]);
    primitive(&[("POSITION", pos), ("NORMAL", nrm)], Some(idx))
}

/// An indexed [`QUAD`] primitive with +Z normals, no material.
fn quad_primitive(b: &mut GltfBuilder) -> Value {
    let pos = b.positions(&QUAD);
    let nrm = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    let idx = b.indices(&QUAD_INDICES);
    primitive(&[("POSITION", pos), ("NORMAL", nrm)], Some(idx))
}

/// One node showing a mesh of `primitives`, in the default scene.
fn single_mesh_file(b: &mut GltfBuilder, primitives: Vec<Value>) {
    let m = b.mesh(primitives);
    let n = b.node(json!({ "mesh": m }));
    let s = b.scene(&[n]);
    b.default_scene(s);
}

/// Two nodes, two materials, one texture:
/// - node 0 `Left`, translated by (10, 0, 0), shows mesh 0: [`TRI_A`] in
///   material 0, [`TRI_B`] in material 1;
/// - node 1 `Right` shows mesh 1: [`QUAD`] in material 1;
/// - material 0 is [`RED`] with texture 0 (a 3×2 PNG in a buffer view) as its
///   base colour; material 1 is [`BLUE`].
fn two_material_file() -> GltfBuilder {
    let mut b = GltfBuilder::new();
    let tri_a = triangle_primitive(&mut b, &TRI_A);
    let tri_b = triangle_primitive(&mut b, &TRI_B);
    let quad = quad_primitive(&mut b);
    let image = b.image_in_buffer(&png(3, 2, PIXEL), "image/png");
    let texture = b.texture(image, None);
    let red = b.material(json!({
        "name": "Red",
        "pbrMetallicRoughness": {
            "baseColorFactor": RED,
            "baseColorTexture": { "index": texture },
        },
    }));
    let blue = b.material(json!({
        "name": "Blue",
        "pbrMetallicRoughness": { "baseColorFactor": BLUE },
    }));
    let m0 = b.mesh(vec![with_material(tri_a, red), with_material(tri_b, blue)]);
    let m1 = b.mesh(vec![with_material(quad, blue)]);
    let left = b.node(json!({ "name": "Left", "mesh": m0, "translation": [10.0, 0.0, 0.0] }));
    let right = b.node(json!({ "name": "Right", "mesh": m1 }));
    let s = b.scene(&[left, right]);
    b.default_scene(s);
    b
}

/// The sub-assets of [`two_material_file`], in the lister's order.
const TWO_MATERIAL_SUBS: [(&str, &str); 8] = [
    ("scene", "prefab"),
    ("scene0", "prefab"),
    ("mesh0/prim0", "mesh"),
    ("mesh0/prim1", "mesh"),
    ("mesh1/prim0", "mesh"),
    ("material0", "material"),
    ("material1", "material"),
    ("texture0", "texture"),
];

fn entries(expected: &[(&str, &str)]) -> Vec<SubAssetEntry> {
    expected
        .iter()
        .map(|(label, type_name)| SubAssetEntry {
            label: (*label).to_string(),
            type_name: (*type_name).to_string(),
        })
        .collect()
}

fn list(bytes: &[u8]) -> Vec<SubAssetEntry> {
    list_gltf_sub_assets(bytes).unwrap_or_else(|e| panic!("the glTF lists: {e:#}"))
}

/// A project on disk: `<tmp>/project/assets/…`, with room for files beside
/// the project and beside its folder.
struct Project {
    dir: TempDir,
    root: PathBuf,
    assets: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().join("project");
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).expect("the assets folder");
        Self { dir, root, assets }
    }

    /// Writes `bytes` at `rel` under the assets folder.
    fn write(&self, rel: &str, bytes: &[u8]) {
        write_file(&self.assets.join(rel), bytes);
    }

    /// The asset service over the project as it is on disk now.
    fn open(&self) -> AssetService {
        AssetService::open_loose_files(
            &self.root,
            &AssetIdRegistry::load(&self.root),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens")
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent folder");
    }
    std::fs::write(path, bytes).expect("the file is written");
}

/// The uuid of the file at `rel` (no registry: the path default).
fn file_uuid(rel: &str) -> AssetUUID {
    AssetUUID::new_v5(rel)
}

/// The uuid of the sub-asset `label` of the file at `rel`.
fn sub(rel: &str, label: &str) -> AssetUUID {
    AssetUUID::sub_asset(file_uuid(rel), label)
}

fn load_mesh(svc: &mut AssetService, uuid: AssetUUID, what: &str) -> AssetHandle<Mesh> {
    svc.load::<Mesh>(&uuid)
        .unwrap_or_else(|e| panic!("{what} loads as a Mesh: {e:#}"))
}

fn load_standard(svc: &mut AssetService, uuid: AssetUUID, what: &str) -> StandardMaterial {
    let handle = svc
        .load::<Box<dyn Material>>(&uuid)
        .unwrap_or_else(|e| panic!("{what} loads as a material: {e:#}"));
    let material: &dyn Material = &**handle;
    material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .unwrap_or_else(|| panic!("{what} is a StandardMaterial"))
        .clone()
}

fn load_texture(svc: &mut AssetService, uuid: AssetUUID, what: &str) -> (u32, u32, Vec<u8>) {
    let texture = svc
        .load::<CpuTexture>(&uuid)
        .unwrap_or_else(|e| panic!("{what} loads as a texture: {e:#}"));
    (
        texture.size.width,
        texture.size.height,
        texture.pixels.clone(),
    )
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

fn assert_rgba(actual: LinearRgba, expected: [f32; 4], what: &str) {
    assert!(
        close(actual.r, expected[0])
            && close(actual.g, expected[1])
            && close(actual.b, expected[2])
            && close(actual.a, expected[3]),
        "{what}: {actual:?} != {expected:?}"
    );
}

fn assert_rgb(actual: LinearRgba, expected: [f32; 3], what: &str) {
    assert!(
        close(actual.r, expected[0])
            && close(actual.g, expected[1])
            && close(actual.b, expected[2]),
        "{what}: {actual:?} != {expected:?}"
    );
}

/// The glTF spec's material defaults: white, fully metallic, fully rough,
/// no emission, opaque, single-sided, untextured.
fn assert_spec_default_material(material: &StandardMaterial, what: &str) {
    assert_rgba(material.base_color, [1.0, 1.0, 1.0, 1.0], what);
    assert!(
        close(material.metallic, 1.0),
        "{what}: metallic {}",
        material.metallic
    );
    assert!(
        close(material.roughness, 1.0),
        "{what}: roughness {}",
        material.roughness
    );
    assert_rgb(material.emissive, [0.0, 0.0, 0.0], what);
    assert_eq!(material.alpha_mode, AlphaMode::Opaque, "{what}: alpha mode");
    assert!(!material.double_sided, "{what}: single-sided");
    assert_eq!(
        material.base_color_texture, None,
        "{what}: base colour texture"
    );
    assert_eq!(
        material.metallic_roughness_texture, None,
        "{what}: metallic-roughness texture"
    );
    assert_eq!(material.normal_map, None, "{what}: normal map");
    assert_eq!(material.occlusion_map, None, "{what}: occlusion map");
    assert_eq!(material.emissive_texture, None, "{what}: emissive texture");
}

// ─── Lister ─────────────────────────────────────────────────────────────

#[test]
fn lister_lists_scenes_primitives_materials_and_textures_in_order() {
    let listed = list(&two_material_file().build());
    assert_eq!(listed, entries(&TWO_MATERIAL_SUBS));
}

#[test]
fn lister_lists_a_glb_like_its_gltf_json() {
    let from_json = list(&two_material_file().build());
    let from_glb = list(&two_material_file().build_glb());
    assert_eq!(from_glb, from_json);
    assert_eq!(from_glb, entries(&TWO_MATERIAL_SUBS));
}

#[test]
fn lister_adds_material_default_after_the_materials_when_a_primitive_has_none() {
    let mut b = GltfBuilder::new();
    let tri_a = triangle_primitive(&mut b, &TRI_A);
    let tri_b = triangle_primitive(&mut b, &TRI_B);
    let image = b.image_in_buffer(&png(1, 1, PIXEL), "image/png");
    let texture = b.texture(image, None);
    let red = b.material(json!({
        "pbrMetallicRoughness": { "baseColorTexture": { "index": texture } },
    }));
    single_mesh_file(&mut b, vec![with_material(tri_a, red), tri_b]);

    assert_eq!(
        list(&b.build()),
        entries(&[
            ("scene", "prefab"),
            ("scene0", "prefab"),
            ("mesh0/prim0", "mesh"),
            ("mesh0/prim1", "mesh"),
            ("material0", "material"),
            ("material_default", "material"),
            ("texture0", "texture"),
        ])
    );
}

#[test]
fn lister_lists_material_default_only_when_a_primitive_has_no_material() {
    let listed = list(&two_material_file().build());
    assert!(
        listed.iter().all(|e| e.label != "material_default"),
        "every primitive has a material: {listed:?}"
    );
}

#[test]
fn lister_lists_the_default_scene_then_every_scene() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    let red = b.material(json!({}));
    let m = b.mesh(vec![with_material(tri, red)]);
    let first = b.node(json!({ "mesh": m }));
    let second = b.node(json!({ "mesh": m, "translation": [0.0, 3.0, 0.0] }));
    b.scene(&[first]);
    let s1 = b.scene(&[second]);
    b.default_scene(s1);

    assert_eq!(
        list(&b.build()),
        entries(&[
            ("scene", "prefab"),
            ("scene0", "prefab"),
            ("scene1", "prefab"),
            ("mesh0/prim0", "mesh"),
            ("material0", "material"),
        ])
    );
}

#[test]
fn lister_lists_one_scene_for_a_file_without_scenes() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    b.mesh(vec![tri]);

    assert_eq!(
        list(&b.build()),
        entries(&[
            ("scene", "prefab"),
            ("mesh0/prim0", "mesh"),
            ("material_default", "material"),
        ])
    );
}

/// A lister reads the JSON only: a buffer or an image that is nowhere on
/// disk does not stop it.
#[test]
fn lister_reads_neither_buffers_nor_images() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    let image = b.image_uri("missing.png");
    let texture = b.texture(image, None);
    let m = b.material(json!({
        "pbrMetallicRoughness": { "baseColorTexture": { "index": texture } },
    }));
    single_mesh_file(&mut b, vec![with_material(tri, m)]);
    let (json, _) = b.build_with_buffer_uri("missing.bin");

    assert_eq!(
        list(&json),
        entries(&[
            ("scene", "prefab"),
            ("scene0", "prefab"),
            ("mesh0/prim0", "mesh"),
            ("material0", "material"),
            ("texture0", "texture"),
        ])
    );
}

#[test]
fn lister_lists_nothing_for_obj_bytes() {
    let obj = b"o tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    assert_eq!(
        list_gltf_sub_assets(obj).expect("OBJ bytes are not an error"),
        Vec::<SubAssetEntry>::new()
    );
}

#[test]
fn lister_refuses_malformed_gltf() {
    let truncated_json: &[u8] = br#"{ "asset": { "version": "#;
    let truncated_glb: &[u8] = b"glTF\x02\x00\x00\x00\x64\x00\x00\x00";
    let dangling_accessor: &[u8] = br#"{
        "asset": { "version": "2.0" },
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 7 } }] }]
    }"#;
    for (what, bytes) in [
        ("truncated JSON", truncated_json),
        ("truncated GLB", truncated_glb),
        ("an accessor index past the accessors", dangling_accessor),
    ] {
        assert!(
            list_gltf_sub_assets(bytes).is_err(),
            "{what} must be refused"
        );
    }
}

// ─── Index ──────────────────────────────────────────────────────────────

#[test]
fn index_lists_the_sub_assets_of_glb_and_gltf_files_under_their_file_uuid() {
    let project = Project::new();
    project.write("models/two.glb", &two_material_file().build_glb());
    project.write("models/two.gltf", &two_material_file().build());
    project.write(
        "models/plain.obj",
        b"o tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    );

    let metadata = IndexBuilder::new(&project.assets)
        .build_metadata()
        .expect("the index builds");

    for rel in ["models/two.glb", "models/two.gltf"] {
        let rel_path = Path::new("models").join(rel.trim_start_matches("models/"));
        let container = file_uuid(rel);
        let own = metadata
            .iter()
            .find(|m| m.uuid == container)
            .unwrap_or_else(|| panic!("{rel} has its own entry"));
        assert_eq!(own.asset_type_name, "mesh", "{rel} stays a mesh file");
        assert!(matches!(
            own.variants.get("default"),
            Some(AssetSource::Path(_))
        ));

        for (label, type_name) in TWO_MATERIAL_SUBS {
            let uuid = AssetUUID::sub_asset(container, label);
            let entry = metadata
                .iter()
                .find(|m| m.uuid == uuid)
                .unwrap_or_else(|| panic!("{rel} lists '{label}': {metadata:#?}"));
            assert_eq!(entry.asset_type_name, type_name, "type of {rel} '{label}'");
            assert_eq!(entry.source_path, rel_path, "source of {rel} '{label}'");
            match entry.variants.get("default") {
                Some(AssetSource::Sub {
                    container: c,
                    label: l,
                }) => {
                    assert_eq!(*c, container, "{rel} '{label}' names its file");
                    assert_eq!(l, label);
                }
                other => panic!("{rel} '{label}' must be a Sub source, got {other:?}"),
            }
        }
    }

    let obj_path = Path::new("models").join("plain.obj");
    assert_eq!(
        metadata
            .iter()
            .filter(|m| m.source_path == obj_path)
            .count(),
        1,
        "an OBJ file has no sub-asset"
    );
    assert_eq!(metadata.len(), 2 * (1 + TWO_MATERIAL_SUBS.len()) + 1);
}

// ─── Meshes ─────────────────────────────────────────────────────────────

#[test]
fn a_primitive_sub_mesh_is_in_local_space() {
    let project = Project::new();
    project.write("models/two.glb", &two_material_file().build_glb());
    let mut svc = project.open();

    let prim0 = load_mesh(
        &mut svc,
        sub("models/two.glb", "mesh0/prim0"),
        "mesh0/prim0",
    );
    let prim1 = load_mesh(
        &mut svc,
        sub("models/two.glb", "mesh0/prim1"),
        "mesh0/prim1",
    );
    let quad = load_mesh(
        &mut svc,
        sub("models/two.glb", "mesh1/prim0"),
        "mesh1/prim0",
    );

    // Node `Left` is translated by (10, 0, 0): its primitives are not moved.
    assert_positions(&prim0, &points(&TRI_A));
    assert_positions(&prim1, &points(&TRI_B));
    assert_positions(&quad, &points(&QUAD));
    assert_eq!(prim0.indices, Some(vec![0, 1, 2]));
    assert_eq!(quad.indices, Some(QUAD_INDICES.to_vec()));
    assert_eq!(prim0.primitive_type, PrimitiveTopology::TriangleList);
}

/// With the container registered under `"mesh"`, the file's own uuid keeps
/// meaning its default scene merged into one mesh, node transforms baked.
#[test]
fn the_file_uuid_still_decodes_the_merged_scene_beside_its_sub_meshes() {
    let glb = two_material_file().build_glb();
    let project = Project::new();
    project.write("models/two.glb", &glb);
    let mut svc = project.open();

    let prim0 = load_mesh(
        &mut svc,
        sub("models/two.glb", "mesh0/prim0"),
        "mesh0/prim0",
    );
    assert_positions(&prim0, &points(&TRI_A));

    let merged = load_mesh(&mut svc, file_uuid("models/two.glb"), "the file");
    let expected = decode_gltf(&glb);
    assert_positions(&merged, &expected.positions);
    assert_eq!(merged.indices, expected.indices);
    assert_vec3(
        merged.positions[0],
        Vec3::new(10.0, 0.0, 0.0),
        "translated first vertex",
    );
    assert_eq!(merged.positions.len(), 3 + 3 + 4, "both nodes' primitives");
}

#[test]
fn line_and_point_primitives_keep_their_topology_as_sub_meshes() {
    let line = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let dots = [[7.0, 7.0, 7.0], [8.0, 7.0, 7.0], [9.0, 7.0, 7.0]];
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    let line_pos = b.positions(&line);
    let dot_pos = b.positions(&dots);
    single_mesh_file(
        &mut b,
        vec![
            tri,
            primitive_with_mode(&[("POSITION", line_pos)], None, MODE_LINES),
            primitive_with_mode(&[("POSITION", dot_pos)], None, MODE_POINTS),
        ],
    );
    let project = Project::new();
    project.write("models/mixed.glb", &b.build_glb());
    let mut svc = project.open();

    let lines = load_mesh(&mut svc, sub("models/mixed.glb", "mesh0/prim1"), "lines");
    assert_eq!(lines.primitive_type, PrimitiveTopology::LineList);
    assert_positions(&lines, &points(&line));
    assert_eq!(lines.indices, Some(vec![0, 1]));

    let dots_mesh = load_mesh(&mut svc, sub("models/mixed.glb", "mesh0/prim2"), "points");
    assert_eq!(dots_mesh.primitive_type, PrimitiveTopology::PointList);
    assert_positions(&dots_mesh, &points(&dots));
    assert_eq!(dots_mesh.indices, Some(vec![0, 1, 2]));
}

#[test]
fn strip_and_fan_primitives_become_triangle_lists_as_sub_meshes() {
    let strip = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let fan = QUAD;
    let mut b = GltfBuilder::new();
    let strip_pos = b.positions(&strip);
    let strip_nrm = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    let fan_pos = b.positions(&fan);
    let fan_nrm = b.vec3(&[[0.0, 0.0, 1.0]; 4]);
    single_mesh_file(
        &mut b,
        vec![
            primitive_with_mode(
                &[("POSITION", strip_pos), ("NORMAL", strip_nrm)],
                None,
                MODE_TRIANGLE_STRIP,
            ),
            primitive_with_mode(
                &[("POSITION", fan_pos), ("NORMAL", fan_nrm)],
                None,
                MODE_TRIANGLE_FAN,
            ),
        ],
    );
    let project = Project::new();
    project.write("models/strips.glb", &b.build_glb());
    let mut svc = project.open();

    let s = |i: usize| v3(strip[i]);
    let f = |i: usize| v3(fan[i]);
    for (label, expected) in [
        ("mesh0/prim0", [[s(0), s(1), s(2)], [s(1), s(3), s(2)]]),
        ("mesh0/prim1", [[f(1), f(2), f(0)], [f(2), f(3), f(0)]]),
    ] {
        let mesh = load_mesh(&mut svc, sub("models/strips.glb", label), label);
        assert_eq!(
            mesh.primitive_type,
            PrimitiveTopology::TriangleList,
            "{label}"
        );
        let found = triangles(&mesh);
        assert_eq!(
            found.len(),
            2,
            "{label}: a 4-vertex strip or fan is two triangles"
        );
        for (k, (t, e)) in found.iter().zip(&expected).enumerate() {
            assert!(same_winding(*t, *e), "{label} triangle {k}: {t:?} != {e:?}");
        }
    }
}

#[test]
fn a_primitive_without_normals_gets_flat_normals_as_a_sub_mesh() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&QUAD);
    let idx = b.indices(&QUAD_INDICES);
    single_mesh_file(&mut b, vec![primitive(&[("POSITION", pos)], Some(idx))]);
    let project = Project::new();
    project.write("models/flat.glb", &b.build_glb());
    let mut svc = project.open();

    let mesh = load_mesh(&mut svc, sub("models/flat.glb", "mesh0/prim0"), "the quad");
    let normals = mesh.normals.clone().expect("flat normals generated");
    assert_eq!(mesh.positions.len(), 6, "each triangle owns its vertices");
    assert_eq!(normals.len(), 6);
    for (i, n) in normals.iter().enumerate() {
        assert_vec3(*n, Vec3::Z, &format!("normal {i}"));
    }
}

#[test]
fn a_non_indexed_primitive_gets_sequential_indices_as_a_sub_mesh() {
    let mut b = GltfBuilder::new();
    let pos = b.positions(&TRI_A);
    let nrm = b.vec3(&UP_Z);
    single_mesh_file(
        &mut b,
        vec![primitive(&[("POSITION", pos), ("NORMAL", nrm)], None)],
    );
    let project = Project::new();
    project.write("models/loose.glb", &b.build_glb());
    let mut svc = project.open();

    let mesh = load_mesh(
        &mut svc,
        sub("models/loose.glb", "mesh0/prim0"),
        "the triangle",
    );
    assert_eq!(mesh.indices, Some(vec![0, 1, 2]));
    assert_positions(&mesh, &points(&TRI_A));
}

// ─── Materials ──────────────────────────────────────────────────────────

/// A file whose one mesh has one triangle primitive per material, each
/// drawn with its material; returns the builder ready to build.
fn materials_file(materials: Vec<Value>, extensions_used: &[&str]) -> GltfBuilder {
    let mut b = GltfBuilder::new();
    let mut primitives = Vec::new();
    for material in materials {
        let tri = triangle_primitive(&mut b, &TRI_A);
        let m = b.material(material);
        primitives.push(with_material(tri, m));
    }
    for extension in extensions_used {
        b.extension_used(extension);
    }
    single_mesh_file(&mut b, primitives);
    b
}

#[test]
fn materials_map_base_colour_metallic_and_roughness() {
    let b = materials_file(
        vec![
            json!({ "pbrMetallicRoughness": {
                "baseColorFactor": RED, "metallicFactor": 0.25, "roughnessFactor": 0.75,
            } }),
            json!({ "pbrMetallicRoughness": {
                "baseColorFactor": BLUE, "metallicFactor": 0.0, "roughnessFactor": 0.1,
            } }),
        ],
        &[],
    );
    let project = Project::new();
    project.write("models/mats.glb", &b.build_glb());
    let mut svc = project.open();

    let red = load_standard(&mut svc, sub("models/mats.glb", "material0"), "material0");
    assert_rgba(red.base_color, RED, "material0 base colour");
    assert!(
        close(red.metallic, 0.25),
        "material0 metallic {}",
        red.metallic
    );
    assert!(
        close(red.roughness, 0.75),
        "material0 roughness {}",
        red.roughness
    );

    let blue = load_standard(&mut svc, sub("models/mats.glb", "material1"), "material1");
    assert_rgba(blue.base_color, BLUE, "material1 base colour");
    assert!(
        close(blue.metallic, 0.0),
        "material1 metallic {}",
        blue.metallic
    );
    assert!(
        close(blue.roughness, 0.1),
        "material1 roughness {}",
        blue.roughness
    );
}

#[test]
fn emissive_is_the_factor_times_the_emissive_strength() {
    let b = materials_file(
        vec![
            json!({
                "emissiveFactor": [1.0, 0.5, 0.25],
                "extensions": { KHR_EMISSIVE_STRENGTH: { "emissiveStrength": 4.0 } },
            }),
            json!({ "emissiveFactor": [0.2, 0.4, 0.6] }),
            json!({}),
        ],
        &[KHR_EMISSIVE_STRENGTH],
    );
    let project = Project::new();
    project.write("models/glow.glb", &b.build_glb());
    let mut svc = project.open();

    let strong = load_standard(&mut svc, sub("models/glow.glb", "material0"), "material0");
    assert_rgb(strong.emissive, [4.0, 2.0, 1.0], "factor × strength 4");
    let plain = load_standard(&mut svc, sub("models/glow.glb", "material1"), "material1");
    assert_rgb(plain.emissive, [0.2, 0.4, 0.6], "factor alone");
    let dark = load_standard(&mut svc, sub("models/glow.glb", "material2"), "material2");
    assert_rgb(dark.emissive, [0.0, 0.0, 0.0], "no emission");
}

#[test]
fn alpha_modes_map_to_opaque_mask_with_its_cutoff_and_blend() {
    let b = materials_file(
        vec![
            json!({}),
            json!({ "alphaMode": "OPAQUE" }),
            json!({ "alphaMode": "MASK", "alphaCutoff": 0.3 }),
            json!({ "alphaMode": "MASK" }),
            json!({ "alphaMode": "BLEND" }),
        ],
        &[],
    );
    let project = Project::new();
    project.write("models/alpha.glb", &b.build_glb());
    let mut svc = project.open();
    let mut material = |i: usize| {
        let label = format!("material{i}");
        load_standard(&mut svc, sub("models/alpha.glb", &label), &label)
    };

    assert_eq!(material(0).alpha_mode, AlphaMode::Opaque, "no alphaMode");
    assert_eq!(material(1).alpha_mode, AlphaMode::Opaque, "OPAQUE");

    let masked = material(2);
    match masked.alpha_mode {
        AlphaMode::Mask(cutoff) => assert!(close(cutoff, 0.3), "MASK cutoff {cutoff}"),
        other => panic!("MASK with a cutoff maps to Mask, got {other:?}"),
    }
    assert!(
        close(masked.alpha_cutoff, 0.3),
        "alpha_cutoff {}",
        masked.alpha_cutoff
    );

    let default_cutoff = material(3);
    match default_cutoff.alpha_mode {
        AlphaMode::Mask(cutoff) => assert!(close(cutoff, 0.5), "default cutoff {cutoff}"),
        other => panic!("MASK maps to Mask, got {other:?}"),
    }
    assert!(close(default_cutoff.alpha_cutoff, 0.5));

    assert_eq!(material(4).alpha_mode, AlphaMode::Blend, "BLEND");
}

#[test]
fn double_sided_is_carried_over() {
    let b = materials_file(vec![json!({ "doubleSided": true }), json!({})], &[]);
    let project = Project::new();
    project.write("models/sides.glb", &b.build_glb());
    let mut svc = project.open();

    assert!(
        load_standard(&mut svc, sub("models/sides.glb", "material0"), "material0").double_sided
    );
    assert!(
        !load_standard(&mut svc, sub("models/sides.glb", "material1"), "material1").double_sided
    );
}

#[test]
fn texture_slots_name_the_texture_sub_assets() {
    let mut b = GltfBuilder::new();
    let image = b.image_in_buffer(&png(2, 2, PIXEL), "image/png");
    let textures: Vec<usize> = (0..5).map(|_| b.texture(image, None)).collect();
    let tri_a = triangle_primitive(&mut b, &TRI_A);
    let tri_b = triangle_primitive(&mut b, &TRI_B);
    let textured = b.material(json!({
        "pbrMetallicRoughness": {
            "baseColorTexture": { "index": textures[1] },
            "metallicRoughnessTexture": { "index": textures[0] },
        },
        "normalTexture": { "index": textures[2] },
        "occlusionTexture": { "index": textures[3] },
        "emissiveTexture": { "index": textures[4] },
        "emissiveFactor": [1.0, 1.0, 1.0],
    }));
    let bare = b.material(json!({}));
    single_mesh_file(
        &mut b,
        vec![with_material(tri_a, textured), with_material(tri_b, bare)],
    );
    let rel = "models/textured.glb";
    let project = Project::new();
    project.write(rel, &b.build_glb());
    let mut svc = project.open();

    let texture = |i: usize| Some(sub(rel, &format!("texture{i}")));
    let m = load_standard(&mut svc, sub(rel, "material0"), "material0");
    assert_eq!(m.base_color_texture, texture(1), "base colour");
    assert_eq!(
        m.metallic_roughness_texture,
        texture(0),
        "metallic-roughness"
    );
    assert_eq!(m.normal_map, texture(2), "normal");
    assert_eq!(m.occlusion_map, texture(3), "occlusion");
    assert_eq!(m.emissive_texture, texture(4), "emissive");

    let bare = load_standard(&mut svc, sub(rel, "material1"), "material1");
    assert_spec_default_material(&bare, "an untextured empty material");
}

#[test]
fn material_default_has_the_gltf_spec_defaults() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    single_mesh_file(&mut b, vec![tri]);
    let project = Project::new();
    project.write("models/bare.glb", &b.build_glb());
    let mut svc = project.open();

    let m = load_standard(
        &mut svc,
        sub("models/bare.glb", "material_default"),
        "material_default",
    );
    assert_spec_default_material(&m, "material_default");
}

// ─── KHR_materials_specular ─────────────────────────────────────────────

/// The `KHR_materials_specular` defaults: strength 1, white, untextured.
fn assert_specular_defaults(material: &StandardMaterial, what: &str) {
    assert!(
        close(material.specular, 1.0),
        "{what}: specular {}",
        material.specular
    );
    assert_rgb(material.specular_color, [1.0, 1.0, 1.0], what);
    assert_eq!(material.specular_texture, None, "{what}: specular texture");
    assert_eq!(
        material.specular_color_texture, None,
        "{what}: specular colour texture"
    );
}

#[test]
fn specular_factors_and_textures_map_from_khr_materials_specular() {
    let mut b = GltfBuilder::new();
    let image = b.image_in_buffer(&png(2, 2, PIXEL), "image/png");
    let strength = b.texture(image, None);
    let colour = b.texture(image, None);
    let tri_a = triangle_primitive(&mut b, &TRI_A);
    let tri_b = triangle_primitive(&mut b, &TRI_B);
    let quad = quad_primitive(&mut b);
    let full = b.material(json!({
        "extensions": { KHR_SPECULAR: {
            "specularFactor": 0.3,
            "specularColorFactor": [0.5, 0.6, 0.7],
            "specularTexture": { "index": strength },
            "specularColorTexture": { "index": colour },
        } },
    }));
    let factor_only = b.material(json!({
        "extensions": { KHR_SPECULAR: { "specularFactor": 0.0 } },
    }));
    let plain = b.material(json!({}));
    b.extension_used(KHR_SPECULAR);
    single_mesh_file(
        &mut b,
        vec![
            with_material(tri_a, full),
            with_material(tri_b, factor_only),
            with_material(quad, plain),
        ],
    );
    let rel = "models/specular.glb";
    let project = Project::new();
    project.write(rel, &b.build_glb());
    let mut svc = project.open();

    let m = load_standard(&mut svc, sub(rel, "material0"), "material0");
    assert!(close(m.specular, 0.3), "specularFactor: {}", m.specular);
    assert_rgb(m.specular_color, [0.5, 0.6, 0.7], "specularColorFactor");
    assert_eq!(
        m.specular_texture,
        Some(sub(rel, &format!("texture{strength}"))),
        "specularTexture names its texture sub-asset"
    );
    assert_eq!(
        m.specular_color_texture,
        Some(sub(rel, &format!("texture{colour}"))),
        "specularColorTexture names its texture sub-asset"
    );

    let factor_only = load_standard(&mut svc, sub(rel, "material1"), "material1");
    assert!(
        close(factor_only.specular, 0.0),
        "a specularFactor of 0 is kept: {}",
        factor_only.specular
    );
    assert_rgb(
        factor_only.specular_color,
        [1.0, 1.0, 1.0],
        "an absent specularColorFactor is white",
    );
    assert_eq!(factor_only.specular_texture, None);
    assert_eq!(factor_only.specular_color_texture, None);

    let plain = load_standard(&mut svc, sub(rel, "material2"), "material2");
    assert_specular_defaults(&plain, "a material without the extension");
}

#[test]
fn material_default_has_the_specular_defaults() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    single_mesh_file(&mut b, vec![tri]);
    let project = Project::new();
    project.write("models/bare.glb", &b.build_glb());
    let mut svc = project.open();

    let m = load_standard(
        &mut svc,
        sub("models/bare.glb", "material_default"),
        "material_default",
    );
    assert_specular_defaults(&m, "material_default");
}

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The warnings logged on the thread that logged them, so tests running side
/// by side do not read each other's.
struct ThreadWarnings;

impl log::Log for ThreadWarnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let line = record.args().to_string();
            WARNINGS.with(|warnings| warnings.borrow_mut().push(line));
        }
    }

    fn flush(&self) {}
}

static THREAD_WARNINGS: ThreadWarnings = ThreadWarnings;

/// Starts recording this thread's warnings afresh.
fn record_warnings() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if log::set_logger(&THREAD_WARNINGS).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
    WARNINGS.with(|warnings| warnings.borrow_mut().clear());
}

/// The warnings recorded on this thread since [`record_warnings`].
fn recorded_warnings() -> Vec<String> {
    WARNINGS.with(|warnings| warnings.borrow().clone())
}

/// `KHR_materials_specular` is imported, so opening a file that uses it does
/// not report it among the extensions left out; an extension that is still
/// left out, used beside it, is reported (the recorder sees the report).
#[test]
fn a_file_using_khr_materials_specular_does_not_report_it_unmapped() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    let m = b.material(json!({
        "extensions": { KHR_SPECULAR: { "specularFactor": 0.5 } },
    }));
    b.extension_used(KHR_SPECULAR);
    b.extension_used(KHR_TEXTURE_TRANSFORM);
    single_mesh_file(&mut b, vec![with_material(tri, m)]);
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());

    record_warnings();
    container
        .open(&b.build_glb(), Path::new("models/specular_warn.glb"))
        .unwrap_or_else(|e| panic!("the GLB opens: {e:#}"));
    let about_file: Vec<String> = recorded_warnings()
        .into_iter()
        .filter(|line| line.contains("specular_warn.glb"))
        .collect();

    assert!(
        about_file
            .iter()
            .any(|line| line.contains(KHR_TEXTURE_TRANSFORM)),
        "the extension still left out is reported: {about_file:?}"
    );
    assert!(
        !about_file.iter().any(|line| line.contains(KHR_SPECULAR)),
        "KHR_materials_specular is imported, not reported: {about_file:?}"
    );
}

/// A specular texture (strength or colour) on UV set 1 is sampled with UV
/// set 0, as every other slot is: opening the file says so.
#[test]
fn a_specular_texture_on_a_second_uv_set_is_reported() {
    for slot in ["specularTexture", "specularColorTexture"] {
        let mut b = GltfBuilder::new();
        let image = b.image_in_buffer(&png(2, 2, PIXEL), "image/png");
        let texture = b.texture(image, None);
        let tri = triangle_primitive(&mut b, &TRI_A);
        let m = b.material(json!({
            "extensions": { KHR_SPECULAR: { slot: { "index": texture, "texCoord": 1 } } },
        }));
        b.extension_used(KHR_SPECULAR);
        single_mesh_file(&mut b, vec![with_material(tri, m)]);
        let dir = tempfile::tempdir().expect("a temp dir");
        let container = GltfContainer::new(dir.path());
        let file = format!("models/{slot}_uv1.glb");

        record_warnings();
        container
            .open(&b.build_glb(), Path::new(&file))
            .unwrap_or_else(|e| panic!("the GLB opens: {e:#}"));
        let about_file: Vec<String> = recorded_warnings()
            .into_iter()
            .filter(|line| line.contains(&file))
            .collect();

        assert!(
            about_file.iter().any(|line| line.contains("UV set")),
            "{slot} on UV set 1 is reported: {about_file:?}"
        );
    }
}

/// UV1, a texture transform, a normal scale, an occlusion strength and a
/// non-default sampler are not mapped (each logs a warning); the material
/// and its textures still decode.
#[test]
fn unmapped_texture_features_still_decode() {
    let mut b = GltfBuilder::new();
    let image = b.image_in_buffer(&png(3, 2, PIXEL), "image/png");
    let sampler = b.sampler(json!({ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071 }));
    let texture = b.texture(image, Some(sampler));
    let pos = b.positions(&TRI_A);
    let nrm = b.vec3(&UP_Z);
    let uv0 = b.vec2(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    let uv1 = b.vec2(&[[0.5, 0.5], [1.0, 0.5], [0.5, 1.0]]);
    let idx = b.indices(&[0, 1, 2]);
    let m = b.material(json!({
        "pbrMetallicRoughness": {
            "baseColorFactor": RED,
            "baseColorTexture": {
                "index": texture,
                "texCoord": 1,
                "extensions": { KHR_TEXTURE_TRANSFORM: { "offset": [0.5, 0.0], "scale": [2.0, 2.0] } },
            },
        },
        "normalTexture": { "index": texture, "scale": 2.0 },
        "occlusionTexture": { "index": texture, "strength": 0.5 },
    }));
    b.extension_used(KHR_TEXTURE_TRANSFORM);
    single_mesh_file(
        &mut b,
        vec![with_material(
            primitive(
                &[
                    ("POSITION", pos),
                    ("NORMAL", nrm),
                    ("TEXCOORD_0", uv0),
                    ("TEXCOORD_1", uv1),
                ],
                Some(idx),
            ),
            m,
        )],
    );
    let rel = "models/unmapped.glb";
    let project = Project::new();
    project.write(rel, &b.build_glb());
    let mut svc = project.open();

    let material = load_standard(&mut svc, sub(rel, "material0"), "material0");
    assert_rgba(material.base_color, RED, "base colour");
    let texture0 = Some(sub(rel, "texture0"));
    assert_eq!(material.base_color_texture, texture0, "base colour texture");
    assert_eq!(material.normal_map, texture0, "normal map");
    assert_eq!(material.occlusion_map, texture0, "occlusion map");

    let (w, h, _) = load_texture(&mut svc, sub(rel, "texture0"), "texture0");
    assert_eq!((w, h), (3, 2));
}

// ─── Textures ───────────────────────────────────────────────────────────

/// A file with one triangle drawn by a material textured by `image`.
fn textured_file(image: impl FnOnce(&mut GltfBuilder) -> usize) -> GltfBuilder {
    let mut b = GltfBuilder::new();
    let image = image(&mut b);
    let texture = b.texture(image, None);
    let tri = triangle_primitive(&mut b, &TRI_A);
    let m = b.material(json!({
        "pbrMetallicRoughness": { "baseColorTexture": { "index": texture } },
    }));
    single_mesh_file(&mut b, vec![with_material(tri, m)]);
    b
}

fn assert_texture(found: (u32, u32, Vec<u8>), size: (u32, u32), rgba: [u8; 4], what: &str) {
    let (w, h, pixels) = found;
    assert_eq!((w, h), size, "{what}: size");
    assert_eq!(
        pixels.len(),
        (size.0 * size.1 * 4) as usize,
        "{what}: pixel bytes"
    );
    assert_eq!(&pixels[..4], &rgba, "{what}: first pixel");
}

#[test]
fn a_png_in_a_glb_buffer_view_decodes_as_a_texture() {
    let b = textured_file(|b| b.image_in_buffer(&png(3, 2, PIXEL), "image/png"));
    let project = Project::new();
    project.write("models/embedded.glb", &b.build_glb());
    let mut svc = project.open();

    let found = load_texture(&mut svc, sub("models/embedded.glb", "texture0"), "texture0");
    assert_texture(found, (3, 2), PIXEL, "buffer-view PNG");
}

#[test]
fn a_png_in_a_data_uri_decodes_as_a_texture() {
    let b = textured_file(|b| b.image_data_uri(&png(5, 4, PIXEL)));
    let project = Project::new();
    project.write("models/inline.gltf", &b.build());
    let mut svc = project.open();

    let found = load_texture(&mut svc, sub("models/inline.gltf", "texture0"), "texture0");
    assert_texture(found, (5, 4), PIXEL, "data-URI PNG");
}

/// An image URI names a file next to the glTF file, in its subfolder — not
/// one at the assets root (a decoy of another size sits there).
#[test]
fn a_png_next_to_a_gltf_in_a_subfolder_decodes_as_a_texture() {
    let b = textured_file(|b| b.image_uri("thing.png"));
    let project = Project::new();
    project.write("models/thing.gltf", &b.build());
    project.write("models/thing.png", &png(7, 3, PIXEL));
    project.write("thing.png", &png(2, 2, DECOY_PIXEL));
    let mut svc = project.open();

    let found = load_texture(&mut svc, sub("models/thing.gltf", "texture0"), "texture0");
    assert_texture(found, (7, 3), PIXEL, "PNG next to the glTF");
}

#[test]
fn a_percent_encoded_image_uri_names_its_file() {
    let b = textured_file(|b| b.image_uri("my%20texture.png"));
    let project = Project::new();
    project.write("models/spaced.gltf", &b.build());
    project.write("models/my texture.png", &png(4, 1, PIXEL));
    let mut svc = project.open();

    let found = load_texture(&mut svc, sub("models/spaced.gltf", "texture0"), "texture0");
    assert_texture(found, (4, 1), PIXEL, "percent-decoded PNG");
}

// ─── External buffers ───────────────────────────────────────────────────

/// A one-triangle `.gltf` (corners `corners`) whose buffer is the external
/// file `uri`: the JSON and the buffer's bytes.
fn external_buffer_file(corners: &[[f32; 3]; 3], uri: &str) -> (Vec<u8>, Vec<u8>) {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, corners);
    single_mesh_file(&mut b, vec![tri]);
    b.build_with_buffer_uri(uri)
}

/// Writes `models/thing.gltf` with its buffer `models/thing.bin` holding
/// [`TRI_A`], and a decoy `thing.bin` holding [`TRI_B`] at the assets root.
fn project_with_external_buffer() -> Project {
    let project = Project::new();
    let (json, bin) = external_buffer_file(&TRI_A, "thing.bin");
    let (_, decoy) = external_buffer_file(&TRI_B, "thing.bin");
    project.write("models/thing.gltf", &json);
    project.write("models/thing.bin", &bin);
    project.write("thing.bin", &decoy);
    project
}

#[test]
fn the_merged_mesh_reads_an_external_buffer_next_to_the_gltf() {
    let project = project_with_external_buffer();
    let mut svc = project.open();

    let mesh = load_mesh(&mut svc, file_uuid("models/thing.gltf"), "the file");
    assert_positions(&mesh, &points(&TRI_A));
}

#[test]
fn a_sub_mesh_reads_an_external_buffer_next_to_the_gltf() {
    let project = project_with_external_buffer();
    let mut svc = project.open();

    let mesh = load_mesh(
        &mut svc,
        sub("models/thing.gltf", "mesh0/prim0"),
        "mesh0/prim0",
    );
    assert_positions(&mesh, &points(&TRI_A));
}

#[test]
fn gltf_decoders_load_at_resolves_buffers_next_to_the_gltf() {
    let project = project_with_external_buffer();
    let json = std::fs::read(project.assets.join("models").join("thing.gltf")).unwrap();
    let at = Path::new("models").join("thing.gltf");

    let gltf = GltfDecoder::new(Arc::new(FileSystemResolver::new(&project.assets)))
        .load_at(&json, &at)
        .unwrap_or_else(|e| panic!("GltfDecoder::load_at: {e}"));
    assert_positions(&gltf, &points(&TRI_A));

    let dispatched = MeshDispatcher::new(Arc::new(FileSystemResolver::new(&project.assets)))
        .load_at(&json, &at)
        .unwrap_or_else(|e| panic!("MeshDispatcher::load_at: {e}"));
    assert_positions(&dispatched, &points(&TRI_A));
}

#[test]
fn a_percent_encoded_buffer_uri_names_its_file() {
    let project = Project::new();
    let (json, bin) = external_buffer_file(&TRI_A, "my%20mesh.bin");
    project.write("models/spaced.gltf", &json);
    project.write("models/my mesh.bin", &bin);
    let mut svc = project.open();

    let merged = load_mesh(&mut svc, file_uuid("models/spaced.gltf"), "the file");
    assert_positions(&merged, &points(&TRI_A));
    let prim = load_mesh(
        &mut svc,
        sub("models/spaced.gltf", "mesh0/prim0"),
        "mesh0/prim0",
    );
    assert_positions(&prim, &points(&TRI_A));
}

/// `../../outside.bin` from `assets/models/` is the project's own folder:
/// outside the assets root, refused — though a buffer sits there, and one
/// sits where the URI would land from the assets root.
#[test]
fn a_buffer_uri_leaving_the_assets_root_is_refused() {
    let project = Project::new();
    let (json, bin) = external_buffer_file(&TRI_A, "../../outside.bin");
    project.write("models/escape.gltf", &json);
    write_file(&project.root.join("outside.bin"), &bin);
    write_file(&project.dir.path().join("outside.bin"), &bin);
    let mut svc = project.open();

    let err = svc
        .load::<Mesh>(&file_uuid("models/escape.gltf"))
        .expect_err("a buffer outside the assets root is refused");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("outside.bin"),
        "the error names the URI: {msg}"
    );
    assert!(
        svc.load::<Mesh>(&sub("models/escape.gltf", "mesh0/prim0"))
            .is_err(),
        "the sub-mesh is refused too"
    );
}

#[test]
fn an_absolute_buffer_uri_is_refused() {
    let project = Project::new();
    let outside = project.dir.path().join("absolute.bin");
    let uri = outside.to_string_lossy().replace('\\', "/");
    let (json, bin) = external_buffer_file(&TRI_A, &uri);
    write_file(&outside, &bin);
    project.write("models/absolute.gltf", &json);
    let mut svc = project.open();

    let err = svc
        .load::<Mesh>(&file_uuid("models/absolute.gltf"))
        .expect_err("an absolute buffer path is refused");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("absolute.bin"),
        "the error names the URI: {msg}"
    );
    assert!(
        svc.load::<Mesh>(&sub("models/absolute.gltf", "mesh0/prim0"))
            .is_err(),
        "the sub-mesh is refused too"
    );
}

// ─── Container ──────────────────────────────────────────────────────────

/// The two-material GLB opened by a container over an empty assets root.
fn opened_two_material_glb() -> (GltfContainer, Arc<dyn std::any::Any + Send + Sync>, TempDir) {
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let opened = container
        .open(
            &two_material_file().build_glb(),
            Path::new("models/two.glb"),
        )
        .unwrap_or_else(|e| panic!("the GLB opens: {e:#}"));
    (container, opened, dir)
}

#[test]
fn the_container_extracts_each_kind_as_its_asset_type() {
    let (container, opened, _dir) = opened_two_material_glb();
    let file = file_uuid("models/two.glb");

    let mesh = container
        .extract(opened.as_ref(), file, "mesh0/prim1", "mesh")
        .expect("a primitive extracts")
        .downcast::<Mesh>()
        .expect("a mesh sub is a Mesh");
    assert_positions(&mesh, &points(&TRI_B));

    let material = container
        .extract(opened.as_ref(), file, "material0", "material")
        .expect("a material extracts")
        .downcast::<Box<dyn Material>>()
        .expect("a material sub is a Box<dyn Material>");
    let material: &dyn Material = &**material;
    let standard = material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .expect("a glTF material is a StandardMaterial");
    assert_rgba(standard.base_color, RED, "material0 base colour");
    assert_eq!(
        standard.base_color_texture,
        Some(AssetUUID::sub_asset(file, "texture0")),
        "the texture slot derives from the container uuid given to extract"
    );

    let texture = container
        .extract(opened.as_ref(), file, "texture0", "texture")
        .expect("a texture extracts")
        .downcast::<CpuTexture>()
        .expect("a texture sub is a CpuTexture");
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

#[test]
fn extracting_an_unknown_label_is_an_error_naming_it() {
    let (container, opened, _dir) = opened_two_material_glb();
    let file = file_uuid("models/two.glb");
    for (label, type_name) in [
        ("mesh7/prim0", "mesh"),
        ("mesh0/prim9", "mesh"),
        ("material9", "material"),
        ("texture9", "texture"),
        ("bogus", "mesh"),
    ] {
        let err = match container.extract(opened.as_ref(), file, label, type_name) {
            Ok(_) => panic!("'{label}' is not in the file"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(msg.contains(label), "the error names '{label}': {msg}");
    }
}

#[test]
fn extracting_a_label_as_another_type_is_an_error() {
    let (container, opened, _dir) = opened_two_material_glb();
    let file = file_uuid("models/two.glb");
    for (label, type_name) in [
        ("material0", "mesh"),
        ("mesh0/prim0", "material"),
        ("texture0", "mesh"),
        ("mesh0/prim0", "texture"),
        ("material0", "texture"),
    ] {
        assert!(
            container
                .extract(opened.as_ref(), file, label, type_name)
                .is_err(),
            "'{label}' is not a {type_name}"
        );
    }
}

#[test]
fn loading_a_sub_asset_as_another_type_is_an_error() {
    let project = Project::new();
    project.write("models/two.glb", &two_material_file().build_glb());
    let mut svc = project.open();

    let material0 = sub("models/two.glb", "material0");
    load_standard(&mut svc, material0, "material0");
    assert!(
        svc.load::<Mesh>(&material0).is_err(),
        "a material is not a Mesh"
    );
    assert!(
        svc.load::<CpuTexture>(&sub("models/two.glb", "mesh0/prim0"))
            .is_err(),
        "a primitive is not a texture"
    );
}

#[test]
fn opening_obj_bytes_is_an_error() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let obj = b"o tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    assert!(container.open(obj, Path::new("models/plain.obj")).is_err());
}

#[test]
fn material_default_does_not_extract_from_a_file_that_does_not_list_it() {
    let (container, opened, _dir) = opened_two_material_glb();
    assert!(
        list(&two_material_file().build_glb())
            .iter()
            .all(|e| e.label != "material_default"),
        "the fixture gives every primitive a material"
    );
    assert!(
        container
            .extract(
                opened.as_ref(),
                file_uuid("models/two.glb"),
                "material_default",
                "material",
            )
            .is_err(),
        "a label the lister does not write is not a sub-asset of the file"
    );
}

// ─── Hostile files ──────────────────────────────────────────────────────

/// The largest byte count or offset a glTF integer can carry.
const HUGE: u64 = u64::MAX;

/// `bytes` (`.gltf` JSON) with `edit` applied to its JSON root.
fn edited(bytes: Vec<u8>, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut root: Value = serde_json::from_slice(&bytes).expect("the builder writes JSON");
    edit(&mut root);
    serde_json::to_vec(&root).expect("glTF JSON serialises")
}

/// A one-triangle `.gltf` (accessor 0 `POSITION`, 1 `NORMAL`, 2 the indices;
/// buffer view `i` holds accessor `i`), with `edit` applied to its JSON.
fn hostile_triangle(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    single_mesh_file(&mut b, vec![tri]);
    edited(b.build(), edit)
}

/// A one-triangle `.gltf` textured by an embedded PNG (buffer view 0 holds
/// the image), with `edit` applied to its JSON.
fn hostile_texture(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let b = textured_file(|b| b.image_in_buffer(&png(2, 2, PIXEL), "image/png"));
    edited(b.build(), edit)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

/// Reads `bytes` as `models/hostile.gltf` every way the engine does — the
/// lister, the merged mesh decoder, the container, and each sub-asset the
/// lister names — and returns the panics, named by where they happened.
fn panics_reading(bytes: &[u8]) -> Vec<String> {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let mut panics = Vec::new();
    let mut guard = |what: &str, run: &mut dyn FnMut()| {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(run)) {
            panics.push(format!("{what}: {}", panic_message(payload.as_ref())));
        }
    };

    let mut listed = Vec::new();
    guard("the lister", &mut || {
        listed = list_gltf_sub_assets(bytes).unwrap_or_default();
    });
    guard("the merged mesh decoder", &mut || {
        let _ = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(bytes);
    });
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let mut opened = None;
    guard("opening the container", &mut || {
        opened = container.open(bytes, Path::new("models/hostile.gltf")).ok();
    });
    if let Some(opened) = opened {
        let file = file_uuid("models/hostile.gltf");
        for entry in &listed {
            guard(&format!("extracting '{}'", entry.label), &mut || {
                let _ = container.extract(opened.as_ref(), file, &entry.label, &entry.type_name);
            });
        }
    }
    panics
}

/// Reads each case and fails listing every case that panicked.
fn assert_no_case_panics(cases: Vec<(&str, Vec<u8>)>) {
    let failures: Vec<String> = cases
        .into_iter()
        .flat_map(|(what, bytes)| {
            panics_reading(&bytes)
                .into_iter()
                .map(move |panic| format!("{what} — {panic}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "a hostile glTF must be refused with an error, never a panic:\n{}",
        failures.join("\n")
    );
}

/// glTF fixes the type of each attribute (`POSITION` and `NORMAL` are float
/// VEC3, `TEXCOORD_0` a float or normalised VEC2, `COLOR_0` a VEC3/VEC4,
/// indices an unsigned SCALAR); the validator does not check it.
#[test]
fn accessors_of_a_type_their_attribute_cannot_hold_never_panic() {
    assert_no_case_panics(vec![
        (
            "POSITION as VEC2",
            hostile_triangle(|root| root["accessors"][0]["type"] = json!("VEC2")),
        ),
        (
            "NORMAL as VEC2",
            hostile_triangle(|root| root["accessors"][1]["type"] = json!("VEC2")),
        ),
        (
            "indices as FLOAT",
            hostile_triangle(|root| root["accessors"][2]["componentType"] = json!(5126)),
        ),
        (
            "TEXCOORD_0 as an UNSIGNED_INT SCALAR",
            hostile_triangle(|root| {
                root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = json!(2)
            }),
        ),
        (
            "TEXCOORD_0 as a FLOAT VEC3",
            hostile_triangle(|root| {
                root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = json!(1)
            }),
        ),
        (
            "COLOR_0 as an UNSIGNED_INT SCALAR",
            hostile_triangle(|root| {
                root["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = json!(2)
            }),
        ),
    ]);
}

#[test]
fn accessor_counts_offsets_and_strides_that_overflow_never_panic() {
    assert_no_case_panics(vec![
        (
            "POSITION with count 0",
            hostile_triangle(|root| root["accessors"][0]["count"] = json!(0)),
        ),
        (
            "indices with count 0",
            hostile_triangle(|root| root["accessors"][2]["count"] = json!(0)),
        ),
        (
            "POSITION with count 2^62",
            hostile_triangle(|root| root["accessors"][0]["count"] = json!(1u64 << 62)),
        ),
        (
            "POSITION at accessor byteOffset u64::MAX",
            hostile_triangle(|root| root["accessors"][0]["byteOffset"] = json!(HUGE)),
        ),
        (
            "POSITION's buffer view at byteOffset u64::MAX",
            hostile_triangle(|root| root["bufferViews"][0]["byteOffset"] = json!(HUGE)),
        ),
        (
            "POSITION's buffer view with byteStride 4, under a VEC3's 12 bytes",
            hostile_triangle(|root| root["bufferViews"][0]["byteStride"] = json!(4)),
        ),
    ]);
}

#[test]
fn sparse_accessors_that_overflow_never_panic() {
    let sparse = |count: u64| {
        json!({
            "count": count,
            "indices": { "bufferView": 2, "componentType": 5125 },
            "values": { "bufferView": 0 },
        })
    };
    assert_no_case_panics(vec![
        (
            "POSITION with a sparse count of 0",
            hostile_triangle(|root| root["accessors"][0]["sparse"] = sparse(0)),
        ),
        (
            "POSITION without a buffer view, sparse, count 2^62",
            hostile_triangle(|root| {
                let accessor = &mut root["accessors"][0];
                accessor.as_object_mut().unwrap().remove("bufferView");
                accessor["count"] = json!(1u64 << 62);
                accessor["sparse"] = sparse(1);
            }),
        ),
    ]);
}

#[test]
fn images_naming_no_readable_bytes_never_panic() {
    assert_no_case_panics(vec![
        (
            "an image in a buffer view without a mimeType",
            hostile_texture(|root| {
                root["images"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("mimeType");
            }),
        ),
        (
            "an image with neither a uri nor a buffer view",
            hostile_texture(|root| root["images"][0] = json!({})),
        ),
        (
            "an image whose buffer view starts at byteOffset u64::MAX",
            hostile_texture(|root| root["bufferViews"][0]["byteOffset"] = json!(HUGE)),
        ),
    ]);
}

/// A primitive's indices or normals that lie outside its buffer are a broken
/// file: drawing it with sequential indices or flat normals instead would
/// show a shape the file does not describe.
#[test]
fn an_index_or_normal_accessor_outside_its_buffer_is_refused() {
    let mut accepted = Vec::new();
    for (what, view) in [("indices", 2), ("NORMAL", 1)] {
        let bytes = hostile_triangle(|root| root["bufferViews"][view]["byteOffset"] = json!(4096));
        if let Ok(mesh) = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(&bytes) {
            accepted.push(format!(
                "{what} past the buffer — the merged mesh decoded: indices {:?}, normals {:?}",
                mesh.indices, mesh.normals
            ));
        }

        let dir = tempfile::tempdir().expect("a temp dir");
        let container = GltfContainer::new(dir.path());
        let opened = container
            .open(&bytes, Path::new("models/hostile.gltf"))
            .unwrap_or_else(|e| panic!("the buffers themselves are whole: {e:#}"));
        let file = file_uuid("models/hostile.gltf");
        if let Ok(mesh) = container.extract(opened.as_ref(), file, "mesh0/prim0", "mesh") {
            let mesh = mesh.downcast::<Mesh>().expect("a mesh sub is a Mesh");
            accepted.push(format!(
                "{what} past the buffer — 'mesh0/prim0' decoded: indices {:?}, normals {:?}",
                mesh.indices, mesh.normals
            ));
        }
    }
    assert!(
        accepted.is_empty(),
        "a primitive whose data lies outside its buffer must be refused:\n{}",
        accepted.join("\n")
    );
}

/// A percent escape is `%` and two hex digits; `u8::from_str_radix` also
/// takes a sign, which a URI escape never has.
#[test]
fn a_percent_escape_with_a_sign_is_malformed() {
    use crate::asset::decoders::mesh::gltf_resources::resolve_relative_uri;
    for uri in ["bad%+1.bin", "bad%+f.bin"] {
        let resolved = resolve_relative_uri(Path::new("models"), uri);
        assert!(
            resolved.is_err(),
            "'{uri}' is not a percent escape, but resolved to {:?}",
            resolved.ok()
        );
    }
}

#[test]
fn a_glb_whose_bin_chunk_is_shorter_than_its_buffer_is_refused() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    single_mesh_file(&mut b, vec![tri]);
    let glb = b.build_glb();
    // Rewrite the JSON chunk to declare 64 more buffer bytes than BIN holds.
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let declared = json["buffers"][0]["byteLength"].as_u64().unwrap();
    let mut longer = edited(serde_json::to_vec(&json).unwrap(), |root| {
        root["buffers"][0]["byteLength"] = json!(declared + 64)
    });
    while !longer.len().is_multiple_of(4) {
        longer.push(b' ');
    }
    let bin = &glb[20 + json_len..];
    let total = 12 + 8 + longer.len() + bin.len();
    let mut short = Vec::new();
    short.extend_from_slice(&glb[..8]);
    short.extend_from_slice(&(total as u32).to_le_bytes());
    short.extend_from_slice(&(longer.len() as u32).to_le_bytes());
    short.extend_from_slice(&glb[16..20]);
    short.extend_from_slice(&longer);
    short.extend_from_slice(bin);

    assert!(
        GltfDecoder::new(Arc::new(NoOpResourceResolver))
            .load(&short)
            .is_err(),
        "the merged mesh is refused"
    );
    let dir = tempfile::tempdir().expect("a temp dir");
    assert!(
        GltfContainer::new(dir.path())
            .open(&short, Path::new("models/short.glb"))
            .is_err(),
        "the container refuses to open it"
    );
}

/// glTF requires a sparse accessor's indices to strictly increase and to lie
/// inside the accessor; a substitution that breaks either names no vertex
/// the file can mean, and dropping it silently draws another shape.
#[test]
fn sparse_indices_unsorted_or_past_the_accessor_are_refused() {
    let cases = [
        (
            // View 2 holds the U32 indices [0, 1, 2]; read as U8 from byte 8
            // they are [2, 0].
            "U8 sparse indices [2, 0]",
            json!({
                "count": 2,
                "indices": { "bufferView": 2, "byteOffset": 8, "componentType": 5121 },
                "values": { "bufferView": 1 },
            }),
        ),
        (
            // Byte 10 of the +Z normals is 0x80: index 128 of 3 vertices.
            "U8 sparse index 128 of 3 vertices",
            json!({
                "count": 1,
                "indices": { "bufferView": 1, "byteOffset": 10, "componentType": 5121 },
                "values": { "bufferView": 1 },
            }),
        ),
    ];
    let mut accepted = Vec::new();
    for (what, sparse) in cases {
        let bytes = hostile_triangle(|root| root["accessors"][0]["sparse"] = sparse);
        if let Ok(mesh) = GltfDecoder::new(Arc::new(NoOpResourceResolver)).load(&bytes) {
            accepted.push(format!(
                "{what} — the merged mesh decoded: positions {:?}",
                mesh.positions
            ));
        }
        let dir = tempfile::tempdir().expect("a temp dir");
        let container = GltfContainer::new(dir.path());
        let opened = container
            .open(&bytes, Path::new("models/hostile.gltf"))
            .unwrap_or_else(|e| panic!("the buffers themselves are whole: {e:#}"));
        let file = file_uuid("models/hostile.gltf");
        if let Ok(mesh) = container.extract(opened.as_ref(), file, "mesh0/prim0", "mesh") {
            let mesh = mesh.downcast::<Mesh>().expect("a mesh sub is a Mesh");
            accepted.push(format!(
                "{what} — 'mesh0/prim0' decoded: positions {:?}",
                mesh.positions
            ));
        }
    }
    assert!(
        accepted.is_empty(),
        "sparse indices that do not strictly increase inside the accessor must be refused:\n{}",
        accepted.join("\n")
    );
}

/// The sparse values view is read with its own `byteStride`; one shorter
/// than the accessor's element hands the reader a partial element.
#[test]
fn a_sparse_values_view_strided_under_its_element_never_panics() {
    let one_substitution = |values: usize| {
        json!({
            "count": 1,
            "indices": { "bufferView": 2, "componentType": 5125 },
            "values": { "bufferView": values },
        })
    };
    assert_no_case_panics(vec![
        (
            "POSITION's sparse values in a view with byteStride 4, under a VEC3's 12 bytes",
            hostile_triangle(|root| {
                root["meshes"][0]["primitives"][0]["attributes"]
                    .as_object_mut()
                    .unwrap()
                    .remove("NORMAL");
                root["bufferViews"][1]["byteStride"] = json!(4);
                root["accessors"][0]["sparse"] = one_substitution(1);
            }),
        ),
        (
            "COLOR_0 (U16 VEC3) sparse values in a view with byteStride 4, under its 6 bytes",
            hostile_triangle(|root| {
                root["bufferViews"].as_array_mut().unwrap().push(
                    json!({ "buffer": 0, "byteOffset": 36, "byteLength": 36, "byteStride": 4 }),
                );
                root["accessors"].as_array_mut().unwrap().push(json!({
                    "bufferView": 1, "componentType": 5123, "count": 3, "type": "VEC3",
                    "normalized": true, "sparse": one_substitution(3),
                }));
                root["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = json!(3);
            }),
        ),
    ]);
}

/// A GLB header carries the file's total length, header included; a length
/// under the 12-byte header itself is malformed.
#[test]
fn a_glb_declaring_a_length_shorter_than_its_header_never_panics() {
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&0u32.to_le_bytes());
    assert_no_case_panics(vec![("a GLB of length 0", glb)]);
}

/// glTF: only the first buffer may be a GLB's BIN chunk. Each further buffer
/// without a uri would be one more full copy of the chunk, so a few bytes of
/// JSON per buffer multiply the file's memory.
#[test]
fn a_glb_whose_second_buffer_names_the_bin_chunk_is_refused() {
    let mut b = GltfBuilder::new();
    let tri = triangle_primitive(&mut b, &TRI_A);
    single_mesh_file(&mut b, vec![tri]);
    let glb = b.build_glb();
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let mut twice = edited(serde_json::to_vec(&json).unwrap(), |root| {
        let first = root["buffers"][0].clone();
        root["buffers"] = json!([first.clone(), first]);
    });
    while !twice.len().is_multiple_of(4) {
        twice.push(b' ');
    }
    let bin = &glb[20 + json_len..];
    let total = 12 + 8 + twice.len() + bin.len();
    let mut doubled = Vec::new();
    doubled.extend_from_slice(&glb[..8]);
    doubled.extend_from_slice(&(total as u32).to_le_bytes());
    doubled.extend_from_slice(&(twice.len() as u32).to_le_bytes());
    doubled.extend_from_slice(&glb[16..20]);
    doubled.extend_from_slice(&twice);
    doubled.extend_from_slice(bin);

    assert!(
        GltfDecoder::new(Arc::new(NoOpResourceResolver))
            .load(&doubled)
            .is_err(),
        "the merged mesh is refused"
    );
    let dir = tempfile::tempdir().expect("a temp dir");
    assert!(
        GltfContainer::new(dir.path())
            .open(&doubled, Path::new("models/doubled.glb"))
            .is_err(),
        "the container refuses to open it"
    );
}

/// The checks refuse what the reader cannot read, never a layout glTF
/// allows: POSITION and NORMAL interleaved in one strided view, U16
/// indices, normalised U16 UVs, normalised U8 RGB colours padded to a
/// 4-byte stride, and a sparse POSITION over its own buffer view.
#[test]
fn interleaved_normalised_and_sparse_attributes_still_decode() {
    use base64::Engine;
    let mut bin = Vec::new();
    // 0: POSITION and NORMAL interleaved, 24-byte stride.
    for (p, n) in TRI_A.iter().zip(UP_Z.iter()) {
        for f in p.iter().chain(n.iter()) {
            bin.extend_from_slice(&f.to_le_bytes());
        }
    }
    // 72: U16 indices, then 2 bytes of padding.
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    bin.extend_from_slice(&[0, 0]);
    // 80: normalised U16 UVs.
    for uv in [[0u16, 0], [65535, 0], [0, 65535]] {
        for c in uv {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    // 92: normalised U8 RGB colours, padded to 4 bytes.
    for rgb in [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]] {
        bin.extend_from_slice(&rgb);
        bin.push(0);
    }
    // 104: one sparse substitution, vertex 2 → (0, 0, 5).
    bin.extend_from_slice(&2u32.to_le_bytes());
    for f in [0.0f32, 0.0, 5.0] {
        bin.extend_from_slice(&f.to_le_bytes());
    }
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bin)
    );
    let root = json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": bin.len(), "uri": uri }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 72, "byteStride": 24 },
            { "buffer": 0, "byteOffset": 72, "byteLength": 6 },
            { "buffer": 0, "byteOffset": 80, "byteLength": 12 },
            { "buffer": 0, "byteOffset": 92, "byteLength": 12, "byteStride": 4 },
            { "buffer": 0, "byteOffset": 104, "byteLength": 4 },
            { "buffer": 0, "byteOffset": 108, "byteLength": 12 },
        ],
        "accessors": [
            {
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 5.0],
                "sparse": {
                    "count": 1,
                    "indices": { "bufferView": 4, "componentType": 5125 },
                    "values": { "bufferView": 5 },
                },
            },
            { "bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" },
            { "bufferView": 2, "componentType": 5123, "count": 3, "type": "VEC2", "normalized": true },
            { "bufferView": 3, "componentType": 5121, "count": 3, "type": "VEC3", "normalized": true },
        ],
        "meshes": [{ "primitives": [{
            "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 3, "COLOR_0": 4 },
            "indices": 2,
        }]}],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }],
        "scene": 0,
    });
    let bytes = serde_json::to_vec(&root).expect("glTF JSON serialises");

    let check = |mesh: &Mesh, what: &str| {
        assert_eq!(
            mesh.positions,
            points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 5.0]]),
            "{what}: positions, vertex 2 substituted"
        );
        assert_eq!(mesh.normals, Some(points(&UP_Z)), "{what}: normals");
        assert_eq!(
            mesh.tex_coords,
            Some(vec![
                khora_core::math::Vec2::new(0.0, 0.0),
                khora_core::math::Vec2::new(1.0, 0.0),
                khora_core::math::Vec2::new(0.0, 1.0)
            ]),
            "{what}: UVs"
        );
        assert_eq!(
            mesh.colors,
            Some(vec![
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(0.0, 1.0, 0.0, 1.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0)
            ]),
            "{what}: colours"
        );
        assert_eq!(mesh.indices, Some(vec![0, 1, 2]), "{what}: indices");
    };
    let merged = GltfDecoder::new(Arc::new(NoOpResourceResolver))
        .load(&bytes)
        .unwrap_or_else(|e| panic!("the merged mesh decodes: {e}"));
    check(&merged, "the merged mesh");

    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let opened = container
        .open(&bytes, Path::new("models/layouts.gltf"))
        .unwrap_or_else(|e| panic!("the container opens: {e:#}"));
    let sub = container
        .extract(
            opened.as_ref(),
            file_uuid("models/layouts.gltf"),
            "mesh0/prim0",
            "mesh",
        )
        .unwrap_or_else(|e| panic!("'mesh0/prim0' extracts: {e:#}"))
        .downcast::<Mesh>()
        .expect("a mesh sub is a Mesh");
    check(&sub, "'mesh0/prim0'");
}
