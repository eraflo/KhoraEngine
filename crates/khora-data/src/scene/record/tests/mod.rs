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

//! Tests of the record codec.
//!
//! The helpers here stand in for a save's entity table: a writer that hands
//! each entity a persistent identity, and a reader that maps it back.

use std::collections::HashMap;
use std::fmt::Debug;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::*;

mod adversarial;
mod damaged;
mod data_model;
mod edge_cases;
mod references;
mod report;
mod resolution;
mod round_trip;
mod widening;

/// An entity at `index`, `generation`.
fn entity(index: u32, generation: u32) -> EntityId {
    EntityId { index, generation }
}

/// A save's entity table, both ways, remembering every call it received.
#[derive(Default)]
struct IdTable {
    /// Every entity the writer was handed, in the order it was handed them.
    written: Vec<EntityId>,
    /// Every reference the reader was handed, in order.
    read: Vec<EntityRef>,
    /// The identity each entity was given.
    ids: HashMap<EntityId, PersistentId>,
}

impl IdTable {
    /// The identity the writer gave `entity`.
    fn id_of(&self, entity: EntityId) -> PersistentId {
        *self
            .ids
            .get(&entity)
            .unwrap_or_else(|| panic!("{entity:?} was never written"))
    }

    /// The entity the writer gave `id` to.
    fn entity_of(&self, id: PersistentId) -> Option<EntityId> {
        self.ids
            .iter()
            .find(|(_, known)| **known == id)
            .map(|(entity, _)| *entity)
    }
}

impl ReferenceWriter for IdTable {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.written.push(entity);
        let next = self.ids.len() as u64 + 1;
        EntityRef::Id(
            *self
                .ids
                .entry(entity)
                .or_insert_with(|| PersistentId::created(next)),
        )
    }
}

impl ReferenceReader for IdTable {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        self.read.push(reference);
        match reference {
            EntityRef::Id(id) => self
                .entity_of(id)
                .ok_or_else(|| RecordError(format!("no entity was written as {id:?}"))),
            EntityRef::Outside => Err(RecordError("an entity outside the save".into())),
        }
    }
}

/// A reader for records that must not hold any entity.
struct NoEntities;

impl ReferenceReader for NoEntities {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        Err(RecordError(format!(
            "unexpected entity reference {reference:?}"
        )))
    }
}

/// Reads a `T` from a record that holds no entity.
fn read<T: DeserializeOwned>(record: &Record) -> Result<T, RecordError> {
    from_record(record, &mut NoEntities)
}

/// Writes `value` and reads it back through the same entity table, checking on
/// the way that writing the result again gives the same record.
fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> T {
    let name = std::any::type_name::<T>();
    let mut table = IdTable::default();
    let record =
        to_record(value, &mut table).unwrap_or_else(|e| panic!("{name} does not write: {e}"));
    let back: T =
        from_record(&record, &mut table).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
    let again =
        to_record(&back, &mut table).unwrap_or_else(|e| panic!("{name} does not rewrite: {e}"));
    assert_eq!(again, record, "{name} reads back as a different record");
    back
}

/// Asserts `value` comes back from a record exactly as it went in.
///
/// Compared through `Debug`: the generated component mirrors do not implement
/// `PartialEq`, and `Debug` prints every field, floats at full precision.
fn assert_round_trips<T: Serialize + DeserializeOwned + Debug>(value: &T) {
    let back = round_trip(value);
    assert_eq!(
        format!("{back:?}"),
        format!("{value:?}"),
        "{} changed through a record",
        std::any::type_name::<T>()
    );
}

/// A string record.
fn text(value: &str) -> Record {
    Record::Str(value.to_owned())
}

/// A struct record named `name` with `fields`, in the order given.
fn structure(name: &str, fields: Vec<(&str, Record)>) -> Record {
    Record::Struct {
        name: name.to_owned(),
        fields: fields
            .into_iter()
            .map(|(field, value)| (field.to_owned(), value))
            .collect(),
    }
}

/// An enum variant record.
fn variant(enum_name: &str, name: &str, payload: VariantPayload) -> Record {
    Record::Variant {
        enum_name: enum_name.to_owned(),
        variant: name.to_owned(),
        payload,
    }
}

/// The named fields of a struct-variant payload.
fn struct_payload(fields: Vec<(&str, Record)>) -> VariantPayload {
    VariantPayload::Struct(
        fields
            .into_iter()
            .map(|(field, value)| (field.to_owned(), value))
            .collect(),
    )
}

/// The value of field `name` in a struct record.
fn field_of<'a>(record: &'a Record, name: &str) -> &'a Record {
    match record {
        Record::Struct { fields, .. } => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("no field `{name}` in {record:?}")),
        other => panic!("expected a struct record, found {other:?}"),
    }
}
