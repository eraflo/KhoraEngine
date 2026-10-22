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

//! A glTF scene is a prefab: a root, one entity per node with its local
//! transform and parent, each showing its primitives' mesh and material
//! sub-assets. Entities are keyed by their name path from the scene's root,
//! so a re-exported file — nodes reordered, a material edited — is the same
//! prefab, and an instance's overrides stay on the node they were made on.

use std::collections::HashSet;
use std::f32::consts::FRAC_1_SQRT_2;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::gltf_builder::{primitive, with_material, GltfBuilder};
use super::*;
use crate::asset::decoders::mesh::GltfContainer;
use crate::asset::{AssetContainer, AssetIdRegistry, AssetService, PrefabAsset};
use crate::serialization::{AssetPrefabs, SerializationService};
use khora_core::asset::{AssetUUID, Material, StandardMaterial};
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::{LinearRgba, Quaternion};
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_data::ecs::{MaterialRef, MeshRef, Name, Parent, PrefabInstance, Transform, World};
use khora_data::scene::record::ReportKind;
use khora_data::scene::{apply, instantiate_prefab, read_scene_file, Identity, SceneRecord};
use khora_telemetry::MetricsRegistry;
use serde_json::{json, Value};
use tempfile::TempDir;

// ─── Fixtures ───────────────────────────────────────────────────────────

/// Where the fixture file sits in its project; its stem names an unnamed
/// scene's root.
const FILE: &str = "models/two.glb";
const FILE_STEM: &str = "two";

const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

const RED: [f32; 4] = [0.8, 0.1, 0.1, 1.0];
const BLUE: [f32; 4] = [0.1, 0.2, 0.9, 1.0];
const GREEN: [f32; 4] = [0.1, 0.9, 0.2, 1.0];

const A_TRANSLATION: [f32; 3] = [10.0, 0.0, 0.0];
/// A quarter turn about +Z, glTF order (x, y, z, w).
const B_ROTATION: [f32; 4] = [0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2];
const B_SCALE: [f32; 3] = [1.0, 2.0, 3.0];

/// An indexed triangle primitive, no material.
fn triangle(b: &mut GltfBuilder) -> Value {
    let pos = b.positions(&TRI);
    let idx = b.indices(&[0, 1, 2]);
    primitive(&[("POSITION", pos)], Some(idx))
}

/// The two-node, two-material file, and the edits a re-export makes to it.
///
/// Node `A` (translated by `a_translation`) shows mesh 0: a triangle in
/// material 0 (`Red`). Its child `B` (rotated by [`B_ROTATION`], scaled by
/// [`B_SCALE`]) shows mesh 1: a triangle in material 1 (`Blue`, coloured
/// `blue`). One scene, the default, lists `A`.
#[derive(Clone, Copy)]
struct TwoNodes {
    a_name: &'static str,
    b_name: &'static str,
    a_translation: [f32; 3],
    blue: [f32; 4],
    /// `B` written before `A` in the file's node array.
    b_first: bool,
    scene_name: Option<&'static str>,
}

impl Default for TwoNodes {
    fn default() -> Self {
        Self {
            a_name: "A",
            b_name: "B",
            a_translation: A_TRANSLATION,
            blue: BLUE,
            b_first: false,
            scene_name: None,
        }
    }
}

impl TwoNodes {
    fn build(self) -> GltfBuilder {
        let mut b = GltfBuilder::new();
        let tri_red = triangle(&mut b);
        let tri_blue = triangle(&mut b);
        let red = b.material(json!({
            "name": "Red",
            "pbrMetallicRoughness": { "baseColorFactor": RED },
        }));
        let blue = b.material(json!({
            "name": "Blue",
            "pbrMetallicRoughness": { "baseColorFactor": self.blue },
        }));
        let m0 = b.mesh(vec![with_material(tri_red, red)]);
        let m1 = b.mesh(vec![with_material(tri_blue, blue)]);
        let node_b = json!({
            "name": self.b_name,
            "mesh": m1,
            "rotation": B_ROTATION,
            "scale": B_SCALE,
        });
        let a = if self.b_first {
            let child = b.node(node_b);
            b.node(json!({
                "name": self.a_name,
                "mesh": m0,
                "translation": self.a_translation,
                "children": [child],
            }))
        } else {
            let a = b.node(json!({
                "name": self.a_name,
                "mesh": m0,
                "translation": self.a_translation,
                "children": [1],
            }));
            b.node(node_b);
            a
        };
        let s = match self.scene_name {
            Some(name) => b.scene_named(name, &[a]),
            None => b.scene(&[a]),
        };
        b.default_scene(s);
        b
    }

    fn glb(self) -> Vec<u8> {
        self.build().build_glb()
    }
}

/// The uuid of the file at `rel` (no registry: the path default).
fn file_uuid(rel: &str) -> AssetUUID {
    AssetUUID::new_v5(rel)
}

/// The uuid of the sub-asset `label` of [`FILE`].
fn sub(label: &str) -> AssetUUID {
    AssetUUID::sub_asset(file_uuid(FILE), label)
}

/// Extracts the scene sub-asset `label` of `bytes`, opened as the file at
/// `path`.
fn try_scene(bytes: &[u8], path: &str, label: &str) -> anyhow::Result<SceneRecord> {
    let dir = tempfile::tempdir().expect("a temp dir");
    let container = GltfContainer::new(dir.path());
    let opened = container
        .open(bytes, Path::new(path))
        .unwrap_or_else(|e| panic!("the glTF opens: {e:#}"));
    let extracted = container.extract(opened.as_ref(), file_uuid(path), label, "prefab")?;
    Ok(extracted
        .downcast::<PrefabAsset>()
        .expect("a scene sub-asset is a PrefabAsset")
        .0)
}

/// The scene sub-asset `label` of `bytes`, as [`FILE`].
fn scene_labelled(bytes: &[u8], label: &str) -> SceneRecord {
    try_scene(bytes, FILE, label).unwrap_or_else(|e| panic!("'{label}' extracts: {e:#}"))
}

/// The default scene of `bytes`, as [`FILE`].
fn scene_of(bytes: &[u8]) -> SceneRecord {
    scene_labelled(bytes, "scene")
}

/// The key of the scene's root.
fn root_key() -> PersistentId {
    PersistentId::named("gltf-node:")
}

/// The key of the node at `path` from the scene's root: each segment (split
/// on the `/` its escaping leaves bare) composed onto its parent's key, from
/// the root's.
fn key(path: &str) -> PersistentId {
    let mut segments = Vec::new();
    let mut segment = String::new();
    let mut escaped = false;
    for c in path.chars() {
        match c {
            '/' if !escaped => segments.push(std::mem::take(&mut segment)),
            '\\' if !escaped => {
                escaped = true;
                segment.push(c);
                continue;
            }
            _ => segment.push(c),
        }
        escaped = false;
    }
    segments.push(segment);
    segments.iter().fold(root_key(), |parent, segment| {
        PersistentId::named(&format!("gltf-node:{:016x}/{segment}", parent.to_bits()))
    })
}

/// `record` brought into a world of its own, its keys kept.
fn opened(record: &SceneRecord) -> World {
    let mut world = World::new();
    apply(&mut world, record, Identity::Keep)
        .unwrap_or_else(|e| panic!("the scene record applies: {e}"));
    world
}

fn at(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the world is {id:?}"))
}

fn name_of(world: &World, id: PersistentId) -> String {
    world
        .get::<Name>(at(world, id))
        .unwrap_or_else(|| panic!("{id:?} has a name"))
        .as_str()
        .to_owned()
}

fn parent_of(world: &World, id: PersistentId) -> Option<PersistentId> {
    world
        .get::<Parent>(at(world, id))
        .and_then(|parent| world.persistent_id(parent.0))
}

fn transform_of(world: &World, id: PersistentId) -> Transform {
    *world
        .get::<Transform>(at(world, id))
        .unwrap_or_else(|| panic!("{id:?} has a transform"))
}

fn mesh_of(world: &World, id: PersistentId) -> Option<AssetUUID> {
    match world.get::<MeshRef>(at(world, id)) {
        Some(MeshRef::Asset(uuid)) => Some(*uuid),
        Some(other) => panic!("{id:?} shows a mesh that is no asset: {other:?}"),
        None => None,
    }
}

fn material_of(world: &World, id: PersistentId) -> Option<AssetUUID> {
    match world.get::<MaterialRef>(at(world, id)) {
        Some(MaterialRef::Asset(uuid)) => Some(*uuid),
        Some(_) => panic!("{id:?} shows an inline material"),
        None => None,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

fn assert_translation(t: &Transform, expected: [f32; 3], what: &str) {
    assert_vec3(t.translation, v3(expected), &format!("{what}: translation"));
}

fn assert_scale(t: &Transform, expected: [f32; 3], what: &str) {
    assert_vec3(t.scale, v3(expected), &format!("{what}: scale"));
}

/// The same rotation: `q` and `-q` turn alike.
fn assert_rotation(actual: Quaternion, xyzw: [f32; 4], what: &str) {
    let expected = Quaternion::new(xyzw[0], xyzw[1], xyzw[2], xyzw[3]);
    assert!(
        close(actual.dot(expected).abs(), 1.0),
        "{what}: rotation {actual:?} != {expected:?}"
    );
}

fn assert_identity(t: &Transform, what: &str) {
    assert_translation(t, [0.0; 3], what);
    assert_rotation(t.rotation, [0.0, 0.0, 0.0, 1.0], what);
    assert_scale(t, [1.0; 3], what);
}

// ─── Record shape ───────────────────────────────────────────────────────

/// The record of a two-node, two-material file: entity 0 is the root, then
/// each node in the file's tree order, each under its parent, with its own
/// transform and its primitive's mesh and material sub-assets.
#[test]
fn a_scene_record_is_its_root_then_each_node_with_parent_transform_and_sub_assets() {
    let record = scene_of(&TwoNodes::default().glb());
    assert_eq!(
        record.entities,
        vec![root_key(), key("A[0]"), key("A[0]/B[0]")],
        "the root first, then the nodes from the scene's root down"
    );
    let world = opened(&record);
    assert_eq!(world.iter_entities().count(), 3, "the root and two nodes");

    let root = root_key();
    assert_eq!(name_of(&world, root), FILE_STEM, "an unnamed scene's root");
    assert_eq!(parent_of(&world, root), None, "the root hangs from nothing");
    assert_identity(&transform_of(&world, root), "root");
    assert_eq!(mesh_of(&world, root), None, "the root shows no mesh");

    let a = key("A[0]");
    assert_eq!(name_of(&world, a), "A");
    assert_eq!(parent_of(&world, a), Some(root));
    let t = transform_of(&world, a);
    assert_translation(&t, A_TRANSLATION, "A");
    assert_rotation(t.rotation, [0.0, 0.0, 0.0, 1.0], "A");
    assert_scale(&t, [1.0; 3], "A");
    assert_eq!(mesh_of(&world, a), Some(sub("mesh0/prim0")), "A's mesh");
    assert_eq!(
        material_of(&world, a),
        Some(sub("material0")),
        "A's material"
    );

    let b = key("A[0]/B[0]");
    assert_eq!(name_of(&world, b), "B");
    assert_eq!(parent_of(&world, b), Some(a), "B hangs from A");
    let t = transform_of(&world, b);
    assert_translation(&t, [0.0; 3], "B");
    assert_rotation(t.rotation, B_ROTATION, "B");
    assert_scale(&t, B_SCALE, "B");
    assert_eq!(mesh_of(&world, b), Some(sub("mesh1/prim0")), "B's mesh");
    assert_eq!(
        material_of(&world, b),
        Some(sub("material1")),
        "B's material"
    );
}

/// The root is named after its scene; an unnamed scene's root after the
/// file.
#[test]
fn the_root_is_named_after_the_scene_else_the_file_stem() {
    let named = scene_of(
        &TwoNodes {
            scene_name: Some("Garage"),
            ..TwoNodes::default()
        }
        .glb(),
    );
    assert_eq!(name_of(&opened(&named), root_key()), "Garage");

    let unnamed = TwoNodes::default().glb();
    let record = try_scene(&unnamed, "models/vehicles/truck.glb", "scene")
        .unwrap_or_else(|e| panic!("the scene extracts: {e:#}"));
    assert_eq!(name_of(&opened(&record), root_key()), "truck");
}

/// A node placed by a `matrix` gets the translation, rotation and scale the
/// matrix is made of.
#[test]
fn a_matrix_node_decomposes_to_its_translation_rotation_and_scale() {
    // Translation (1, 2, 3), a quarter turn about +Y, scale (2, 3, 4),
    // column-major: x → -z, y → y, z → x, each column scaled.
    let matrix = [
        0.0, 0.0, -2.0, 0.0, //
        0.0, 3.0, 0.0, 0.0, //
        4.0, 0.0, 0.0, 0.0, //
        1.0, 2.0, 3.0, 1.0,
    ];
    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let n = b.node(json!({ "name": "M", "mesh": m, "matrix": matrix }));
    let s = b.scene(&[n]);
    b.default_scene(s);

    let world = opened(&scene_of(&b.build_glb()));
    let t = transform_of(&world, key("M[0]"));
    assert_translation(&t, [1.0, 2.0, 3.0], "M");
    assert_rotation(t.rotation, [0.0, FRAC_1_SQRT_2, 0.0, FRAC_1_SQRT_2], "M");
    assert_scale(&t, [2.0, 3.0, 4.0], "M");
}

/// A primitive that names no material shows the file's `material_default`.
#[test]
fn a_primitive_without_material_shows_material_default() {
    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let n = b.node(json!({ "name": "Plain", "mesh": m }));
    let s = b.scene(&[n]);
    b.default_scene(s);

    let world = opened(&scene_of(&b.build_glb()));
    let plain = key("Plain[0]");
    assert_eq!(mesh_of(&world, plain), Some(sub("mesh0/prim0")));
    assert_eq!(material_of(&world, plain), Some(sub("material_default")));
}

/// A node shows its mesh's primitive 0 itself; each further primitive is a
/// child entity `prim{p}` at the node's origin, with its own mesh and
/// material, keyed by the node's path and `#prim{p}`.
#[test]
fn a_node_shows_primitive_zero_and_a_prim_child_per_further_primitive() {
    let mut b = GltfBuilder::new();
    let first = triangle(&mut b);
    let second = triangle(&mut b);
    let red = b.material(json!({ "pbrMetallicRoughness": { "baseColorFactor": RED } }));
    let blue = b.material(json!({ "pbrMetallicRoughness": { "baseColorFactor": BLUE } }));
    let m = b.mesh(vec![with_material(first, red), with_material(second, blue)]);
    let car = b.node(json!({ "name": "Car", "mesh": m, "translation": [5.0, 0.0, 0.0] }));
    let s = b.scene(&[car]);
    b.default_scene(s);

    let record = scene_of(&b.build_glb());
    assert_eq!(
        record.entities,
        vec![root_key(), key("Car[0]"), key("Car[0]/#prim1")]
    );
    let world = opened(&record);

    let car = key("Car[0]");
    assert_eq!(mesh_of(&world, car), Some(sub("mesh0/prim0")));
    assert_eq!(material_of(&world, car), Some(sub("material0")));

    let prim1 = key("Car[0]/#prim1");
    assert_eq!(name_of(&world, prim1), "prim1");
    assert_eq!(parent_of(&world, prim1), Some(car));
    assert_identity(&transform_of(&world, prim1), "prim1");
    assert_eq!(mesh_of(&world, prim1), Some(sub("mesh0/prim1")));
    assert_eq!(material_of(&world, prim1), Some(sub("material1")));
}

// ─── Node keys ──────────────────────────────────────────────────────────

/// The key of every entity of `record`, by its name.
fn keys_by_name(record: &SceneRecord) -> Vec<(String, PersistentId)> {
    let world = opened(record);
    let mut keyed: Vec<(String, PersistentId)> = record
        .entities
        .iter()
        .map(|id| (name_of(&world, *id), *id))
        .collect();
    keyed.sort();
    keyed
}

/// A file whose scene lists `roots` (name, children's names) in that
/// order; nodes are written to the file's array in `file_order`.
fn named_tree(roots: &[(&str, &[&str])], file_order: &[&str]) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let index = |name: &str| {
        file_order
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("{name} is in the file order"))
    };
    let mut nodes: Vec<Value> = file_order
        .iter()
        .map(|name| json!({ "name": name, "mesh": m }))
        .collect();
    for (name, children) in roots {
        if !children.is_empty() {
            let children: Vec<usize> = children.iter().map(|c| index(c)).collect();
            nodes[index(name)]["children"] = json!(children);
        }
    }
    for node in nodes {
        b.node(node);
    }
    let roots: Vec<usize> = roots.iter().map(|(name, _)| index(name)).collect();
    let s = b.scene(&roots);
    b.default_scene(s);
    b.build_glb()
}

/// Siblings reordered — in the scene, in the children list and in the
/// file's node array — keep their keys: a key is a name path, not an index.
#[test]
fn node_keys_follow_names_not_file_order() {
    let before = named_tree(&[("X", &[]), ("Y", &["Z", "W"])], &["X", "Y", "Z", "W"]);
    let after = named_tree(&[("Y", &["W", "Z"]), ("X", &[])], &["W", "Z", "Y", "X"]);

    let expected = {
        let mut keyed = vec![
            (FILE_STEM.to_owned(), root_key()),
            ("W".to_owned(), key("Y[0]/W[0]")),
            ("X".to_owned(), key("X[0]")),
            ("Y".to_owned(), key("Y[0]")),
            ("Z".to_owned(), key("Y[0]/Z[0]")),
        ];
        keyed.sort();
        keyed
    };
    assert_eq!(keys_by_name(&scene_of(&before)), expected, "before");
    assert_eq!(keys_by_name(&scene_of(&after)), expected, "reordered");
}

/// Siblings sharing a name are told apart by their ordinal among the
/// earlier siblings of that name; a unique name is `[0]`.
#[test]
fn duplicate_sibling_names_take_their_ordinal() {
    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let wheel_0 = b.node(json!({ "name": "Wheel", "mesh": m, "translation": [1.0, 0.0, 0.0] }));
    let hub = b.node(json!({ "name": "Hub", "mesh": m }));
    let wheel_1 = b.node(json!({ "name": "Wheel", "mesh": m, "translation": [2.0, 0.0, 0.0] }));
    let door_0 = b.node(json!({ "name": "Door", "translation": [3.0, 0.0, 0.0] }));
    let door_1 = b.node(json!({ "name": "Door", "translation": [4.0, 0.0, 0.0] }));
    let body = b.node(json!({ "name": "Body", "children": [door_0, door_1] }));
    let s = b.scene(&[wheel_0, hub, wheel_1, body]);
    b.default_scene(s);

    let world = opened(&scene_of(&b.build_glb()));
    for (path, x) in [
        ("Wheel[0]", 1.0),
        ("Wheel[1]", 2.0),
        ("Body[0]/Door[0]", 3.0),
        ("Body[0]/Door[1]", 4.0),
    ] {
        assert_translation(&transform_of(&world, key(path)), [x, 0.0, 0.0], path);
    }
    assert_eq!(name_of(&world, key("Hub[0]")), "Hub");
    assert_eq!(name_of(&world, key("Body[0]")), "Body");
}

/// An unnamed node is keyed `#i` by its index among its siblings, and named
/// `Node{i}` by its index in the file.
#[test]
fn unnamed_nodes_are_keyed_by_their_index_among_siblings() {
    let mut b = GltfBuilder::new();
    // File order: Named (0), its unnamed child (1), two unnamed roots (2, 3).
    let named = b.node(json!({ "name": "Named", "children": [1] }));
    b.node(json!({ "translation": [1.0, 0.0, 0.0] }));
    let first = b.node(json!({ "translation": [2.0, 0.0, 0.0] }));
    let last = b.node(json!({ "translation": [3.0, 0.0, 0.0] }));
    let s = b.scene(&[first, named, last]);
    b.default_scene(s);

    let world = opened(&scene_of(&b.build_glb()));
    for (path, x, name) in [
        ("#0", 2.0, "Node2"),
        ("#2", 3.0, "Node3"),
        ("Named[0]/#0", 1.0, "Node1"),
    ] {
        assert_translation(&transform_of(&world, key(path)), [x, 0.0, 0.0], path);
        assert_eq!(name_of(&world, key(path)), name, "{path}");
    }
    assert_eq!(name_of(&world, key("Named[0]")), "Named");
}

/// `\`, `/`, `[`, `]` and `#` in a name are escaped with a `\`.
#[test]
fn node_names_escape_path_characters() {
    let mut b = GltfBuilder::new();
    let roots: Vec<usize> = ["a/b", "c[1]", "#2", "back\\slash"]
        .iter()
        .map(|name| b.node(json!({ "name": name })))
        .collect();
    let s = b.scene(&roots);
    b.default_scene(s);

    let world = opened(&scene_of(&b.build_glb()));
    for (path, name) in [
        ("a\\/b[0]", "a/b"),
        ("c\\[1\\][0]", "c[1]"),
        ("\\#2[0]", "#2"),
        ("back\\\\slash[0]", "back\\slash"),
    ] {
        assert_eq!(name_of(&world, key(path)), name, "{path}");
    }
}

/// The keys of a record's entities, but its root.
fn node_keys(record: &SceneRecord) -> HashSet<PersistentId> {
    record.entities[1..].iter().copied().collect()
}

/// A file whose scene lists one node per `roots` entry (a name, or `None`
/// for an unnamed node), each with the named children given.
fn tree(roots: &[(Option<&str>, &[&str])]) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let mut root_indices = Vec::new();
    for (name, children) in roots {
        let children: Vec<usize> = children
            .iter()
            .map(|child| b.node(json!({ "name": child })))
            .collect();
        let mut node = json!({ "children": children });
        if let Some(name) = name {
            node["name"] = json!(name);
        }
        root_indices.push(b.node(node));
    }
    let s = b.scene(&root_indices);
    b.default_scene(s);
    b.build_glb()
}

/// Two different trees never share a node key, whatever their names hold.
#[test]
fn distinct_trees_never_share_a_node_key() {
    for (case, one, other) in [
        (
            "a name holding '/' against a child",
            tree(&[(Some("a/b"), &[])]),
            tree(&[(Some("a"), &["b"])]),
        ),
        (
            "a name holding an ordinal and a '/' against a child",
            tree(&[(Some("x[0]/y"), &[])]),
            tree(&[(Some("x"), &["y"])]),
        ),
        (
            "a name holding '#' against an unnamed node",
            tree(&[(Some("#0"), &[])]),
            tree(&[(None, &[])]),
        ),
        (
            "a name ending in '\\' against a child",
            tree(&[(Some("a\\"), &["b"])]),
            tree(&[(Some("a\\/b"), &[])]),
        ),
    ] {
        let one = node_keys(&scene_of(&one));
        let other = node_keys(&scene_of(&other));
        assert!(
            one.is_disjoint(&other),
            "{case}: shared keys {:?}",
            one.intersection(&other).collect::<Vec<_>>()
        );
    }
}

/// A child node named like a primitive child is not that primitive child.
#[test]
fn a_node_named_like_a_primitive_child_has_another_key() {
    let mut b = GltfBuilder::new();
    let first = triangle(&mut b);
    let second = triangle(&mut b);
    let m = b.mesh(vec![first, second]);
    let car = b.node(json!({ "name": "Car", "mesh": m }));
    let s = b.scene(&[car]);
    b.default_scene(s);
    let with_primitive = node_keys(&scene_of(&b.build_glb()));

    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let child = b.node(json!({ "name": "#prim1" }));
    let car = b.node(json!({ "name": "Car", "mesh": m, "children": [child] }));
    let s = b.scene(&[car]);
    b.default_scene(s);
    let with_node = node_keys(&scene_of(&b.build_glb()));

    assert!(with_primitive.contains(&key("Car[0]/#prim1")));
    assert!(with_node.contains(&key("Car[0]/\\#prim1[0]")));
    assert!(!with_node.contains(&key("Car[0]/#prim1")));
}

// ─── Which scene ────────────────────────────────────────────────────────

/// Two named scenes, `First` showing node `P` and `Second` showing node
/// `Q`; `default` flags one of them.
fn two_scenes(default: Option<usize>) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let tri = triangle(&mut b);
    let m = b.mesh(vec![tri]);
    let p = b.node(json!({ "name": "P", "mesh": m }));
    let q = b.node(json!({ "name": "Q", "mesh": m }));
    b.scene_named("First", &[p]);
    b.scene_named("Second", &[q]);
    if let Some(scene) = default {
        b.default_scene(scene);
    }
    b.build_glb()
}

/// `scene{i}` is scene `i`; `scene` is the one the file flags as default.
#[test]
fn scene_is_the_default_scene_and_scene_i_is_scene_i() {
    let bytes = two_scenes(Some(1));
    for (label, root_name, node) in [
        ("scene", "Second", "Q[0]"),
        ("scene0", "First", "P[0]"),
        ("scene1", "Second", "Q[0]"),
    ] {
        let record = scene_labelled(&bytes, label);
        assert_eq!(record.entities, vec![root_key(), key(node)], "{label}");
        assert_eq!(name_of(&opened(&record), root_key()), root_name, "{label}");
    }
}

/// A file that flags no scene as default shows its first scene as `scene`.
#[test]
fn without_a_default_scene_scene_is_the_first() {
    let record = scene_of(&two_scenes(None));
    assert_eq!(record.entities, vec![root_key(), key("P[0]")]);
    assert_eq!(name_of(&opened(&record), root_key()), "First");
}

/// A file without scenes shows each mesh once, at the origin under the
/// root, named after the mesh (else `Mesh{m}`), keyed `#mesh{m}`.
#[test]
fn a_file_without_scenes_shows_each_mesh_once_named_after_it() {
    let mut b = GltfBuilder::new();
    let hull = triangle(&mut b);
    let other = triangle(&mut b);
    b.mesh_named("Hull", vec![hull]);
    b.mesh(vec![other]);

    let record = scene_of(&b.build_glb());
    assert_eq!(
        record.entities,
        vec![root_key(), key("#mesh0"), key("#mesh1")]
    );
    let world = opened(&record);
    assert_eq!(name_of(&world, root_key()), FILE_STEM);
    for (m, name) in [(0, "Hull"), (1, "Mesh1")] {
        let id = key(&format!("#mesh{m}"));
        assert_eq!(name_of(&world, id), name);
        assert_eq!(parent_of(&world, id), Some(root_key()), "{name}");
        assert_identity(&transform_of(&world, id), name);
        assert_eq!(
            mesh_of(&world, id),
            Some(sub(&format!("mesh{m}/prim0"))),
            "{name}"
        );
        assert_eq!(
            material_of(&world, id),
            Some(sub("material_default")),
            "{name}"
        );
    }
}

// ─── Refusals and skipped features ──────────────────────────────────────

/// A matrix with shear has no translation, rotation and scale: the scene is
/// refused, naming the node.
#[test]
fn a_shear_matrix_is_refused_naming_the_node() {
    let mut b = GltfBuilder::new();
    // Nodes 0..3 sit outside the scene, so the sheared node is node 4.
    for _ in 0..4 {
        b.node(json!({}));
    }
    let shear = [
        1.0, 0.0, 0.0, 0.0, //
        0.5, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let sheared = b.node(json!({ "name": "Sheared", "matrix": shear }));
    assert_eq!(sheared, 4);
    let s = b.scene(&[sheared]);
    b.default_scene(s);

    let err = match try_scene(&b.build_glb(), FILE, "scene") {
        Ok(record) => panic!("a sheared node was imported: {record:?}"),
        Err(e) => format!("{e:#}"),
    };
    assert!(err.contains('4'), "the error names node 4: {err}");
}

/// A scene the file does not have is refused, naming it.
#[test]
fn a_scene_index_out_of_range_is_refused() {
    let bytes = TwoNodes::default().glb();
    let err = match try_scene(&bytes, FILE, "scene1") {
        Ok(record) => panic!("the file has one scene, yet scene1 extracted: {record:?}"),
        Err(e) => format!("{e:#}"),
    };
    assert!(
        err.contains("scene1") || err.contains("scene 1"),
        "the error names the scene: {err}"
    );
}

/// A node listed under two parents, or reached again from itself, is
/// refused: a node is one entity.
#[test]
fn a_node_reached_twice_is_refused() {
    let two_parents = {
        let mut b = GltfBuilder::new();
        let shared = b.node(json!({ "name": "Shared" }));
        let left = b.node(json!({ "name": "Left", "children": [shared] }));
        let right = b.node(json!({ "name": "Right", "children": [shared] }));
        let s = b.scene(&[left, right]);
        b.default_scene(s);
        b.build_glb()
    };
    let listed_twice = {
        let mut b = GltfBuilder::new();
        let n = b.node(json!({ "name": "Twice" }));
        let s = b.scene(&[n, n]);
        b.default_scene(s);
        b.build_glb()
    };
    for (case, bytes) in [("two parents", two_parents), ("listed twice", listed_twice)] {
        let dir = tempfile::tempdir().expect("a temp dir");
        let container = GltfContainer::new(dir.path());
        // Refused when opened or when the scene is extracted: never a record
        // that brings one node twice.
        if let Ok(opened) = container.open(&bytes, Path::new(FILE)) {
            assert!(
                container
                    .extract(opened.as_ref(), file_uuid(FILE), "scene", "prefab")
                    .is_err(),
                "{case}: a node reached twice was imported"
            );
        }
    }
}

/// The two-node file with a camera on `A`, a skin and a morph target
/// weight on `B`, and a punctual light on `A` — or without them.
fn two_nodes_with_extras(extras: bool) -> Vec<u8> {
    let mut b = TwoNodes::default().build();
    if extras {
        b.extension_used("KHR_lights_punctual");
        b.top_level(
            "cameras",
            json!([{ "type": "perspective", "perspective": { "yfov": 1.0, "znear": 0.1 } }]),
        );
        b.top_level("skins", json!([{ "joints": [0, 1] }]));
        b.top_level(
            "extensions",
            json!({ "KHR_lights_punctual": { "lights": [{ "type": "point" }] } }),
        );
    }
    let mut glb = b.build_glb();
    if extras {
        glb = with_node_fields(
            &glb,
            &[
                (
                    0,
                    json!({
                        "camera": 0,
                        "extensions": { "KHR_lights_punctual": { "light": 0 } },
                    }),
                ),
                (1, json!({ "skin": 0, "weights": [0.5] })),
            ],
        );
    }
    glb
}

/// `glb` with `fields` written into the nodes they name, by index.
fn with_node_fields(glb: &[u8], fields: &[(usize, Value)]) -> Vec<u8> {
    let json_len = u32::from_le_bytes(glb[12..16].try_into().expect("a chunk length")) as usize;
    let mut root: Value =
        serde_json::from_slice(&glb[20..20 + json_len]).expect("the GLB's JSON chunk");
    for (node, extra) in fields {
        let node = root["nodes"][*node]
            .as_object_mut()
            .expect("the node is an object");
        for (key, value) in extra.as_object().expect("fields are an object") {
            node.insert(key.clone(), value.clone());
        }
    }
    let mut json = serde_json::to_vec(&root).expect("the JSON serialises");
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let bin = &glb[20 + json_len..];
    let total = 12 + 8 + json.len() + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&glb[0..8]);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&glb[16..20]);
    out.extend_from_slice(&json);
    out.extend_from_slice(bin);
    out
}

/// Cameras, skins, morph target weights and lights are not imported: the
/// scene decodes to the record of the same file without them.
#[test]
fn cameras_skins_weights_and_lights_are_skipped() {
    let plain = scene_of(&two_nodes_with_extras(false));
    let with_extras = scene_of(&two_nodes_with_extras(true));
    assert_eq!(with_extras, plain);
}

// ─── Through the asset service, as a prefab ─────────────────────────────

/// A project on disk: `<tmp>/assets/…`.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    assets: PathBuf,
}

impl Project {
    /// A project holding `bytes` at [`FILE`].
    fn with_file(bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().to_path_buf();
        let assets = root.join("assets");
        let project = Self {
            _dir: dir,
            root,
            assets,
        };
        project.write(bytes);
        project
    }

    /// Writes `bytes` at [`FILE`], as an exporter does.
    fn write(&self, bytes: &[u8]) {
        let path = self.assets.join(FILE);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
        std::fs::write(path, bytes).expect("the file is written");
    }

    /// The asset service over the project as it is on disk now, shared.
    fn open(&self) -> Arc<Mutex<AssetService>> {
        let service = AssetService::open_loose_files(
            &self.root,
            &AssetIdRegistry::load(&self.root),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        Arc::new(Mutex::new(service))
    }
}

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

fn standard_material(assets: &Mutex<AssetService>, uuid: AssetUUID) -> StandardMaterial {
    let mut assets = assets.lock().expect("the asset service");
    let handle = assets
        .load::<Box<dyn Material>>(&uuid)
        .unwrap_or_else(|e| panic!("{uuid} loads as a material: {e:#}"));
    let material: &dyn Material = &**handle;
    material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .expect("a glTF material is a StandardMaterial")
        .clone()
}

fn assert_colour(actual: LinearRgba, expected: [f32; 4], what: &str) {
    assert!(
        close(actual.r, expected[0])
            && close(actual.g, expected[1])
            && close(actual.b, expected[2])
            && close(actual.a, expected[3]),
        "{what}: {actual:?} != {expected:?}"
    );
}

/// The scene sub-asset loads, through the service, as the prefab record of
/// the scene.
#[test]
fn the_scene_sub_asset_loads_as_a_prefab_through_the_service() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let record = assets
        .lock()
        .expect("the asset service")
        .load::<PrefabAsset>(&sub("scene"))
        .unwrap_or_else(|e| panic!("the scene loads as a prefab: {e:#}"))
        .0
        .clone();
    assert_eq!(
        record.entities,
        vec![root_key(), key("A[0]"), key("A[0]/B[0]")]
    );
    assert_eq!(
        record,
        scene_of(&TwoNodes::default().glb()),
        "the service extracts what the container does"
    );
}

/// A level with one instance of the glTF's default scene, as dropped in the
/// editor, and the instance root's identity.
fn level(assets: &Arc<Mutex<AssetService>>) -> (World, PersistentId) {
    let prefabs = AssetPrefabs::new(Arc::clone(assets));
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, sub("scene"), &prefabs)
        .unwrap_or_else(|e| panic!("the scene instantiates: {e}"));
    let instance = id(&world, root);
    (world, instance)
}

/// Instantiating the scene sub-asset spawns its root, linked to the scene,
/// and each node under it, keyed within the instance.
#[test]
fn instantiating_the_scene_spawns_its_root_and_nodes_linked_to_the_scene() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let (world, instance) = level(&assets);

    assert_eq!(world.iter_entities().count(), 3, "the root and two nodes");
    let root = at(&world, instance);
    assert_eq!(
        world.get::<PrefabInstance>(root).map(|link| link.prefab),
        Some(sub("scene")),
        "the root links to the scene sub-asset"
    );
    assert_eq!(name_of(&world, instance), FILE_STEM);

    let a = PersistentId::within(instance, key("A[0]"));
    let b = PersistentId::within(instance, key("A[0]/B[0]"));
    assert_eq!(name_of(&world, a), "A");
    assert_eq!(parent_of(&world, a), Some(instance));
    assert_eq!(mesh_of(&world, a), Some(sub("mesh0/prim0")));
    assert_eq!(material_of(&world, a), Some(sub("material0")));
    assert_eq!(name_of(&world, b), "B");
    assert_eq!(parent_of(&world, b), Some(a));
    assert_eq!(mesh_of(&world, b), Some(sub("mesh1/prim0")));
    assert_eq!(material_of(&world, b), Some(sub("material1")));
}

/// The service saving with the project's prefabs.
fn service(assets: &Arc<Mutex<AssetService>>) -> SerializationService {
    SerializationService::with_prefabs(Arc::new(AssetPrefabs::new(Arc::clone(assets))))
}

/// Saving a level holding the glTF's instance writes a link to the scene
/// sub-asset, and none of the nodes.
#[test]
fn a_saved_level_keeps_the_gltf_instance_as_a_link() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let (world, instance) = level(&assets);

    let file = service(&assets)
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the level saves");
    let record = read_scene_file(&file).expect("the level reads");
    assert_eq!(record.instances.len(), 1, "the instance is a link");
    assert_eq!(record.instances[0].prefab, sub("scene"));
    assert_eq!(record.instances[0].root, instance);
    for node in [key("A[0]"), key("A[0]/B[0]")] {
        let member = PersistentId::within(instance, node);
        assert!(
            !record.pages.iter().any(|page| page.rows.contains(&member)),
            "the level holds a node's row: {node:?}"
        );
    }
}

/// The level, saved with a designer's override on `B`: its translation
/// moved to (7, 7, 7).
fn level_with_override_on_b(assets: &Arc<Mutex<AssetService>>) -> (SceneFile, PersistentId) {
    let (mut world, instance) = level(assets);
    let b = at(&world, PersistentId::within(instance, key("A[0]/B[0]")));
    world
        .get_mut::<Transform>(b)
        .expect("B is placed")
        .translation = Vec3::new(7.0, 7.0, 7.0);
    let file = service(assets)
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the level saves");
    (file, instance)
}

/// The glTF re-exported — nodes reordered in the file, `A` moved, material
/// 1 recoloured — and the saved level opened again: the override stays on
/// `B`, which keeps its material; what the designer left alone follows the
/// file.
#[test]
fn a_reexported_gltf_keeps_a_designer_override_on_the_same_node() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let (file, instance) = level_with_override_on_b(&assets);

    project.write(
        &TwoNodes {
            b_first: true,
            a_translation: [20.0, 0.0, 0.0],
            blue: GREEN,
            ..TwoNodes::default()
        }
        .glb(),
    );
    assert!(
        assets
            .lock()
            .expect("the asset service")
            .invalidate(&file_uuid(FILE)),
        "the service held the file's sub-assets"
    );

    let mut world = World::new();
    service(&assets)
        .replace_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the level loads: {e}"));

    let a = PersistentId::within(instance, key("A[0]"));
    let b = PersistentId::within(instance, key("A[0]/B[0]"));
    let t = transform_of(&world, b);
    assert_translation(&t, [7.0, 7.0, 7.0], "B keeps its override");
    assert_rotation(t.rotation, B_ROTATION, "B");
    assert_eq!(
        material_of(&world, b),
        Some(sub("material1")),
        "B's material"
    );
    assert_eq!(mesh_of(&world, b), Some(sub("mesh1/prim0")), "B's mesh");
    assert_translation(
        &transform_of(&world, a),
        [20.0, 0.0, 0.0],
        "A follows the re-export",
    );
    assert_colour(
        standard_material(&assets, sub("material1")).base_color,
        GREEN,
        "material 1 after the re-export",
    );
}

/// The glTF re-exported with `B` renamed: `B`'s override has no node to
/// land on any more — the renamed node comes in as the file has it, and
/// the load reports the override's node gone.
#[test]
fn a_renamed_node_orphans_its_override_and_the_load_reports_it() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let (file, instance) = level_with_override_on_b(&assets);

    project.write(
        &TwoNodes {
            b_name: "Bee",
            ..TwoNodes::default()
        }
        .glb(),
    );
    assets
        .lock()
        .expect("the asset service")
        .invalidate(&file_uuid(FILE));

    let mut world = World::new();
    let report = service(&assets)
        .replace_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the level loads: {e}"));

    let old_b = PersistentId::within(instance, key("A[0]/B[0]"));
    let bee = PersistentId::within(instance, key("A[0]/Bee[0]"));
    assert!(
        world.entity_with_id(old_b).is_none(),
        "the node under its old name is gone"
    );
    assert_eq!(name_of(&world, bee), "Bee");
    assert_translation(
        &transform_of(&world, bee),
        [0.0; 3],
        "the renamed node is placed as the file has it",
    );
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.entity == Some(old_b) && entry.kind == ReportKind::RemovedFromScene),
        "the load reports the orphaned override: {:?}",
        report.entries
    );
}

// ─── Matrices that are translation, rotation and scale ──────────────────

/// The column-major matrix `T · R · S`, computed in double precision as an
/// exporter writes it; `axis` need not be unit.
fn trs_matrix(translation: [f64; 3], axis: [f64; 3], angle: f64, scale: [f64; 3]) -> [f32; 16] {
    let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let [x, y, z] = [axis[0] / len, axis[1] / len, axis[2] / len];
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    // The rotation, r[row][column].
    let r = [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ];
    let mut m = [0.0f32; 16];
    for column in 0..3 {
        for row in 0..3 {
            m[column * 4 + row] = (r[row][column] * scale[column]) as f32;
        }
    }
    m[12] = translation[0] as f32;
    m[13] = translation[1] as f32;
    m[14] = translation[2] as f32;
    m[15] = 1.0;
    m
}

/// A one-node file whose node `M` is placed by `matrix`.
fn matrix_node(matrix: &[f32; 16]) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    let n = b.node(json!({ "name": "M", "matrix": matrix }));
    let s = b.scene(&[n]);
    b.default_scene(s);
    b.build_glb()
}

/// A matrix that is a rotation and a scale far from 1 — an exporter's unit
/// conversion (×100 centimetres, ×1000 millimetres, ÷1000) — is no shear:
/// it imports with that scale.
#[test]
fn a_rotated_matrix_with_a_scale_far_from_one_is_not_a_shear() {
    let axes = [[1.0, 2.0, 3.0], [0.0, 1.0, 0.0], [-0.3, 0.2, 0.9]];
    let angles = [0.7, 2.5, -1.9];
    let mut refused = Vec::new();
    for scale in [0.001, 100.0, 1000.0, 10_000.0] {
        for (axis, angle) in axes.iter().zip(angles) {
            let matrix = trs_matrix([5.0, -2.0, 1.0], *axis, angle, [scale; 3]);
            let case = format!("scale {scale}, axis {axis:?}, angle {angle}");
            let record = match try_scene(&matrix_node(&matrix), FILE, "scene") {
                Ok(record) => record,
                Err(e) => {
                    refused.push(format!("{case}: {e:#}"));
                    continue;
                }
            };
            let t = transform_of(&opened(&record), key("M[0]"));
            for got in [t.scale.x, t.scale.y, t.scale.z] {
                assert!(
                    ((got as f64 - scale) / scale).abs() < 1e-4,
                    "{case}: scale {:?}",
                    t.scale
                );
            }
        }
    }
    assert!(refused.is_empty(), "refused:\n{}", refused.join("\n"));
}

/// A matrix that scales an axis to nothing — a node hidden by a zero scale
/// — is a translation, rotation and scale: it imports with that zero.
#[test]
fn a_matrix_scaling_an_axis_to_zero_is_not_a_shear() {
    let matrix = [
        0.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        1.0, 2.0, 3.0, 1.0,
    ];
    let record = try_scene(&matrix_node(&matrix), FILE, "scene")
        .unwrap_or_else(|e| panic!("a zero-scaled node was refused: {e:#}"));
    let t = transform_of(&opened(&record), key("M[0]"));
    assert_translation(&t, [1.0, 2.0, 3.0], "M");
    assert_scale(&t, [0.0, 1.0, 1.0], "M");
}

/// A shear is a shear at any size: the same sheared matrix scaled down to
/// millimetres is refused, as it is at full size, rather than imported
/// with its shear dropped.
#[test]
fn a_shear_in_a_matrix_scaled_down_is_refused() {
    for scale in [1.0f32, 0.001] {
        // Uniform `scale`, the y column leaning 10% along x.
        let mut matrix = [0.0f32; 16];
        matrix[0] = scale;
        matrix[4] = 0.1 * scale;
        matrix[5] = scale;
        matrix[10] = scale;
        matrix[15] = 1.0;
        if let Ok(record) = try_scene(&matrix_node(&matrix), FILE, "scene") {
            let t = transform_of(&opened(&record), key("M[0]"));
            panic!("a shear scaled by {scale} was imported as {t:?}");
        }
    }
}

/// A mirroring matrix (negative determinant) is a rotation and a negative
/// scale: it imports, its mirror and its volume kept.
#[test]
fn a_mirroring_matrix_imports_with_a_negative_scale() {
    let matrix = trs_matrix([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.5, [1.0, 1.0, -2.0]);
    let record = try_scene(&matrix_node(&matrix), FILE, "scene")
        .unwrap_or_else(|e| panic!("a mirroring node was refused: {e:#}"));
    let t = transform_of(&opened(&record), key("M[0]"));
    let product = t.scale.x * t.scale.y * t.scale.z;
    assert!(
        (product + 2.0).abs() < EPS,
        "the mirror and the volume are kept: scale {:?}",
        t.scale
    );
}

/// A node whose numbers overflow a float — `1e39` reads as infinity — has
/// no place: the scene is refused naming the node, as a shear is, rather
/// than bringing in an infinite transform whose children propagate NaN.
#[test]
fn a_node_transform_that_overflows_a_float_is_refused() {
    for (case, field, value) in [
        ("translation", "translation", json!([1e39, 0.0, 0.0])),
        ("scale", "scale", json!([1.0, 1e39, 1.0])),
    ] {
        let mut b = GltfBuilder::new();
        b.node(json!({}));
        let mut node = json!({ "name": "Far" });
        node[field] = value;
        let far = b.node(node);
        assert_eq!(far, 1);
        let s = b.scene(&[far]);
        b.default_scene(s);
        match try_scene(&b.build_glb(), FILE, "scene") {
            Ok(record) => {
                let t = transform_of(&opened(&record), key("Far[0]"));
                panic!("{case}: imported as {t:?}");
            }
            Err(e) => {
                let err = format!("{e:#}");
                assert!(err.contains('1'), "{case}: the error names node 1: {err}");
            }
        }
    }
}

// ─── Shared and deep hierarchies ────────────────────────────────────────

/// A node two scenes both list is in each scene's record: each scene is a
/// tree of its own.
#[test]
fn a_node_listed_by_two_scenes_is_in_both_records() {
    let mut b = GltfBuilder::new();
    let shared = b.node(json!({ "name": "Shared" }));
    let only = b.node(json!({ "name": "Only" }));
    b.scene_named("One", &[shared]);
    b.scene_named("Two", &[shared, only]);
    let bytes = b.build_glb();
    assert_eq!(
        scene_labelled(&bytes, "scene0").entities,
        vec![root_key(), key("Shared[0]")]
    );
    assert_eq!(
        scene_labelled(&bytes, "scene1").entities,
        vec![root_key(), key("Shared[0]"), key("Only[0]")]
    );
}

/// A chain of `depth` unnamed nodes, each the only child of the one before.
fn chain(depth: usize) -> Vec<u8> {
    let mut b = GltfBuilder::new();
    for i in 0..depth {
        if i + 1 < depth {
            b.node(json!({ "children": [i + 1] }));
        } else {
            b.node(json!({}));
        }
    }
    let s = b.scene(&[0]);
    b.default_scene(s);
    b.build_glb()
}

/// A deep hierarchy imports in time proportional to its depth: a node
/// costs neither the walk up to the root nor its whole path again.
#[test]
fn a_deep_node_chain_imports_in_time_proportional_to_its_depth() {
    let time = |depth: usize| {
        let bytes = chain(depth);
        let start = std::time::Instant::now();
        let record = scene_of(&bytes);
        let elapsed = start.elapsed();
        assert_eq!(record.entities.len(), depth + 1);
        elapsed
    };
    let small = time(500);
    let large = time(4_000);
    // Eight times the nodes: linear work takes ~8×, quadratic ~64×.
    assert!(
        large < small * 24,
        "4000 nodes took {large:?}, 500 took {small:?}: the import is quadratic in depth"
    );
}

// ─── A glTF scene inside a prefab ───────────────────────────────────────

/// A `.kprefab` holding an instance of a glTF scene keeps the link: the
/// glTF re-exported, the prefab instantiates its nodes as the file has them
/// now.
#[test]
fn a_prefab_holding_a_gltf_instance_expands_the_file_as_it_is_now() {
    let project = Project::with_file(&TwoNodes::default().glb());
    let assets = project.open();
    let prefabs = AssetPrefabs::new(Arc::clone(&assets));
    let mut world = World::new();
    let car = world.spawn((Name::new("Car"), Transform::identity()));
    world.mark_authored(car).expect("authored");
    let body = instantiate_prefab(&mut world, sub("scene"), &prefabs).expect("the scene");
    assert!(world.set_parent(body, Some(car)));

    let car_uuid = file_uuid("props/car.kprefab");
    let bytes = khora_data::scene::serialize_prefab(&world, car, car_uuid, &prefabs)
        .expect("the car saves");
    let path = project.assets.join("props/car.kprefab");
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
    std::fs::write(&path, bytes).expect("the car is written");
    project.write(
        &TwoNodes {
            a_translation: [20.0, 0.0, 0.0],
            ..TwoNodes::default()
        }
        .glb(),
    );

    let assets = project.open();
    let prefabs = AssetPrefabs::new(Arc::clone(&assets));
    let mut level = World::new();
    instantiate_prefab(&mut level, car_uuid, &prefabs)
        .unwrap_or_else(|e| panic!("the car instantiates: {e}"));
    let named = |name: &str| {
        level
            .iter_entities()
            .find(|e| level.get::<Name>(*e).is_some_and(|n| n.as_str() == name))
            .unwrap_or_else(|| panic!("the level holds {name}"))
    };
    let (a, b) = (named("A"), named("B"));
    assert_eq!(level.iter_entities().count(), 4, "car, scene root, A, B");
    assert_eq!(
        level.get::<MeshRef>(b).cloned(),
        Some(MeshRef::Asset(sub("mesh1/prim0")))
    );
    assert_eq!(level.get::<Parent>(b).map(|p| p.0), Some(a));
    let a_transform = level.get::<Transform>(a).expect("A is placed");
    assert_translation(a_transform, [20.0, 0.0, 0.0], "A after the re-export");
}
