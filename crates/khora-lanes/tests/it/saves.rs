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

//! The ways a scene carries a script snapshot, for the tests that save a
//! behavior part-way: as JSON directly (a Definition save), as a record (what
//! the text, compact and MessagePack scene encodings hold), as a record written
//! out as JSON and read back, and by position (what a `FastestLoad` snapshot
//! holds).

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::script::ScriptSnapshot;
use khora_data::scene::positional::{from_positional, to_positional};
use khora_data::scene::record::{
    from_record, to_record, EntityRef, Record, RecordError, ReferenceReader, ReferenceWriter,
};

/// Entities kept as themselves: an entity is written as an identity made of
/// its two numbers and read back from it, so a carried snapshot compares
/// equal to the one that was saved.
struct SameEntities;

impl ReferenceWriter for SameEntities {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        let bits = (u64::from(entity.generation) << 32) | u64::from(entity.index);
        EntityRef::Id(PersistentId::from_bits(bits))
    }
}

impl ReferenceReader for SameEntities {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        match reference {
            EntityRef::Id(id) => {
                let bits = id.to_bits();
                Ok(EntityId {
                    index: bits as u32,
                    generation: (bits >> 32) as u32,
                })
            }
            EntityRef::Outside => Err(RecordError(
                "a snapshot written here refers to no entity outside it".to_owned(),
            )),
        }
    }
}

/// A Definition save: the snapshot as JSON and back.
pub fn through_json(snapshot: &ScriptSnapshot) -> ScriptSnapshot {
    let json = serde_json::to_string(snapshot).expect("a snapshot serialises");
    serde_json::from_str(&json).expect("and parses")
}

/// A scene record: the snapshot as a record and back.
pub fn through_record(snapshot: &ScriptSnapshot) -> ScriptSnapshot {
    let record = to_record(snapshot, &mut SameEntities).expect("a snapshot is recorded");
    from_record(&record, &mut SameEntities).expect("and read back")
}

/// A record on disk: the snapshot as a record, the record as JSON, and back.
pub fn through_record_as_json(snapshot: &ScriptSnapshot) -> ScriptSnapshot {
    let record = to_record(snapshot, &mut SameEntities).expect("a snapshot is recorded");
    let json = serde_json::to_string(&record).expect("the record serialises");
    let parsed: Record = serde_json::from_str(&json).expect("and parses");
    from_record(&parsed, &mut SameEntities).expect("and is read back")
}

/// A snapshot save: the snapshot by position and back, every byte read.
pub fn through_positional(snapshot: &ScriptSnapshot) -> ScriptSnapshot {
    let mut bytes = Vec::new();
    to_positional(snapshot, &mut bytes, &mut SameEntities).expect("a snapshot is written");
    from_positional(&bytes, &mut SameEntities).expect("and read back")
}

/// Carries a snapshot through one encoding and back.
pub type Carry = fn(&ScriptSnapshot) -> ScriptSnapshot;

/// Every way a scene can carry a snapshot, by name — and one that moves it
/// between two of them.
pub fn every_encoding() -> [(&'static str, Carry); 5] {
    [
        ("JSON", through_json),
        ("record", through_record),
        ("record as JSON", through_record_as_json),
        ("positional", through_positional),
        ("JSON then positional", |s| {
            through_positional(&through_json(s))
        }),
    ]
}
