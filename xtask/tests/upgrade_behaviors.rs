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

//! `cargo xtask assets upgrade-behaviors <project>`: a project's scenes,
//! prefabs and game saves, written with the legacy `Script` / `ScriptState`
//! components, come out holding behavior components — `Guard` with the
//! authored fields, and in a save `Guard#state` with what the game was doing.
//!
//! Each test builds a project in a temporary directory with this build's
//! writers, runs the xtask binary on it, and reads the files back the way
//! the engine would: the project's scripts compiled and their declarations
//! registered before anything loads.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use khora_core::asset::{asset_key, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::{InstanceLifecycle, ScriptSnapshot, ScriptValue, TimerRemaining};
use khora_io::script::declarations::{apply_declarations, declaration_channel, DeclaredComponents};
use khora_io::script::hot_reload::{load_all, reload_channel};
use khora_io::serialization::SerializationService;
use khora_sdk::khora_data;

use khora_data::ecs::{ComponentKey, Name, PrefabInstance, Script, ScriptState, Transform, World};
use khora_data::scene::snapshot::SNAPSHOT_ENCODING_ID;
use khora_data::scene::{
    encoding_of, instantiate_prefab, read_scene_file, serialize_prefab, NoPrefabs, PrefabSource,
    SceneRecord,
};

/// The project's one behavior: two fields, two states.
const GUARD: &str = "behavior Guard {
    float speed = 1.0;
    int health = 100;

    state Patrol { }
    state Attack { }

    void Update(float dt) { }
}
";

/// The module `GUARD` is written to, relative to the script root.
const GUARD_MODULE: &str = "guard.erg";

const SCENE: &str = "assets/scenes/level.kscene";
const PREFAB: &str = "assets/prefabs/sentry.kprefab";
const KEEP: &str = "assets/scenes/keep.kscene";
const SAVES: &str = "saves";
const SAVE: &str = "saves/slot1.ksave";

// ─── The project on disk ────────────────────────────────────────────────────

/// A project in a temporary directory, removed when dropped: `assets/` with
/// `scripts/guard.erg`, and whatever the test writes beside it.
struct Project {
    root: PathBuf,
}

impl Project {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-upgrade-behaviors-{}-{test}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let project = Self { root };
        project.write(&format!("assets/scripts/{GUARD_MODULE}"), GUARD.as_bytes());
        project
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("creates {}: {e}", parent.display()));
        }
        fs::write(&path, bytes).unwrap_or_else(|e| panic!("writes {}: {e}", path.display()));
    }

    fn write_file(&self, rel: &str, file: &SceneFile) {
        self.write(rel, &file.to_bytes());
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        let path = self.path(rel);
        fs::read(&path).unwrap_or_else(|e| panic!("reads {}: {e}", path.display()))
    }

    fn file(&self, rel: &str) -> SceneFile {
        SceneFile::from_bytes(&self.read(rel))
            .unwrap_or_else(|e| panic!("{rel} is a scene or save file: {e:?}"))
    }

    /// Every file of the project, by its path, with its bytes.
    fn bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut dirs = vec![self.root.clone()];
        while let Some(dir) = dirs.pop() {
            let entries =
                fs::read_dir(&dir).unwrap_or_else(|e| panic!("lists {}: {e}", dir.display()));
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    let bytes = fs::read(&path).expect("reads a project file");
                    files.insert(path, bytes);
                }
            }
        }
        files
    }

    /// Runs `cargo xtask assets upgrade-behaviors <project> <args>`.
    fn upgrade(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["assets", "upgrade-behaviors"])
            .arg(&self.root)
            .args(args)
            .output()
            .expect("the xtask binary runs");
        Run(output)
    }

    /// `--saves <project>/saves`.
    fn saves_arg(&self) -> String {
        self.path(SAVES).to_string_lossy().into_owned()
    }

    /// A world holding what the project's scripts declare, registered as the
    /// engine registers them before a scene loads.
    fn declared_world(&self) -> World {
        let declarations = declaration_channel();
        let compiled = load_all(
            &self.path("assets/scripts"),
            &reload_channel(),
            &declarations,
        );
        assert!(compiled > 0, "the project's scripts compile");
        let mut world = World::new();
        apply_declarations(
            &mut world,
            &mut DeclaredComponents::default(),
            &declarations.drain(),
        );
        assert!(
            world.components().key_named("Guard").is_some(),
            "`Guard` is declared by the project's scripts"
        );
        world
    }

    /// The prefabs the project's files hold now, by asset id.
    fn library(&self, prefabs: &[&str]) -> Library {
        let mut library = Library::default();
        for rel in prefabs {
            let record = read_scene_file(&self.file(rel))
                .unwrap_or_else(|e| panic!("{rel} reads as a prefab: {e}"));
            library.0.insert(asset_id(rel), record);
        }
        library
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// One run of the tool.
struct Run(Output);

impl Run {
    fn succeeded(&self) -> bool {
        self.0.status.success()
    }

    /// Everything it printed, stdout then stderr.
    fn text(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&self.0.stdout),
            String::from_utf8_lossy(&self.0.stderr)
        )
    }

    fn assert_succeeded(&self) {
        assert!(
            self.succeeded(),
            "the upgrade succeeds — exit {:?}:\n{}",
            self.0.status.code(),
            self.text()
        );
    }

    /// Failed, as an error the tool reports — not a panic.
    fn assert_refused(&self) {
        assert!(
            !self.succeeded(),
            "the upgrade fails — it printed:\n{}",
            self.text()
        );
        assert!(
            !self.text().contains("panicked"),
            "a refused upgrade is reported, not a panic:\n{}",
            self.text()
        );
    }
}

/// The asset id of the project file `rel` (`assets/…`), as the engine
/// derives it: the key of its path under `assets/`.
fn asset_id(rel: &str) -> AssetUUID {
    let under_assets = Path::new(rel)
        .strip_prefix("assets")
        .expect("an asset lives under `assets/`");
    AssetUUID::new_v5(&asset_key(under_assets))
}

/// Prefabs held in memory, by asset id.
#[derive(Debug, Clone, Default)]
struct Library(HashMap<AssetUUID, SceneRecord>);

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?} in the library"))
    }
}

// ─── Reading a world ────────────────────────────────────────────────────────

fn named(world: &World, name: &str) -> EntityId {
    world
        .iter_entities()
        .find(|&e| world.get::<Name>(e).is_some_and(|n| n.as_str() == name))
        .unwrap_or_else(|| panic!("an entity named `{name}`"))
}

fn key(world: &World, component: &str) -> ComponentKey {
    world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"))
}

/// `entity`'s `component` field `field`, or `None` if it does not hold it.
fn field_of(world: &World, entity: EntityId, component: &str, field: &str) -> Option<ScriptValue> {
    let key = key(world, component);
    let slot = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|packed| packed.slot_of(field))
        .unwrap_or_else(|| panic!("`{component}` has a field `{field}`"));
    world.row(entity, key)?.field(slot)
}

fn holds(world: &World, entity: EntityId, component: &str) -> bool {
    world.row(entity, key(world, component)).is_some()
}

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

// ─── Fixtures, written with the legacy `Script` ─────────────────────────────

/// A guard at speed 2.5 — `health` left to its default — and a crate.
fn guard_and_crate() -> World {
    let mut world = World::new();
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(2.5)),
    ));
    let crate_ = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        Name::new("Crate"),
    ));
    for entity in [guard, crate_] {
        world.mark_authored(entity).expect("authored");
    }
    world
}

/// Writes `PREFAB`: a sentry at speed 2.5. Returns its record.
fn write_sentry_prefab(project: &Project) -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(2.5)),
    ));
    world.mark_authored(root).expect("authored");
    let bytes = serialize_prefab(&world, root, asset_id(PREFAB), &NoPrefabs)
        .expect("the prefab serializes");
    project.write(PREFAB, &bytes);
    read_scene_file(&SceneFile::from_bytes(&bytes).expect("a scene file")).expect("reads")
}

/// Writes `SCENE`: an instance of `PREFAB` overriding its speed to 4.0.
fn write_level_with_instance(project: &Project, prefab: SceneRecord) {
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), prefab);
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, asset_id(PREFAB), &library)
        .expect("the prefab instantiates");
    let script = world
        .get_mut::<Script>(root)
        .expect("the instance holds the Script");
    *script = script.clone().with_field("speed", ScriptValue::Float(4.0));
    let file = SerializationService::new()
        .save_world_with(&world, SerializationGoal::EditorInterchange, &library)
        .expect("the level saves");
    assert_eq!(
        read_scene_file(&file).expect("reads").instances.len(),
        1,
        "the level links the prefab"
    );
    project.write_file(SCENE, &file);
}

/// Writes `KEEP` and a save of it at `SAVE`, taken while the guard was in
/// `Attack` with a countdown running and its speed changed to 7.0 — then
/// edits the scene after the save, `health` 80 → 90.
fn write_keep_and_save(project: &Project) {
    let service = SerializationService::new();
    let mut world = World::new();
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard")
            .with_field("speed", ScriptValue::Float(2.5))
            .with_field("health", ScriptValue::Int(80)),
    ));
    world.mark_authored(guard).expect("authored");
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    let observed = |speed| {
        vec![
            ("speed".to_owned(), ScriptValue::Float(speed)),
            ("health".to_owned(), ScriptValue::Int(80)),
        ]
    };
    world
        .add_component(
            guard,
            ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot {
                    fields: observed(7.0),
                    authored: observed(2.5),
                    overridden: vec!["speed".to_owned(), "health".to_owned()],
                    state: Some("Attack".to_owned()),
                    state_fields: Vec::new(),
                    timers: vec![TimerRemaining {
                        repeating: true,
                        interval: 0.5,
                        state: Some("Attack".to_owned()),
                        ordinal: 0,
                        remaining: Some(0.25),
                    }],
                    pending: None,
                    lifecycle: InstanceLifecycle {
                        spawned: true,
                        ..Default::default()
                    },
                },
            },
        )
        .expect("the state attaches");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);

    // The scene, edited after the save.
    world
        .remove_component::<ScriptState>(guard)
        .expect("the state detaches");
    let script = world.get_mut::<Script>(guard).expect("the Script");
    *script = script.clone().with_field("health", ScriptValue::Int(90));
    let edited = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the edited scene saves");
    project.write_file(KEEP, &edited);
}

// ─── Scenes ─────────────────────────────────────────────────────────────────

/// **A scene's `Script` becomes its behavior component**: the guard holds
/// `Guard` with the authored speed and the declared default health, and no
/// `Script`; the crate is as it was; every identity is kept; the file stays
/// in its encoding.
#[test]
fn a_scene_script_becomes_its_behavior_component_and_a_plain_entity_is_untouched() {
    let project = Project::new("scene");
    let before = guard_and_crate();
    let ids = (
        id(&before, named(&before, "Sentry")),
        id(&before, named(&before, "Crate")),
    );
    let file = SerializationService::new()
        .save_world(&before, SerializationGoal::HumanReadableDebug)
        .expect("the scene saves");
    let encoding = encoding_of(&file);
    project.write_file(SCENE, &file);

    project.upgrade(&[]).assert_succeeded();

    let converted = project.file(SCENE);
    assert_eq!(encoding_of(&converted), encoding, "the encoding is kept");
    let mut world = project.declared_world();
    SerializationService::new()
        .load_world(&converted, &mut world)
        .expect("the converted scene loads");

    let guard = named(&world, "Sentry");
    assert_eq!(id(&world, guard), ids.0, "the guard keeps its identity");
    assert_eq!(
        field_of(&world, guard, "Guard", "speed"),
        Some(ScriptValue::Float(2.5)),
        "the authored speed is the behavior's"
    );
    assert_eq!(
        field_of(&world, guard, "Guard", "health"),
        Some(ScriptValue::Int(100)),
        "a field the Script left unset takes its declared default"
    );
    assert!(world.get::<Script>(guard).is_none(), "no `Script` is left");

    let crate_ = named(&world, "Crate");
    assert_eq!(id(&world, crate_), ids.1, "the crate keeps its identity");
    assert_eq!(
        world.get::<Transform>(crate_).map(|t| t.translation),
        Some(Vec3::new(0.0, 2.0, 0.0)),
        "the crate is where it was"
    );
    assert!(
        !holds(&world, crate_, "Guard"),
        "the crate gains no behavior"
    );
}

/// **A prefab and the instance overriding it both convert**: the prefab's
/// sentry holds `Guard` at the prefab's speed; the level still links the
/// prefab, and its instance loads with the speed it overrode.
#[test]
fn a_prefab_and_an_instance_overriding_it_convert_and_the_override_survives() {
    let project = Project::new("prefab");
    let prefab = write_sentry_prefab(&project);
    write_level_with_instance(&project, prefab);

    project.upgrade(&[]).assert_succeeded();

    let mut prefab_world = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(PREFAB), &mut prefab_world)
        .expect("the converted prefab loads");
    let sentry = named(&prefab_world, "Sentry");
    assert_eq!(
        field_of(&prefab_world, sentry, "Guard", "speed"),
        Some(ScriptValue::Float(2.5)),
        "the prefab's speed is its behavior's"
    );
    assert!(prefab_world.get::<Script>(sentry).is_none());

    let level = project.file(SCENE);
    let record = read_scene_file(&level).expect("the converted level reads");
    assert_eq!(
        record.instances.len(),
        1,
        "the level still links the prefab"
    );
    assert_eq!(record.instances[0].prefab, asset_id(PREFAB));

    let library = project.library(&[PREFAB]);
    let mut world = project.declared_world();
    SerializationService::new()
        .load_world_with(&level, &mut world, &library)
        .expect("the converted level loads with the converted prefab");
    let instance = named(&world, "Sentry");
    assert_eq!(
        world
            .get::<PrefabInstance>(instance)
            .map(|link| link.prefab),
        Some(asset_id(PREFAB)),
        "the instance is still an instance of the prefab"
    );
    assert_eq!(
        field_of(&world, instance, "Guard", "speed"),
        Some(ScriptValue::Float(4.0)),
        "the instance's override survives"
    );
    assert!(world.get::<Script>(instance).is_none());
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **A save taken mid-state resumes as the behavior and its companion**: the
/// speed the game changed is the save's; the health it left alone is the
/// scene's as edited after the save — and stays the scene's when the scene
/// is edited again after the upgrade; `Guard#state` holds the state, the
/// countdown and the lifecycle `ScriptState` recorded.
#[test]
fn a_save_mid_state_converts_to_the_behavior_and_its_state_companion() {
    let project = Project::new("save");
    write_keep_and_save(&project);
    let save_encoding = encoding_of(&project.file(SAVE));

    project
        .upgrade(&["--saves", &project.saves_arg()])
        .assert_succeeded();

    let save = project.file(SAVE);
    assert_eq!(
        encoding_of(&save),
        save_encoding,
        "the save's encoding is kept"
    );
    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &save, &project.file(KEEP))
        .expect("the converted save loads against the converted scene");

    let guard = named(&world, "Sentry");
    assert_eq!(
        field_of(&world, guard, "Guard", "speed"),
        Some(ScriptValue::Float(7.0)),
        "the speed the game changed is the save's"
    );
    assert_eq!(
        field_of(&world, guard, "Guard", "health"),
        Some(ScriptValue::Int(90)),
        "the health the game left alone is the scene's current one"
    );
    assert!(world.get::<Script>(guard).is_none(), "no `Script` is left");
    assert!(
        world.get::<ScriptState>(guard).is_none(),
        "no `ScriptState` is left"
    );
    assert_eq!(
        field_of(&world, guard, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned())),
        "the guard resumes in the state it was saved in"
    );
    assert_eq!(
        field_of(&world, guard, "Guard#state", "spawned"),
        Some(ScriptValue::Bool(true))
    );
    assert_eq!(
        field_of(&world, guard, "Guard#state", "initialised"),
        Some(ScriptValue::Bool(true)),
        "a saved instance has run its initialiser"
    );
    match field_of(&world, guard, "Guard#state", "timers") {
        Some(ScriptValue::Array(timers)) => {
            assert_eq!(timers.len(), 1, "the countdown is kept: {timers:?}")
        }
        other => panic!("the countdowns are an array, not {other:?}"),
    }

    // The scene edited after the upgrade still reaches what the game left.
    let mut scene_world = project.declared_world();
    let scene_service = SerializationService::new();
    let scene = project.file(KEEP);
    scene_service
        .load_world(&scene, &mut scene_world)
        .expect("the converted scene loads");
    let sentry = named(&scene_world, "Sentry");
    let guard_key = key(&scene_world, "Guard");
    let health = scene_world
        .components()
        .vtable(guard_key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|packed| packed.slot_of("health"))
        .expect("`Guard` has a `health`");
    scene_world
        .row_mut(sentry, guard_key)
        .expect("the sentry holds `Guard`")
        .set_field(health, &ScriptValue::Int(95))
        .expect("`health` takes an int");
    let edited = scene_service
        .save_world(&scene_world, SerializationGoal::EditorInterchange)
        .expect("the edited scene saves");

    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &save, &edited)
        .expect("the converted save loads against the edited scene");
    let guard = named(&world, "Sentry");
    assert_eq!(
        field_of(&world, guard, "Guard", "health"),
        Some(ScriptValue::Int(95)),
        "the save holds no value for a field the game left alone"
    );
    assert_eq!(
        field_of(&world, guard, "Guard", "speed"),
        Some(ScriptValue::Float(7.0))
    );
}

/// **A save whose base scene is not in the project fails the run**, and no
/// file is written — not even the scene that would have converted.
#[test]
fn a_save_whose_base_scene_is_missing_fails_and_writes_nothing() {
    let project = Project::new("missing-base");
    write_keep_and_save(&project);
    // The save's base, gone; another scene that would convert, beside it.
    fs::remove_file(project.path(KEEP)).expect("removes the base");
    let file = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &file);
    let before = project.bytes();

    let run = project.upgrade(&["--saves", &project.saves_arg()]);

    run.assert_refused();
    assert!(
        run.text().contains("slot1.ksave"),
        "the failure names the save:\n{}",
        run.text()
    );
    assert_eq!(project.bytes(), before, "nothing is written");
}

// ─── What is left alone ─────────────────────────────────────────────────────

/// **A file with no `Script` is left byte-identical**, as is every file that
/// is not a scene — while a scene beside them converts.
#[test]
fn a_file_without_script_is_left_byte_identical() {
    let project = Project::new("untouched");
    let mut plain = World::new();
    let crate_ = plain.spawn((Transform::identity(), Name::new("Crate")));
    plain.mark_authored(crate_).expect("authored");
    let plain_file = SerializationService::new()
        .save_world(&plain, SerializationGoal::EditorInterchange)
        .expect("the plain scene saves");
    project.write_file("assets/scenes/plain.kscene", &plain_file);
    project.write("assets/notes.txt", b"not a scene");
    let scripted = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::EditorInterchange)
        .expect("the scripted scene saves");
    project.write_file(SCENE, &scripted);
    let plain_bytes = project.read("assets/scenes/plain.kscene");
    let script_bytes = project.read(&format!("assets/scripts/{GUARD_MODULE}"));

    project.upgrade(&[]).assert_succeeded();

    assert_ne!(
        project.read(SCENE),
        scripted.to_bytes(),
        "the scene holding a `Script` is converted"
    );
    assert_eq!(
        project.read("assets/scenes/plain.kscene"),
        plain_bytes,
        "a scene with no `Script` keeps its bytes"
    );
    assert_eq!(project.read("assets/notes.txt"), b"not a scene");
    assert_eq!(
        project.read(&format!("assets/scripts/{GUARD_MODULE}")),
        script_bytes
    );
}

/// **A snapshot-encoded file is skipped with a warning**: it is a same-build
/// cache, regenerated rather than converted. The run still succeeds.
#[test]
fn a_snapshot_encoded_file_is_skipped_with_a_warning() {
    let project = Project::new("snapshot");
    let file = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::FastestLoad)
        .expect("the scene saves");
    assert_eq!(
        encoding_of(&file),
        SNAPSHOT_ENCODING_ID,
        "the fixture is a snapshot"
    );
    project.write_file("assets/scenes/cache.kscene", &file);
    let before = project.bytes();

    let run = project.upgrade(&[]);

    run.assert_succeeded();
    assert_eq!(project.bytes(), before, "the snapshot is not rewritten");
    assert!(
        run.text().contains("cache.kscene"),
        "the warning names the skipped file:\n{}",
        run.text()
    );
}

/// **`--dry-run` writes nothing** and succeeds, printing what it would
/// convert.
#[test]
fn a_dry_run_writes_nothing_and_prints_the_planned_conversions() {
    let project = Project::new("dry-run");
    let prefab = write_sentry_prefab(&project);
    write_level_with_instance(&project, prefab);
    write_keep_and_save(&project);
    let before = project.bytes();

    let run = project.upgrade(&["--saves", &project.saves_arg(), "--dry-run"]);

    run.assert_succeeded();
    assert_eq!(project.bytes(), before, "a dry run changes no byte");
    let text = run.text();
    for file in [
        "level.kscene",
        "sentry.kprefab",
        "keep.kscene",
        "slot1.ksave",
    ] {
        assert!(text.contains(file), "the plan names `{file}`:\n{text}");
    }
}

// ─── Refusals and warnings ──────────────────────────────────────────────────

/// **A `Script` naming a behavior no script declares fails the run**, and
/// nothing is written — not even the other scene, which alone would convert.
#[test]
fn a_script_naming_an_undeclared_behavior_fails_and_writes_nothing() {
    let project = Project::new("undeclared");
    let good = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &good);
    let mut ghostly = World::new();
    let ghost = ghostly.spawn((
        Transform::identity(),
        Name::new("Ghost"),
        Script::new("ghost.erg", "Ghost"),
    ));
    ghostly.mark_authored(ghost).expect("authored");
    let bad = SerializationService::new()
        .save_world(&ghostly, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file("assets/scenes/haunted.kscene", &bad);
    let before = project.bytes();

    let run = project.upgrade(&[]);

    run.assert_refused();
    assert!(
        run.text().contains("Ghost"),
        "the failure names the undeclared behavior:\n{}",
        run.text()
    );
    assert_eq!(project.bytes(), before, "nothing is written");
}

/// **A field the behavior lacks, or holds as another type, is a warning**:
/// the run succeeds, the field keeps its declared default, the output names
/// it — and a field that fits is still taken.
#[test]
fn a_field_the_behavior_lacks_or_types_differently_is_a_warning() {
    let project = Project::new("fields");
    let mut world = World::new();
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard")
            .with_field("speed", ScriptValue::Str("fast".to_owned()))
            .with_field("stealth", ScriptValue::Bool(true))
            .with_field("health", ScriptValue::Int(50)),
    ));
    world.mark_authored(guard).expect("authored");
    let file = SerializationService::new()
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &file);

    let run = project.upgrade(&[]);

    run.assert_succeeded();
    let text = run.text();
    assert!(
        text.contains("stealth"),
        "the unknown field is named:\n{text}"
    );
    assert!(
        text.contains("speed"),
        "the mistyped field is named:\n{text}"
    );

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(SCENE), &mut loaded)
        .expect("the converted scene loads");
    let guard = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, guard, "Guard", "speed"),
        Some(ScriptValue::Float(1.0)),
        "a mistyped field keeps its declared default"
    );
    assert_eq!(
        field_of(&loaded, guard, "Guard", "health"),
        Some(ScriptValue::Int(50)),
        "a field that fits is taken"
    );
}

// ─── Running again ──────────────────────────────────────────────────────────

/// **A second run converts nothing and changes no byte**: once upgraded, a
/// project holds no `Script` or `ScriptState` to convert.
#[test]
fn a_second_run_changes_no_byte() {
    let project = Project::new("twice");
    let prefab = write_sentry_prefab(&project);
    write_level_with_instance(&project, prefab);
    write_keep_and_save(&project);
    let saves = project.saves_arg();

    project.upgrade(&["--saves", &saves]).assert_succeeded();
    let once = project.bytes();
    project.upgrade(&["--saves", &saves]).assert_succeeded();

    assert_eq!(project.bytes(), once, "the second run changes no byte");
}

// ─── What a file relative to a converted one keeps ──────────────────────────

/// **An instance that removed its prefab's `Script` still runs no behavior**
/// once the prefab is converted: its removal of `Script` must become a
/// removal of `Guard`, even when the scene holds nothing else to convert.
#[test]
fn an_instance_that_removed_its_prefab_script_gains_no_behavior() {
    let project = Project::new("instance-removed-script");
    let prefab = write_sentry_prefab(&project);
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), prefab);
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, asset_id(PREFAB), &library)
        .expect("the prefab instantiates");
    world
        .remove_component::<Script>(root)
        .expect("the instance drops the prefab's Script");
    let file = SerializationService::new()
        .save_world_with(&world, SerializationGoal::EditorInterchange, &library)
        .expect("the level saves");
    project.write_file(SCENE, &file);

    project.upgrade(&[]).assert_succeeded();

    let library = project.library(&[PREFAB]);
    let mut world = project.declared_world();
    SerializationService::new()
        .load_world_with(&project.file(SCENE), &mut world, &library)
        .expect("the level loads with the converted prefab");
    let instance = named(&world, "Sentry");
    assert!(
        !holds(&world, instance, "Guard"),
        "the instance removed the prefab's behavior; it must not gain `Guard`"
    );
}

/// **A save in which the game detached a scene entity's `Script` still runs
/// no behavior on it** once the scene is converted, even when the save holds
/// nothing else to convert.
#[test]
fn a_save_that_detached_a_script_gains_no_behavior_against_the_converted_scene() {
    let project = Project::new("save-detached-script");
    let service = SerializationService::new();
    let mut world = World::new();
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(2.5)),
    ));
    world.mark_authored(guard).expect("authored");
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(KEEP, &base);
    world
        .remove_component::<Script>(guard)
        .expect("the game detaches the Script");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);

    project
        .upgrade(&["--saves", &project.saves_arg()])
        .assert_succeeded();

    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &project.file(SAVE), &project.file(KEEP))
        .expect("the save loads against the converted scene");
    let guard = named(&world, "Sentry");
    assert!(
        !holds(&world, guard, "Guard"),
        "the game had detached the behavior; the save must not bring `Guard` back"
    );
}

/// **Upgrading the scenes first, then the saves, keeps what the game was
/// doing**: `--saves` is optional, so a project may be upgraded in two runs.
/// The second run either converts the save's `ScriptState` onto the scene's
/// `Guard` or refuses and leaves the save as it was — it never drops the
/// state and rewrites the save.
#[test]
fn saves_upgraded_after_their_scenes_keep_the_state() {
    let project = Project::new("saves-after-scenes");
    write_keep_and_save(&project);
    project.upgrade(&[]).assert_succeeded();
    let save_before = project.read(SAVE);

    let run = project.upgrade(&["--saves", &project.saves_arg()]);

    if !run.succeeded() {
        assert_eq!(
            project.read(SAVE),
            save_before,
            "a refused save is untouched"
        );
        return;
    }
    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &project.file(SAVE), &project.file(KEEP))
        .expect("the save loads against the converted scene");
    let guard = named(&world, "Sentry");
    assert_eq!(
        field_of(&world, guard, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned())),
        "the state the save recorded is kept — it printed:\n{}",
        run.text()
    );
    assert_eq!(
        field_of(&world, guard, "Guard", "speed"),
        Some(ScriptValue::Float(7.0)),
        "the speed the game changed is kept"
    );
}

// ─── Values that name other entities ────────────────────────────────────────

const SCOUT_MODULE: &str = "scout.erg";
const SCOUT: &str = "struct Point { float x = 0.0; }
behavior Scout {
    Entity target;
    Point at;
    Entity[] seen;
    int[] route;
    void Update(float dt) { }
}
";

/// **A `Script`'s entity references, structs and arrays reach the behavior**:
/// `target` and `seen` still name the crate after the scene loads in a fresh
/// world; `at` and `route` keep their values.
#[test]
fn script_fields_naming_entities_structs_and_arrays_convert_and_still_resolve() {
    let project = Project::new("references");
    project.write(&format!("assets/scripts/{SCOUT_MODULE}"), SCOUT.as_bytes());
    let mut world = World::new();
    let crate_ = world.spawn((Transform::identity(), Name::new("Crate")));
    let scout = world.spawn((
        Transform::identity(),
        Name::new("Scout"),
        Script::new(SCOUT_MODULE, "Scout")
            .with_field("target", ScriptValue::Entity(crate_))
            .with_field(
                "at",
                ScriptValue::Struct(vec![("x".to_owned(), ScriptValue::Float(3.0))]),
            )
            .with_field(
                "seen",
                ScriptValue::Array(vec![ScriptValue::Entity(crate_)]),
            )
            .with_field(
                "route",
                ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(2)]),
            ),
    ));
    for entity in [crate_, scout] {
        world.mark_authored(entity).expect("authored");
    }
    let file = SerializationService::new()
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &file);

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(SCENE), &mut loaded)
        .expect("the converted scene loads");
    let crate_ = named(&loaded, "Crate");
    let scout = named(&loaded, "Scout");
    assert_eq!(
        field_of(&loaded, scout, "Scout", "target"),
        Some(ScriptValue::Entity(crate_)),
        "`target` names the crate — it printed:\n{}",
        run.text()
    );
    assert_eq!(
        field_of(&loaded, scout, "Scout", "seen"),
        Some(ScriptValue::Array(vec![ScriptValue::Entity(crate_)])),
        "`seen` names the crate"
    );
    match field_of(&loaded, scout, "Scout", "at") {
        Some(ScriptValue::Struct(fields)) => assert!(
            fields.contains(&("x".to_owned(), ScriptValue::Float(3.0))),
            "`at.x` is kept: {fields:?}"
        ),
        other => panic!("`at` is a struct, not {other:?}"),
    }
    assert_eq!(
        field_of(&loaded, scout, "Scout", "route"),
        Some(ScriptValue::Array(vec![
            ScriptValue::Int(1),
            ScriptValue::Int(2)
        ])),
        "`route` is kept"
    );
}

/// The first `ScriptValue::Entity` anywhere inside `value`.
fn first_entity(value: &ScriptValue) -> Option<EntityId> {
    match value {
        ScriptValue::Entity(entity) => Some(*entity),
        ScriptValue::Array(items) => items.iter().find_map(first_entity),
        ScriptValue::Struct(fields) => fields.iter().find_map(|(_, v)| first_entity(v)),
        _ => None,
    }
}

/// **A save taken mid-`await` resumes with its suspended sequence**: the
/// companion's `pending` is kept, and the entity a register of the frozen
/// machine names is still the crate after the save loads in a fresh world.
#[test]
fn a_save_mid_await_keeps_its_pending_sequence_and_the_entity_it_names() {
    use khora_core::script::{
        FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence,
    };

    let project = Project::new("mid-await");
    let service = SerializationService::new();
    let mut world = World::new();
    let crate_ = world.spawn((Transform::identity(), Name::new("Crate")));
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard"),
    ));
    for entity in [crate_, guard] {
        world.mark_authored(entity).expect("authored");
    }
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(KEEP, &base);
    let pending = PendingSequence {
        fingerprint: 0xDEAD_BEEF,
        remaining: 0.75,
        machine: FrozenMachine {
            body: PendingBody::Sequence,
            registers: vec![FrozenValue::Entity(crate_), FrozenValue::Int(3)],
            frames: vec![FrozenFrame {
                function: "Guard::Update".to_owned(),
                base: 0,
                return_pc: 0,
                result: 0,
                site: "expr.0:await".to_owned(),
                fingerprint: 0x1234,
                locals: Vec::new(),
                temporaries: Vec::new(),
            }],
            program_counter: 2,
            arguments: Vec::new(),
        },
    };
    world
        .add_component(
            guard,
            ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot {
                    pending: Some(pending),
                    lifecycle: InstanceLifecycle {
                        spawned: true,
                        ..Default::default()
                    },
                    ..ScriptSnapshot::default()
                },
            },
        )
        .expect("the state attaches");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);

    let run = project.upgrade(&["--saves", &project.saves_arg()]);
    run.assert_succeeded();

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_game(&mut loaded, &project.file(SAVE), &project.file(KEEP))
        .expect("the converted save loads");
    let crate_ = named(&loaded, "Crate");
    let guard = named(&loaded, "Sentry");
    let pending = field_of(&loaded, guard, "Guard#state", "pending")
        .unwrap_or_else(|| panic!("the companion holds `pending`:\n{}", run.text()));
    assert_ne!(pending, ScriptValue::Null, "the suspended sequence is kept");
    assert_eq!(
        first_entity(&pending),
        Some(crate_),
        "the frozen register still names the crate: {pending:?}"
    );
}

// ─── Saves: what the scene lacks, what the state disagrees with ─────────────

/// **An entity the game spawned with a `Script` converts in the save**, with
/// its state, though the save's scene holds no `Script` at all.
#[test]
fn a_scripted_entity_spawned_during_play_converts_in_the_save() {
    let project = Project::new("spawned-in-play");
    let service = SerializationService::new();
    let mut world = World::new();
    let crate_ = world.spawn((Transform::identity(), Name::new("Crate")));
    world.mark_authored(crate_).expect("authored");
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(KEEP, &base);
    let base_bytes = base.to_bytes();
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Spawned"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(3.0)),
    ));
    world
        .add_component(
            guard,
            ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot {
                    fields: vec![("speed".to_owned(), ScriptValue::Float(6.0))],
                    authored: vec![("speed".to_owned(), ScriptValue::Float(3.0))],
                    state: Some("Patrol".to_owned()),
                    lifecycle: InstanceLifecycle {
                        spawned: true,
                        ..Default::default()
                    },
                    ..ScriptSnapshot::default()
                },
            },
        )
        .expect("the state attaches");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::HumanReadableDebug,
        )
        .expect("the game saves");
    let encoding = encoding_of(&save);
    project.write_file(SAVE, &save);

    project
        .upgrade(&["--saves", &project.saves_arg()])
        .assert_succeeded();

    assert_eq!(
        project.read(KEEP),
        base_bytes,
        "the scene holds no Script: untouched"
    );
    let save = project.file(SAVE);
    assert_eq!(encoding_of(&save), encoding, "a text save stays text");
    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_game(&mut loaded, &save, &project.file(KEEP))
        .expect("the converted save loads");
    let spawned = named(&loaded, "Spawned");
    assert!(loaded.get::<Script>(spawned).is_none());
    assert!(loaded.get::<ScriptState>(spawned).is_none());
    assert_eq!(
        field_of(&loaded, spawned, "Guard", "speed"),
        Some(ScriptValue::Float(6.0))
    );
    assert_eq!(
        field_of(&loaded, spawned, "Guard#state", "state"),
        Some(ScriptValue::Str("Patrol".to_owned()))
    );
}

/// **A state recorded for another behavior is dropped with a warning, and a
/// `Script` that never ran converts with no companion**: both in one save.
#[test]
fn a_state_of_another_behavior_is_dropped_and_a_script_that_never_ran_converts_alone() {
    let project = Project::new("state-mismatch");
    let service = SerializationService::new();
    let mut world = World::new();
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(KEEP, &base);
    let renamed = world.spawn((
        Transform::identity(),
        Name::new("Renamed"),
        Script::new(GUARD_MODULE, "Guard"),
    ));
    world
        .add_component(
            renamed,
            ScriptState {
                behavior: "OldGuard".to_owned(),
                snapshot: ScriptSnapshot {
                    fields: vec![("speed".to_owned(), ScriptValue::Float(9.0))],
                    state: Some("Attack".to_owned()),
                    ..ScriptSnapshot::default()
                },
            },
        )
        .expect("the state attaches");
    world.spawn((
        Transform::identity(),
        Name::new("Fresh"),
        Script::new(GUARD_MODULE, "Guard").with_field("health", ScriptValue::Int(7)),
    ));
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);

    let run = project.upgrade(&["--saves", &project.saves_arg()]);
    run.assert_succeeded();
    assert!(
        run.text().contains("OldGuard"),
        "the dropped state is named:\n{}",
        run.text()
    );

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_game(&mut loaded, &project.file(SAVE), &project.file(KEEP))
        .expect("the converted save loads");
    let renamed = named(&loaded, "Renamed");
    assert_eq!(
        field_of(&loaded, renamed, "Guard", "speed"),
        Some(ScriptValue::Float(1.0)),
        "another behavior's observed values are not taken"
    );
    assert!(!holds(&loaded, renamed, "Guard#state"));
    assert!(loaded.get::<ScriptState>(renamed).is_none());
    let fresh = named(&loaded, "Fresh");
    assert_eq!(
        field_of(&loaded, fresh, "Guard", "health"),
        Some(ScriptValue::Int(7))
    );
    assert!(!holds(&loaded, fresh, "Guard#state"));
}

// ─── Prefabs ────────────────────────────────────────────────────────────────

const OUTPOST: &str = "assets/prefabs/outpost.kprefab";

/// **A prefab holding an instance of another prefab converts after it**: the
/// outpost, whose sentry overrides the speed, still links the sentry
/// prefab, and a scene holding an outpost loads with the override.
#[test]
fn a_nested_prefab_instance_converts_and_keeps_its_override() {
    let project = Project::new("nested-prefab");
    let sentry = write_sentry_prefab(&project);
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), sentry);
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Outpost")));
    world.mark_authored(root).expect("authored");
    let inner = instantiate_prefab(&mut world, asset_id(PREFAB), &library)
        .expect("the sentry instantiates");
    assert!(world.set_parent(inner, Some(root)));
    let script = world.get_mut::<Script>(inner).expect("the Script");
    *script = script.clone().with_field("speed", ScriptValue::Float(5.0));
    let bytes = serialize_prefab(&world, root, asset_id(OUTPOST), &library)
        .expect("the outpost serializes");
    project.write(OUTPOST, &bytes);
    let outpost = read_scene_file(&SceneFile::from_bytes(&bytes).expect("a file")).expect("reads");
    assert_eq!(outpost.instances.len(), 1, "the outpost links the sentry");
    library.0.insert(asset_id(OUTPOST), outpost);
    let mut level = World::new();
    instantiate_prefab(&mut level, asset_id(OUTPOST), &library).expect("the outpost instantiates");
    let file = SerializationService::new()
        .save_world_with(&level, SerializationGoal::EditorInterchange, &library)
        .expect("the level saves");
    project.write_file(SCENE, &file);

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let outpost = read_scene_file(&project.file(OUTPOST)).expect("the outpost reads");
    assert_eq!(
        outpost.instances.len(),
        1,
        "the outpost still links the sentry"
    );
    let library = project.library(&[PREFAB, OUTPOST]);
    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world_with(&project.file(SCENE), &mut loaded, &library)
        .expect("the level loads with the converted prefabs");
    let sentry = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, sentry, "Guard", "speed"),
        Some(ScriptValue::Float(5.0)),
        "the outpost's override survives:\n{}",
        run.text()
    );
    assert!(loaded.get::<Script>(sentry).is_none());
}

/// **Two instances of one prefab keep each its own override**, and an
/// instance that added a `Script` its prefab lacks converts it.
#[test]
fn two_instances_keep_their_own_overrides_and_an_added_script_converts() {
    let project = Project::new("two-instances");
    let sentry = write_sentry_prefab(&project);
    let mut plain = World::new();
    let post = plain.spawn((Transform::identity(), Name::new("Post")));
    plain.mark_authored(post).expect("authored");
    let post_prefab = "assets/prefabs/post.kprefab";
    let bytes = serialize_prefab(&plain, post, asset_id(post_prefab), &NoPrefabs)
        .expect("the post serializes");
    project.write(post_prefab, &bytes);
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), sentry);
    library.0.insert(
        asset_id(post_prefab),
        read_scene_file(&SceneFile::from_bytes(&bytes).expect("a file")).expect("reads"),
    );

    let mut world = World::new();
    let a = instantiate_prefab(&mut world, asset_id(PREFAB), &library).expect("instance a");
    let b = instantiate_prefab(&mut world, asset_id(PREFAB), &library).expect("instance b");
    let c = instantiate_prefab(&mut world, asset_id(post_prefab), &library).expect("instance c");
    *world.get_mut::<Name>(a).expect("a name") = Name::new("A");
    *world.get_mut::<Name>(b).expect("a name") = Name::new("B");
    let sa = world.get_mut::<Script>(a).expect("a Script");
    *sa = sa.clone().with_field("speed", ScriptValue::Float(4.0));
    let sb = world.get_mut::<Script>(b).expect("b Script");
    *sb = sb.clone().with_field("health", ScriptValue::Int(42));
    world
        .add_component(
            c,
            Script::new(GUARD_MODULE, "Guard").with_field("health", ScriptValue::Int(3)),
        )
        .expect("c gains a Script");
    let file = SerializationService::new()
        .save_world_with(&world, SerializationGoal::HumanReadableDebug, &library)
        .expect("the level saves");
    let encoding = encoding_of(&file);
    project.write_file(SCENE, &file);

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let converted = project.file(SCENE);
    assert_eq!(encoding_of(&converted), encoding, "a text scene stays text");
    let library = project.library(&[PREFAB, post_prefab]);
    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world_with(&converted, &mut loaded, &library)
        .expect("the level loads");
    let (a, b, c) = (
        named(&loaded, "A"),
        named(&loaded, "B"),
        named(&loaded, "Post"),
    );
    assert_eq!(
        field_of(&loaded, a, "Guard", "speed"),
        Some(ScriptValue::Float(4.0))
    );
    assert_eq!(
        field_of(&loaded, a, "Guard", "health"),
        Some(ScriptValue::Int(100))
    );
    assert_eq!(
        field_of(&loaded, b, "Guard", "speed"),
        Some(ScriptValue::Float(2.5))
    );
    assert_eq!(
        field_of(&loaded, b, "Guard", "health"),
        Some(ScriptValue::Int(42))
    );
    assert_eq!(
        field_of(&loaded, c, "Guard", "health"),
        Some(ScriptValue::Int(3))
    );
}

/// **A prefab whose `Script` sits on a child, written in text, converts**:
/// the child holds `Guard`, the root none; the change of encoding to the
/// prefab writer's compact form is reported.
#[test]
fn a_text_prefab_with_a_scripted_child_converts_and_reports_its_new_encoding() {
    let project = Project::new("child-script");
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Tower")));
    let child = world.spawn((
        Transform::identity(),
        Name::new("Lookout"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(8.0)),
    ));
    assert!(world.set_parent(child, Some(root)));
    for entity in [root, child] {
        world.mark_authored(entity).expect("authored");
    }
    // A prefab file in text: the whole subtree, saved as a scene.
    let file = SerializationService::new()
        .save_world(&world, SerializationGoal::HumanReadableDebug)
        .expect("the prefab saves");
    let text = encoding_of(&file);
    project.write_file(PREFAB, &file);

    let run = project.upgrade(&[]);
    run.assert_succeeded();
    assert!(
        run.text().contains(&text),
        "the report names the encoding it left:\n{}",
        run.text()
    );

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(PREFAB), &mut loaded)
        .expect("the converted prefab loads");
    let lookout = named(&loaded, "Lookout");
    let tower = named(&loaded, "Tower");
    assert_eq!(
        field_of(&loaded, lookout, "Guard", "speed"),
        Some(ScriptValue::Float(8.0))
    );
    assert!(!holds(&loaded, tower, "Guard"));
}

// ─── Identities, failures, and the run itself ───────────────────────────────

/// **A prefab renamed with its id frozen in the asset registry converts and
/// stays linked**: the level's instance still names the frozen id.
#[test]
fn a_renamed_prefab_with_a_frozen_id_converts_and_stays_linked() {
    let project = Project::new("frozen-id");
    let prefab = write_sentry_prefab(&project);
    write_level_with_instance(&project, prefab);
    let renamed = "assets/prefabs/watchman.kprefab";
    fs::rename(project.path(PREFAB), project.path(renamed)).expect("renames the prefab");
    let mut registry = khora_io::asset::AssetIdRegistry::load(project.root.clone());
    registry.freeze(
        &asset_key(Path::new("prefabs/watchman.kprefab")),
        asset_id(PREFAB),
    );
    registry.save().expect("the registry saves");

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let level = project.file(SCENE);
    let record = read_scene_file(&level).expect("the level reads");
    assert_eq!(record.instances.len(), 1, "still linked:\n{}", run.text());
    assert_eq!(record.instances[0].prefab, asset_id(PREFAB));
    let mut library = Library::default();
    library.0.insert(
        asset_id(PREFAB),
        read_scene_file(&project.file(renamed)).expect("the renamed prefab reads"),
    );
    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world_with(&level, &mut loaded, &library)
        .expect("the level loads");
    let sentry = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, sentry, "Guard", "speed"),
        Some(ScriptValue::Float(4.0))
    );
}

/// **A script that does not compile is named when a scene needs it**.
#[test]
fn a_behavior_whose_script_does_not_compile_names_the_script() {
    let project = Project::new("uncompiled");
    project.write(
        &format!("assets/scripts/{GUARD_MODULE}"),
        b"behavior Guard { float speed = ; }",
    );
    let file = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &file);
    let before = project.bytes();

    let run = project.upgrade(&[]);

    run.assert_refused();
    assert!(run.text().contains(GUARD_MODULE), "{}", run.text());
    assert_eq!(project.bytes(), before);
}

/// **A project with no `assets/` is refused**, not a panic.
#[test]
fn a_project_with_no_assets_is_refused() {
    let project = Project::new("no-assets");
    fs::remove_dir_all(project.path("assets")).expect("removes assets");
    project.write("readme.txt", b"empty");

    project.upgrade(&[]).assert_refused();
}

/// **A file that cannot be replaced leaves every file as it was**: the save
/// is read-only — as a file checked out from a locking VCS is — so replacing
/// it fails. The scene converted before it must not be left converted alone
/// (its save would then be relative to a scene it was not written against),
/// and no temporary file is left behind.
#[cfg(windows)]
#[test]
fn a_file_that_cannot_be_replaced_leaves_the_project_as_it_was() {
    let project = Project::new("read-only");
    write_keep_and_save(&project);
    let save = project.path(SAVE);
    let mut permissions = fs::metadata(&save).expect("the save").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&save, permissions.clone()).expect("the save is read-only");
    let scene_before = project.read(KEEP);
    let before = project.bytes();

    let run = project.upgrade(&["--saves", &project.saves_arg()]);

    let scene_after = project.read(KEEP);
    let after = project.bytes();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    let _ = fs::set_permissions(&save, permissions);
    assert!(
        !run.succeeded(),
        "the run reports the failure:\n{}",
        run.text()
    );
    assert!(
        scene_after == scene_before,
        "the scene is not left converted while its save is not:\n{}",
        run.text()
    );
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>(),
        "no temporary file is left behind"
    );
}

/// **A save of a scene holding a prefab instance converts against the
/// converted prefab**: the speed the game changed on the instance and its
/// state reach `Guard` and `Guard#state`.
#[test]
fn a_save_of_a_prefab_instance_converts_against_the_converted_prefab() {
    let project = Project::new("save-of-instance");
    let prefab = write_sentry_prefab(&project);
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), prefab);
    let library = std::sync::Arc::new(library);
    let service = SerializationService::with_prefabs(library.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, asset_id(PREFAB), library.as_ref())
        .expect("the prefab instantiates");
    let base = service
        .save_world_with(
            &world,
            SerializationGoal::EditorInterchange,
            library.as_ref(),
        )
        .expect("the level saves");
    project.write_file(KEEP, &base);
    world
        .add_component(
            root,
            ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot {
                    fields: vec![("speed".to_owned(), ScriptValue::Float(9.0))],
                    authored: vec![("speed".to_owned(), ScriptValue::Float(2.5))],
                    state: Some("Attack".to_owned()),
                    lifecycle: InstanceLifecycle {
                        spawned: true,
                        ..Default::default()
                    },
                    ..ScriptSnapshot::default()
                },
            },
        )
        .expect("the state attaches");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);

    let run = project.upgrade(&["--saves", &project.saves_arg()]);
    run.assert_succeeded();

    let converted = std::sync::Arc::new(project.library(&[PREFAB]));
    let mut loaded = project.declared_world();
    SerializationService::with_prefabs(converted)
        .load_game(&mut loaded, &project.file(SAVE), &project.file(KEEP))
        .expect("the converted save loads with the converted prefab");
    let guard = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, guard, "Guard", "speed"),
        Some(ScriptValue::Float(9.0)),
        "the speed the game changed is kept:\n{}",
        run.text()
    );
    assert_eq!(
        field_of(&loaded, guard, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned()))
    );
    assert!(loaded.get::<Script>(guard).is_none());
    assert!(loaded.get::<ScriptState>(guard).is_none());
}

/// **A component a script declares, held beside a `Script`, is kept**: the
/// converted scene still carries `Loot` with its value.
#[test]
fn a_script_declared_component_beside_a_script_is_kept() {
    let project = Project::new("declared-beside");
    project.write(
        "assets/scripts/loot.erg",
        b"component Loot { int coins = 0; }\n",
    );
    let mut world = project.declared_world();
    let loot = key(&world, "Loot");
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard"),
    ));
    world.mark_authored(guard).expect("authored");
    world
        .add_runtime_component(
            guard,
            loot,
            &ScriptValue::Struct(vec![("coins".to_owned(), ScriptValue::Int(12))]),
        )
        .expect("Loot attaches");
    let file = SerializationService::new()
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(SCENE, &file);

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(SCENE), &mut loaded)
        .expect("the converted scene loads");
    let guard = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, guard, "Loot", "coins"),
        Some(ScriptValue::Int(12)),
        "{}",
        run.text()
    );
    assert!(holds(&loaded, guard, "Guard"));
}

// ─── Two runs, nested links, and where the files are written ────────────────

/// Writes `KEEP` with a sentry running `Guard`, and a save of it at `SAVE` in
/// which the game detached the sentry's `Script`.
fn write_keep_and_save_detaching_the_script(project: &Project) {
    let service = SerializationService::new();
    let mut world = World::new();
    let guard = world.spawn((
        Transform::identity(),
        Name::new("Sentry"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(2.5)),
    ));
    world.mark_authored(guard).expect("authored");
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(KEEP, &base);
    world
        .remove_component::<Script>(guard)
        .expect("the game detaches the Script");
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);
}

/// **A save in which the game detached a `Script`, upgraded after its scene,
/// still runs no behavior on that entity**: the scene was converted in a run
/// without `--saves`; the save, upgraded in a second run, records the removal
/// of `Script` — which must become the removal of `Guard`, not be lost.
#[test]
fn a_save_that_detached_a_script_gains_no_behavior_when_its_scene_was_converted_first() {
    let project = Project::new("detached-after-scenes");
    write_keep_and_save_detaching_the_script(&project);
    project.upgrade(&[]).assert_succeeded();

    let run = project.upgrade(&["--saves", &project.saves_arg()]);

    if !run.succeeded() {
        // Refusing is acceptable; silently bringing the behavior back is not.
        return;
    }
    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &project.file(SAVE), &project.file(KEEP))
        .expect("the save loads against the converted scene");
    let guard = named(&world, "Sentry");
    assert!(
        !holds(&world, guard, "Guard"),
        "the game had detached the behavior; the save must not bring `Guard` back — it printed:\n{}",
        run.text()
    );
}

/// **A scene and a prefab named alike in one directory are each written
/// their own bytes**: `tower.kscene` and `tower.kprefab` both convert; each
/// must come out as itself, and the run must succeed.
#[test]
fn a_scene_and_a_prefab_with_the_same_stem_are_each_written_their_own() {
    let project = Project::new("same-stem");
    let scene_rel = "assets/levels/tower.kscene";
    let prefab_rel = "assets/levels/tower.kprefab";
    let scene = SerializationService::new()
        .save_world(&guard_and_crate(), SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    project.write_file(scene_rel, &scene);
    let mut world = World::new();
    let root = world.spawn((
        Transform::identity(),
        Name::new("Lookout"),
        Script::new(GUARD_MODULE, "Guard").with_field("speed", ScriptValue::Float(8.0)),
    ));
    world.mark_authored(root).expect("authored");
    let bytes = serialize_prefab(&world, root, asset_id(prefab_rel), &NoPrefabs)
        .expect("the prefab serializes");
    project.write(prefab_rel, &bytes);

    let run = project.upgrade(&[]);
    run.assert_succeeded();

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(scene_rel), &mut loaded)
        .expect("the converted scene loads");
    let guard = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, guard, "Guard", "speed"),
        Some(ScriptValue::Float(2.5)),
        "the scene is the scene converted"
    );
    let mut prefab = project.declared_world();
    SerializationService::new()
        .load_world(&project.file(prefab_rel), &mut prefab)
        .expect("the converted prefab loads");
    let lookout = named(&prefab, "Lookout");
    assert_eq!(
        field_of(&prefab, lookout, "Guard", "speed"),
        Some(ScriptValue::Float(8.0)),
        "the prefab is the prefab converted"
    );
}

/// **A save found twice — under `assets/` and through `--saves` — is
/// converted once**, and the run succeeds.
#[test]
fn a_save_found_under_assets_and_through_saves_converts_once() {
    let project = Project::new("save-found-twice");
    write_keep_and_save(&project);
    let inside = "assets/saves/slot1.ksave";
    fs::create_dir_all(project.path("assets/saves")).expect("creates assets/saves");
    fs::rename(project.path(SAVE), project.path(inside)).expect("moves the save under assets");
    let saves = project.path("assets/saves").to_string_lossy().into_owned();

    let run = project.upgrade(&["--saves", &saves]);

    run.assert_succeeded();
    let mut world = project.declared_world();
    SerializationService::new()
        .load_game(&mut world, &project.file(inside), &project.file(KEEP))
        .expect("the converted save loads");
    let guard = named(&world, "Sentry");
    assert_eq!(
        field_of(&world, guard, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned()))
    );
}

/// **A change propagates through nested prefabs to a save**: the sentry
/// prefab converts; the outpost holding a sentry, the level holding an
/// outpost and a save of the level are written again against it — the
/// outpost's override, the speed the game changed and the state reach the
/// game; a second run changes no byte.
#[test]
fn a_save_of_a_level_of_nested_prefabs_converts_and_a_second_run_changes_no_byte() {
    let project = Project::new("nested-chain-save");
    let sentry = write_sentry_prefab(&project);
    let mut library = Library::default();
    library.0.insert(asset_id(PREFAB), sentry);
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Outpost")));
    world.mark_authored(root).expect("authored");
    let inner = instantiate_prefab(&mut world, asset_id(PREFAB), &library)
        .expect("the sentry instantiates");
    assert!(world.set_parent(inner, Some(root)));
    let script = world.get_mut::<Script>(inner).expect("the Script");
    *script = script.clone().with_field("health", ScriptValue::Int(60));
    let bytes = serialize_prefab(&world, root, asset_id(OUTPOST), &library)
        .expect("the outpost serializes");
    project.write(OUTPOST, &bytes);
    library.0.insert(
        asset_id(OUTPOST),
        read_scene_file(&SceneFile::from_bytes(&bytes).expect("a file")).expect("reads"),
    );
    let library = std::sync::Arc::new(library);
    let service = SerializationService::with_prefabs(library.clone());
    let mut level = World::new();
    instantiate_prefab(&mut level, asset_id(OUTPOST), library.as_ref())
        .expect("the outpost instantiates");
    let base = service
        .save_world_with(
            &level,
            SerializationGoal::EditorInterchange,
            library.as_ref(),
        )
        .expect("the level saves");
    assert_eq!(read_scene_file(&base).expect("reads").instances.len(), 1);
    project.write_file(KEEP, &base);
    let sentry = named(&level, "Sentry");
    level
        .add_component(
            sentry,
            ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot {
                    fields: vec![
                        ("speed".to_owned(), ScriptValue::Float(9.0)),
                        ("health".to_owned(), ScriptValue::Int(60)),
                    ],
                    authored: vec![
                        ("speed".to_owned(), ScriptValue::Float(2.5)),
                        ("health".to_owned(), ScriptValue::Int(60)),
                    ],
                    state: Some("Attack".to_owned()),
                    lifecycle: InstanceLifecycle {
                        spawned: true,
                        ..Default::default()
                    },
                    ..ScriptSnapshot::default()
                },
            },
        )
        .expect("the state attaches");
    let save = service
        .save_game(
            &level,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);
    let saves = project.saves_arg();

    let run = project.upgrade(&["--saves", &saves]);
    run.assert_succeeded();

    let converted = std::sync::Arc::new(project.library(&[PREFAB, OUTPOST]));
    let mut loaded = project.declared_world();
    SerializationService::with_prefabs(converted)
        .load_game(&mut loaded, &project.file(SAVE), &project.file(KEEP))
        .expect("the converted save loads with the converted prefabs");
    let guard = named(&loaded, "Sentry");
    assert_eq!(
        field_of(&loaded, guard, "Guard", "speed"),
        Some(ScriptValue::Float(9.0)),
        "the speed the game changed:\n{}",
        run.text()
    );
    assert_eq!(
        field_of(&loaded, guard, "Guard", "health"),
        Some(ScriptValue::Int(60)),
        "the outpost's override"
    );
    assert_eq!(
        field_of(&loaded, guard, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned()))
    );
    assert!(loaded.get::<Script>(guard).is_none());
    assert!(loaded.get::<ScriptState>(guard).is_none());

    let once = project.bytes();
    project.upgrade(&["--saves", &saves]).assert_succeeded();
    assert_eq!(project.bytes(), once, "the second run changes no byte");
}

/// **Saves upgraded after their scenes, holding states for several entities**:
/// a guard whose state converts, keeping the scene's edit for the field the
/// game left; a guard the game destroyed stays destroyed; a guard whose state
/// is another declared behavior's runs `Guard` with no state.
#[test]
fn saves_upgraded_after_their_scenes_convert_each_entity_state_on_its_own() {
    let project = Project::new("several-after-scenes");
    project.write(&format!("assets/scripts/{SCOUT_MODULE}"), SCOUT.as_bytes());
    let service = SerializationService::new();
    let mut world = World::new();
    let spawn = |world: &mut World, name: &str| {
        let entity = world.spawn((
            Transform::identity(),
            Name::new(name),
            Script::new(GUARD_MODULE, "Guard")
                .with_field("speed", ScriptValue::Float(2.5))
                .with_field("health", ScriptValue::Int(80)),
        ));
        world.mark_authored(entity).expect("authored");
        entity
    };
    let kept = spawn(&mut world, "Kept");
    let killed = spawn(&mut world, "Killed");
    let other = spawn(&mut world, "Other");
    let base = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let state = |behavior: &str| ScriptState {
        behavior: behavior.to_owned(),
        snapshot: ScriptSnapshot {
            fields: vec![
                ("speed".to_owned(), ScriptValue::Float(7.0)),
                ("health".to_owned(), ScriptValue::Int(80)),
            ],
            authored: vec![
                ("speed".to_owned(), ScriptValue::Float(2.5)),
                ("health".to_owned(), ScriptValue::Int(80)),
            ],
            state: Some("Attack".to_owned()),
            lifecycle: InstanceLifecycle {
                spawned: true,
                ..Default::default()
            },
            ..ScriptSnapshot::default()
        },
    };
    world.add_component(kept, state("Guard")).expect("attaches");
    world
        .add_component(other, state("Scout"))
        .expect("attaches");
    assert!(world.despawn(killed));
    let save = service
        .save_game(
            &world,
            asset_id(KEEP),
            &base,
            SerializationGoal::PortableBinary,
        )
        .expect("the game saves");
    project.write_file(SAVE, &save);
    // The scene, edited after the save: the kept guard's health 80 to 90.
    let mut edited = World::new();
    service
        .load_world(&base, &mut edited)
        .expect("the scene loads");
    let kept_now = named(&edited, "Kept");
    let script = edited.get_mut::<Script>(kept_now).expect("the Script");
    *script = script.clone().with_field("health", ScriptValue::Int(90));
    project.write_file(
        KEEP,
        &service
            .save_world(&edited, SerializationGoal::EditorInterchange)
            .expect("the edited scene saves"),
    );

    project.upgrade(&[]).assert_succeeded();
    let run = project.upgrade(&["--saves", &project.saves_arg()]);
    run.assert_succeeded();

    let mut loaded = project.declared_world();
    SerializationService::new()
        .load_game(&mut loaded, &project.file(SAVE), &project.file(KEEP))
        .expect("the converted save loads");
    let kept = named(&loaded, "Kept");
    assert_eq!(
        field_of(&loaded, kept, "Guard", "speed"),
        Some(ScriptValue::Float(7.0)),
        "{}",
        run.text()
    );
    assert_eq!(
        field_of(&loaded, kept, "Guard", "health"),
        Some(ScriptValue::Int(90)),
        "the scene's edit stands for the field the game left"
    );
    assert_eq!(
        field_of(&loaded, kept, "Guard#state", "state"),
        Some(ScriptValue::Str("Attack".to_owned()))
    );
    assert!(
        !loaded.iter_entities().any(|e| loaded
            .get::<Name>(e)
            .is_some_and(|n| n.as_str() == "Killed")),
        "the destroyed guard stays destroyed"
    );
    let other = named(&loaded, "Other");
    assert!(holds(&loaded, other, "Guard"));
    assert!(!holds(&loaded, other, "Guard#state"));
    assert!(!holds(&loaded, other, "Scout"));
    assert_eq!(
        field_of(&loaded, other, "Guard", "speed"),
        Some(ScriptValue::Float(2.5)),
        "another behavior's observed values are not taken"
    );
    assert!(loaded.get::<ScriptState>(other).is_none());
    assert!(
        run.text().contains("Scout"),
        "the dropped state is named:\n{}",
        run.text()
    );
}
