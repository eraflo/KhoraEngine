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

//! A record that does not describe the value asked for.

use khora_core::asset::AssetUUID;
use khora_core::script::ScriptValue;
use serde::de::IgnoredAny;
use serde::{Serialize, Serializer};

use crate::ecs::{ProjectionType, SerializableBodyMotion, SerializableParent};

use super::*;

/// How deep the hostile records below nest: far past anything a real value
/// holds, and far past what reading by plain recursion survives.
const HOSTILE_DEPTH: usize = 10_000;

/// `depth` optionals, one inside the other.
fn some_chain(depth: usize) -> Record {
    let mut record = Record::Unit;
    for _ in 0..depth {
        record = Record::Some(Box::new(record));
    }
    record
}

/// A script value that is an array holding an array, `depth` times.
fn array_chain(depth: usize) -> Record {
    let mut record = variant("ScriptValue", "Unit", VariantPayload::Unit);
    for _ in 0..depth {
        record = variant(
            "ScriptValue",
            "Array",
            VariantPayload::Newtype(Box::new(Record::Seq(vec![record]))),
        );
    }
    record
}

/// Takes a deep chain apart one level at a time, so that dropping it does not
/// recurse as deep as it nests.
fn dismantle(mut record: Record) {
    loop {
        record = match record {
            Record::Some(inner) => *inner,
            Record::Variant {
                payload: VariantPayload::Newtype(inner),
                ..
            } => *inner,
            Record::Seq(mut items) => match items.pop() {
                Some(inner) => inner,
                None => return,
            },
            _ => return,
        };
    }
}

/// A value that nests itself `self.0` times as it writes, without holding
/// anything: deep only on the way through the serializer.
struct Deep(usize);

impl Serialize for Deep {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            0 => serializer.serialize_unit(),
            depth => serializer.serialize_some(&Deep(depth - 1)),
        }
    }
}

/// A record comes from a file, and a file is input: a record of the wrong
/// shape for the value asked for — a wrong kind, an unknown variant, an
/// entity where a struct is expected or a raw struct where an entity is,
/// nesting built to exhaust the stack — is an error the load can report, and
/// never a panic or an abort that takes the process down with it.
#[test]
fn a_damaged_record_is_an_error_not_a_panic() {
    // Wrong kinds.
    assert!(read::<u32>(&text("12")).is_err());
    assert!(read::<String>(&Record::Seq(vec![])).is_err());
    assert!(read::<Vec<u32>>(&structure("Probe", vec![("a", Record::U64(1))])).is_err());
    assert!(read::<SerializableBodyMotion>(&Record::U64(3)).is_err());
    assert!(read::<SerializableBodyMotion>(&Record::Seq(vec![])).is_err());
    assert!(read::<[u32; 2]>(&Record::Seq(vec![Record::U64(1)])).is_err());
    assert!(read::<Option<u32>>(&text("none")).is_err());
    assert!(read::<bool>(&Record::Unit).is_err());

    // Enums: a variant the type does not have, or the wrong payload for one
    // it has.
    assert!(
        read::<ProjectionType>(&variant("ProjectionType", "Fisheye", VariantPayload::Unit))
            .is_err()
    );
    assert!(read::<ProjectionType>(&variant(
        "ProjectionType",
        "Perspective",
        VariantPayload::Unit
    ))
    .is_err());
    assert!(read::<ProjectionType>(&variant(
        "ProjectionType",
        "Perspective",
        VariantPayload::Tuple(vec![Record::F32(1.0)])
    ))
    .is_err());
    assert!(read::<ScriptValue>(&variant(
        "ScriptValue",
        "Int",
        VariantPayload::Newtype(Box::new(text("seven")))
    ))
    .is_err());
    assert!(read::<ProjectionType>(&Record::Map(vec![
        (text("Perspective"), Record::Unit),
        (text("Orthographic"), Record::Unit),
    ]))
    .is_err());

    // A reference where a value is expected, and a value where a reference is.
    let mut table = IdTable::default();
    let known = match table.write_entity(entity(1, 1)) {
        EntityRef::Id(id) => id,
        EntityRef::Outside => unreachable!("the table always hands out an id"),
    };
    let reference = Record::Entity(EntityRef::Id(known));
    assert!(from_record::<SerializableBodyMotion>(&reference, &mut table).is_err());
    assert!(from_record::<u64>(&reference, &mut table).is_err());
    assert!(from_record::<AssetUUID>(&reference, &mut table).is_err());
    let raw_id = structure(
        "khora.EntityId",
        vec![("index", Record::U64(1)), ("generation", Record::U64(1))],
    );
    assert!(from_record::<EntityId>(&raw_id, &mut table).is_err());
    assert!(from_record::<SerializableParent>(
        &Record::Newtype {
            name: "SerializableParent".into(),
            value: Box::new(raw_id.clone()),
        },
        &mut table
    )
    .is_err());
    assert!(from_record::<EntityId>(&Record::U64(1), &mut table).is_err());
    let asset = Record::Asset(AssetUUID::new_v5("textures/wall.png"));
    assert!(from_record::<EntityId>(&asset, &mut table).is_err());
    assert!(read::<u64>(&asset).is_err());

    // A reference the reader refuses fails the read.
    assert!(from_record::<EntityId>(&Record::Entity(EntityRef::Outside), &mut table).is_err());
    assert!(read::<SerializableParent>(&Record::Newtype {
        name: "SerializableParent".into(),
        value: Box::new(reference.clone()),
    })
    .is_err());

    // Nesting built to exhaust the stack, reading and writing.
    let deep = some_chain(HOSTILE_DEPTH);
    assert!(read::<IgnoredAny>(&deep).is_err());
    dismantle(deep);
    let deep = array_chain(HOSTILE_DEPTH);
    assert!(read::<ScriptValue>(&deep).is_err());
    dismantle(deep);
    assert!(to_record(&Deep(HOSTILE_DEPTH), &mut IdTable::default()).is_err());
}
