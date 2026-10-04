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

//! Scene records end to end, through the public API only: a world written
//! down, encoded, read back and brought into another world — identities,
//! references, hierarchy and lifecycle facts included.
//!
//! The test components declared here go through `#[derive(Component)]`, which
//! spells its output with `crate::ecs` / `crate::scene` paths; the two modules
//! below re-export `khora_data`'s, so the derive expands here exactly as it
//! does inside the crate.

use std::collections::{BTreeMap, HashMap};

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_data::ecs::{Children, World};
use khora_data::scene::record::Record;
use khora_data::scene::{
    apply, capture_world, Applied, CompactEncoding, ComponentRegistration, Identity,
    MsgPackEncoding, PageRecord, SceneEncoding, SceneRecord, TextEncoding,
};

mod decimals;
mod failures;
mod hierarchy_and_identity;
mod hostile_records;
mod identity;
mod lazy_identity;
mod load_limits;
mod not_saved;
mod pages;
mod prefab_files;
mod prefab_instances;
mod prefab_overrides;
mod prefab_refusals;
mod prefab_sample;
mod prefab_scale;
mod prefab_structure;
mod references;
mod renames;
mod round_trip;
mod sample;
mod save_against_edits;
mod save_files;
mod save_renames_and_damage;
mod saves;
mod snapshot;
mod snapshot_bounds;
mod snapshot_refusals;
mod snapshot_schema;
mod stack_depth;
mod subtree;

/// The `crate::ecs` the derive expands against.
mod ecs {
    pub use khora_data::ecs::*;
}

/// The `crate::scene` the derive expands against.
mod scene {
    pub use khora_data::scene::*;
}

/// The three encodings, each with the name a failure message uses for it.
fn every_encoding() -> [(&'static str, &'static dyn SceneEncoding); 3] {
    [
        ("compact", &CompactEncoding),
        ("text", &TextEncoding),
        ("msgpack", &MsgPackEncoding),
    ]
}

/// Every registration a scene records: what an author or a tool wrote.
fn saved_registrations() -> impl Iterator<Item = &'static ComponentRegistration> {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .filter(|reg| reg.provenance.is_copied_on_duplicate())
}

/// The JSON view of every saved component `entity` carries, by type name.
fn components_of(world: &World, entity: EntityId) -> BTreeMap<&'static str, serde_json::Value> {
    saved_registrations()
        .filter_map(|reg| (reg.to_json)(world, entity).map(|json| (reg.type_name, json)))
        .collect()
}

/// `value` with every entity of `map`'s keys replaced by the entity it maps to.
///
/// An entity's JSON is its `{index, generation}` pair; any object equal to a
/// source entity's is one of its references.
fn remap(value: &serde_json::Value, map: &HashMap<EntityId, EntityId>) -> serde_json::Value {
    for (from, to) in map {
        if *value == serde_json::to_value(from).expect("an entity is JSON") {
            return serde_json::to_value(to).expect("an entity is JSON");
        }
    }
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(|item| remap(item, map)).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, item)| (key.clone(), remap(item, map)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Each live entity of `src`, and the entity of `dst` known by the same
/// persistent identity.
fn entity_map(src: &World, dst: &World) -> HashMap<EntityId, EntityId> {
    src.iter_entities()
        .map(|entity| {
            let id = src
                .persistent_id(entity)
                .unwrap_or_else(|| panic!("{entity:?} has no persistent id"));
            let twin = dst
                .entity_with_id(id)
                .unwrap_or_else(|| panic!("no entity known as {id:?} after the load"));
            (entity, twin)
        })
        .collect()
}

/// Asserts `dst` holds what `src` held: the same entities by identity, each
/// with the same saved components, references pointing at the same entities,
/// and the hierarchy's inverse index rebuilt.
fn assert_same_world(src: &World, dst: &World, encoding: &str) {
    assert_eq!(
        dst.iter_entities().count(),
        src.iter_entities().count(),
        "{encoding}: the entity count changed"
    );
    let map = entity_map(src, dst);
    for (&entity, &twin) in &map {
        assert_eq!(
            dst.persistent_id(twin),
            src.persistent_id(entity),
            "{encoding}: identity of {entity:?}"
        );
        let expected: BTreeMap<_, _> = components_of(src, entity)
            .into_iter()
            .map(|(name, json)| (name, remap(&json, &map)))
            .collect();
        assert_eq!(
            components_of(dst, twin),
            expected,
            "{encoding}: components of {:?}",
            src.persistent_id(entity)
        );

        let mut children: Vec<EntityId> = src
            .get::<Children>(entity)
            .map(|c| c.0.iter().map(|child| map[child]).collect())
            .unwrap_or_default();
        let mut children_back: Vec<EntityId> = dst
            .get::<Children>(twin)
            .map(|c| c.0.clone())
            .unwrap_or_default();
        children.sort_by_key(|e| (e.index, e.generation));
        children_back.sort_by_key(|e| (e.index, e.generation));
        assert_eq!(
            children_back,
            children,
            "{encoding}: children of {:?}",
            src.persistent_id(entity)
        );
    }
}

/// Captures `src`, writes it in `encoding`, reads it back and applies it to a
/// fresh world with the saved identities.
fn reload(src: &World, name: &str, encoding: &dyn SceneEncoding) -> (World, Applied) {
    let record = capture_world(src).unwrap_or_else(|e| panic!("{name}: capture failed: {e}"));
    let back = through(&record, name, encoding);
    let mut dst = World::new();
    let applied = apply(&mut dst, &back, Identity::Keep)
        .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
    (dst, applied)
}

/// `record` written in `encoding` and read back.
fn through(record: &SceneRecord, name: &str, encoding: &dyn SceneEncoding) -> SceneRecord {
    let bytes = encoding
        .encode(record)
        .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
    encoding
        .decode(&bytes)
        .unwrap_or_else(|e| panic!("{name}: decode of its own output failed: {e}"))
}

/// Where `record` holds `entity`'s `component`: the page, the column and the
/// row.
fn slot_of(
    record: &SceneRecord,
    entity: PersistentId,
    component: &str,
) -> Option<(usize, usize, usize)> {
    record.pages.iter().enumerate().find_map(|(index, page)| {
        let column = page.components.iter().position(|name| name == component)?;
        let row = page.rows.iter().position(|id| *id == entity)?;
        Some((index, column, row))
    })
}

/// The value `record` holds for `entity`'s `component`, to change it.
fn value_mut<'a>(
    record: &'a mut SceneRecord,
    entity: PersistentId,
    component: &str,
) -> &'a mut Record {
    let (page, column, row) = slot_of(record, entity, component)
        .unwrap_or_else(|| panic!("the record holds no `{component}` for {entity:?}"));
    &mut record.pages[page].columns[column][row]
}

/// Moves the row of `entity` that holds `component` out of its page record
/// into a page record of its own, with the same components — still a valid
/// record — and returns that page's index.
fn isolate(record: &mut SceneRecord, entity: PersistentId, component: &str) -> usize {
    let (index, _, row) = slot_of(record, entity, component)
        .unwrap_or_else(|| panic!("the record holds no `{component}` for {entity:?}"));
    let page = &mut record.pages[index];
    let alone = PageRecord {
        components: page.components.clone(),
        rows: vec![page.rows.remove(row)],
        columns: page
            .columns
            .iter_mut()
            .map(|column| vec![column.remove(row)])
            .collect(),
    };
    record.pages.push(alone);
    record.pages.len() - 1
}

/// Adds a `name` column to page `page`, holding `value` on every row.
fn add_column(record: &mut SceneRecord, page: usize, name: &str, value: Record) {
    let page = &mut record.pages[page];
    page.components.push(name.to_owned());
    page.columns.push(vec![value; page.rows.len()]);
}

/// Renames every `from` column of `record` to `to`, returning how many.
fn rename_component(record: &mut SceneRecord, from: &str, to: &str) -> usize {
    let mut renamed = 0;
    for name in record
        .pages
        .iter_mut()
        .flat_map(|page| page.components.iter_mut())
    {
        if name == from {
            *name = to.to_owned();
            renamed += 1;
        }
    }
    renamed
}

/// `record` without the entities `gone`: not listed, and holding no row.
fn without_entities(record: &mut SceneRecord, gone: &[PersistentId]) {
    record.entities.retain(|id| !gone.contains(id));
    for page in &mut record.pages {
        let keep: Vec<bool> = page.rows.iter().map(|id| !gone.contains(id)).collect();
        let mut kept = keep.iter();
        page.rows
            .retain(|_| *kept.next().expect("one flag per row"));
        for column in &mut page.columns {
            let mut kept = keep.iter();
            column.retain(|_| *kept.next().expect("one flag per row"));
        }
    }
    record.pages.retain(|page| !page.rows.is_empty());
}
