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

//! Breaker round 3: the loaded hierarchy, lazy identities and the shared
//! nowhere entity.

use khora_core::script::ScriptValue;
use khora_data::ecs::{EcsMaintenance, Name, Parent, Script, Transform};
use khora_data::scene::prepare;

use super::*;

fn pages_to_compact(world: &mut World) -> usize {
    let mut maintenance = EcsMaintenance::with_budget(usize::MAX);
    maintenance.tick(world);
    maintenance.last_compacted_count()
}

fn script_naming(target: EntityId) -> Script {
    Script {
        module: "ai/holder.erg".into(),
        behavior: "Holder".into(),
        fields: vec![("target".into(), ScriptValue::Entity(target))],
    }
}

/// Sibling order is what a UI layout lays children out by (`get_children`
/// reads `Children` as is). A save and a load must not reorder siblings.
#[test]
fn sibling_order_survives_a_world_round_trip() {
    let mut src = World::new();
    let parent = src.spawn((Transform::identity(), Name::new("Parent")));
    let first_spawned = src.spawn((Transform::identity(), Name::new("A")));
    let second_spawned = src.spawn((Transform::identity(), Name::new("B")));
    // Parented in the reverse of spawn order: B is the first child.
    assert!(src.set_parent(second_spawned, Some(parent)));
    assert!(src.set_parent(first_spawned, Some(parent)));
    assert_eq!(
        src.get::<Children>(parent).map(|c| c.0.clone()),
        Some(vec![second_spawned, first_spawned])
    );

    for (name, encoding) in every_encoding() {
        let (dst, _) = reload(&src, name, encoding);
        let twin = |e: EntityId| {
            dst.entity_with_id(src.persistent_id(e).expect("id"))
                .expect("loaded")
        };
        assert_eq!(
            dst.get::<Children>(twin(parent)).map(|c| c.0.clone()),
            Some(vec![twin(second_spawned), twin(first_spawned)]),
            "{name}: the siblings were reordered by the round trip"
        );
    }
}

/// The nowhere entity is "never alive". Its id reaches game code through every
/// dead reference; the public `spawn_reserved` must not bring it to life.
#[test]
fn spawn_reserved_cannot_bring_nowhere_to_life() {
    let mut src = World::new();
    let gone = src.spawn(Transform::identity());
    let holder = src.spawn((Transform::identity(), script_naming(gone)));
    let gone_id = src.persistent_id(gone).expect("id");
    src.persistent_id(holder).expect("id");
    let mut record = capture_world(&src).expect("captures");
    record.entities.retain(|id| *id != gone_id);
    for page in &mut record.pages {
        let keep: Vec<bool> = page.rows.iter().map(|id| *id != gone_id).collect();
        let mut flags = keep.iter();
        page.rows.retain(|_| *flags.next().unwrap());
        for column in &mut page.columns {
            let mut flags = keep.iter();
            column.retain(|_| *flags.next().unwrap());
        }
    }
    record.pages.retain(|page| !page.rows.is_empty());

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("loads");
    let holder_back = applied.entities[0].1;
    let nowhere = match dst
        .get::<Script>(holder_back)
        .and_then(|s| s.field("target"))
    {
        Some(ScriptValue::Entity(entity)) => *entity,
        other => panic!("target holds {other:?}"),
    };
    assert!(!dst.contains(nowhere));

    assert!(
        !dst.spawn_reserved(nowhere),
        "spawn_reserved brought the nowhere entity to life"
    );
    assert!(!dst.contains(nowhere), "nowhere is alive");
}

/// Coverage: a loaded parent whose saved components are all outside the hierarchy's
/// domain gains `Children` as a first Spatial component — no migration, so no
/// orphan row.
#[test]
fn a_parent_with_no_spatial_page_leaves_no_orphan_rows() {
    let mut src = World::new();
    let other = src.spawn(Transform::identity());
    let parent = src.spawn(script_naming(other));
    let child = src.spawn((Transform::identity(), Name::new("Child")));
    assert!(src.set_parent(child, Some(parent)));

    for (name, encoding) in every_encoding() {
        let (mut dst, _) = reload(&src, name, encoding);
        let parent_back = dst
            .entity_with_id(src.persistent_id(parent).unwrap())
            .unwrap();
        assert_eq!(
            dst.get::<Children>(parent_back).map(|c| c.0.len()),
            Some(1),
            "{name}"
        );
        assert_eq!(pages_to_compact(&mut dst), 0, "{name}: orphan rows");
    }
}

/// Coverage: a loaded parent that holds no saved component at all is brought to life
/// empty, then given its `Children`. A load leaves no orphan row behind.
#[test]
fn a_parent_with_no_saved_component_leaves_no_orphan_rows() {
    let mut src = World::new();
    let parent = src.spawn(khora_data::ecs::GlobalTransform::identity());
    let child = src.spawn((Transform::identity(), Name::new("Child")));
    assert!(src.set_parent(child, Some(parent)));

    for (name, encoding) in every_encoding() {
        let (mut dst, _) = reload(&src, name, encoding);
        let parent_back = dst
            .entity_with_id(src.persistent_id(parent).unwrap())
            .unwrap();
        assert_eq!(
            dst.get::<Children>(parent_back).map(|c| c.0.len()),
            Some(1),
            "{name}"
        );
        assert_eq!(pages_to_compact(&mut dst), 0, "{name}: orphan rows");
    }
}

/// Coverage: a child whose only saved component is a `Parent` naming an entity outside
/// the load: the `Parent` is dropped, the child still loads, and no orphan row
/// is left behind.
#[test]
fn a_child_whose_only_component_is_a_dropped_parent_loads_cleanly() {
    let mut src = World::new();
    let outside = src.spawn(Transform::identity());
    let child = src.spawn(Transform::identity());
    assert!(src.set_parent(child, Some(outside)));
    src.remove_component::<Transform>(child).expect("removes");
    let outside_id = src.persistent_id(outside).unwrap();
    let child_id = src.persistent_id(child).unwrap();
    let mut record = capture_world(&src).expect("captures");
    record.entities.retain(|id| *id != outside_id);
    record
        .pages
        .retain(|page| page.rows.iter().all(|id| *id != outside_id));
    assert!(
        record
            .pages
            .iter()
            .any(|p| p.components == vec!["Parent".to_owned()]),
        "{:?}",
        record.pages
    );

    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("loads");
    let back = dst.entity_with_id(child_id).expect("loaded");
    assert!(dst.contains(back));
    assert!(dst.get::<Parent>(back).is_none());
    assert_eq!(pages_to_compact(&mut dst), 0, "orphan rows");
}

/// Coverage: prepare is `&mut`, but the `Prepared` borrows nothing: a resident can be
/// asked for its identity between prepare and commit. The identity it gets
/// stays its own, and the loaded entity takes another.
#[test]
fn a_resident_identified_between_prepare_and_commit_keeps_its_id() {
    let mut src = World::new();
    let saved = src.spawn((Transform::identity(), Name::new("Saved")));
    let saved_id = src.persistent_id(saved).unwrap();
    assert_eq!(saved_id, PersistentId::created(0));
    let record = capture_world(&src).unwrap();

    let mut dst = World::new();
    let resident = dst.spawn((Transform::identity(), Name::new("Resident")));
    let prepared = prepare(&mut dst, &record).expect("prepares");
    let resident_id = dst.persistent_id(resident).unwrap();
    assert_eq!(resident_id, PersistentId::created(0));
    let applied = prepared.commit(&mut dst, Identity::Keep);
    assert_eq!(dst.persistent_id(resident), Some(resident_id));
    assert_eq!(dst.entity_with_id(resident_id), Some(resident));
    let loaded = applied.entities[0].1;
    let loaded_id = dst.persistent_id(loaded).unwrap();
    assert_ne!(loaded_id, resident_id);
    assert!(loaded_id.is_created());
}
