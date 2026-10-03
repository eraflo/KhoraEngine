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

//! Attacks on `cargo xtask assets upgrade-scenes`: hand-built v1 recipes
//! (hierarchies, every material and mesh form, refused components, damaged
//! bytes) and file-system situations (an existing `.v1`, a read-only file,
//! an extension in capitals).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bincode::config::standard;
use khora_core::asset::{
    AssetUUID, EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
};
use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Vec3};
use khora_core::scene::{SceneFile, SceneHeader, HEADER_MAGIC_BYTES};
use khora_io::serialization::SerializationService;
use khora_sdk::khora_data::ecs::{
    Children, MaterialRef, MeshRef, Name, Parent, ProceduralMeshKind, SerializableName,
    SerializableTransform, Transform, World,
};
use khora_sdk::khora_data::scene::instantiate_subtree;

// ─────────────────────────────────────────────────────────────────────────────
// A project on disk, and the command run on it
// ─────────────────────────────────────────────────────────────────────────────

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy_scenes")
        .join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn empty(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "khora-upgrade-breaker-{label}-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("assets")).expect("create the project");
        Self { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create the directory");
        }
        fs::write(&path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        let path = self.path(rel);
        fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(dir).expect("list a directory") {
                let entry = entry.expect("a directory entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, &rel, out);
                } else {
                    out.insert(rel, fs::read(&path).expect("read a project file"));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.root, "", &mut out);
        out
    }

    fn upgrade(&self, dry_run: bool) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["assets", "upgrade-scenes"])
            .arg(&self.root)
            .current_dir(&self.root);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().expect("run xtask");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            success: output.status.success(),
            text,
        }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Run {
    success: bool,
    text: String,
}

impl Run {
    fn expect_success(&self, what: &str) {
        assert!(self.success, "{what}: the command failed:\n{}", self.text);
    }

    fn expect_failure(&self, what: &str) {
        assert!(
            !self.success,
            "{what}: the command succeeded although it should refuse:\n{}",
            self.text
        );
    }

    fn says(&self, file: &str, word: &str) -> bool {
        let file = file.to_lowercase();
        self.text.lines().any(|line| {
            let line = line.to_lowercase();
            line.contains(&file) && line.contains(word)
        })
    }

    fn expect_says(&self, file: &str, word: &str) {
        assert!(
            self.says(file, word),
            "no line names `{file}` as {word}; the output:\n{}",
            self.text
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The v1 layout, encoder side
// ─────────────────────────────────────────────────────────────────────────────

#[derive(bincode::Encode, Clone, Copy)]
struct V1Id {
    index: u32,
    generation: u32,
}

fn id(index: u32) -> V1Id {
    V1Id {
        index,
        generation: 0,
    }
}

#[derive(bincode::Encode)]
enum V1Command {
    Spawn {
        id: V1Id,
    },
    AddComponent {
        entity_id: V1Id,
        component_type: String,
        component_data: Vec<u8>,
    },
    SetParent {
        child_id: V1Id,
        parent_id: V1Id,
    },
}

#[derive(bincode::Encode)]
struct V1Recipe {
    commands: Vec<V1Command>,
}

#[derive(bincode::Encode)]
enum V1MeshKind {
    Cube,
    Sphere,
    Plane,
}

#[derive(bincode::Encode)]
enum V1MeshRef {
    Procedural { kind: V1MeshKind, params: [f32; 4] },
    Asset([u8; 16]),
}

#[derive(bincode::Encode)]
enum V1MaterialRef {
    Inline(Vec<u8>),
    Asset([u8; 16]),
}

#[derive(bincode::Encode)]
struct V1MaterialData {
    type_name: String,
    data: Vec<u8>,
}

/// A v1 recipe under construction.
#[derive(Default)]
struct V1 {
    commands: Vec<V1Command>,
}

impl V1 {
    fn spawn(&mut self, i: u32) -> &mut Self {
        self.commands.push(V1Command::Spawn { id: id(i) });
        self
    }

    fn raw(&mut self, i: u32, name: &str, bytes: Vec<u8>) -> &mut Self {
        self.commands.push(V1Command::AddComponent {
            entity_id: id(i),
            component_type: name.to_owned(),
            component_data: bytes,
        });
        self
    }

    fn name(&mut self, i: u32, name: &str) -> &mut Self {
        let bytes =
            bincode::serde::encode_to_vec(SerializableName::from(Name::new(name)), standard())
                .expect("encode a name");
        self.raw(i, "Name", bytes)
    }

    fn transform(&mut self, i: u32, transform: Transform) -> &mut Self {
        let bytes =
            bincode::serde::encode_to_vec(SerializableTransform::from(transform), standard())
                .expect("encode a transform");
        self.raw(i, "Transform", bytes)
    }

    fn mesh(&mut self, i: u32, mesh: V1MeshRef) -> &mut Self {
        let bytes = bincode::encode_to_vec(mesh, standard()).expect("encode a mesh");
        self.raw(i, "MeshRef", bytes)
    }

    fn material_inline<M: bincode::Encode>(&mut self, i: u32, type_name: &str, m: &M) -> &mut Self {
        let data = bincode::encode_to_vec(m, standard()).expect("encode a material");
        let inline = bincode::encode_to_vec(
            V1MaterialData {
                type_name: type_name.to_owned(),
                data,
            },
            standard(),
        )
        .expect("encode material data");
        let bytes = bincode::encode_to_vec(V1MaterialRef::Inline(inline), standard())
            .expect("encode a material ref");
        self.raw(i, "MaterialRef", bytes)
    }

    fn material_asset(&mut self, i: u32, uuid: [u8; 16]) -> &mut Self {
        let bytes = bincode::encode_to_vec(V1MaterialRef::Asset(uuid), standard())
            .expect("encode a material ref");
        self.raw(i, "MaterialRef", bytes)
    }

    fn parent(&mut self, child: u32, parent: u32) -> &mut Self {
        self.commands.push(V1Command::SetParent {
            child_id: id(child),
            parent_id: id(parent),
        });
        self
    }

    /// The bare recipe: a v1 prefab.
    fn prefab(&mut self) -> Vec<u8> {
        let commands = std::mem::take(&mut self.commands);
        bincode::encode_to_vec(V1Recipe { commands }, standard()).expect("encode a recipe")
    }

    /// The recipe behind a v1 `KH_RECIPE_V1` header: a v1 scene.
    fn scene(&mut self) -> Vec<u8> {
        let payload = self.prefab();
        v1_scene_named("KH_RECIPE_V1", 1, &payload)
    }
}

fn v1_scene_named(encoding: &str, version: u8, payload: &[u8]) -> Vec<u8> {
    let mut encoding_id = [0u8; 32];
    encoding_id[..encoding.len()].copy_from_slice(encoding.as_bytes());
    SceneFile {
        header: SceneHeader {
            magic_bytes: HEADER_MAGIC_BYTES,
            format_version: version,
            encoding_id,
            payload_length: payload.len() as u64,
        },
        payload: payload.to_vec(),
    }
    .to_bytes()
}

// ─────────────────────────────────────────────────────────────────────────────
// Reading upgraded files back, as a game would
// ─────────────────────────────────────────────────────────────────────────────

fn load_scene(bytes: &[u8], what: &str) -> World {
    let file = SceneFile::from_bytes(bytes)
        .unwrap_or_else(|e| panic!("{what}: the upgraded file is not a scene file: {e:?}"));
    let mut world = World::new();
    SerializationService::new()
        .load_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("{what}: the upgraded scene does not load: {e}"));
    world
}

fn named(world: &World, name: &str) -> EntityId {
    world
        .iter_entities()
        .find(|e| world.get::<Name>(*e).map(|n| n.as_str()) == Some(name))
        .unwrap_or_else(|| panic!("no entity named `{name}`"))
}

fn name_of(world: &World, entity: EntityId) -> String {
    world
        .get::<Name>(entity)
        .map(|n| n.as_str().to_owned())
        .unwrap_or_else(|| "<unnamed>".to_owned())
}

fn children_names(world: &World, parent: EntityId) -> Vec<String> {
    world
        .get::<Children>(parent)
        .map(|c| c.0.iter().map(|e| name_of(world, *e)).collect())
        .unwrap_or_default()
}

fn parent_name(world: &World, child: EntityId) -> Option<String> {
    world.get::<Parent>(child).map(|p| name_of(world, p.0))
}

fn inline_material<T: 'static>(world: &World, entity: EntityId) -> &T {
    let Some(MaterialRef::Inline { material, .. }) = world.get::<MaterialRef>(entity) else {
        panic!("{} has no inline MaterialRef", name_of(world, entity));
    };
    material.as_any().downcast_ref::<T>().unwrap_or_else(|| {
        panic!(
            "{}: the inline material changed type",
            name_of(world, entity)
        )
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Defects
// ─────────────────────────────────────────────────────────────────────────────

/// A `.v1` already beside a v1 file (left by an earlier run, then the scene
/// saved again as v1 by an older editor, say) must not cost the file's
/// current original: either the file is refused and left as it is, or its
/// bytes are kept somewhere. Today the existing `.v1` is kept, the file is
/// replaced, and its bytes exist nowhere any more.
#[test]
fn an_existing_backup_does_not_cost_the_original() {
    let project = Project::empty("existing-backup");
    let original = fixture("test_default.kscene");
    let older = fixture("test2_default.kscene");
    assert_ne!(original, older);
    project.write("assets/scenes/level.kscene", &original);
    project.write("assets/scenes/level.kscene.v1", &older);

    let _run = project.upgrade(false);

    let kept = project.files().values().any(|bytes| *bytes == original);
    assert!(
        kept,
        "level.kscene was replaced and its original bytes are in no file of the project \
         (level.kscene.v1 still holds the older file)"
    );
}

/// A file the command refuses is left as it was, with nothing beside it.
/// A read-only scene cannot be replaced: the command refuses it — but it has
/// already written `<file>.v1` and `<file>.upgrading` next to it.
#[test]
fn a_refused_read_only_file_leaves_nothing_beside_it() {
    let project = Project::empty("read-only");
    let original = fixture("test_default.kscene");
    project.write("assets/scenes/locked.kscene", &original);
    let path = project.path("assets/scenes/locked.kscene");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).expect("make the scene read-only");

    let run = project.upgrade(false);

    // Let the project be cleaned up whatever happens next.
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    let _ = fs::set_permissions(&path, permissions);

    if run.success {
        // Replaced despite read-only: then the backup must be right.
        assert_eq!(project.read("assets/scenes/locked.kscene.v1"), original);
        return;
    }
    run.expect_says("locked.kscene", "refused");
    assert_eq!(project.read("assets/scenes/locked.kscene"), original);
    let leftovers: Vec<String> = project
        .files()
        .keys()
        .filter(|rel| rel.as_str() != "assets/scenes/locked.kscene")
        .cloned()
        .collect();
    assert!(
        leftovers.is_empty(),
        "the refused file left {leftovers:?} beside it; the output:\n{}",
        run.text
    );
}

/// A prefab entity the root's tree does not reach — a v1 prefab saved from a
/// world whose `Children` named an entity whose `Parent` did not agree, so
/// it got a `Spawn` but no `SetParent` — is silently dropped: the command
/// reports two entities, succeeds, and writes one. The plan's check
/// ("comparing entity and component counts before writing") compares the
/// written subtree with itself, never with what the file held.
#[test]
fn a_prefab_never_drops_an_entity_silently() {
    let project = Project::empty("prefab-orphan");
    let bytes = V1::default()
        .spawn(1)
        .name(1, "Root")
        .spawn(2)
        .name(2, "Stray")
        .prefab();
    project.write("assets/prefabs/two.kprefab", &bytes);

    let run = project.upgrade(false);

    if !run.success {
        // Refusing it, untouched, is an acceptable answer.
        assert_eq!(project.read("assets/prefabs/two.kprefab"), bytes);
        return;
    }
    let upgraded = project.read("assets/prefabs/two.kprefab");
    let mut world = World::new();
    instantiate_subtree(&mut world, &upgraded).expect("the upgraded prefab instantiates");
    let names: Vec<String> = world.iter_entities().map(|e| name_of(&world, e)).collect();
    assert_eq!(
        world.iter_entities().count(),
        2,
        "the v1 prefab held Root and Stray; the upgraded one instantiates {names:?}; the \
         output:\n{}",
        run.text
    );
}

/// The engine indexes a scene or prefab whatever the case of its extension
/// (`asset_type_for_extension` lowercases it); a file named `Level.KSCENE`
/// is a scene to the engine but invisible to the command — not upgraded, not
/// refused, not even named.
#[test]
fn an_extension_in_capitals_is_not_skipped() {
    let project = Project::empty("capitals");
    let original = fixture("test_default.kscene");
    project.write("assets/scenes/Level.KSCENE", &original);

    let run = project.upgrade(false);

    assert!(
        run.text.to_lowercase().contains("level.kscene"),
        "Level.KSCENE, a scene to the engine, is not named by the command; the output:\n{}",
        run.text
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Coverage: promises no test checked
// ─────────────────────────────────────────────────────────────────────────────

/// A v1 scene with a hierarchy: parents and children, a grandchild, sibling
/// order as `SetParent` gave it, and a `Parent` component carrying a v1 id
/// that means nothing any more — skipped, the hierarchy taken from
/// `SetParent` alone.
#[test]
fn a_v1_hierarchy_upgrades_with_its_sibling_order() {
    let project = Project::empty("hierarchy");
    let stale_parent = bincode::encode_to_vec(
        EntityId {
            index: 999,
            generation: 7,
        },
        standard(),
    )
    .expect("encode a stale parent");
    let bytes = V1::default()
        .spawn(10)
        .name(10, "P")
        .spawn(30)
        .name(30, "B")
        .raw(30, "Parent", stale_parent)
        .parent(30, 10)
        .spawn(20)
        .name(20, "A")
        .parent(20, 10)
        .spawn(40)
        .name(40, "C")
        .parent(40, 10)
        .spawn(50)
        .name(50, "Grandchild")
        .parent(50, 20)
        .scene();
    project.write("assets/scenes/tree.kscene", &bytes);

    let run = project.upgrade(false);
    run.expect_success("a v1 scene with a hierarchy");

    let world = load_scene(&project.read("assets/scenes/tree.kscene"), "tree");
    assert_eq!(world.iter_entities().count(), 5);
    let p = named(&world, "P");
    assert_eq!(children_names(&world, p), ["B", "A", "C"], "sibling order");
    assert_eq!(
        parent_name(&world, named(&world, "B")).as_deref(),
        Some("P")
    );
    assert_eq!(
        parent_name(&world, named(&world, "Grandchild")).as_deref(),
        Some("A")
    );
    assert_eq!(parent_name(&world, p), None);
}

/// A v1 prefab with a tree: instantiated, the root first, its children in
/// order and the grandchild under its parent.
#[test]
fn a_v1_prefab_tree_upgrades_whole() {
    let project = Project::empty("prefab-tree");
    let bytes = V1::default()
        .spawn(1)
        .name(1, "Root")
        .spawn(3)
        .name(3, "Second")
        .parent(3, 1)
        .spawn(2)
        .name(2, "First")
        .parent(2, 1)
        .spawn(4)
        .name(4, "Leaf")
        .parent(4, 3)
        .prefab();
    project.write("assets/prefabs/tree.kprefab", &bytes);

    let run = project.upgrade(false);
    run.expect_success("a v1 prefab with a tree");

    let mut world = World::new();
    let root = instantiate_subtree(&mut world, &project.read("assets/prefabs/tree.kprefab"))
        .expect("the upgraded prefab instantiates");
    assert_eq!(name_of(&world, root), "Root");
    assert_eq!(world.iter_entities().count(), 4);
    assert_eq!(children_names(&world, root), ["Second", "First"]);
    assert_eq!(
        parent_name(&world, named(&world, "Leaf")).as_deref(),
        Some("Second")
    );
}

/// Every v1 inline material type, the `__unknown__` fallback, an asset
/// material, an asset mesh and each procedural kind come through with their
/// values.
#[test]
fn every_v1_material_and_mesh_form_upgrades() {
    let project = Project::empty("materials");
    let red = LinearRgba::new(0.9, 0.1, 0.2, 1.0);
    let texture = AssetUUID::new();
    let standard_material = StandardMaterial {
        base_color: red,
        roughness: 0.37,
        metallic: 0.61,
        base_color_texture: Some(texture),
        ..Default::default()
    };
    let unlit = UnlitMaterial {
        base_color: LinearRgba::new(0.1, 0.2, 0.3, 0.4),
        alpha_cutoff: 0.25,
        ..Default::default()
    };
    let emissive = EmissiveMaterial {
        emissive_color: LinearRgba::new(0.5, 0.6, 0.7, 1.0),
        ..Default::default()
    };
    let wireframe = WireframeMaterial {
        color: LinearRgba::new(0.0, 1.0, 0.5, 1.0),
        line_width: 2.5,
    };
    let material_asset = AssetUUID::new();
    let mesh_asset = AssetUUID::new();

    let bytes = V1::default()
        .spawn(1)
        .name(1, "Standard")
        .material_inline(1, "StandardMaterial", &standard_material)
        .mesh(
            1,
            V1MeshRef::Procedural {
                kind: V1MeshKind::Sphere,
                params: [0.5, 16.0, 8.0, 0.0],
            },
        )
        .spawn(2)
        .name(2, "Unlit")
        .material_inline(2, "UnlitMaterial", &unlit)
        .mesh(
            2,
            V1MeshRef::Procedural {
                kind: V1MeshKind::Plane,
                params: [4.0, 4.0, 0.0, 0.0],
            },
        )
        .spawn(3)
        .name(3, "Emissive")
        .material_inline(3, "EmissiveMaterial", &emissive)
        .mesh(
            3,
            V1MeshRef::Procedural {
                kind: V1MeshKind::Cube,
                params: [2.0, 0.0, 0.0, 0.0],
            },
        )
        .spawn(4)
        .name(4, "Wireframe")
        .material_inline(4, "WireframeMaterial", &wireframe)
        .mesh(4, V1MeshRef::Asset(*mesh_asset.as_bytes()))
        .spawn(5)
        .name(5, "Unknown")
        .material_inline(5, "__unknown__", &red)
        .spawn(6)
        .name(6, "Asset")
        .material_asset(6, *material_asset.as_bytes())
        .scene();
    project.write("assets/scenes/materials.kscene", &bytes);

    let run = project.upgrade(false);
    run.expect_success("a scene of every material form");

    let world = load_scene(&project.read("assets/scenes/materials.kscene"), "materials");
    let e = |name: &str| named(&world, name);

    assert_eq!(
        format!(
            "{:?}",
            inline_material::<StandardMaterial>(&world, e("Standard"))
        ),
        format!("{standard_material:?}")
    );
    assert_eq!(
        format!("{:?}", inline_material::<UnlitMaterial>(&world, e("Unlit"))),
        format!("{unlit:?}")
    );
    assert_eq!(
        format!(
            "{:?}",
            inline_material::<EmissiveMaterial>(&world, e("Emissive"))
        ),
        format!("{emissive:?}")
    );
    assert_eq!(
        format!(
            "{:?}",
            inline_material::<WireframeMaterial>(&world, e("Wireframe"))
        ),
        format!("{wireframe:?}")
    );
    let unknown = inline_material::<StandardMaterial>(&world, e("Unknown"));
    assert_eq!(
        format!("{unknown:?}"),
        format!(
            "{:?}",
            StandardMaterial {
                base_color: red,
                ..Default::default()
            }
        )
    );
    assert!(matches!(
        world.get::<MaterialRef>(e("Asset")),
        Some(MaterialRef::Asset(uuid)) if *uuid == material_asset
    ));

    assert_eq!(
        world.get::<MeshRef>(e("Standard")),
        Some(&MeshRef::procedural(
            ProceduralMeshKind::Sphere,
            [0.5, 16.0, 8.0, 0.0]
        ))
    );
    assert_eq!(
        world.get::<MeshRef>(e("Unlit")),
        Some(&MeshRef::procedural(
            ProceduralMeshKind::Plane,
            [4.0, 4.0, 0.0, 0.0]
        ))
    );
    assert_eq!(
        world.get::<MeshRef>(e("Emissive")),
        Some(&MeshRef::procedural(
            ProceduralMeshKind::Cube,
            [2.0, 0.0, 0.0, 0.0]
        ))
    );
    assert_eq!(
        world.get::<MeshRef>(e("Wireframe")),
        Some(&MeshRef::Asset(mesh_asset))
    );
}

/// A file holding a component the command cannot decode (`UiImage`,
/// `UiText`, `Script`) or a name no component has is refused untouched,
/// named, with a non-zero exit; the project's other files are upgraded.
/// A dry run says the same and exits non-zero too.
#[test]
fn a_component_the_command_cannot_read_refuses_only_its_file() {
    let project = Project::empty("refused-components");
    let good = V1::default().spawn(1).name(1, "Fine").scene();
    project.write("assets/scenes/good.kscene", &good);
    let mut refused = Vec::new();
    for name in ["UiImage", "UiText", "Script", "NoSuchComponent"] {
        let bytes = V1::default()
            .spawn(1)
            .name(1, "Holder")
            .raw(1, name, vec![0; 20])
            .scene();
        let rel = format!("assets/scenes/with_{name}.kscene");
        project.write(&rel, &bytes);
        refused.push((name, rel, bytes));
    }

    let dry = project.upgrade(true);
    dry.expect_failure("a dry run over refused files");
    for (name, _, _) in &refused {
        dry.expect_says(&format!("with_{name}.kscene"), "refused");
    }

    let before = project.files();
    let run = project.upgrade(false);
    run.expect_failure("a run over refused files");
    for (name, rel, bytes) in &refused {
        run.expect_says(&format!("with_{name}.kscene"), "refused");
        assert_eq!(&project.read(rel), bytes, "{rel} was touched");
        assert!(
            !project.exists(&format!("{rel}.v1")),
            "{rel}.v1 was written"
        );
    }
    run.expect_says("good.kscene", "upgraded");
    assert_ne!(project.read("assets/scenes/good.kscene"), good);
    assert_eq!(project.read("assets/scenes/good.kscene.v1"), good);
    assert_eq!(
        project.files().len(),
        before.len() + 1,
        "one backup, nothing else"
    );
}

/// Components the engine derives or keeps at runtime are skipped: the file
/// upgrades without them.
#[test]
fn derived_and_runtime_components_are_skipped() {
    let project = Project::empty("derived");
    let bytes = V1::default()
        .spawn(1)
        .name(1, "Moved")
        .transform(1, Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)))
        .raw(1, "GlobalTransform", vec![1, 2, 3])
        .raw(1, "Children", vec![9])
        .raw(1, "Teleported", vec![])
        .scene();
    project.write("assets/scenes/derived.kscene", &bytes);

    let run = project.upgrade(false);
    run.expect_success("a scene with derived components");
    let world = load_scene(&project.read("assets/scenes/derived.kscene"), "derived");
    let moved = named(&world, "Moved");
    assert_eq!(
        world.get::<Transform>(moved).map(|t| t.translation),
        Some(Vec3::new(1.0, 2.0, 3.0))
    );
    assert!(world.get::<Children>(moved).is_none());
}

/// An empty v1 scene upgrades to an empty scene; an entity with no component
/// is kept; an empty prefab is refused untouched.
#[test]
fn empty_recipes_and_bare_entities() {
    let project = Project::empty("empty");
    let empty_scene = V1::default().scene();
    let bare = V1::default().spawn(1).spawn(2).name(2, "Named").scene();
    let empty_prefab = V1::default().prefab();
    project.write("assets/scenes/empty.kscene", &empty_scene);
    project.write("assets/scenes/bare.kscene", &bare);
    project.write("assets/prefabs/empty.kprefab", &empty_prefab);

    let run = project.upgrade(false);
    run.expect_failure("an empty prefab");
    run.expect_says("empty.kprefab", "refused");
    assert_eq!(project.read("assets/prefabs/empty.kprefab"), empty_prefab);

    run.expect_says("empty.kscene", "upgraded");
    let world = load_scene(&project.read("assets/scenes/empty.kscene"), "empty");
    assert_eq!(world.iter_entities().count(), 0);

    run.expect_says("bare.kscene", "upgraded");
    let world = load_scene(&project.read("assets/scenes/bare.kscene"), "bare");
    assert_eq!(world.iter_entities().count(), 2, "the bare entity was lost");
}

/// Damaged or foreign files are refused untouched: trailing bytes after the
/// recipe, a truncated component, a component for an entity never spawned,
/// a spawn twice, a header with no recipe behind it, a v1 Definition or
/// MessagePack header, a header claiming a version the engine does not know,
/// a headerless `.kscene`, a cycle.
#[test]
fn damaged_and_foreign_files_are_refused_untouched() {
    let project = Project::empty("damaged");
    let mut trailing = V1::default().spawn(1).name(1, "X").prefab();
    trailing.extend_from_slice(&[0, 0, 0]);
    let truncated = {
        let mut name = bincode::serde::encode_to_vec(
            SerializableName::from(Name::new("Truncated")),
            standard(),
        )
        .expect("encode a name");
        name.truncate(name.len() - 2);
        V1::default().spawn(1).raw(1, "Name", name).scene()
    };
    let headerless_scene = V1::default().spawn(1).name(1, "Headerless").prefab();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("assets/prefabs/trailing.kprefab", trailing),
        ("assets/scenes/truncated.kscene", truncated),
        (
            "assets/scenes/never_spawned.kscene",
            V1::default().spawn(1).name(2, "Ghost").scene(),
        ),
        (
            "assets/scenes/twice.kscene",
            V1::default().spawn(1).spawn(1).scene(),
        ),
        (
            "assets/scenes/cycle.kscene",
            V1::default()
                .spawn(1)
                .spawn(2)
                .parent(1, 2)
                .parent(2, 1)
                .scene(),
        ),
        (
            "assets/scenes/definition.kscene",
            v1_scene_named("KH_DEFINITION_V1", 1, b"{}"),
        ),
        (
            "assets/scenes/msgpack.kscene",
            v1_scene_named("KH_MESSAGEPACK_V1", 1, &[0x90]),
        ),
        (
            "assets/scenes/future.kscene",
            v1_scene_named("KH_COMPACT_V3", 3, &[1, 2, 3]),
        ),
        ("assets/scenes/headerless.kscene", headerless_scene),
        (
            "assets/scenes/no_recipe.kscene",
            v1_scene_named("KH_RECIPE_V1", 1, b"not a recipe at all"),
        ),
    ];
    for (rel, bytes) in &cases {
        project.write(rel, bytes);
    }

    for dry_run in [true, false] {
        let run = project.upgrade(dry_run);
        run.expect_failure("damaged files");
        for (rel, bytes) in &cases {
            let file = rel.rsplit('/').next().unwrap_or(rel);
            run.expect_says(file, "refused");
            assert_eq!(&project.read(rel), bytes, "{rel} was touched");
            assert!(
                !project.exists(&format!("{rel}.v1")),
                "{rel}.v1 was written"
            );
            assert!(
                !project.exists(&format!("{rel}.upgrading")),
                "{rel}.upgrading was left"
            );
        }
    }
}

/// A leftover `<file>.upgrading` from an interrupted run does not stop the
/// next one, and does not stay behind.
#[test]
fn a_leftover_temporary_is_replaced() {
    let project = Project::empty("leftover");
    let original = fixture("test_default.kscene");
    project.write("assets/scenes/level.kscene", &original);
    project.write("assets/scenes/level.kscene.upgrading", b"half a file");

    let run = project.upgrade(false);
    run.expect_success("a leftover temporary");
    assert!(!project.exists("assets/scenes/level.kscene.upgrading"));
    assert_eq!(project.read("assets/scenes/level.kscene.v1"), original);
    load_scene(&project.read("assets/scenes/level.kscene"), "level");
}
