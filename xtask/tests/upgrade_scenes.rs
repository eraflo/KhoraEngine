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

//! `cargo xtask assets upgrade-scenes <project> [--dry-run]` converts, once,
//! the scenes and prefabs a project saved before scene records into version-2
//! files, keeping each original beside it as `<file>.v1`.
//!
//! The fixtures are real files from the owner's projects: two v1 scenes
//! (`KH_RECIPE_V1` header) and a headerless prefab recipe. What each test
//! expects of an upgraded file is read from the fixture itself, through a
//! small v1 decoder below — the only place in this test the v1 layout appears.
//! The upgraded file is then read back with the engine, as a game would.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bincode::config::standard;
use khora_core::asset::StandardMaterial;
use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::scene::{
    SceneFile, SceneHeader, SerializationGoal, HEADER_MAGIC_BYTES, SCENE_FORMAT_VERSION,
};
use khora_io::serialization::SerializationService;
use khora_sdk::khora_data::ecs::{
    Camera, Light, MaterialRef, MeshRef, Name, Parent, ProceduralMeshKind, SerializableCamera,
    SerializableLight, SerializableName, SerializableTransform, Transform, World,
};
use khora_sdk::khora_data::scene::{
    encoding_of, instantiate_subtree, read_scene_file, serialize_subtree, SceneRecord,
};

/// The encoding an upgraded scene or prefab is written in.
const COMPACT: &str = "KH_COMPACT_V2";

/// Components a scene saves, which an upgraded world must hold as v1 did.
const SAVED: [&str; 6] = [
    "Transform",
    "Name",
    "Camera",
    "Light",
    "MeshRef",
    "MaterialRef",
];

/// Components v1 wrote that the engine derives or keeps at runtime: the
/// upgrade drops them, and the world rebuilds them.
const DERIVED: [&str; 3] = ["GlobalTransform", "Children", "Parent"];

/// The legacy fixtures, and where each sits in a project — as on the owner's
/// disk: scenes under `scenes/`, one prefab under `prefabs/`, one at the root
/// of `assets/`.
const LEGACY: [(&str, &str); 4] = [
    ("test_default.kscene", "assets/scenes/test_default.kscene"),
    ("test2_default.kscene", "assets/scenes/test2_default.kscene"),
    ("prefab.kprefab", "assets/prefabs/prefab.kprefab"),
    ("cube.kprefab", "assets/Cube.kprefab"),
];

/// The two legacy scenes, by project path.
const LEGACY_SCENES: [&str; 2] = [
    "assets/scenes/test_default.kscene",
    "assets/scenes/test2_default.kscene",
];

/// The two legacy prefabs, by project path.
const LEGACY_PREFABS: [&str; 2] = ["assets/prefabs/prefab.kprefab", "assets/Cube.kprefab"];

// ─────────────────────────────────────────────────────────────────────────────
// A project on disk, and the command run on it
// ─────────────────────────────────────────────────────────────────────────────

/// A fixture's bytes.
fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy_scenes")
        .join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

/// The bytes of the fixture a project path was copied from.
fn legacy_fixture_at(rel: &str) -> Vec<u8> {
    let (name, _) = LEGACY
        .iter()
        .find(|(_, at)| *at == rel)
        .unwrap_or_else(|| panic!("{rel} is not a legacy fixture path"));
    fixture(name)
}

/// The last component of a project path.
fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// A project directory under the system temp directory, removed on drop.
struct Project {
    root: PathBuf,
}

impl Project {
    /// An empty project, `assets/` included.
    fn empty(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "khora-upgrade-scenes-{label}-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("assets")).expect("create the project");
        Self { root }
    }

    /// A project holding every legacy fixture, laid out as the owner's.
    fn legacy(label: &str) -> Self {
        let project = Self::empty(label);
        for (name, rel) in LEGACY {
            project.write(rel, &fixture(name));
        }
        project
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

    /// Every file in the project, by `/`-separated relative path.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
            let entries =
                fs::read_dir(dir).unwrap_or_else(|e| panic!("list {}: {e}", dir.display()));
            for entry in entries {
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

    /// Runs `cargo xtask assets upgrade-scenes` on this project.
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

/// What a run of the command left: its exit status and everything it printed.
struct Run {
    success: bool,
    text: String,
}

impl Run {
    /// Panics, showing the output, unless the command succeeded.
    fn expect_success(&self, what: &str) {
        assert!(
            self.success,
            "{what}: `upgrade-scenes` failed; its output:\n{}",
            self.text
        );
    }

    /// Whether one line of the output names `file` and says `word` about it.
    fn says(&self, file: &str, word: &str) -> bool {
        let file = file.to_lowercase();
        self.text.lines().any(|line| {
            let line = line.to_lowercase();
            line.contains(&file) && line.contains(word)
        })
    }

    /// Panics unless one line of the output names `file` and says `word`.
    fn expect_says(&self, file: &str, word: &str) {
        assert!(
            self.says(file, word),
            "no line of the output names `{file}` as {word}; the output:\n{}",
            self.text
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The v1 layout, read only to know what an upgraded file must hold
// ─────────────────────────────────────────────────────────────────────────────

/// A v1 entity id, as the recipe numbered it.
#[derive(bincode::Decode, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct V1Id {
    index: u32,
    generation: u32,
}

#[derive(bincode::Decode, Debug)]
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

#[derive(bincode::Decode, Debug)]
struct V1Recipe {
    commands: Vec<V1Command>,
}

#[derive(bincode::Decode, Debug)]
enum V1MeshKind {
    Cube,
    Sphere,
    Plane,
}

#[derive(bincode::Decode, Debug)]
enum V1MeshRef {
    Procedural { kind: V1MeshKind, params: [f32; 4] },
    Asset([u8; 16]),
}

#[derive(bincode::Decode, Debug)]
enum V1MaterialRef {
    Inline(Vec<u8>),
    Asset([u8; 16]),
}

#[derive(bincode::Decode, Debug)]
struct V1MaterialData {
    type_name: String,
    data: Vec<u8>,
}

/// One v1 entity: its components by name, its parent.
#[derive(Debug, Default)]
struct V1Entity {
    components: BTreeMap<String, Vec<u8>>,
    parent: Option<V1Id>,
}

/// The entities of a v1 recipe, in spawn order. `headered` for a scene file,
/// whose recipe sits behind a v1 `KH_RECIPE_V1` header; a prefab is the bare
/// recipe.
fn decode_v1(bytes: &[u8], headered: bool) -> Vec<(V1Id, V1Entity)> {
    let payload = if headered {
        let file = SceneFile::from_bytes(bytes).expect("a v1 scene parses as a scene file");
        assert_eq!(file.header.format_version, 1, "the fixture is a v1 scene");
        assert_eq!(
            encoding_of(&file),
            "KH_RECIPE_V1",
            "the fixture is a recipe"
        );
        file.payload
    } else {
        bytes.to_vec()
    };
    let (recipe, read): (V1Recipe, usize) =
        bincode::decode_from_slice(&payload, standard()).expect("the fixture is a v1 recipe");
    assert_eq!(read, payload.len(), "the recipe is the whole payload");

    let mut entities: Vec<(V1Id, V1Entity)> = Vec::new();
    for command in recipe.commands {
        match command {
            V1Command::Spawn { id } => entities.push((id, V1Entity::default())),
            V1Command::AddComponent {
                entity_id,
                component_type,
                component_data,
            } => {
                let (_, entity) = entities
                    .iter_mut()
                    .find(|(id, _)| *id == entity_id)
                    .expect("a component for a spawned entity");
                entity.components.insert(component_type, component_data);
            }
            V1Command::SetParent {
                child_id,
                parent_id,
            } => {
                let (_, entity) = entities
                    .iter_mut()
                    .find(|(id, _)| *id == child_id)
                    .expect("a parent for a spawned entity");
                entity.parent = Some(parent_id);
            }
        }
    }
    entities
}

/// A plain component's v1 bytes, decoded through its current mirror.
fn v1_plain<M: serde::de::DeserializeOwned>(bytes: &[u8]) -> M {
    let (mirror, read): (M, usize) =
        bincode::serde::decode_from_slice(bytes, standard()).expect("a v1 component decodes");
    assert_eq!(read, bytes.len(), "the component is its whole payload");
    mirror
}

fn v1_mesh(bytes: &[u8]) -> MeshRef {
    let (mesh, _): (V1MeshRef, usize) =
        bincode::decode_from_slice(bytes, standard()).expect("a v1 MeshRef decodes");
    match mesh {
        V1MeshRef::Procedural { kind, params } => {
            let kind = match kind {
                V1MeshKind::Cube => ProceduralMeshKind::Cube,
                V1MeshKind::Sphere => ProceduralMeshKind::Sphere,
                V1MeshKind::Plane => ProceduralMeshKind::Plane,
            };
            MeshRef::procedural(kind, params)
        }
        V1MeshRef::Asset(bytes) => MeshRef::Asset(khora_core::asset::AssetUUID::from_bytes(bytes)),
    }
}

/// The `StandardMaterial` a v1 inline `MaterialRef` embeds.
fn v1_inline_standard_material(bytes: &[u8]) -> StandardMaterial {
    let (material, _): (V1MaterialRef, usize) =
        bincode::decode_from_slice(bytes, standard()).expect("a v1 MaterialRef decodes");
    let inline = match material {
        V1MaterialRef::Inline(inline) => inline,
        V1MaterialRef::Asset(uuid) => {
            panic!("the fixture's material is inline, not the asset {uuid:02x?}")
        }
    };
    let (data, _): (V1MaterialData, usize) =
        bincode::decode_from_slice(&inline, standard()).expect("v1 material data decodes");
    assert_eq!(data.type_name, "StandardMaterial");
    let (standard_material, _): (StandardMaterial, usize) =
        bincode::decode_from_slice(&data.data, standard()).expect("a v1 StandardMaterial decodes");
    standard_material
}

// ─────────────────────────────────────────────────────────────────────────────
// One entity as a test sees it, from v1 or from a loaded world
// ─────────────────────────────────────────────────────────────────────────────

/// What a test compares of one entity. `Light` has no `PartialEq`: it is
/// compared through its `Debug` form.
#[derive(Debug, PartialEq)]
struct Seen {
    name: Option<String>,
    transform: Option<Transform>,
    camera: Option<Camera>,
    light: Option<String>,
    parent: Option<String>,
    components: BTreeSet<&'static str>,
}

/// The entities a v1 recipe holds, sorted by name.
fn seen_in_v1(entities: &[(V1Id, V1Entity)]) -> Vec<Seen> {
    let name_of = |entity: &V1Entity| -> Option<String> {
        entity.components.get("Name").map(|bytes| {
            Name::from(v1_plain::<SerializableName>(bytes))
                .as_str()
                .to_owned()
        })
    };
    let mut seen: Vec<Seen> = entities
        .iter()
        .map(|(_, entity)| {
            for component in entity.components.keys() {
                assert!(
                    SAVED.contains(&component.as_str()) || DERIVED.contains(&component.as_str()),
                    "the fixture holds `{component}`, which this test does not know"
                );
            }
            let get = |name: &str| entity.components.get(name);
            Seen {
                name: name_of(entity),
                transform: get("Transform")
                    .map(|b| Transform::from(v1_plain::<SerializableTransform>(b))),
                camera: get("Camera").map(|b| Camera::from(v1_plain::<SerializableCamera>(b))),
                light: get("Light")
                    .map(|b| format!("{:?}", Light::from(v1_plain::<SerializableLight>(b)))),
                parent: entity.parent.map(|parent| {
                    let (_, parent) = entities
                        .iter()
                        .find(|(id, _)| *id == parent)
                        .expect("the parent was spawned");
                    name_of(parent).expect("a parent with a name")
                }),
                components: SAVED
                    .iter()
                    .copied()
                    .filter(|name| entity.components.contains_key(*name))
                    .collect(),
            }
        })
        .collect();
    seen.sort_by(|a, b| a.name.cmp(&b.name));
    seen
}

/// One entity of a loaded world.
fn seen_in_world(world: &World, entity: EntityId) -> Seen {
    let name = |e: EntityId| world.get::<Name>(e).map(|n| n.as_str().to_owned());
    let mut components = BTreeSet::new();
    let has = [
        ("Transform", world.get::<Transform>(entity).is_some()),
        ("Name", world.get::<Name>(entity).is_some()),
        ("Camera", world.get::<Camera>(entity).is_some()),
        ("Light", world.get::<Light>(entity).is_some()),
        ("MeshRef", world.get::<MeshRef>(entity).is_some()),
        ("MaterialRef", world.get::<MaterialRef>(entity).is_some()),
    ];
    for (component, present) in has {
        if present {
            components.insert(component);
        }
    }
    Seen {
        name: name(entity),
        transform: world.get::<Transform>(entity).copied(),
        camera: world.get::<Camera>(entity).copied(),
        light: world.get::<Light>(entity).map(|l| format!("{l:?}")),
        parent: world
            .get::<Parent>(entity)
            .map(|p| name(p.0).expect("a parent with a name")),
        components,
    }
}

/// Every entity of a loaded world, sorted by name.
fn seen_in(world: &World) -> Vec<Seen> {
    let mut seen: Vec<Seen> = world
        .iter_entities()
        .map(|entity| seen_in_world(world, entity))
        .collect();
    seen.sort_by(|a, b| a.name.cmp(&b.name));
    seen
}

/// Parses an upgraded file: a version-2 file, in the compact encoding, whose
/// record names only authored entities and holds no derived component.
fn upgraded_file(bytes: &[u8], what: &str) -> (SceneFile, SceneRecord) {
    let file = SceneFile::from_bytes(bytes)
        .unwrap_or_else(|e| panic!("{what}: the upgraded file is not a scene file: {e:?}"));
    assert_eq!(file.header.magic_bytes, HEADER_MAGIC_BYTES, "{what}");
    assert_eq!(
        file.header.format_version, SCENE_FORMAT_VERSION,
        "{what}: the upgraded file is not version {SCENE_FORMAT_VERSION}"
    );
    assert_eq!(encoding_of(&file), COMPACT, "{what}: encoding");
    let record = read_scene_file(&file)
        .unwrap_or_else(|e| panic!("{what}: the upgraded file does not read: {e}"));
    for id in &record.entities {
        assert!(
            !id.is_created(),
            "{what}: {id:?} is not authored — an upgraded entity is the author's"
        );
    }
    for page in &record.pages {
        for component in &page.components {
            assert_ne!(
                component, "GlobalTransform",
                "{what}: the derived GlobalTransform was saved"
            );
        }
    }
    (file, record)
}

/// A version-2 scene written by this engine, in the text encoding: one
/// authored entity.
fn current_scene_bytes() -> Vec<u8> {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("Already current"),
    ));
    world.mark_authored(entity).expect("authored");
    SerializationService::new()
        .save_world(&world, SerializationGoal::HumanReadableDebug)
        .expect("a current scene saves")
        .to_bytes()
}

/// A version-2 prefab written by this engine.
fn current_prefab_bytes() -> Vec<u8> {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Current prefab")));
    world.mark_authored(root).expect("authored");
    serialize_subtree(&world, root).expect("a current prefab saves")
}

/// A version-1 scene file whose header names `encoding`.
fn v1_scene_named(encoding: &str, payload: &[u8]) -> Vec<u8> {
    let mut encoding_id = [0u8; 32];
    encoding_id[..encoding.len()].copy_from_slice(encoding.as_bytes());
    SceneFile {
        header: SceneHeader {
            magic_bytes: HEADER_MAGIC_BYTES,
            format_version: 1,
            encoding_id,
            payload_length: payload.len() as u64,
        },
        payload: payload.to_vec(),
    }
    .to_bytes()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Each v1 scene becomes a version-2 compact file that loads into a fresh
/// world holding what v1 held: as many entities, the same names, transforms,
/// camera and light, every one of them authored — and the derived
/// `GlobalTransform` v1 wrote is not in the file.
#[test]
fn a_v1_scene_upgrades_to_an_equal_world() {
    let project = Project::legacy("scene");
    let expected: Vec<(&str, Vec<Seen>)> = LEGACY_SCENES
        .iter()
        .map(|rel| (*rel, seen_in_v1(&decode_v1(&legacy_fixture_at(rel), true))))
        .collect();

    let run = project.upgrade(false);
    run.expect_success("a project of v1 scenes");

    for (rel, expected) in expected {
        let (file, _) = upgraded_file(&project.read(rel), rel);
        let mut world = World::new();
        let report = SerializationService::new()
            .load_world(&file, &mut world)
            .unwrap_or_else(|e| panic!("{rel}: the upgraded scene does not load: {e}"));
        assert!(report.is_clean(), "{rel}: the load adapted: {report:?}");

        assert_eq!(
            world.iter_entities().count(),
            expected.len(),
            "{rel}: entity count"
        );
        assert_eq!(
            seen_in(&world),
            expected,
            "{rel}: the world differs from v1"
        );
        for entity in world.iter_entities() {
            let id = world.persistent_id(entity).expect("a live entity");
            assert!(!id.is_created(), "{rel}: {id:?} is not authored");
        }

        // What the owner's default scene is, whatever the decoder above reads.
        let camera = seen_in(&world)
            .into_iter()
            .find(|s| s.name.as_deref() == Some("Main Camera"))
            .unwrap_or_else(|| panic!("{rel}: no Main Camera"));
        assert!(camera.camera.is_some(), "{rel}: Main Camera has no Camera");
        assert_eq!(
            camera.transform.map(|t| t.translation),
            Some(Vec3::new(0.0, 5.0, 10.0)),
            "{rel}: Main Camera's position"
        );
        let light = seen_in(&world)
            .into_iter()
            .find(|s| s.name.as_deref() == Some("Directional Light"))
            .unwrap_or_else(|| panic!("{rel}: no Directional Light"));
        assert!(
            light.light.is_some(),
            "{rel}: Directional Light has no Light"
        );
        assert_eq!(
            light.transform.map(|t| t.translation),
            Some(Vec3::new(0.0, 10.0, 0.0)),
            "{rel}: Directional Light's position"
        );
    }
}

/// A headerless v1 prefab — wherever it sits under `assets/` — becomes a
/// version-2 compact file with a header; instantiated, its root comes first
/// and keeps its procedural `MeshRef` and its inline `StandardMaterial`.
#[test]
fn a_v1_prefab_without_header_is_recognised() {
    let project = Project::legacy("prefab");
    let expected: Vec<(&str, usize, Seen, MeshRef, StandardMaterial)> = LEGACY_PREFABS
        .iter()
        .map(|rel| {
            let entities = decode_v1(&legacy_fixture_at(rel), false);
            let (_, root) = &entities[0];
            let root_seen = seen_in_v1(&entities[..1])
                .pop()
                .expect("the prefab has a root");
            let mesh = v1_mesh(root.components.get("MeshRef").expect("the root has a mesh"));
            let material = v1_inline_standard_material(
                root.components
                    .get("MaterialRef")
                    .expect("the root has a material"),
            );
            (*rel, entities.len(), root_seen, mesh, material)
        })
        .collect();

    let run = project.upgrade(false);
    run.expect_success("a project with headerless prefabs");

    for (rel, count, root_expected, mesh_expected, material_expected) in expected {
        let bytes = project.read(rel);
        let (_, record) = upgraded_file(&bytes, rel);
        assert_eq!(record.entities.len(), count, "{rel}: entity count");

        let mut world = World::new();
        let before = world.iter_entities().count();
        let root = instantiate_subtree(&mut world, &bytes)
            .unwrap_or_else(|e| panic!("{rel}: the upgraded prefab does not instantiate: {e:?}"));
        assert_eq!(
            world.iter_entities().count() - before,
            count,
            "{rel}: instantiated entity count"
        );

        assert_eq!(
            seen_in_world(&world, root),
            root_expected,
            "{rel}: the root differs from v1"
        );
        assert_eq!(
            root_expected.name.as_deref(),
            Some("Cube"),
            "{rel}: root name"
        );
        assert!(
            world.get::<Parent>(root).is_none(),
            "{rel}: the root has a parent"
        );

        let mesh = world
            .get::<MeshRef>(root)
            .expect("the root keeps its MeshRef");
        assert_eq!(*mesh, mesh_expected, "{rel}: MeshRef");
        assert!(
            matches!(
                mesh,
                MeshRef::Procedural {
                    kind: ProceduralMeshKind::Cube,
                    ..
                }
            ),
            "{rel}: the mesh is a procedural cube: {mesh:?}"
        );

        let material = world
            .get::<MaterialRef>(root)
            .expect("the root keeps its MaterialRef");
        let MaterialRef::Inline { material, .. } = material else {
            panic!("{rel}: the material is no longer inline");
        };
        let standard = material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .unwrap_or_else(|| panic!("{rel}: the inline material is not a StandardMaterial"));
        assert_eq!(
            format!("{standard:?}"),
            format!("{material_expected:?}"),
            "{rel}: the StandardMaterial differs from v1"
        );
    }
}

/// A version-2 file is current: the command leaves it byte for byte and keeps
/// no `.v1` of it. Run again on a project it upgraded, it changes nothing at
/// all and says every file is current.
#[test]
fn an_upgrade_is_idempotent() {
    let project = Project::legacy("idempotent");
    let scene = current_scene_bytes();
    let prefab = current_prefab_bytes();
    project.write("assets/scenes/already_v2.kscene", &scene);
    project.write("assets/prefabs/already_v2.kprefab", &prefab);

    let first = project.upgrade(false);
    first.expect_success("the first run");
    assert_eq!(project.read("assets/scenes/already_v2.kscene"), scene);
    assert_eq!(project.read("assets/prefabs/already_v2.kprefab"), prefab);
    assert!(!project.exists("assets/scenes/already_v2.kscene.v1"));
    assert!(!project.exists("assets/prefabs/already_v2.kprefab.v1"));
    first.expect_says("already_v2.kscene", "current");
    first.expect_says("already_v2.kprefab", "current");

    let after_first = project.files();
    let second = project.upgrade(false);
    second.expect_success("the second run");
    assert_eq!(
        project.files(),
        after_first,
        "the second run changed the project"
    );
    for (_, rel) in LEGACY {
        second.expect_says(file_name(rel), "current");
    }
}

/// The original of every upgraded file is kept beside it as `<file>.v1`, its
/// exact bytes; nothing else is left behind, and files that are not scenes
/// or prefabs are not touched.
#[test]
fn the_original_is_kept() {
    let project = Project::legacy("original");
    let unrelated = b"not a scene, never touched".to_vec();
    project.write("assets/textures/notes.txt", &unrelated);

    let run = project.upgrade(false);
    run.expect_success("a project of v1 files");

    let mut expected_files: BTreeSet<String> = BTreeSet::new();
    expected_files.insert("assets/textures/notes.txt".to_owned());
    for (name, rel) in LEGACY {
        let original = fixture(name);
        let kept = format!("{rel}.v1");
        assert_eq!(
            project.read(&kept),
            original,
            "{kept} does not hold the original bytes"
        );
        assert_ne!(project.read(rel), original, "{rel} was not upgraded");
        expected_files.insert(rel.to_owned());
        expected_files.insert(kept);
    }
    let files = project.files();
    assert_eq!(
        files.keys().cloned().collect::<BTreeSet<_>>(),
        expected_files,
        "the project holds other files than the upgraded ones and their originals"
    );
    assert_eq!(files["assets/textures/notes.txt"], unrelated);
}

/// `--dry-run` reports on every file and writes nothing.
#[test]
fn a_dry_run_writes_nothing() {
    let project = Project::legacy("dry-run");
    let before = project.files();

    let run = project.upgrade(true);
    run.expect_success("a dry run");

    assert_eq!(project.files(), before, "a dry run changed the project");
    for (_, rel) in LEGACY {
        let name = file_name(rel).to_lowercase();
        assert!(
            run.text.to_lowercase().contains(&name),
            "the dry run does not report `{name}`; its output:\n{}",
            run.text
        );
    }
}

/// A v1 scene in another encoding than the recipe, and a `.kprefab` that is
/// no recipe, are refused: left as they are, no `.v1`, a non-zero exit, and
/// the output names each — while the project's other files are upgraded.
#[test]
fn an_unsupported_file_is_refused_untouched() {
    let project = Project::legacy("refused");
    let archetype = v1_scene_named("KH_ARCHETYPE_V1", &[3, 1, 4, 1, 5, 9, 2, 6]);
    let garbage = b"this is not a prefab".to_vec();
    project.write("assets/scenes/archetype.kscene", &archetype);
    project.write("assets/prefabs/garbage.kprefab", &garbage);

    let run = project.upgrade(false);
    assert!(
        !run.success,
        "the command succeeded although it refused files; its output:\n{}",
        run.text
    );

    assert_eq!(project.read("assets/scenes/archetype.kscene"), archetype);
    assert_eq!(project.read("assets/prefabs/garbage.kprefab"), garbage);
    assert!(!project.exists("assets/scenes/archetype.kscene.v1"));
    assert!(!project.exists("assets/prefabs/garbage.kprefab.v1"));
    run.expect_says("archetype.kscene", "refused");
    run.expect_says("garbage.kprefab", "refused");

    for (name, rel) in LEGACY {
        upgraded_file(&project.read(rel), rel);
        assert_eq!(
            project.read(&format!("{rel}.v1")),
            fixture(name),
            "{rel}.v1"
        );
        run.expect_says(file_name(rel), "upgraded");
    }
}

/// The command says, file by file, what it did: upgraded, already current,
/// or refused.
#[test]
fn the_command_reports_each_file() {
    let project = Project::legacy("report");
    project.write("assets/scenes/already_v2.kscene", &current_scene_bytes());
    project.write("assets/prefabs/garbage.kprefab", b"this is not a prefab");

    let run = project.upgrade(false);

    for (_, rel) in LEGACY {
        run.expect_says(file_name(rel), "upgraded");
    }
    run.expect_says("already_v2.kscene", "current");
    run.expect_says("garbage.kprefab", "refused");
}
