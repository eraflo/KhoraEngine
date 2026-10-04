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

//! Moving an instance's entities to where it is rooted: a prefab's ids
//! re-keyed under an instance root, and an instance moved to a new root.

use std::collections::HashMap;

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;

use super::super::apply::LoadFailure;
use super::super::record::{EntityRef, Record, VariantPayload};
use super::super::save::{RemovedComponents, SaveRecord};
use super::super::scene_record::{InstanceRecord, PageRecord, SceneRecord};
use super::{read_prefab, PrefabSource, Stack};

/// The id an entity of a prefab's record takes in an instance rooted at
/// `root`: the prefab's root is the instance root, every other entity is
/// known within it.
pub(super) fn rekeyed(
    id: PersistentId,
    prefab_root: PersistentId,
    root: PersistentId,
) -> PersistentId {
    if id == prefab_root {
        root
    } else {
        PersistentId::within(root, id)
    }
}

/// Every entity of an instance of `prefab` rooted at `root`, by its path in
/// the prefab — its own id, preceded by the roots of the instances it sits
/// in — and the id it takes there.
pub(super) fn members(
    prefab: AssetUUID,
    root: PersistentId,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<Vec<(Vec<PersistentId>, PersistentId)>, LoadFailure> {
    let record = read_prefab(prefab, prefabs, stack)?;
    let prefab_root = record.entities[0];
    let mut all: Vec<(Vec<PersistentId>, PersistentId)> = record
        .entities
        .iter()
        .map(|id| (vec![*id], rekeyed(*id, prefab_root, root)))
        .collect();
    stack.push(prefab);
    for nested in &record.instances {
        let nested_root = rekeyed(nested.root, prefab_root, root);
        for (path, id) in members(nested.prefab, nested_root, prefabs, stack)? {
            if path.len() == 1 && id == nested_root {
                continue;
            }
            let mut full = vec![nested.root];
            full.extend(path);
            all.push((full, id));
        }
    }
    stack.pop();
    Ok(all)
}

/// The re-keying that moves an instance of `prefab` from `from` to `to`.
pub(super) fn moved(
    prefab: AssetUUID,
    from: PersistentId,
    to: PersistentId,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<HashMap<PersistentId, PersistentId>, LoadFailure> {
    let old = members(prefab, from, prefabs, stack)?;
    let new: HashMap<Vec<PersistentId>, PersistentId> =
        members(prefab, to, prefabs, stack)?.into_iter().collect();
    Ok(old
        .into_iter()
        .filter_map(|(path, id)| new.get(&path).map(|moved| (id, *moved)))
        .collect())
}

/// `value` with every entity it refers to re-keyed through `map`.
pub(super) fn remap_value(value: &Record, map: &HashMap<PersistentId, PersistentId>) -> Record {
    let all = |items: &[Record]| items.iter().map(|item| remap_value(item, map)).collect();
    let named = |fields: &[(String, Record)]| {
        fields
            .iter()
            .map(|(name, value)| (name.clone(), remap_value(value, map)))
            .collect()
    };
    match value {
        Record::Entity(EntityRef::Id(id)) => {
            Record::Entity(EntityRef::Id(map.get(id).copied().unwrap_or(*id)))
        }
        Record::Some(inner) => Record::Some(Box::new(remap_value(inner, map))),
        Record::Seq(items) => Record::Seq(all(items)),
        Record::Map(entries) => Record::Map(
            entries
                .iter()
                .map(|(key, value)| (remap_value(key, map), remap_value(value, map)))
                .collect(),
        ),
        Record::Struct { name, fields } => Record::Struct {
            name: name.clone(),
            fields: named(fields),
        },
        Record::TupleStruct { name, fields } => Record::TupleStruct {
            name: name.clone(),
            fields: all(fields),
        },
        Record::Newtype { name, value } => Record::Newtype {
            name: name.clone(),
            value: Box::new(remap_value(value, map)),
        },
        Record::Variant {
            enum_name,
            variant,
            payload,
        } => Record::Variant {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            payload: match payload {
                VariantPayload::Unit => VariantPayload::Unit,
                VariantPayload::Newtype(inner) => {
                    VariantPayload::Newtype(Box::new(remap_value(inner, map)))
                }
                VariantPayload::Tuple(items) => VariantPayload::Tuple(all(items)),
                VariantPayload::Struct(fields) => VariantPayload::Struct(named(fields)),
            },
        },
        other => other.clone(),
    }
}

pub(super) fn remap_ids(
    ids: &[PersistentId],
    map: &HashMap<PersistentId, PersistentId>,
) -> Vec<PersistentId> {
    ids.iter()
        .map(|id| map.get(id).copied().unwrap_or(*id))
        .collect()
}

/// `record` with every entity — its ids, its references, its instances'
/// roots and differences — re-keyed through `map`.
pub(super) fn remap_record(
    record: &SceneRecord,
    map: &HashMap<PersistentId, PersistentId>,
) -> SceneRecord {
    SceneRecord {
        entities: remap_ids(&record.entities, map),
        pages: record
            .pages
            .iter()
            .map(|page| PageRecord {
                components: page.components.clone(),
                rows: remap_ids(&page.rows, map),
                columns: page
                    .columns
                    .iter()
                    .map(|column| column.iter().map(|value| remap_value(value, map)).collect())
                    .collect(),
            })
            .collect(),
        instances: record
            .instances
            .iter()
            .map(|instance| InstanceRecord {
                root: map.get(&instance.root).copied().unwrap_or(instance.root),
                prefab: instance.prefab,
                delta: remap_save(&instance.delta, map),
            })
            .collect(),
    }
}

pub(super) fn remap_save(
    save: &SaveRecord,
    map: &HashMap<PersistentId, PersistentId>,
) -> SaveRecord {
    SaveRecord {
        base: save.base,
        order: remap_ids(&save.order, map),
        destroyed: remap_ids(&save.destroyed, map),
        created: remap_ids(&save.created, map),
        removed: save
            .removed
            .iter()
            .map(|entry| RemovedComponents {
                entity: map.get(&entry.entity).copied().unwrap_or(entry.entity),
                components: entry.components.clone(),
            })
            .collect(),
        changes: remap_record(&save.changes, map),
        before: remap_record(&save.before, map),
    }
}

/// `record` re-keyed so its instances' differences name the members of
/// instances rooted where `map` puts their roots: every id the record names,
/// and every member of every instance it links to.
pub(super) fn moved_record(
    record: &SceneRecord,
    map: &mut HashMap<PersistentId, PersistentId>,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<SceneRecord, LoadFailure> {
    for instance in &record.instances {
        let to = map.get(&instance.root).copied().unwrap_or(instance.root);
        if to != instance.root {
            let members = moved(instance.prefab, instance.root, to, prefabs, stack)?;
            map.extend(members);
        }
    }
    Ok(remap_record(record, map))
}
