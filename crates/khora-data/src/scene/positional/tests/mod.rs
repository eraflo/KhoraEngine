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

//! Tests of the positional codec.
//!
//! The helpers stand in for a snapshot's entity table: a writer that hands
//! each entity a persistent identity (or calls it outside), and a reader that
//! places every identity somewhere else than it was — so a value that comes
//! back with the reader's entities proves it went through the hooks.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::*;
use crate::scene::record::EntityRef;

mod adversarial;
mod references;
mod round_trip;

/// What every reference outside the snapshot reads back as here.
const NOWHERE: EntityId = EntityId {
    index: u32::MAX,
    generation: 0,
};

/// Where an entity landed in the world being loaded.
fn moved(entity: EntityId) -> EntityId {
    EntityId {
        index: entity.index + 1000,
        generation: entity.generation + 1,
    }
}

/// A snapshot's entity table, both ways, remembering what the reader was
/// handed.
#[derive(Default)]
struct Table {
    /// The identity each entity was given.
    ids: HashMap<EntityId, PersistentId>,
    /// Entities the writer calls outside the snapshot.
    outside: Vec<EntityId>,
    /// Every reference the reader was handed, in order.
    read: Vec<EntityRef>,
    /// Whether the reader relocates entities, or gives them back as written.
    relocate: bool,
}

impl Table {
    fn relocating() -> Self {
        Self {
            relocate: true,
            ..Self::default()
        }
    }

    fn id_of(&self, entity: EntityId) -> PersistentId {
        *self
            .ids
            .get(&entity)
            .unwrap_or_else(|| panic!("{entity:?} was never written"))
    }

    fn entity_of(&self, id: PersistentId) -> Option<EntityId> {
        self.ids
            .iter()
            .find(|(_, known)| **known == id)
            .map(|(entity, _)| *entity)
    }
}

impl ReferenceWriter for Table {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        if self.outside.contains(&entity) {
            return EntityRef::Outside;
        }
        let next = self.ids.len() as u64 + 1;
        EntityRef::Id(
            *self
                .ids
                .entry(entity)
                .or_insert_with(|| PersistentId::created(next)),
        )
    }
}

impl ReferenceReader for Table {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        self.read.push(reference);
        match reference {
            EntityRef::Id(id) => self
                .entity_of(id)
                .map(|entity| if self.relocate { moved(entity) } else { entity })
                .ok_or_else(|| RecordError(format!("no entity was written as {id:?}"))),
            EntityRef::Outside => Ok(NOWHERE),
        }
    }
}

/// A reader for bytes that must not hold any entity.
struct NoEntities;

impl ReferenceReader for NoEntities {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        Err(RecordError(format!(
            "unexpected entity reference {reference:?}"
        )))
    }
}

/// A writer for values that must not hold any entity.
impl ReferenceWriter for NoEntities {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        panic!("unexpected entity {entity:?}")
    }
}

/// `value`'s positional bytes.
fn encode<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    to_positional(value, &mut out, &mut Table::default())
        .unwrap_or_else(|e| panic!("{} does not write: {e}", std::any::type_name::<T>()));
    out
}

/// Reads a `T` from bytes that hold no entity.
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RecordError> {
    from_positional(bytes, &mut NoEntities)
}

/// Writes `value` and reads it back through the same table, checking that
/// writing the result again gives the same bytes.
fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> T {
    let name = std::any::type_name::<T>();
    let mut table = Table::default();
    let mut bytes = Vec::new();
    to_positional(value, &mut bytes, &mut table)
        .unwrap_or_else(|e| panic!("{name} does not write: {e}"));
    let back: T =
        from_positional(&bytes, &mut table).unwrap_or_else(|e| panic!("{name} does not read: {e}"));
    let mut again = Vec::new();
    to_positional(&back, &mut again, &mut table)
        .unwrap_or_else(|e| panic!("{name} does not rewrite: {e}"));
    assert_eq!(again, bytes, "{name} reads back as different bytes");
    back
}

/// Asserts `value` comes back exactly as it went in, compared through
/// `Debug` (component mirrors do not implement `PartialEq`).
fn assert_round_trips<T: Serialize + DeserializeOwned + Debug>(value: &T) {
    let back = round_trip(value);
    assert_eq!(
        format!("{back:?}"),
        format!("{value:?}"),
        "{} changed through the positional codec",
        std::any::type_name::<T>()
    );
}

/// A byte string, written as serde bytes rather than as a sequence.
#[derive(Debug, Clone, PartialEq)]
struct Blob(Vec<u8>);

impl Serialize for Blob {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BlobVisitor;
        impl<'de> serde::de::Visitor<'de> for BlobVisitor {
            type Value = Blob;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Blob, E> {
                Ok(Blob(v.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Blob, E> {
                Ok(Blob(v))
            }
        }
        deserializer.deserialize_byte_buf(BlobVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Numbers {
    a_i8: i8,
    a_i16: i16,
    a_i32: i32,
    a_i64: i64,
    a_u8: u8,
    a_u16: u16,
    a_u32: u32,
    a_u64: u64,
    a_f32: f32,
    a_f64: f64,
    flag: bool,
    letter: char,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Marker;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Meters(f32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Pair(u8, String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Shape {
    Empty,
    Circle(f32),
    Rect(f32, f32),
    Poly {
        points: Vec<(f32, f32)>,
        closed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Scene {
    name: String,
    marker: Marker,
    length: Meters,
    pair: Pair,
    tuple: (u8, i64, String),
    array: [f32; 4],
    shapes: Vec<Shape>,
    by_name: BTreeMap<String, Pair>,
    by_id: HashMap<u32, String>,
    origin: Option<[f32; 3]>,
    nothing: Option<u32>,
    layered: Option<Option<u8>>,
    nested: Vec<Vec<u16>>,
    blob: Blob,
    numbers: Numbers,
}

/// A value of every kind the data model has, nested.
fn rich_scene() -> Scene {
    Scene {
        name: "héllo wörld ✓ 🦀".into(),
        marker: Marker,
        length: Meters(12.5),
        pair: Pair(7, "pair".into()),
        tuple: (255, -9_000_000_000, String::new()),
        array: [0.0, -1.5, 3.25, f32::MAX],
        shapes: vec![
            Shape::Empty,
            Shape::Circle(2.0),
            Shape::Rect(3.0, 4.0),
            Shape::Poly {
                points: vec![(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)],
                closed: true,
            },
        ],
        by_name: [
            ("a".to_owned(), Pair(1, "x".into())),
            ("b".to_owned(), Pair(2, "y".into())),
        ]
        .into_iter()
        .collect(),
        by_id: [(1, "one".to_owned()), (300, "three hundred".to_owned())]
            .into_iter()
            .collect(),
        origin: Some([1.0, 2.0, 3.0]),
        nothing: None,
        layered: Some(None),
        nested: vec![vec![], vec![1, 2, 3], (0..300).collect()],
        blob: Blob(vec![0, 1, 0x7F, 0x80, 0xFF]),
        numbers: Numbers {
            a_i8: i8::MIN,
            a_i16: i16::MIN,
            a_i32: i32::MIN,
            a_i64: i64::MIN,
            a_u8: u8::MAX,
            a_u16: u16::MAX,
            a_u32: u32::MAX,
            a_u64: u64::MAX,
            a_f32: f32::MIN_POSITIVE,
            a_f64: f64::EPSILON,
            flag: true,
            letter: 'ß',
        },
    }
}
