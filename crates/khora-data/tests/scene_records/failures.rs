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

//! Loads that cannot go through, and what they must leave behind.

use khora_core::math::Vec3;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::record::{Record, ReportKind};
use khora_data::scene::{is_retired, RetiredComponent};

use super::*;

inventory::submit! {
    RetiredComponent { name: "Compass" }
}

/// Everything observable about a world: each entity's identity and saved
/// components, in iteration order.
pub(super) fn fingerprint(
    world: &World,
) -> Vec<(
    EntityId,
    Option<PersistentId>,
    BTreeMap<&'static str, serde_json::Value>,
)> {
    world
        .iter_entities()
        .map(|e| (e, world.persistent_id(e), components_of(world, e)))
        .collect()
}

/// A world already holding a small hierarchy, authored and spawned.
pub(super) fn occupied_world() -> World {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Existing root")));
    let child = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("Existing child"),
    ));
    world.set_parent(child, Some(root));
    world.mark_authored(root).expect("authored");
    world.spawn(Transform::identity());
    world
}

/// A record of four entities, captured from a world of its own.
fn four_entities() -> SceneRecord {
    let mut src = World::new();
    for i in 0..4 {
        let entity = src.spawn((
            Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)),
            Name::new(format!("Incoming {i}")),
        ));
        src.mark_authored(entity).expect("authored");
    }
    capture_world(&src).expect("captures")
}

/// A load is all or nothing. When the third entity of a record sits in a page
/// holding a component nobody knows, or holds a value that cannot be read,
/// nothing of the record reaches the world: not the entities before it, not a
/// component, not an identity — and the world's own entities are untouched.
#[test]
fn a_failed_load_leaves_the_world_untouched() {
    let mut unknown = four_entities();
    let third = unknown.entities[2];
    let page = isolate(&mut unknown, third, "Transform");
    add_column(&mut unknown, page, "NoSuchComponent", Record::Unit);

    let mut damaged = four_entities();
    let third = damaged.entities[2];
    *value_mut(&mut damaged, third, "Transform") = Record::Str("not a transform".into());

    for (case, record) in [("unknown component", unknown), ("damaged value", damaged)] {
        let incoming = record.entities.clone();
        let mut world = occupied_world();
        let before = fingerprint(&world);

        let failure = apply(&mut world, &record, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{case}: the load must fail"));
        assert!(!failure.message.is_empty(), "{case}: the failure says why");

        assert_eq!(fingerprint(&world), before, "{case}: the world changed");
        for id in incoming {
            assert_eq!(
                world.entity_with_id(id),
                None,
                "{case}: {id:?} from the failed record is in the world"
            );
        }
    }
}

/// A component type the code does not know is not something to guess at: the
/// load stops, and says which type and which entity holds it.
#[test]
fn an_unknown_component_is_an_error() {
    let mut record = four_entities();
    let culprit = record.entities[1];
    let page = isolate(&mut record, culprit, "Transform");
    add_column(&mut record, page, "NoSuchComponent", Record::Unit);

    let failure = apply(&mut World::new(), &record, Identity::Keep)
        .expect_err("an unknown component is an error");
    assert!(
        failure.message.contains("NoSuchComponent"),
        "the error names the type: {}",
        failure.message
    );
    let bits = culprit.to_bits();
    assert!(
        [
            bits.to_string(),
            format!("{bits:x}"),
            format!("{bits:#x}"),
            format!("{culprit:?}"),
        ]
        .iter()
        .any(|name| failure.message.contains(name.as_str())),
        "the error names the entity {culprit:?}: {}",
        failure.message
    );
}

/// A type removed on purpose is declared retired; a save still holding it
/// loads everything else, skips it, and reports that it did.
#[test]
fn a_retired_component_is_skipped_and_reported() {
    assert!(is_retired("Compass"));
    assert!(!is_retired("Transform"));
    assert!(!is_retired("NoSuchComponent"));

    let mut record = four_entities();
    let holder = record.entities[0];
    let page = isolate(&mut record, holder, "Transform");
    add_column(
        &mut record,
        page,
        "Compass",
        Record::Struct {
            name: "SerializableCompass".into(),
            fields: vec![("heading".into(), Record::F32(90.0))],
        },
    );

    let mut world = World::new();
    let applied = apply(&mut world, &record, Identity::Keep).expect("a retired type is no error");
    assert_eq!(applied.entities.len(), 4, "the rest loads");
    let entity = world.entity_with_id(holder).expect("the holder loaded");
    assert_eq!(
        world.get::<Name>(entity).map(|n| n.as_str().to_owned()),
        Some("Incoming 0".to_owned())
    );
    assert!(world.get::<Transform>(entity).is_some());

    let retired: Vec<_> = applied
        .report
        .entries
        .iter()
        .filter(|entry| entry.kind == ReportKind::Retired)
        .collect();
    assert_eq!(retired.len(), 1, "{:?}", applied.report);
    assert_eq!(retired[0].entity, Some(holder));
    assert_eq!(retired[0].component.as_deref(), Some("Compass"));
}
