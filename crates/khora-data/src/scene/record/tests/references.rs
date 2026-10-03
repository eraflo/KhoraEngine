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

//! Entities and assets inside a value, as references rather than numbers.

use khora_core::asset::{AssetUUID, StandardMaterial};
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    ScriptValue,
};
use serde::{Deserialize, Serialize};

use crate::ecs::{
    Children, Parent, Script, SerializableChildren, SerializableParent, SerializableScript,
};
use crate::ui::{SerializableUiImage, UiImage};

use super::*;

/// Where an entity landed in the world being loaded: somewhere else than it
/// was, as it always is.
fn moved(entity: EntityId) -> EntityId {
    EntityId {
        index: entity.index + 1000,
        generation: entity.generation + 1,
    }
}

/// A reader that places every entity a save names somewhere new.
struct Relocate<'a> {
    table: &'a IdTable,
    read: Vec<EntityRef>,
}

impl ReferenceReader for Relocate<'_> {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        self.read.push(reference);
        match reference {
            EntityRef::Id(id) => self
                .table
                .entity_of(id)
                .map(moved)
                .ok_or_else(|| RecordError(format!("no entity was written as {id:?}"))),
            EntityRef::Outside => Err(RecordError("an entity outside the save".into())),
        }
    }
}

/// Writes `value` with a fresh table, then reads it into a world where every
/// entity moved. Returns the record, the table, what was read and the
/// references the reader was handed.
fn through_hooks<T: Serialize + DeserializeOwned>(
    value: &T,
) -> (Record, IdTable, T, Vec<EntityRef>) {
    let mut table = IdTable::default();
    let record = to_record(value, &mut table).expect("writes");
    let mut reader = Relocate {
        table: &table,
        read: Vec::new(),
    };
    let back: T = from_record(&record, &mut reader).expect("reads");
    let read = reader.read;
    (record, table, back, read)
}

/// The reference the table gave `entity`, as a record.
fn reference(table: &IdTable, entity: EntityId) -> Record {
    Record::Entity(EntityRef::Id(table.id_of(entity)))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Target {
    entity: Option<EntityId>,
    fallback: Option<EntityId>,
}

/// An entity id means nothing outside the world that issued it, so every one
/// inside a saved value — a parent, a list of children, a script value, a
/// frozen register, an optional — goes to the writer on the way out and to
/// the reader on the way back, once per occurrence, wherever it is nested.
/// The value read back holds the entities the reader chose, not the numbers
/// that were written.
#[test]
fn an_entity_reference_goes_through_the_hooks() {
    let a = entity(3, 1);
    let b = entity(8, 0);
    let c = entity(11, 4);

    // A component that is nothing but a reference.
    let (record, table, back, read) = through_hooks(&SerializableParent::from(Parent(a)));
    assert_eq!(
        record,
        Record::Newtype {
            name: "SerializableParent".into(),
            value: Box::new(reference(&table, a)),
        }
    );
    assert_eq!(table.written, vec![a]);
    assert_eq!(read, vec![EntityRef::Id(table.id_of(a))]);
    assert_eq!(back.0, moved(a));

    // A list of them, one repeated: each occurrence is a call.
    let (record, table, back, read) =
        through_hooks(&SerializableChildren::from(Children(vec![a, b, a])));
    assert_eq!(
        record,
        Record::Newtype {
            name: "SerializableChildren".into(),
            value: Box::new(Record::Seq(vec![
                reference(&table, a),
                reference(&table, b),
                reference(&table, a),
            ])),
        }
    );
    assert_eq!(table.written, vec![a, b, a]);
    assert_eq!(read.len(), 3);
    assert_eq!(back.0, vec![moved(a), moved(b), moved(a)]);

    // A script value.
    let (record, table, back, _) = through_hooks(&ScriptValue::Entity(c));
    assert_eq!(
        record,
        variant(
            "ScriptValue",
            "Entity",
            VariantPayload::Newtype(Box::new(reference(&table, c))),
        )
    );
    assert_eq!(back, ScriptValue::Entity(moved(c)));

    // A frozen register.
    let (record, table, back, _) = through_hooks(&FrozenValue::Entity(b));
    assert_eq!(
        record,
        variant(
            "FrozenValue",
            "Entity",
            VariantPayload::Newtype(Box::new(reference(&table, b))),
        )
    );
    assert_eq!(back, FrozenValue::Entity(moved(b)));

    // An optional one, present and absent.
    let (record, table, back, _) = through_hooks(&Target {
        entity: Some(c),
        fallback: None,
    });
    assert_eq!(
        record,
        structure(
            "Target",
            vec![
                ("entity", Record::Some(Box::new(reference(&table, c)))),
                ("fallback", Record::None),
            ],
        )
    );
    assert_eq!(
        back,
        Target {
            entity: Some(moved(c)),
            fallback: None,
        }
    );

    // Deep inside a script component: authored fields nesting arrays and
    // structs, and a frozen machine — which a save holds as an untagged enum,
    // so the reference is read without its type being asked for by name.
    let script = Script {
        module: "scripts/guard.erg".into(),
        behavior: "Guard".into(),
        fields: vec![
            ("leader".into(), ScriptValue::Entity(a)),
            (
                "squad".into(),
                ScriptValue::Array(vec![
                    ScriptValue::Entity(b),
                    ScriptValue::Struct(vec![("medic".into(), ScriptValue::Entity(c))]),
                ]),
            ),
        ],
        runtime: ScriptSnapshot {
            pending: Some(PendingSequence {
                fingerprint: 9,
                remaining: 0.5,
                machine: FrozenMachine {
                    body: PendingBody::Sequence,
                    registers: vec![FrozenValue::Int(1), FrozenValue::Entity(c)],
                    frames: vec![FrozenFrame {
                        function: "on_alarm".into(),
                        base: 0,
                        return_pc: 0,
                        result: 0,
                    }],
                    program_counter: 4,
                },
            }),
            ..ScriptSnapshot::default()
        },
    };
    let (_, table, back, read) = through_hooks(&SerializableScript::from(script));
    assert_eq!(table.written, vec![a, b, c, c]);
    assert_eq!(read.len(), 4);
    let back = Script::from(back);
    assert_eq!(back.field("leader"), Some(&ScriptValue::Entity(moved(a))));
    assert_eq!(
        back.field("squad"),
        Some(&ScriptValue::Array(vec![
            ScriptValue::Entity(moved(b)),
            ScriptValue::Struct(vec![("medic".into(), ScriptValue::Entity(moved(c)))]),
        ]))
    );
    let pending = back.runtime.pending.expect("the pending sequence survives");
    assert_eq!(
        pending.machine.registers,
        vec![FrozenValue::Int(1), FrozenValue::Entity(moved(c))]
    );
}

/// A writer may say an entity is outside what the save records. The record
/// keeps that as such, and the reader is handed it to decide.
#[test]
fn an_entity_outside_the_save_reaches_the_reader_as_outside() {
    struct Outside;
    impl ReferenceWriter for Outside {
        fn write_entity(&mut self, _: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    struct Anywhere(Vec<EntityRef>);
    impl ReferenceReader for Anywhere {
        fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
            self.0.push(reference);
            Ok(entity(0, 0))
        }
    }

    let record = to_record(
        &SerializableParent::from(Parent(entity(4, 4))),
        &mut Outside,
    )
    .expect("writes");
    assert_eq!(
        record,
        Record::Newtype {
            name: "SerializableParent".into(),
            value: Box::new(Record::Entity(EntityRef::Outside)),
        }
    );
    let mut reader = Anywhere(Vec::new());
    let back: SerializableParent = from_record(&record, &mut reader).expect("reads");
    assert_eq!(back.0, entity(0, 0));
    assert_eq!(reader.0, vec![EntityRef::Outside]);
}

/// An asset UUID is already an identity that outlives the world, so it is
/// kept verbatim — as an asset reference, wherever it sits — and never goes
/// through the entity hooks.
#[test]
fn an_asset_uuid_is_kept_verbatim() {
    let texture = AssetUUID::new_v5("ui/button.png");
    let mut table = IdTable::default();

    assert_eq!(to_record(&texture, &mut table), Ok(Record::Asset(texture)));
    assert_eq!(read::<AssetUUID>(&Record::Asset(texture)), Ok(texture));

    let image = SerializableUiImage::from(UiImage { texture });
    let record = to_record(&image, &mut table).expect("writes");
    assert_eq!(
        record,
        structure(
            "SerializableUiImage",
            vec![("texture", Record::Asset(texture))]
        )
    );
    let back: SerializableUiImage = read(&record).expect("reads");
    assert_eq!(back.texture, texture);

    let material = StandardMaterial {
        base_color_texture: Some(texture),
        ..StandardMaterial::default()
    };
    let record = to_record(&material, &mut table).expect("writes");
    assert_eq!(
        field_of(&record, "base_color_texture"),
        &Record::Some(Box::new(Record::Asset(texture)))
    );
    assert_eq!(field_of(&record, "normal_map"), &Record::None);
    let back: StandardMaterial = read(&record).expect("reads");
    assert_eq!(back.base_color_texture, Some(texture));

    assert!(
        table.written.is_empty(),
        "an asset went through the entity hook"
    );
}

/// The editor's inspector reads and writes JSON of these two types. Giving
/// them reserved serde names for the record codec must not change a byte of
/// that JSON, nor of any value holding them.
#[test]
fn json_of_entity_id_is_unchanged() {
    let id = entity(3, 7);
    let json = r#"{"index":3,"generation":7}"#;
    assert_eq!(serde_json::to_string(&id).expect("serializes"), json);
    assert_eq!(serde_json::from_str::<EntityId>(json).expect("parses"), id);
    assert_eq!(
        serde_json::to_string(&SerializableParent::from(Parent(id))).expect("serializes"),
        json
    );

    let uuid_json = r#""67e55044-10b1-426f-9247-bb680e5fe0c8""#;
    let uuid: AssetUUID = serde_json::from_str(uuid_json).expect("parses");
    assert_eq!(serde_json::to_string(&uuid).expect("serializes"), uuid_json);
    assert_eq!(
        serde_json::to_string(&SerializableUiImage::from(UiImage { texture: uuid }))
            .expect("serializes"),
        r#"{"texture":"67e55044-10b1-426f-9247-bb680e5fe0c8"}"#
    );
}
