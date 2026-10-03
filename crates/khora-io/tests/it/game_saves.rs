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
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal, SAVE_MAGIC_BYTES};
use khora_core::script::{ScriptSnapshot, ScriptValue};
use khora_data::ecs::{Name, Script, ScriptState, Transform, World};
use khora_io::serialization::SerializationService;

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
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Name::new("Guard"),
        Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100)),
    ));
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
    world
        .add_component(
            guard,
            ScriptState {
                behavior: "Guard".into(),
                snapshot: ScriptSnapshot {
                    state: Some("Chase".into()),
                    ..ScriptSnapshot::default()
                }
                .with_field("health", ScriptValue::Int(40)),
            },
        )
        .expect("observed");
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

fn twin(world: &World, id: PersistentId) -> khora_core::ecs::entity::EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// **The round trip a player relies on.** A game saved in any goal's encoding
/// names its base, survives its own bytes, and loads back — with the base —
/// as the world that was saved: the moves, the observed state, the spawn, and
/// the destruction.
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
            loaded.get::<Script>(guard),
            Some(&Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100))),
            "{goal:?}: the authored script"
        );
        let state = loaded
            .get::<ScriptState>(guard)
            .expect("the observed state");
        assert_eq!(state.snapshot.state.as_deref(), Some("Chase"), "{goal:?}");
        assert_eq!(
            state.snapshot.field("health"),
            Some(&ScriptValue::Int(40)),
            "{goal:?}"
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
