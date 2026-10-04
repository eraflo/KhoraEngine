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

//! The entities a record's values refer to, and the parent a row names.

use std::collections::HashSet;

use khora_core::ecs::PersistentId;

use super::record::Record;
use super::scene_record::SceneRecord;

/// Every entity `value` refers to, added to `found`.
fn entities_in(value: &Record, found: &mut HashSet<PersistentId>) {
    use super::record::{EntityRef, VariantPayload};
    match value {
        Record::Entity(EntityRef::Id(id)) => {
            found.insert(*id);
        }
        Record::Some(inner) | Record::Newtype { value: inner, .. } => entities_in(inner, found),
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            items.iter().for_each(|item| entities_in(item, found))
        }
        Record::Map(entries) => entries.iter().for_each(|(key, value)| {
            entities_in(key, found);
            entities_in(value, found);
        }),
        Record::Struct { fields, .. } => fields
            .iter()
            .for_each(|(_, value)| entities_in(value, found)),
        Record::Variant { payload, .. } => match payload {
            VariantPayload::Unit => {}
            VariantPayload::Newtype(inner) => entities_in(inner, found),
            VariantPayload::Tuple(items) => items.iter().for_each(|item| entities_in(item, found)),
            VariantPayload::Struct(fields) => fields
                .iter()
                .for_each(|(_, value)| entities_in(value, found)),
        },
        _ => {}
    }
}

/// Every entity `record`'s values refer to.
pub(crate) fn referenced(record: &SceneRecord) -> Vec<PersistentId> {
    let mut found = HashSet::new();
    for value in record
        .pages
        .iter()
        .flat_map(|page| page.columns.iter().flatten())
    {
        entities_in(value, &mut found);
    }
    found.into_iter().collect()
}

/// The entity an entity's `Parent` names, if it has one.
pub(crate) fn parent_in(components: &[(String, Record)]) -> Option<PersistentId> {
    let (_, parent) = components.iter().find(|(name, _)| name == "Parent")?;
    let mut found = HashSet::new();
    entities_in(parent, &mut found);
    found.into_iter().next()
}
