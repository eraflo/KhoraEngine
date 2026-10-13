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

//! Game saves through the serialization service: written against a base
//! scene, read back with it, refused whole when they cannot be.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal, SAVE_MAGIC_BYTES};
use khora_core::script::ScriptValue;
use khora_data::ecs::{ComponentKey, Name, Transform, World};
use khora_io::script::declarations::{apply_declarations, DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_io::serialization::SerializationService;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const GUARD_MODULE: &str = "ai/guard.erg";

/// Registers `Guard` and its state companion `Guard#state` in `world`, as
/// the engine does once the guard's module compiles.
fn declare_guard(world: &mut World) {
    let loader = PreludeLoader::new(
        MemoryLoader::new().with(GUARD_MODULE, "behavior Guard { int health = 100; }"),
    );
    let result = compile_module(&loader, GUARD_MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "the guard should compile: {errors:?}");
    let program = result.program.expect("a program with no errors");
    let report = apply_declarations(
        world,
        &mut DeclaredComponents::default(),
        &[ScriptDeclarations {
            module: GUARD_MODULE.to_owned(),
            components: program.declared,
        }],
    );
    assert_eq!(
        report.registered,
        vec!["Guard".to_owned(), "Guard#state".to_owned()]
    );
}

/// The slot of `component`'s field `name`, with the component's key.
fn slot_of(world: &World, component: &str, name: &str) -> (ComponentKey, usize) {
    let key = world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    let slot = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|packed| packed.slot_of(name))
        .unwrap_or_else(|| panic!("`{component}` has a field `{name}`"));
    (key, slot)
}

/// The value of `entity`'s `component` field `name`.
fn field_of(world: &World, entity: EntityId, component: &str, name: &str) -> Option<ScriptValue> {
    let (key, slot) = slot_of(world, component, name);
    world.row(entity, key)?.field(slot)
}

/// Adds `component` to `entity`, its named fields set, the others defaulted.
fn add(world: &mut World, entity: EntityId, component: &str, fields: &[(&str, ScriptValue)]) {
    let key = world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    let value = ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    );
    world
        .add_runtime_component(entity, key, &value)
        .unwrap_or_else(|e| panic!("`{component}` attaches: {e:?}"));
}

/// Writes `entity`'s `component` field `name` in place.
fn set(world: &mut World, entity: EntityId, component: &str, name: &str, value: ScriptValue) {
    let (key, slot) = slot_of(world, component, name);
    world
        .row_mut(entity, key)
        .unwrap_or_else(|| panic!("the entity holds `{component}`"))
        .set_field(slot, &value)
        .unwrap_or_else(|e| panic!("`{component}.{name}` takes {value:?}: {e:?}"));
}

/// A scene, played: the world as played, the base scene's file, the base's
/// identity, and the identities of the entities the test follows.
struct Played {
    world: World,
    base_file: SceneFile,
    base_id: AssetUUID,
    guard: PersistentId,
    bystander: PersistentId,
    doomed: PersistentId,
    spawned: PersistentId,
}

fn played() -> Played {
    let service = SerializationService::new();
    let mut world = World::new();
    declare_guard(&mut world);
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Name::new("Guard"),
    ));
    add(
        &mut world,
        guard,
        "Guard",
        &[("health", ScriptValue::Int(100))],
    );
    let bystander = world.spawn((Transform::identity(), Name::new("Bystander")));
    let doomed = world.spawn((Transform::identity(), Name::new("Doomed")));
    for entity in [guard, bystander, doomed] {
        world.mark_authored(entity).expect("authored");
    }
    let base_file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let id = |world: &World, entity| world.persistent_id(entity).expect("an id");
    let (guard_id, bystander_id, doomed_id) =
        (id(&world, guard), id(&world, bystander), id(&world, doomed));

    // The run.
    world
        .get_mut::<Transform>(guard)
        .expect("placed")
        .translation = Vec3::new(1.0, 0.0, 7.0);
    set(&mut world, guard, "Guard", "health", ScriptValue::Int(40));
    add(
        &mut world,
        guard,
        "Guard#state",
        &[("state", ScriptValue::Str("Chase".into()))],
    );
    world.despawn(doomed);
    let spawned = world.spawn((
        Transform::from_translation(Vec3::new(3.0, 3.0, 3.0)),
        Name::new("Spawned"),
    ));
    let spawned_id = id(&world, spawned);

    Played {
        world,
        base_file,
        base_id: AssetUUID::new_v5("scenes/level.kscene"),
        guard: guard_id,
        bystander: bystander_id,
        doomed: doomed_id,
        spawned: spawned_id,
    }
}

fn twin(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// **The round trip a player relies on.** A game saved in any goal's encoding
/// names its base, survives its own bytes, and loads back — with the base —
/// as the world that was saved: the moves, the behavior's changed field and
/// its state companion, the spawn, and the destruction.
#[test]
fn a_game_round_trips_through_save_game_and_load_game() {
    let service = SerializationService::new();
    let played = played();

    for goal in [
        SerializationGoal::EditorInterchange,
        SerializationGoal::SmallestFileSize,
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::LongTermStability,
        SerializationGoal::PortableBinary,
        SerializationGoal::FastestLoad,
    ] {
        let save = service
            .save_game(&played.world, played.base_id, &played.base_file, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: the game does not save: {e}"));
        assert_eq!(save.header.magic_bytes, SAVE_MAGIC_BYTES, "{goal:?}");
        let save = SceneFile::from_bytes(&save.to_bytes())
            .unwrap_or_else(|e| panic!("{goal:?}: the save file does not parse: {e:?}"));
        assert_eq!(
            service.save_base(&save).expect("the save names its base"),
            played.base_id,
            "{goal:?}"
        );

        let mut loaded = World::new();
        let report = service
            .load_game(&mut loaded, &save, &played.base_file)
            .unwrap_or_else(|e| panic!("{goal:?}: the game does not load: {e}"));
        assert!(report.is_clean(), "{goal:?}: {report:?}");

        let guard = twin(&loaded, played.guard);
        assert_eq!(
            loaded.get::<Transform>(guard).map(|t| t.translation),
            Some(Vec3::new(1.0, 0.0, 7.0)),
            "{goal:?}: the move"
        );
        assert_eq!(
            field_of(&loaded, guard, "Guard", "health"),
            Some(ScriptValue::Int(40)),
            "{goal:?}: the field the game changed"
        );
        assert_eq!(
            field_of(&loaded, guard, "Guard#state", "state"),
            Some(ScriptValue::Str("Chase".into())),
            "{goal:?}: the state companion"
        );
        assert_eq!(
            loaded
                .get::<Name>(twin(&loaded, played.bystander))
                .map(Name::as_str),
            Some("Bystander"),
            "{goal:?}: the untouched entity, from the base"
        );
        assert!(
            loaded.entity_with_id(played.doomed).is_none(),
            "{goal:?}: the destroyed entity stays destroyed"
        );
        assert_eq!(
            loaded
                .get::<Name>(twin(&loaded, played.spawned))
                .map(Name::as_str),
            Some("Spawned"),
            "{goal:?}: the spawn"
        );
        assert_eq!(loaded.iter_entities().count(), 3, "{goal:?}");
    }
}

/// **`FastestLoad` has no save form of its own.** A snapshot refuses runtime
/// components by design, so a save asked for fastest load is written as a
/// compact record.
#[test]
fn a_fastest_load_save_falls_back_to_compact() {
    let service = SerializationService::new();
    let played = played();

    let save = service
        .save_game(
            &played.world,
            played.base_id,
            &played.base_file,
            SerializationGoal::FastestLoad,
        )
        .expect("the game saves");

    let id = std::str::from_utf8(&save.header.encoding_id)
        .expect("UTF-8")
        .trim_end_matches('\0');
    assert_eq!(id, "KH_COMPACT_V2");
}

/// **All of it or none of it.** A damaged save, a scene handed over as a
/// save, a save handed over as its own base: each is refused and the world it
/// was loaded into is left exactly as it was.
#[test]
fn load_game_is_atomic() {
    let service = SerializationService::new();
    let played = played();
    let save = service
        .save_game(
            &played.world,
            played.base_id,
            &played.base_file,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut damaged = save.clone();
    let half = damaged.payload.len() / 2;
    damaged.payload.truncate(half);
    damaged.header.payload_length = half as u64;

    for (what, save_file, base_file) in [
        ("a damaged save", &damaged, &played.base_file),
        ("a scene as the save", &played.base_file, &played.base_file),
        ("a save as the base", &save, &save),
    ] {
        let mut world = World::new();
        let resident = world.spawn((Transform::identity(), Name::new("Resident")));
        let resident_id = world.mark_authored(resident).expect("authored");

        assert!(
            service.load_game(&mut world, save_file, base_file).is_err(),
            "{what}: the load was not refused"
        );
        assert_eq!(world.iter_entities().count(), 1, "{what}");
        assert_eq!(world.persistent_id(resident), Some(resident_id), "{what}");
        assert_eq!(
            world.get::<Name>(resident).map(Name::as_str),
            Some("Resident"),
            "{what}"
        );
    }
}

/// A scene is not a save: asking it for the base it was taken against is an
/// error, not an answer.
#[test]
fn a_scene_has_no_save_base() {
    let service = SerializationService::new();
    let played = played();

    assert!(service.save_base(&played.base_file).is_err());
}

/// **What reading the save adapts is in the load's report.** A hand edit
/// wrote text into the save's `Guard.health`, an `int`: composing the save
/// drops it, and `load_game` says so — the guard, `Guard`, `health` — as it
/// would for any other adaptation of the load.
#[test]
fn load_game_reports_a_value_the_save_could_not_hold() {
    use khora_data::scene::record::{Record, ReportKind};
    use khora_data::scene::{read_save_file, write_save_file, CompactEncoding};

    let service = SerializationService::new();
    let played = played();
    let save_file = service
        .save_game(
            &played.world,
            played.base_id,
            &played.base_file,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    /// Field `name` of a component's cell.
    fn field_in<'r>(cell: &'r mut Record, name: &str) -> Option<&'r mut Record> {
        match cell {
            Record::Struct { fields, .. } => fields
                .iter_mut()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value),
            Record::Map(entries) => entries
                .iter_mut()
                .find(|(field, _)| *field == Record::Str(name.to_owned()))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    // The hand edit: the guard's `Guard.health` given the text its
    // `Guard#state.state` holds ("Chase") — a well-formed value, of another
    // type than the field declares.
    let mut save = read_save_file(&save_file).expect("the save reads");
    let mut edited = 0;
    for page in &mut save.changes.pages {
        let column_of = |component: &str| page.components.iter().position(|n| n == component);
        let (Some(guard), Some(state)) = (column_of("Guard"), column_of("Guard#state")) else {
            continue;
        };
        let Some(row) = page.rows.iter().position(|id| *id == played.guard) else {
            continue;
        };
        let text = field_in(&mut page.columns[state][row], "state")
            .expect("the state companion holds `state`")
            .clone();
        let health = field_in(&mut page.columns[guard][row], "health")
            .expect("the guard's `Guard` holds `health`");
        *health = text;
        edited += 1;
    }
    assert_eq!(
        edited, 1,
        "the save holds the guard's changed `Guard` beside its state companion"
    );
    let save_file = write_save_file(&save, &CompactEncoding).expect("the edited save writes");

    let mut loaded = World::new();
    let report = service
        .load_game(&mut loaded, &save_file, &played.base_file)
        .unwrap_or_else(|e| panic!("the edited save does not load: {e}"));
    let dropped: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| {
            entry.entity == Some(played.guard)
                && entry.component.as_deref() == Some("Guard")
                && entry.path == "health"
                && entry.kind == ReportKind::Dropped
        })
        .collect();
    assert_eq!(
        dropped.len(),
        1,
        "the dropped `Guard.health` is reported exactly once: {report:?}"
    );
    assert!(
        field_of(&loaded, twin(&loaded, played.guard), "Guard", "health").is_some(),
        "the guard keeps its `Guard`"
    );
}
