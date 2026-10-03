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

//! A world through a snapshot and back: every component, every identity,
//! every reference.

use khora_core::asset::AssetUUID;
use khora_core::math::{Vec2, Vec3};
use khora_core::scene::{SceneFile, SCENE_FORMAT_VERSION};
use khora_core::script::{FrozenValue, ScriptValue, SuspendedMachine};
use khora_data::ecs::{MaterialRef, MeshRef, Name, Parent, Script, Transform};
use khora_data::scene::record::ReportKind;
use khora_data::scene::snapshot::{prepare_snapshot, write_snapshot, SNAPSHOT_ENCODING_ID};
use khora_data::scene::LoadFailure;
use khora_data::ui::UiImage;

use super::renames::{Beacon, Stamina};
use super::sample::{sample_world, suspended_snapshot};
use super::*;

/// `world` written as a snapshot, then read back as a file on disk would be.
pub fn snapshot_of(world: &World) -> SceneFile {
    let file = write_snapshot(world).unwrap_or_else(|e| panic!("the snapshot failed: {e}"));
    SceneFile::from_bytes(&file.to_bytes()).expect("a snapshot file parses")
}

/// The encoding a file's header names.
pub fn encoding_of(file: &SceneFile) -> String {
    String::from_utf8_lossy(&file.header.encoding_id)
        .trim_end_matches('\0')
        .to_owned()
}

/// Brings the snapshot `file` into `world` under its saved identities.
pub fn load_snapshot(world: &mut World, file: &SceneFile) -> Result<Applied, LoadFailure> {
    let prepared = prepare_snapshot(world, file)?;
    Ok(prepared.commit(world, Identity::Keep))
}

/// The entity a script field holds.
fn field_entity(script: &Script, field: &str) -> EntityId {
    match script.field(field) {
        Some(ScriptValue::Entity(entity)) => *entity,
        other => panic!("field `{field}` holds {other:?}, not an entity"),
    }
}

/// A snapshot holds every component a scene records — the sample's, and
/// the test's own — and brings all of them back equal, each entity under the
/// identity it had, with nothing adapted: a snapshot is read by the schema
/// that wrote it.
#[test]
fn a_snapshot_round_trips_every_registration() {
    let (mut src, sample) = sample_world();
    src.spawn((
        Beacon { range: 42.5 },
        Stamina {
            reserve: 7.5,
            regen: 0.25,
        },
    ));
    for reg in saved_registrations() {
        assert!(
            src.iter_entities()
                .any(|entity| (reg.to_json)(&src, entity).is_some()),
            "the world carries no `{}`",
            reg.type_name
        );
    }
    for entity in [sample.root, sample.child, sample.panel] {
        src.mark_authored(entity).expect("authored");
    }

    let file = snapshot_of(&src);
    assert_eq!(encoding_of(&file), SNAPSHOT_ENCODING_ID);
    assert_eq!(encoding_of(&file), "KH_SNAPSHOT_V1");
    assert_eq!(file.header.format_version, SCENE_FORMAT_VERSION);

    let mut dst = World::new();
    let applied =
        load_snapshot(&mut dst, &file).unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));
    assert!(
        applied.report.is_clean(),
        "a snapshot adapted something: {:?}",
        applied.report.entries
    );
    assert_eq!(applied.entities.len(), src.iter_entities().count());
    for (id, entity) in &applied.entities {
        assert_eq!(dst.persistent_id(*entity), Some(*id));
    }
    assert_same_world(&src, &dst, "snapshot");
}

/// A snapshot writes values by position: the names of a component's fields
/// are not in it.
#[test]
fn a_snapshot_holds_no_field_names() {
    let (src, _) = sample_world();
    let file = snapshot_of(&src);
    for field in [
        b"translation".as_slice(),
        b"flex_direction",
        b"initial_velocity",
    ] {
        assert!(
            !file
                .payload
                .windows(field.len())
                .any(|window| window == field),
            "the field name `{}` is in the snapshot",
            String::from_utf8_lossy(field)
        );
    }
}

/// Identities and references survive a snapshot: authored and created ids,
/// entity references in a script field, in a list and in a frozen register,
/// asset references, and the hierarchy with its children in their order —
/// loaded beside entities the world already has.
#[test]
fn a_snapshot_keeps_identities_and_references() {
    let mut src = World::new();
    // Ids a fresh world would not hand out in this order.
    let scratch: Vec<_> = (0..4).map(|_| src.spawn(Transform::identity())).collect();
    for entity in scratch {
        src.despawn(entity);
    }
    let root = src.spawn((Transform::identity(), Name::new("Root")));
    let third = src.spawn((Transform::identity(), Name::new("Third")));
    let first = src.spawn((Transform::identity(), Name::new("First")));
    let second = src.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Second"),
        MeshRef::Asset(AssetUUID::new_v5("meshes/rock.gltf")),
        MaterialRef::Asset(AssetUUID::new_v5("materials/rock.kmat")),
    ));
    let panel = src.spawn(UiImage {
        texture: AssetUUID::new_v5("textures/wall.png"),
    });
    for child in [first, second, third] {
        src.set_parent(child, Some(root));
    }
    let holder = src.spawn((
        Transform::identity(),
        Name::new("Holder"),
        Script {
            module: "ai/holder.erg".into(),
            behavior: "Holder".into(),
            fields: vec![
                ("target".into(), ScriptValue::Entity(second)),
                (
                    "crowd".into(),
                    ScriptValue::Array(vec![ScriptValue::Entity(third), ScriptValue::Entity(root)]),
                ),
                ("aim".into(), ScriptValue::Vec2(Vec2::new(1.0, -2.0))),
            ],
            runtime: suspended_snapshot(first),
        },
    ));
    src.set_parent(holder, Some(first));
    for entity in [root, first, holder] {
        src.mark_authored(entity).expect("authored");
    }

    let file = snapshot_of(&src);

    let mut dst = World::new();
    // An authored resident: its identity cannot be one of the created ids
    // the snapshot numbers its never-identified entities with, so every
    // loaded entity keeps the identity it was saved under.
    let resident = dst.spawn((Transform::identity(), Name::new("Resident")));
    let resident_id = dst.mark_authored(resident).expect("authored");
    load_snapshot(&mut dst, &file).unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));

    let twin = |entity: EntityId| {
        let id = src.persistent_id(entity).expect("a live entity has an id");
        dst.entity_with_id(id)
            .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
    };
    assert_eq!(dst.persistent_id(resident), Some(resident_id));
    assert_eq!(
        dst.get::<Name>(resident).map(Name::as_str),
        Some("Resident")
    );
    assert_eq!(dst.iter_entities().count(), src.iter_entities().count() + 1);

    // The hierarchy, siblings in the order the scene had them.
    let children = dst
        .get::<Children>(twin(root))
        .expect("the root has children");
    assert_eq!(
        children.0,
        vec![twin(first), twin(second), twin(third)],
        "children keep their order"
    );
    for child in [first, second, third] {
        assert_eq!(dst.get::<Parent>(twin(child)), Some(&Parent(twin(root))));
    }
    assert_eq!(dst.get::<Parent>(twin(holder)), Some(&Parent(twin(first))));

    // Entity references, wherever they sit.
    let script = dst.get::<Script>(twin(holder)).expect("the script loaded");
    assert_eq!(field_entity(script, "target"), twin(second));
    assert_eq!(
        script.field("crowd"),
        Some(&ScriptValue::Array(vec![
            ScriptValue::Entity(twin(third)),
            ScriptValue::Entity(twin(root)),
        ]))
    );
    let pending = script.runtime.pending.as_ref().expect("a pending sequence");
    let SuspendedMachine::Frozen(machine) = &pending.machine else {
        panic!("the machine is still frozen: {:?}", pending.machine);
    };
    assert!(
        machine
            .registers
            .contains(&FrozenValue::Entity(twin(first))),
        "the frozen register names the reloaded entity: {:?}",
        machine.registers
    );

    // Asset references, verbatim.
    assert_eq!(
        dst.get::<MeshRef>(twin(second)),
        Some(&MeshRef::Asset(AssetUUID::new_v5("meshes/rock.gltf")))
    );
    assert!(
        matches!(
            dst.get::<MaterialRef>(twin(second)),
            Some(MaterialRef::Asset(uuid)) if *uuid == AssetUUID::new_v5("materials/rock.kmat")
        ),
        "the material asset reference is kept"
    );
    assert_eq!(
        dst.get::<UiImage>(twin(panel)).map(|image| image.texture),
        Some(AssetUUID::new_v5("textures/wall.png"))
    );

    // Identities of both kinds, and a later spawn does not take one of them.
    for entity in [root, first, second, third, holder, panel] {
        assert_eq!(dst.persistent_id(twin(entity)), src.persistent_id(entity));
    }
    let later = dst.spawn(Transform::identity());
    let later_id = dst.persistent_id(later).expect("id");
    for entity in src.iter_entities() {
        assert_ne!(
            Some(later_id),
            src.persistent_id(entity),
            "a spawn after the load reuses a loaded identity"
        );
    }
}

/// A reference to an entity the snapshot does not hold names the world's
/// one nowhere entity — the same one a record load binds such a reference
/// to — and is reported.
#[test]
fn a_dead_reference_in_a_snapshot_names_the_nowhere_entity() {
    let mut src = World::new();
    let gone_one = src.spawn((Transform::identity(), Name::new("GoneOne")));
    let gone_two = src.spawn((Transform::identity(), Name::new("GoneTwo")));
    let holder = src.spawn((
        Transform::identity(),
        Name::new("Holder"),
        Script {
            module: "ai/holder.erg".into(),
            behavior: "Holder".into(),
            fields: vec![
                ("first".into(), ScriptValue::Entity(gone_one)),
                ("second".into(), ScriptValue::Entity(gone_two)),
            ],
            runtime: Default::default(),
        },
    ));
    let holder_id = src.persistent_id(holder).expect("id");
    src.despawn(gone_one);
    src.despawn(gone_two);

    let file = snapshot_of(&src);
    let mut dst = World::new();
    let applied =
        load_snapshot(&mut dst, &file).unwrap_or_else(|e| panic!("a dead reference loads: {e}"));
    let fields = |world: &World, entity: EntityId| {
        let script = world.get::<Script>(entity).expect("the script loaded");
        [
            field_entity(script, "first"),
            field_entity(script, "second"),
        ]
    };
    let holder_back = dst.entity_with_id(holder_id).expect("the holder loaded");
    let [first, second] = fields(&dst, holder_back);
    assert_eq!(first, second, "two missing entities, one nowhere entity");
    let nowhere = first;
    assert!(!dst.contains(nowhere), "nowhere is not alive");
    assert_eq!(dst.persistent_id(nowhere), None, "nowhere has no identity");

    let dead: Vec<_> = applied
        .report
        .entries
        .iter()
        .filter(|entry| entry.kind == ReportKind::DeadReference)
        .collect();
    assert!(!dead.is_empty(), "dead references are reported");
    for entry in dead {
        assert_eq!(entry.entity, Some(holder_id), "{entry:?}");
        assert_eq!(entry.component.as_deref(), Some("Script"), "{entry:?}");
    }

    // The record path, into the same world, binds to the same entity.
    let record = capture_world(&src).expect("captures");
    dst.despawn(holder_back);
    let applied = apply(&mut dst, &record, Identity::Keep).expect("the record loads");
    let &(_, holder_again) = applied
        .entities
        .iter()
        .find(|(id, _)| *id == holder_id)
        .expect("the holder loaded");
    assert_eq!(fields(&dst, holder_again), [nowhere; 2]);

    // And a second snapshot load reuses it too.
    dst.despawn(holder_again);
    load_snapshot(&mut dst, &file).expect("loads again");
    let holder_third = dst.entity_with_id(holder_id).expect("the holder loaded");
    assert_eq!(fields(&dst, holder_third), [nowhere; 2]);
    assert!(!dst.contains(nowhere));
}
