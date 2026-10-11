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

//! Components declared at run time through the serialization service: a
//! world holding one saved under every goal and loaded back — into a world
//! that never declared it for the record encodings, into the same world for
//! the snapshot, as the editor's Stop does — and through a game save.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform, World,
};
use khora_io::serialization::SerializationService;

/// The value of `entity`'s `component` field `name`.
fn field_of(world: &World, entity: EntityId, component: &str, name: &str) -> Option<ScriptValue> {
    let key = world.components().key_named(component)?;
    let slot = world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(name)?;
    world.row(entity, key)?.field(slot)
}

/// A world of two placed entities, the first holding a `Health` (Physics)
/// that targets the second: the world and both identities.
fn guarded() -> (World, PersistentId, PersistentId) {
    let mut world = World::new();
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Health".into(),
            domain: SemanticDomain::Physics,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(vec![
                PackedField {
                    name: "current".into(),
                    kind: FieldKind::Int,
                    default: ScriptValue::Int(100),
                },
                PackedField {
                    name: "label".into(),
                    kind: FieldKind::Value,
                    default: ScriptValue::Str(String::new()),
                },
                PackedField {
                    name: "ally".into(),
                    kind: FieldKind::Value,
                    default: ScriptValue::Null,
                },
            ])
            .expect("a valid layout"),
        })
        .expect("registers");
    let guard = world.spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
    let post = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            guard,
            key,
            &ScriptValue::Struct(vec![
                ("current".into(), ScriptValue::Int(40)),
                ("label".into(), ScriptValue::Str("guard".into())),
                ("ally".into(), ScriptValue::Entity(post)),
            ]),
        )
        .expect("attaches");
    for entity in [guard, post] {
        world.mark_authored(entity).expect("authored");
    }
    let id = |entity| world.persistent_id(entity).expect("an id");
    let (guard, post) = (id(guard), id(post));
    (world, guard, post)
}

/// `file` written out and read back, as one on disk.
fn on_disk(file: &SceneFile) -> SceneFile {
    SceneFile::from_bytes(&file.to_bytes()).expect("the file parses")
}

/// Asserts `world`'s `guard` holds the `Health` `guarded()` gave it.
fn assert_guarded(world: &World, guard: PersistentId, post: PersistentId, case: &str) {
    let guard = world
        .entity_with_id(guard)
        .unwrap_or_else(|| panic!("{case}: the guard is back"));
    let post = world
        .entity_with_id(post)
        .unwrap_or_else(|| panic!("{case}: the post is back"));
    assert_eq!(
        field_of(world, guard, "Health", "current"),
        Some(ScriptValue::Int(40)),
        "{case}"
    );
    assert_eq!(
        field_of(world, guard, "Health", "label"),
        Some(ScriptValue::Str("guard".into())),
        "{case}"
    );
    assert_eq!(
        field_of(world, guard, "Health", "ally"),
        Some(ScriptValue::Entity(post)),
        "{case}"
    );
}

/// Every record goal saves a world holding a declared component and loads
/// it back into a world that never declared it.
#[test]
fn every_record_goal_round_trips_a_declared_component() {
    let service = SerializationService::new();
    let (world, guard, post) = guarded();
    for goal in [
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::EditorInterchange,
        SerializationGoal::PortableBinary,
    ] {
        let file = service
            .save_world(&world, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: the save failed: {e:?}"));
        let mut fresh = World::new();
        service
            .load_world(&on_disk(&file), &mut fresh)
            .unwrap_or_else(|e| panic!("{goal:?}: the load failed: {e:?}"));
        assert_guarded(&fresh, guard, post, &format!("{goal:?}"));
    }
}

/// `FastestLoad` on Play, `replace_world` on Stop: the declared component
/// is back as it was on Play.
#[test]
fn a_fastest_load_snapshot_restores_a_declared_component() {
    let service = SerializationService::new();
    let (mut world, guard, post) = guarded();
    let file = service
        .save_world(&world, SerializationGoal::FastestLoad)
        .expect("the snapshot saves");
    let key = world.components().key_named("Health").expect("registered");
    let entity = world.entity_with_id(guard).expect("alive");
    world
        .row_mut(entity, key)
        .expect("holds Health")
        .set_field(0, &ScriptValue::Int(1))
        .expect("writes");

    service
        .replace_world(&on_disk(&file), &mut world)
        .unwrap_or_else(|e| panic!("the restore failed: {e:?}"));
    assert_guarded(&world, guard, post, "FastestLoad");
}

/// A game save through the service keeps a declared field the game changed
/// and brings it back, with its scene, into a world that never declared it.
#[test]
fn a_game_save_through_the_service_keeps_a_declared_component() {
    let service = SerializationService::new();
    let (mut world, guard, _) = guarded();
    let base_file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let key = world.components().key_named("Health").expect("registered");
    let entity = world.entity_with_id(guard).expect("alive");
    world
        .row_mut(entity, key)
        .expect("holds Health")
        .set_field(0, &ScriptValue::Int(3))
        .expect("writes");

    let save_file = service
        .save_game(
            &world,
            AssetUUID::new(),
            &base_file,
            SerializationGoal::EditorInterchange,
        )
        .unwrap_or_else(|e| panic!("the game save failed: {e:?}"));
    let mut fresh = World::new();
    service
        .load_game(&mut fresh, &on_disk(&save_file), &on_disk(&base_file))
        .unwrap_or_else(|e| panic!("the game load failed: {e:?}"));
    let entity = fresh.entity_with_id(guard).expect("the guard is back");
    assert_eq!(
        field_of(&fresh, entity, "Health", "current"),
        Some(ScriptValue::Int(3))
    );
    assert_eq!(
        field_of(&fresh, entity, "Health", "label"),
        Some(ScriptValue::Str("guard".into()))
    );
}
