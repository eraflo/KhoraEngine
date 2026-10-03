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

//! `World::entity_count` counts the live entities — not the slots ever
//! allocated, which a dead entity or an id held back for a load would inflate.
//!
//! The DCC reads it every frame as its workload size, so a count that only
//! ever grows would drift over a session. Each test leaves at least one dead
//! or reserved slot behind, so a slot count cannot pass for a live count.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::{Position, RenderTag, Velocity};
use crate::ecs::{Parent, SemanticDomain, Transform, World};
use crate::scene::record::{EntityRef, Record, ReportKind, VariantPayload};
use crate::scene::{apply, capture_world, prepare, Identity, SceneRecord};

/// What `entity_count` must always agree with: a walk over the live entities.
fn live(world: &World) -> usize {
    world.iter_entities().count()
}

fn world_with_components() -> World {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);
    world
}

fn spawn_n(world: &mut World, n: i32) -> Vec<EntityId> {
    (0..n).map(|i| world.spawn(Position(i))).collect()
}

#[test]
fn despawning_drops_the_count_by_exactly_the_despawned() {
    let mut world = world_with_components();
    let entities = spawn_n(&mut world, 10);
    assert_eq!(world.entity_count(), 10);
    for entity in &entities[2..5] {
        assert!(world.despawn(*entity));
    }
    assert_eq!(world.entity_count(), 7);
    assert_eq!(world.entity_count(), live(&world));
}

#[test]
fn despawning_twice_drops_the_count_once() {
    let mut world = world_with_components();
    let entities = spawn_n(&mut world, 3);
    assert!(world.despawn(entities[1]));
    assert!(
        !world.despawn(entities[1]),
        "a dead entity cannot die again"
    );
    assert_eq!(world.entity_count(), 2);
}

#[test]
fn despawning_a_stale_id_does_not_touch_the_count() {
    let mut world = world_with_components();
    let old = world.spawn(Position(1));
    let other = world.spawn(Position(2));
    assert!(world.despawn(old));
    let recycled = world.spawn(Position(3));
    assert_eq!(recycled.index, old.index, "the freed index is recycled");
    assert!(world.despawn(other));

    assert!(!world.despawn(old), "a stale id names no live entity");
    assert_eq!(world.entity_count(), 1);
    assert!(world.contains(recycled), "the recycled entity is untouched");
}

#[test]
fn despawning_an_unknown_id_does_not_touch_the_count() {
    let mut world = world_with_components();
    let entities = spawn_n(&mut world, 2);
    assert!(world.despawn(entities[0]));
    let unknown = EntityId {
        index: 1_000,
        generation: 0,
    };
    assert!(!world.despawn(unknown));
    assert_eq!(world.entity_count(), 1);
}

#[test]
fn recycled_slots_keep_the_count() {
    let mut world = world_with_components();
    let keep = spawn_n(&mut world, 2);
    for round in 0..8 {
        let churn = world.spawn(Position(round));
        assert_eq!(world.entity_count(), 3);
        assert!(world.despawn(churn));
        assert_eq!(world.entity_count(), 2);
    }
    // One extra dead slot is left behind, never a growing tail.
    let last = world.spawn(Position(99));
    assert!(world.despawn(keep[0]));
    assert_eq!(world.entity_count(), 2);
    assert!(world.contains(last) && world.contains(keep[1]));
}

#[test]
fn a_reserved_id_is_not_counted() {
    let mut world = world_with_components();
    spawn_n(&mut world, 3);
    world.reserve_entity();
    world.reserve_entity();
    assert_eq!(world.entity_count(), 3);
}

#[test]
fn spawning_a_reserved_id_counts_it() {
    let mut world = world_with_components();
    spawn_n(&mut world, 2);
    let held = world.reserve_entity();
    let other = world.reserve_entity();
    assert!(world.spawn_reserved(held));
    assert_eq!(world.entity_count(), 3);
    assert!(
        !world.spawn_reserved(held),
        "an id brought to life is no longer held"
    );
    assert_eq!(world.entity_count(), 3, "a refused spawn does not count");
    world.release_reserved(other);
    assert_eq!(world.entity_count(), live(&world));
}

#[test]
fn releasing_a_reserved_id_does_not_change_the_count() {
    let mut world = world_with_components();
    spawn_n(&mut world, 2);
    let held = world.reserve_entity();
    world.release_reserved(held);
    assert_eq!(world.entity_count(), 2);

    // Releasing again, or releasing a live entity's id, changes nothing.
    world.release_reserved(held);
    let alive = world.spawn(Position(5));
    world.release_reserved(alive);
    assert_eq!(world.entity_count(), 3);
    assert!(world.contains(alive));
}

#[test]
fn a_reserved_id_cannot_be_despawned() {
    let mut world = world_with_components();
    spawn_n(&mut world, 1);
    let held = world.reserve_entity();
    assert!(!world.despawn(held), "a reserved id is not alive");
    assert_eq!(world.entity_count(), 1);
    assert!(world.spawn_reserved(held), "the reservation survives");
    assert_eq!(world.entity_count(), 2);
}

#[test]
fn adding_and_removing_components_leaves_the_count_unchanged() {
    let mut world = world_with_components();
    let entities = spawn_n(&mut world, 4);
    assert!(world.despawn(entities[3]));
    let count = world.entity_count();
    assert_eq!(count, 3);

    // A migration within a domain, a first component in another domain, and
    // their removals: each takes the metadata out of its slot and puts it back.
    world
        .add_component(entities[0], Velocity(1))
        .expect("the entity is alive");
    assert_eq!(world.entity_count(), count);
    world
        .add_component(entities[1], RenderTag)
        .expect("the entity is alive");
    assert_eq!(world.entity_count(), count);
    world
        .remove_component::<Velocity>(entities[0])
        .expect("the entity holds a velocity");
    assert_eq!(world.entity_count(), count);
    world
        .remove_component::<RenderTag>(entities[1])
        .expect("the entity holds a tag");
    assert_eq!(world.entity_count(), count);
    assert!(
        world
            .remove_component_domain::<Position>(entities[2])
            .is_some(),
        "the entity holds a position"
    );
    assert_eq!(world.entity_count(), count);

    // A refused change, on a dead entity, changes nothing either.
    assert!(world.add_component(entities[3], Velocity(9)).is_err());
    assert_eq!(world.entity_count(), count);
    assert_eq!(world.entity_count(), live(&world));
}

#[test]
fn the_count_matches_the_live_entities_after_a_mixed_sequence() {
    let mut world = world_with_components();
    let a = spawn_n(&mut world, 6);
    let held = world.reserve_entity();
    assert!(world.despawn(a[0]));
    assert!(world.despawn(a[4]));
    world
        .add_component(a[1], Velocity(2))
        .expect("the entity is alive");
    let released = world.reserve_entity();
    let b = world.spawn((Position(10), Velocity(10)));
    assert!(world.spawn_reserved(held));
    world.release_reserved(released);
    assert!(world.despawn(a[1]));
    assert!(!world.despawn(a[1]));
    world
        .add_component(b, RenderTag)
        .expect("the entity is alive");
    world.spawn(Position(11));
    world.reserve_entity();

    assert_eq!(world.entity_count(), live(&world));
    assert_eq!(world.entity_count(), 6);
}

/// A world one of whose entities points at an entity the save will not hold:
/// its `Parent` names an entity despawned before the capture.
fn record_with_a_dead_reference() -> SceneRecord {
    let mut source = World::new();
    let gone = source.spawn(Transform::identity());
    source.spawn((Transform::identity(), Parent(gone)));
    source.spawn(Transform::identity());
    assert!(source.despawn(gone));
    capture_world(&source).expect("the world is captured")
}

/// Rewrites every reference written as outside into one by identity, to an
/// identity the record does not hold. Returns how many it rewrote.
fn point_outside_at(record: &mut Record, missing: PersistentId) -> usize {
    let mut rewritten = 0;
    rewrite(record, missing, &mut rewritten);
    rewritten
}

fn rewrite(record: &mut Record, missing: PersistentId, rewritten: &mut usize) {
    match record {
        Record::Entity(reference @ EntityRef::Outside) => {
            *reference = EntityRef::Id(missing);
            *rewritten += 1;
        }
        Record::Some(inner) | Record::Newtype { value: inner, .. } => {
            rewrite(inner, missing, rewritten)
        }
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            for item in items {
                rewrite(item, missing, rewritten);
            }
        }
        Record::Struct { fields, .. } => {
            for (_, value) in fields {
                rewrite(value, missing, rewritten);
            }
        }
        Record::Map(entries) => {
            for (key, value) in entries {
                rewrite(key, missing, rewritten);
                rewrite(value, missing, rewritten);
            }
        }
        Record::Variant { payload, .. } => match payload {
            VariantPayload::Unit => {}
            VariantPayload::Newtype(inner) => rewrite(inner, missing, rewritten),
            VariantPayload::Tuple(items) => {
                for item in items {
                    rewrite(item, missing, rewritten);
                }
            }
            VariantPayload::Struct(fields) => {
                for (_, value) in fields {
                    rewrite(value, missing, rewritten);
                }
            }
        },
        _ => {}
    }
}

/// Loads `record` beside the entities already in a world with no dead slot,
/// so the only id the load leaves allocated but not alive is the sentinel it
/// reserves for the dead reference.
fn assert_load_counts_only_the_live(record: &SceneRecord) {
    let mut world = World::new();
    world.spawn(Transform::identity());
    world.spawn(Transform::identity());

    let applied = apply(&mut world, record, Identity::Keep).expect("the record loads");
    assert!(
        applied
            .report
            .entries
            .iter()
            .any(|entry| entry.kind == ReportKind::DeadReference),
        "the load meets the dead reference: {:?}",
        applied.report
    );
    assert_eq!(live(&world), 2 + record.entities.len());
    assert_eq!(
        world.entity_count(),
        live(&world),
        "the sentinel a dead reference reserves is not alive"
    );
}

#[test]
fn a_load_with_a_reference_outside_the_save_counts_only_the_live() {
    let record = record_with_a_dead_reference();
    assert_load_counts_only_the_live(&record);
}

#[test]
fn a_load_with_a_reference_to_an_identity_the_save_lacks_counts_only_the_live() {
    let mut record = record_with_a_dead_reference();
    let missing = PersistentId::authored(0x00de_ad00_0000_0001);
    assert!(
        !record.entities.contains(&missing),
        "the identity is one the save does not hold"
    );
    let mut rewritten = 0;
    for page in &mut record.pages {
        for column in &mut page.columns {
            for value in column {
                rewritten += point_outside_at(value, missing);
            }
        }
    }
    assert_eq!(rewritten, 1, "the dead parent is now named by identity");
    assert_load_counts_only_the_live(&record);
}

#[test]
fn an_abandoned_load_leaves_the_count_as_it_was() {
    let record = record_with_a_dead_reference();
    let mut world = World::new();
    world.spawn(Transform::identity());
    let gone = world.spawn(Transform::identity());
    assert!(world.despawn(gone));

    let prepared = prepare(&mut world, &record).expect("the record is read");
    assert_eq!(
        world.entity_count(),
        1,
        "a prepared load adds nothing alive"
    );
    prepared.abandon(&mut world);
    assert_eq!(world.entity_count(), 1);
    assert_eq!(world.entity_count(), live(&world));
}
