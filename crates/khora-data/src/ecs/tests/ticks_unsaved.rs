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

//! Ticks are the running world's, never the scene's.
//!
//! A tick says when a running world wrote a row; a scene says what was
//! authored. So no tick enters a scene record or a snapshot — two worlds
//! holding the same scene save the same, whatever their clocks read — and a
//! load restores none: what it places is written by the world loading it, at
//! that world's tick.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;

use super::page_ticks::{advance_to, page_of, row_tick, ticks};
use crate::ecs::{ComponentKey, SemanticDomain, Tick, Transform, World};
use crate::scene::snapshot::{prepare_snapshot, write_snapshot};
use crate::scene::{apply, capture_world, Identity};

/// The identity the one entity of these scenes is saved under.
const SAVED: u64 = 0x5EED_0000_0000_0001;

/// A world holding one entity with a `Transform`, placed at tick `tick`,
/// `Transform` watched or not.
fn one_transform_at(tick: u64, watched: bool) -> (World, EntityId) {
    let mut world = World::new();
    if watched {
        world.observe(ComponentKey::of::<Transform>());
    }
    advance_to(&mut world, tick);
    let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
    world.set_persistent_id(entity, PersistentId::from_bits(SAVED));
    (world, entity)
}

/// The summary of the `Transform` column `entity` is stored in.
fn transform_summary(world: &World, entity: EntityId) -> Tick {
    let (page, _) = page_of(world, entity, SemanticDomain::Spatial);
    ticks(page, ComponentKey::of::<Transform>()).summary
}

/// Two worlds, the same scene, different clocks: one placed its entity at
/// tick 41 and keeps its rows' ticks, the other at tick 1 and keeps only a
/// summary. Their scene records are the same.
#[test]
fn a_scene_record_is_the_same_whatever_the_ticks() {
    let (late, late_entity) = one_transform_at(41, true);
    let (early, early_entity) = one_transform_at(1, false);
    assert_eq!(transform_summary(&late, late_entity), Tick(41));
    assert_eq!(transform_summary(&early, early_entity), Tick(1));

    let late_record = capture_world(&late).expect("the late world is captured");
    let early_record = capture_world(&early).expect("the early world is captured");
    assert_eq!(late_record, early_record, "no tick is in a scene record");
}

/// Two worlds, the same scene, different clocks: their snapshots are the
/// same bytes.
#[test]
fn a_snapshot_is_the_same_whatever_the_ticks() {
    let (late, late_entity) = one_transform_at(41, true);
    let (early, early_entity) = one_transform_at(1, false);
    assert_eq!(transform_summary(&late, late_entity), Tick(41));
    assert_eq!(transform_summary(&early, early_entity), Tick(1));

    let late_file = write_snapshot(&late).expect("the late world is snapshotted");
    let early_file = write_snapshot(&early).expect("the early world is snapshotted");
    assert_eq!(
        late_file.payload, early_file.payload,
        "no tick is in a snapshot"
    );
}

/// A world at tick 7 that watches `Transform`, empty.
fn loader() -> World {
    let mut world = World::new();
    world.observe(ComponentKey::of::<Transform>());
    advance_to(&mut world, 7);
    world
}

/// The tick the row of the `Transform` loaded under [`SAVED`] reads, and the
/// summary of its column.
fn loaded_ticks(world: &World) -> (u64, Tick) {
    let entity = world
        .entity_with_id(PersistentId::from_bits(SAVED))
        .expect("the saved entity is loaded");
    let (page, row) = page_of(world, entity, SemanticDomain::Spatial);
    let column = ticks(page, ComponentKey::of::<Transform>());
    (row_tick(column, row), column.summary)
}

/// A scene saved by a world at tick 41 and loaded by one at tick 7: the
/// loaded row is written by the loading world, now — its tick is 7, never
/// the 41 the saving world had.
#[test]
fn a_scene_load_restores_no_tick() {
    let (source, _) = one_transform_at(41, true);
    let record = capture_world(&source).expect("the world is captured");

    let mut world = loader();
    apply(&mut world, &record, Identity::Keep).expect("the record loads");
    let (row, summary) = loaded_ticks(&world);
    assert_eq!(row, 7, "the loaded row is written at the loader's tick");
    assert_eq!(summary, Tick(7), "the column was written now, by the load");
}

/// The same through a snapshot — the editor's Play/Stop restore: the
/// restored row is written at the restoring world's tick.
#[test]
fn a_snapshot_load_restores_no_tick() {
    let (source, _) = one_transform_at(41, true);
    let file = write_snapshot(&source).expect("the world is snapshotted");

    let mut world = loader();
    prepare_snapshot(&mut world, &file)
        .expect("the snapshot is read")
        .commit(&mut world, Identity::Keep);
    let (row, summary) = loaded_ticks(&world);
    assert_eq!(row, 7, "the restored row is written at the loader's tick");
    assert_eq!(
        summary,
        Tick(7),
        "the column was written now, by the restore"
    );
}

/// A loaded hierarchy places each row once, its `Children` list pushed into
/// the same row as its other values: every watched column keeps exactly one
/// tick per row, each the loader's.
#[test]
fn a_loaded_hierarchy_notes_one_tick_per_row() {
    use crate::ecs::{Children, Parent};

    let mut source = World::new();
    let parent = source.spawn(Transform::identity());
    let child = source.spawn(Transform::identity());
    let other = source.spawn(Transform::identity());
    source.set_parent(child, Some(parent));
    source.set_parent(other, Some(parent));
    for (entity, bits) in [(parent, 1u64), (child, 2), (other, 3)] {
        source.set_persistent_id(entity, PersistentId::from_bits(SAVED + bits));
    }
    let record = capture_world(&source).expect("the world is captured");

    let mut world = loader();
    world.observe(ComponentKey::of::<Parent>());
    world.observe(ComponentKey::of::<Children>());
    apply(&mut world, &record, Identity::Keep).expect("the record loads");

    let mut rows_seen = 0;
    for (page_id, page) in world.storage.pages.iter().enumerate() {
        for &key in &page.keys {
            let column = ticks(page, key);
            if let Some(rows) = &column.rows {
                assert_eq!(rows.len(), page.row_count(), "page {page_id} {key:?}");
                assert!(
                    rows.iter().all(|&t| t == 7),
                    "page {page_id} {key:?}: {rows:?}"
                );
                rows_seen += rows.len();
            }
            assert!(column.summary <= world.current_tick());
        }
    }
    assert!(
        rows_seen >= 5,
        "Transform ×3, Parent ×2 (+ Children) watched"
    );
}
