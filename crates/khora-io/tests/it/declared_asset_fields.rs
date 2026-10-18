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

//! A declared component holding asset references — `Prefab? bullet;`, one
//! field of each kind — through the serialization service: saved under every
//! record goal and loaded into a world that never declared it, snapshotted
//! and restored, and kept by a game save.
//!
//! A reference is saved by its identity, the `AssetUUID` (`khora.AssetUUID`),
//! never as text or as sixteen numbers, and its field's type — the kind — is
//! saved with the component's declaration.

use khora_core::asset::{AssetKind, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::{ErgonType, ScriptValue};
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform, World,
};
use khora_io::serialization::SerializationService;

/// The fields of `Armory`: one optional asset of each kind, named after it.
const FIELDS: [(&str, AssetKind); 6] = [
    ("bullet", AssetKind::Prefab),
    ("shot", AssetKind::Sound),
    ("skin", AssetKind::Material),
    ("hull", AssetKind::Mesh),
    ("decal", AssetKind::Texture),
    ("level", AssetKind::Scene),
];

fn optional(kind: AssetKind) -> ErgonType {
    ErgonType::Optional(Box::new(ErgonType::Asset(kind)))
}

/// The asset a field named `name` refers to in the armed world.
fn chosen(name: &str) -> AssetUUID {
    AssetUUID::new_v5(&format!("assets/{name}"))
}

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

/// The declared type of `component`'s field `name` in `world`.
fn type_of(world: &World, component: &str, name: &str) -> Option<ErgonType> {
    let key = world.components().key_named(component)?;
    let layout = world.components().vtable(key)?.columns.packed()?;
    layout.ty(layout.slot_of(name)?).cloned()
}

/// Sets `entity`'s `Armory` field `name` to `value`.
fn set(world: &mut World, entity: EntityId, name: &str, value: ScriptValue) {
    let key = world.components().key_named("Armory").expect("registered");
    let slot = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|layout| layout.slot_of(name))
        .expect("a declared field");
    world
        .row_mut(entity, key)
        .expect("holds Armory")
        .set_field(slot, &value)
        .unwrap_or_else(|e| panic!("`{name}` takes {value:?}: {e:?}"));
}

/// A world of one placed entity holding an `Armory` (Script), typed as a
/// script declares it, every field set to the asset it is named after when
/// `armed`, every field `null` otherwise. The world and the entity's id.
fn armory(armed: bool) -> (World, PersistentId) {
    let mut world = World::new();
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Armory".into(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                FIELDS
                    .iter()
                    .map(|(name, kind)| PackedField {
                        name: (*name).into(),
                        kind: FieldKind::of(&optional(*kind)),
                        default: ScriptValue::Null,
                    })
                    .collect(),
                FIELDS.iter().map(|(_, kind)| optional(*kind)).collect(),
            )
            .expect("a valid layout"),
        })
        .expect("registers");
    let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
    let values = FIELDS
        .iter()
        .filter(|_| armed)
        .map(|(name, _)| ((*name).to_owned(), ScriptValue::Asset(chosen(name))))
        .collect();
    world
        .add_runtime_component(entity, key, &ScriptValue::Struct(values))
        .expect("attaches");
    world.mark_authored(entity).expect("authored");
    let id = world.persistent_id(entity).expect("an id");
    (world, id)
}

/// `file` written out and read back, as one on disk.
fn on_disk(file: &SceneFile) -> SceneFile {
    SceneFile::from_bytes(&file.to_bytes()).expect("the file parses")
}

/// Asserts `world`'s entity `id` holds the armed `Armory`: each field the
/// asset it is named after.
fn assert_armed(world: &World, id: PersistentId, case: &str) {
    let entity = world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("{case}: the entity is back"));
    for (name, _) in FIELDS {
        assert_eq!(
            field_of(world, entity, "Armory", name),
            Some(ScriptValue::Asset(chosen(name))),
            "{case}: `{name}`"
        );
    }
}

/// **Every record goal saves an asset field of each kind and loads it back**
/// into a world that never declared the component: each reference is the
/// one saved, and each field's type is its kind, read from the file's
/// declaration.
#[test]
fn every_record_goal_round_trips_an_asset_field_of_each_kind() {
    let service = SerializationService::new();
    let (world, id) = armory(true);
    for goal in [
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::EditorInterchange,
        SerializationGoal::PortableBinary,
    ] {
        let file = service
            .save_world(&world, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: the save failed: {e:?}"));
        let mut fresh = World::new();
        let report = service
            .load_world(&on_disk(&file), &mut fresh)
            .unwrap_or_else(|e| panic!("{goal:?}: the load failed: {e:?}"));
        assert_armed(&fresh, id, &format!("{goal:?}"));
        for (name, kind) in FIELDS {
            assert_eq!(
                type_of(&fresh, "Armory", name),
                Some(optional(kind)),
                "{goal:?}: `{name}` is declared `{kind:?}?` by the file"
            );
            assert!(
                !report.entries.iter().any(|entry| entry.path.contains(name)),
                "{goal:?}: `{name}` read as written: {report:?}"
            );
        }
    }
}

/// **A field never chosen loads back `null`** — not a default asset, not a
/// value the load made up.
#[test]
fn an_asset_field_left_null_loads_back_null() {
    let service = SerializationService::new();
    let (world, id) = armory(false);
    let file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the save failed: {e:?}"));
    let mut fresh = World::new();
    service
        .load_world(&on_disk(&file), &mut fresh)
        .unwrap_or_else(|e| panic!("the load failed: {e:?}"));
    let entity = fresh.entity_with_id(id).expect("the entity is back");
    for (name, kind) in FIELDS {
        assert_eq!(
            field_of(&fresh, entity, "Armory", name),
            Some(ScriptValue::Null),
            "`{name}`"
        );
        assert_eq!(type_of(&fresh, "Armory", name), Some(optional(kind)));
    }
}

/// **The readable scene names each asset by its identity** — the UUID's
/// hyphenated form, as every asset reference in a scene is written — not
/// by a path, nor as sixteen loose numbers.
#[test]
fn a_readable_scene_names_each_asset_by_its_identity() {
    let service = SerializationService::new();
    let (world, _) = armory(true);
    let file = service
        .save_world(&world, SerializationGoal::HumanReadableDebug)
        .unwrap_or_else(|e| panic!("the save failed: {e:?}"));
    let text = std::str::from_utf8(&file.payload).expect("a readable scene is UTF-8");
    for (name, _) in FIELDS {
        let uuid = chosen(name).to_string();
        assert!(
            text.contains(&uuid),
            "`{name}`'s asset {uuid} is written in the scene:\n{text}"
        );
    }
}

/// **A `FastestLoad` snapshot restores every asset field** as it was on
/// Play, after the game changed one and cleared another.
#[test]
fn a_fastest_load_snapshot_restores_asset_fields() {
    let service = SerializationService::new();
    let (mut world, id) = armory(true);
    let file = service
        .save_world(&world, SerializationGoal::FastestLoad)
        .unwrap_or_else(|e| panic!("the snapshot failed: {e:?}"));
    let entity = world.entity_with_id(id).expect("alive");
    set(
        &mut world,
        entity,
        "bullet",
        ScriptValue::Asset(AssetUUID::new_v5("assets/other")),
    );
    set(&mut world, entity, "shot", ScriptValue::Null);

    service
        .replace_world(&on_disk(&file), &mut world)
        .unwrap_or_else(|e| panic!("the restore failed: {e:?}"));
    assert_armed(&world, id, "FastestLoad");
}

/// **A game save keeps an asset the game chose and one it cleared**, and
/// brings both back, with their scene, into a world that never declared
/// the component.
#[test]
fn a_game_save_keeps_asset_fields_the_game_changed() {
    let service = SerializationService::new();
    let (mut world, id) = armory(true);
    let base_file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the scene save failed: {e:?}"));
    let entity = world.entity_with_id(id).expect("alive");
    let other = AssetUUID::new_v5("assets/other");
    set(&mut world, entity, "bullet", ScriptValue::Asset(other));
    set(&mut world, entity, "shot", ScriptValue::Null);

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
    let entity = fresh.entity_with_id(id).expect("the entity is back");
    assert_eq!(
        field_of(&fresh, entity, "Armory", "bullet"),
        Some(ScriptValue::Asset(other)),
        "the asset the game chose"
    );
    assert_eq!(
        field_of(&fresh, entity, "Armory", "shot"),
        Some(ScriptValue::Null),
        "the asset the game cleared"
    );
    assert_eq!(
        field_of(&fresh, entity, "Armory", "skin"),
        Some(ScriptValue::Asset(chosen("skin"))),
        "an asset the game left as the scene had it"
    );
}

/// `Armory`'s layout as a script declares it once its `bullet` field is
/// retyped `kind?` — the edit a hot reload applies.
fn armory_with_bullet_of(kind: AssetKind) -> PackedLayout {
    let kinds: Vec<(&str, AssetKind)> = FIELDS
        .iter()
        .map(|(name, k)| (*name, if *name == "bullet" { kind } else { *k }))
        .collect();
    PackedLayout::typed(
        kinds
            .iter()
            .map(|(name, kind)| PackedField {
                name: (*name).into(),
                kind: FieldKind::of(&optional(*kind)),
                default: ScriptValue::Null,
            })
            .collect(),
        kinds.iter().map(|(_, kind)| optional(*kind)).collect(),
    )
    .expect("a valid layout")
}

/// **A snapshot restores no asset into a field retyped to another kind**:
/// a prefab taken on Play does not come back as the `Sound?` the field
/// became while playing — the field starts at `null`, as the hot reload
/// that retyped it left it, and the load says so.
#[test]
fn a_snapshot_restores_no_asset_into_a_field_retyped_to_another_kind() {
    let service = SerializationService::new();
    let (mut world, id) = armory(true);
    let file = service
        .save_world(&world, SerializationGoal::FastestLoad)
        .unwrap_or_else(|e| panic!("the snapshot failed: {e:?}"));
    let key = world.components().key_named("Armory").expect("registered");
    world
        .relayout(key, armory_with_bullet_of(AssetKind::Sound))
        .expect("relays out");
    let entity = world.entity_with_id(id).expect("alive");
    assert_eq!(
        field_of(&world, entity, "Armory", "bullet"),
        Some(ScriptValue::Null),
        "the hot reload defaults a field retyped to another kind"
    );

    let report = service
        .replace_world(&on_disk(&file), &mut world)
        .unwrap_or_else(|e| panic!("the restore failed: {e:?}"));
    let entity = world.entity_with_id(id).expect("the entity is back");
    assert_eq!(
        type_of(&world, "Armory", "bullet"),
        Some(optional(AssetKind::Sound)),
        "the live declaration stays"
    );
    assert_eq!(
        field_of(&world, entity, "Armory", "bullet"),
        Some(ScriptValue::Null),
        "a prefab is no sound: report {report:?}"
    );
}

/// **A scene loads no asset into a field the world declares of another
/// kind**: the file's `Prefab? bullet` read onto a world whose `bullet` is
/// `Sound?` leaves it `null`, and the load says so.
#[test]
fn a_scene_loads_no_asset_into_a_field_declared_of_another_kind() {
    let service = SerializationService::new();
    let (world, id) = armory(true);
    let file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the save failed: {e:?}"));
    let mut fresh = World::new();
    fresh
        .register_runtime_component(RuntimeComponentDecl {
            name: "Armory".into(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: armory_with_bullet_of(AssetKind::Sound),
        })
        .expect("registers");
    let report = service
        .load_world(&on_disk(&file), &mut fresh)
        .unwrap_or_else(|e| panic!("the load failed: {e:?}"));
    let entity = fresh.entity_with_id(id).expect("the entity is back");
    assert_eq!(
        field_of(&fresh, entity, "Armory", "bullet"),
        Some(ScriptValue::Null),
        "a prefab is no sound: report {report:?}"
    );
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.path.contains("bullet")),
        "the load reports the field it could not read: {report:?}"
    );
}

/// **A field saved `null` and retyped since reports nothing**: `null` is a
/// value of every optional, so a `Prefab? bullet` left unset loads into a
/// `Sound? bullet` losing nothing — the load has nothing to say about it.
/// Reporting it `Dropped` would flag every entity that never chose an asset.
#[test]
fn a_null_asset_field_retyped_to_another_kind_reports_nothing() {
    let service = SerializationService::new();
    let (world, id) = armory(false);
    let file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the save failed: {e:?}"));
    let mut fresh = World::new();
    fresh
        .register_runtime_component(RuntimeComponentDecl {
            name: "Armory".into(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: armory_with_bullet_of(AssetKind::Sound),
        })
        .expect("registers");
    let report = service
        .load_world(&on_disk(&file), &mut fresh)
        .unwrap_or_else(|e| panic!("the load failed: {e:?}"));
    let entity = fresh.entity_with_id(id).expect("the entity is back");
    assert_eq!(
        field_of(&fresh, entity, "Armory", "bullet"),
        Some(ScriptValue::Null)
    );
    assert!(report.entries.is_empty(), "nothing was lost: {report:?}");
}

/// Prefabs held in memory, by asset id.
#[derive(Clone, Default)]
struct Prefabs(std::collections::HashMap<AssetUUID, khora_data::scene::SceneRecord>);

impl khora_data::scene::PrefabSource for Prefabs {
    fn prefab(&self, id: AssetUUID) -> Result<khora_data::scene::SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id}"))
    }
}

/// The asset an `Armory` prefab's `bullet` held while it was a `Sound?`.
fn the_old_sound() -> AssetUUID {
    AssetUUID::new_v5("assets/old-sound")
}

/// A scene saved while `Armory.bullet` was a `Sound?` — an instance of the
/// `Armory` prefab `prefab`, as that prefab was then (`bullet` the old
/// sound, left as the prefab had it), and an entity of the scene's own
/// holding an `Armory`, so the scene's file declares the component — and
/// the prefab as it is now: re-saved since, under `Prefab? bullet`, holding
/// the prefab `chosen("bullet")`. The scene file, the prefabs as they are
/// now, and the instance root's id.
fn a_scene_older_than_its_prefab(prefab: AssetUUID) -> (SceneFile, Prefabs, PersistentId) {
    let (mut then, id) = armory(true);
    let key = then.components().key_named("Armory").expect("registered");
    then.relayout(key, armory_with_bullet_of(AssetKind::Sound))
        .expect("relays out");
    let root = then.entity_with_id(id).expect("alive");
    set(
        &mut then,
        root,
        "bullet",
        ScriptValue::Asset(the_old_sound()),
    );
    let mut prefabs_then = Prefabs::default();
    prefabs_then.0.insert(
        prefab,
        khora_data::scene::capture_subtree(&then, root).expect("the prefab captures"),
    );

    let mut scene = World::new();
    let instance = khora_data::scene::instantiate_prefab(&mut scene, prefab, &prefabs_then)
        .unwrap_or_else(|e| panic!("the prefab instantiates: {e:?}"));
    let instance = scene.persistent_id(instance).expect("an id");
    let key = scene.components().key_named("Armory").expect("registered");
    let own = scene.spawn(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
    scene
        .add_runtime_component(own, key, &ScriptValue::Struct(Vec::new()))
        .expect("attaches");
    scene.mark_authored(own).expect("authored");
    let file = SerializationService::with_prefabs(std::sync::Arc::new(prefabs_then))
        .save_world(&scene, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the scene saves: {e:?}"));

    let (now, id) = armory(true);
    let mut prefabs_now = Prefabs::default();
    prefabs_now.0.insert(
        prefab,
        khora_data::scene::capture_subtree(&now, now.entity_with_id(id).expect("alive"))
            .expect("the prefab captures"),
    );
    (on_disk(&file), prefabs_now, instance)
}

/// **An instance keeps the asset its prefab holds** when its scene was saved
/// while the field had another kind: the prefab, re-saved since under
/// `Prefab? bullet`, holds a prefab, and the world declares `Prefab? bullet`
/// as the script now does — the scene's stale `Sound?` declaration is no
/// reason to drop what the prefab's own declaration says it is.
#[test]
fn an_instance_keeps_the_asset_its_prefab_holds_under_a_scene_declaring_another_kind() {
    let prefab = AssetUUID::new_v5("prefabs/armory");
    let (file, prefabs, instance) = a_scene_older_than_its_prefab(prefab);
    let (mut world, _) = armory(false);
    let report = SerializationService::with_prefabs(std::sync::Arc::new(prefabs))
        .load_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the scene loads: {e:?}"));
    let root = world
        .entity_with_id(instance)
        .expect("the instance is back");
    assert_eq!(
        field_of(&world, root, "Armory", "bullet"),
        Some(ScriptValue::Asset(chosen("bullet"))),
        "the prefab's `Prefab? bullet` into the world's `Prefab? bullet`: report {report:?}"
    );
}

/// **An instance puts no asset its prefab holds into a field its scene
/// declares of another kind**: loaded into a world that never declared
/// `Armory`, the scene's `Sound? bullet` is registered — and the prefab its
/// prefab now holds must not land in it, as a scene's own `Prefab?` value
/// does not.
#[test]
fn an_instance_puts_no_prefab_into_a_field_its_scene_declares_a_sound() {
    let prefab = AssetUUID::new_v5("prefabs/armory");
    let (file, prefabs, instance) = a_scene_older_than_its_prefab(prefab);
    let mut world = World::new();
    let report = SerializationService::with_prefabs(std::sync::Arc::new(prefabs))
        .load_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the scene loads: {e:?}"));
    let root = world
        .entity_with_id(instance)
        .expect("the instance is back");
    let ty = type_of(&world, "Armory", "bullet");
    let value = field_of(&world, root, "Armory", "bullet");
    assert!(
        !(ty == Some(optional(AssetKind::Sound))
            && value == Some(ScriptValue::Asset(chosen("bullet")))),
        "a prefab in a `Sound?` field: {ty:?} = {value:?}, report {report:?}"
    );
}
