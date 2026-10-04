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

//! Tests of the scene encodings, on records built by hand — no world involved,
//! so a failure here is the encoding's and nobody else's.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;
use crate::scene::record::{
    from_record, to_record, EntityRef, Record, RecordError, ReferenceReader, ReferenceWriter,
    VariantPayload,
};
use crate::scene::scene_record::PageRecord;

/// The three encodings, each with the name a failure message uses for it.
fn every_encoding() -> [(&'static str, &'static dyn SceneEncoding); 3] {
    [
        ("compact", &CompactEncoding),
        ("text", &TextEncoding),
        ("msgpack", &MsgPackEncoding),
    ]
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

/// A `Vec3`-shaped struct record.
fn vec3(x: f32, y: f32, z: f32) -> Record {
    structure(
        "Vec3",
        vec![
            ("x", Record::F32(x)),
            ("y", Record::F32(y)),
            ("z", Record::F32(z)),
        ],
    )
}

/// A transform record shaped like the one a scene holds.
fn transform(seed: f32) -> Record {
    structure(
        "SerializableTransform",
        vec![
            ("translation", vec3(seed * 1.37, -seed * 0.5, seed + 0.25)),
            (
                "rotation",
                structure(
                    "Quaternion",
                    vec![
                        ("x", Record::F32(0.0)),
                        ("y", Record::F32(0.38268343)),
                        ("z", Record::F32(0.0)),
                        ("w", Record::F32(0.9238795)),
                    ],
                ),
            ),
            ("scale", vec3(1.0, 1.0, 1.0)),
        ],
    )
}

/// A scene of `count` entities, each holding one transform: one page.
fn transforms_scene(count: u64) -> SceneRecord {
    let rows: Vec<PersistentId> = (0..count)
        .map(|n| PersistentId::authored(0x5eed_0000_0000 + n * 7919))
        .collect();
    SceneRecord {
        entities: rows.clone(),
        instances: Vec::new(),
        pages: vec![PageRecord {
            components: vec!["Transform".into()],
            rows,
            columns: vec![(0..count).map(|n| transform(n as f32)).collect()],
        }],
    }
}

/// One value of every kind a record can hold, nested.
fn every_kind() -> Record {
    let texture = AssetUUID::new_v5("textures/wall.png");
    structure(
        "Everything",
        vec![
            ("unit", Record::Unit),
            ("yes", Record::Bool(true)),
            ("no", Record::Bool(false)),
            ("negative", Record::I64(i64::MIN)),
            ("big", Record::U64(u64::MAX)),
            ("small", Record::U64(0)),
            ("single", Record::F32(0.1)),
            ("double", Record::F64(-2.5e-300)),
            ("letter", Record::Char('λ')),
            ("text", Record::Str("a line\nand \"quotes\"".into())),
            ("empty_text", Record::Str(String::new())),
            ("bytes", Record::Bytes(vec![0, 255, 7, 128])),
            ("absent", Record::None),
            ("present", Record::Some(Box::new(Record::I64(3)))),
            ("nested_option", Record::Some(Box::new(Record::None))),
            (
                "list",
                Record::Seq(vec![
                    Record::I64(1),
                    Record::Seq(vec![]),
                    vec3(1.0, 2.0, 3.0),
                ]),
            ),
            (
                "map",
                Record::Map(vec![
                    (Record::Str("key".into()), Record::U64(1)),
                    (Record::U64(9), Record::Str("value".into())),
                ]),
            ),
            (
                "marker",
                Record::UnitStruct {
                    name: "Marker".into(),
                },
            ),
            (
                "pair",
                Record::TupleStruct {
                    name: "Pair".into(),
                    fields: vec![Record::F32(1.0), Record::F32(2.0)],
                },
            ),
            (
                "wrapped",
                Record::Newtype {
                    name: "Meters".into(),
                    value: Box::new(Record::F32(3.0)),
                },
            ),
            ("idle", variant("Mode", "Idle", VariantPayload::Unit)),
            (
                "sphere",
                variant(
                    "Shape",
                    "Sphere",
                    VariantPayload::Newtype(Box::new(Record::F32(0.5))),
                ),
            ),
            (
                "capsule",
                variant(
                    "Shape",
                    "Capsule",
                    VariantPayload::Tuple(vec![Record::F32(1.0), Record::F32(0.25)]),
                ),
            ),
            (
                "point",
                variant(
                    "Light",
                    "Point",
                    VariantPayload::Struct(vec![("range".into(), Record::F32(12.0))]),
                ),
            ),
            (
                "someone",
                Record::Entity(EntityRef::Id(PersistentId::authored(77))),
            ),
            (
                "spawned",
                Record::Entity(EntityRef::Id(PersistentId::created(3))),
            ),
            ("nobody", Record::Entity(EntityRef::Outside)),
            ("texture", Record::Asset(texture)),
            // Two structs sharing a name but not a field list: the shape table
            // must not assume one name means one layout.
            ("short", structure("Vec3", vec![("x", Record::F32(4.0))])),
        ],
    )
}

/// A scene holding every kind of value, on entities of both namespaces:
/// one entity in two pages, one in none, a page of several rows listed in
/// another order than the entities.
fn every_kind_scene() -> SceneRecord {
    let first = PersistentId::authored(0x0123_4567_89ab_cdef);
    let bare = PersistentId::created(0);
    let last = PersistentId::created(u64::MAX >> 1);
    let other = PersistentId::authored(5);
    let marker = || Record::UnitStruct {
        name: "Marker".into(),
    };
    SceneRecord {
        entities: vec![first, bare, last, other],
        instances: Vec::new(),
        pages: vec![
            PageRecord {
                components: vec!["Everything".into(), "Transform".into()],
                rows: vec![first],
                columns: vec![vec![every_kind()], vec![transform(2.0)]],
            },
            PageRecord {
                components: vec!["Transform".into()],
                rows: vec![last, other],
                columns: vec![vec![transform(-1.0), transform(0.5)]],
            },
            PageRecord {
                components: vec!["Marker".into()],
                rows: vec![other, first],
                columns: vec![vec![marker(), marker()]],
            },
        ],
    }
}

/// A page with components but no rows: columns of nothing.
fn empty_page_scene() -> SceneRecord {
    SceneRecord {
        entities: vec![PersistentId::authored(9)],
        instances: Vec::new(),
        pages: vec![PageRecord {
            components: vec!["Transform".into(), "Marker".into()],
            rows: vec![],
            columns: vec![vec![], vec![]],
        }],
    }
}

/// How many times `needle` appears in `haystack`.
fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// The compact encoding carries the record's names in its own tables, so it
/// owes back exactly the record it was given — every kind of value, both
/// identity namespaces, and two structs that share a name but not a layout.
#[test]
fn the_compact_encoding_keeps_every_record_exactly() {
    for record in [
        SceneRecord::default(),
        every_kind_scene(),
        transforms_scene(3),
        empty_page_scene(),
    ] {
        let bytes = CompactEncoding.encode(&record).expect("a record encodes");
        let back = CompactEncoding
            .decode(&bytes)
            .expect("its own output decodes");
        assert_eq!(back, record, "the compact encoding changed the record");
    }
}

/// Every encoding keeps a record's layout: the entities in order, and each
/// page's components, its rows in order and one value per row in each column.
/// (The values themselves are the next test's business: a self-describing
/// form keeps what a value means, not the width it was held at.)
#[test]
fn every_encoding_keeps_pages_rows_and_columns() {
    for record in [every_kind_scene(), transforms_scene(5), empty_page_scene()] {
        for (name, encoding) in every_encoding() {
            let bytes = encoding
                .encode(&record)
                .unwrap_or_else(|e| panic!("{name} does not encode: {e}"));
            let back = encoding
                .decode(&bytes)
                .unwrap_or_else(|e| panic!("{name} does not decode its own output: {e}"));
            assert_eq!(back.entities, record.entities, "{name}: the entities");
            assert_eq!(back.pages.len(), record.pages.len(), "{name}: the pages");
            for (page, expected) in back.pages.iter().zip(&record.pages) {
                assert_eq!(page.components, expected.components, "{name}: a signature");
                assert_eq!(page.rows, expected.rows, "{name}: a page's rows");
                assert_eq!(
                    page.columns.iter().map(Vec::len).collect::<Vec<_>>(),
                    expected.columns.iter().map(Vec::len).collect::<Vec<_>>(),
                    "{name}: a page's columns"
                );
            }
        }
    }
}

/// A scene of a hundred transforms says `translation` a hundred times; the
/// compact encoding writes it once, in its symbol table, and every value after
/// that by position. That is what makes it compact — a quarter of the text
/// form at most on this scene.
#[test]
fn the_compact_encoding_writes_each_name_once() {
    let record = transforms_scene(100);
    let compact = CompactEncoding.encode(&record).expect("compact encodes");
    let text = TextEncoding.encode(&record).expect("text encodes");

    for name in [
        &b"translation"[..],
        b"rotation",
        b"scale",
        b"SerializableTransform",
        b"Quaternion",
    ] {
        assert_eq!(
            occurrences(&compact, name),
            1,
            "`{}` must be written exactly once in the compact form",
            String::from_utf8_lossy(name)
        );
    }
    assert!(
        compact.len() * 4 <= text.len(),
        "compact ({} bytes) should be at most a quarter of the text form ({} bytes)",
        compact.len(),
        text.len()
    );
}

/// The text encoding is for people: pretty JSON in which a scene reads as the
/// pages it is stored in. The entities are listed by identity; a page lists
/// its rows' identities and keys its columns by component type name, one value
/// per row, fields by field name; a float keeps the decimal it was written as,
/// and the two reference kinds carry the markers that tell them apart from
/// data.
#[test]
fn the_text_encoding_is_readable_json() {
    let texture = AssetUUID::new_v5("textures/wall.png");
    let record = SceneRecord {
        entities: vec![PersistentId::authored(42), PersistentId::created(3)],
        instances: Vec::new(),
        pages: vec![PageRecord {
            components: vec!["Transform".into(), "Holder".into()],
            rows: vec![PersistentId::authored(42)],
            columns: vec![
                vec![structure(
                    "SerializableTransform",
                    vec![("translation", vec3(0.1, 2.0, -3.5))],
                )],
                vec![structure(
                    "SerializableHolder",
                    vec![
                        (
                            "target",
                            Record::Entity(EntityRef::Id(PersistentId::authored(7))),
                        ),
                        ("gone", Record::Entity(EntityRef::Outside)),
                        ("texture", Record::Asset(texture)),
                        ("mode", variant("Mode", "Idle", VariantPayload::Unit)),
                        (
                            "shape",
                            variant(
                                "Shape",
                                "Sphere",
                                VariantPayload::Newtype(Box::new(Record::F32(0.5))),
                            ),
                        ),
                        ("label", Record::None),
                        ("nick", Record::Some(Box::new(Record::Str("x".into())))),
                    ],
                )],
            ],
        }],
    };

    let bytes = TextEncoding.encode(&record).expect("text encodes");
    let text = std::str::from_utf8(&bytes).expect("the text encoding is UTF-8");
    assert!(
        text.contains('\n'),
        "the text form is pretty-printed:\n{text}"
    );
    assert!(
        text.contains("0.1") && !text.contains("0.10000000149011612"),
        "an f32 0.1 is written as `0.1`:\n{text}"
    );

    let json: serde_json::Value = serde_json::from_str(text).expect("the text form is JSON");
    assert_eq!(
        json["entities"],
        json!([42, PersistentId::created(3).to_bits()]),
        "the entities by identity:\n{text}"
    );
    let page = &json["pages"][0];
    assert_eq!(
        page["rows"],
        json!([42]),
        "a page's rows by identity:\n{text}"
    );

    let columns = &page["columns"];
    assert!(
        columns["Transform"][0]["translation"]["x"].is_number(),
        "columns by type name, a value per row, fields by field name:\n{text}"
    );
    assert_eq!(columns["Transform"][0]["translation"]["y"], json!(2.0));
    assert_eq!(
        columns["Transform"].as_array().map(Vec::len),
        Some(1),
        "one value per row:\n{text}"
    );

    let holder = &columns["Holder"][0];
    assert_eq!(holder["target"], json!({ "$entity": 7 }));
    assert_eq!(holder["gone"], json!({ "$entity": null }));
    assert_eq!(
        holder["texture"],
        json!({ "$asset": serde_json::to_value(texture).expect("a uuid is JSON") })
    );
    assert_eq!(holder["mode"], json!("Idle"));
    assert_eq!(holder["shape"], json!({ "Sphere": 0.5 }));
    assert_eq!(holder["label"], json!(null));
    assert_eq!(holder["nick"], json!("x"));
}

/// The entity table a test value is written and read through: entity `n` is
/// the authored identity `n`, and back.
struct ByIndex;

impl ReferenceWriter for ByIndex {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        EntityRef::Id(PersistentId::authored(entity.index as u64))
    }
}

impl ReferenceReader for ByIndex {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        match reference {
            EntityRef::Id(id) => Ok(EntityId {
                index: id.to_bits() as u32,
                generation: 0,
            }),
            EntityRef::Outside => Err(RecordError("an entity outside the scene".into())),
        }
    }
}

/// A mode, written as a unit variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Mode {
    Idle,
    Chase { speed: f32 },
    Aim(f32, f32),
    Track(EntityId),
}

/// A component-like value holding every shape a field can take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Probe {
    speed: f32,
    precise: f64,
    count: i32,
    flags: u64,
    target: EntityId,
    crowd: Vec<EntityId>,
    texture: AssetUUID,
    label: Option<String>,
    nick: Option<String>,
    modes: Vec<Mode>,
    by_slot: BTreeMap<u32, String>,
    pair: (u8, bool),
}

fn probe() -> Probe {
    let at = |index| EntityId {
        index,
        generation: 0,
    };
    Probe {
        speed: 0.1,
        precise: 0.1,
        count: -17,
        flags: u64::MAX,
        target: at(4),
        crowd: vec![at(1), at(2), at(4)],
        texture: AssetUUID::new_v5("textures/wall.png"),
        label: None,
        nick: Some("Ace".into()),
        modes: vec![
            Mode::Idle,
            Mode::Chase { speed: 2.5 },
            Mode::Aim(0.3, -1.0),
            Mode::Track(at(2)),
        ],
        by_slot: BTreeMap::from([(3, "three".into()), (10, "ten".into())]),
        pair: (200, true),
    }
}

/// Struct and variant names are gone from the self-describing encodings — a
/// struct is a map, a variant a key. What they still owe is that a value
/// written through them reads back as itself, matched by name: floats at full
/// precision, integer map keys from the strings JSON wrote them as, entities
/// and assets as references.
#[test]
fn every_encoding_reads_a_value_back_by_name() {
    let value = probe();
    let record = SceneRecord {
        entities: vec![PersistentId::authored(1)],
        instances: Vec::new(),
        pages: vec![PageRecord {
            components: vec!["Probe".into()],
            rows: vec![PersistentId::authored(1)],
            columns: vec![vec![
                to_record(&value, &mut ByIndex).expect("the probe writes")
            ]],
        }],
    };

    for (name, encoding) in every_encoding() {
        let bytes = encoding
            .encode(&record)
            .unwrap_or_else(|e| panic!("{name} does not encode: {e}"));
        let back = encoding
            .decode(&bytes)
            .unwrap_or_else(|e| panic!("{name} does not decode its own output: {e}"));
        assert_eq!(back.entities, vec![PersistentId::authored(1)], "{name}");
        assert_eq!(back.pages.len(), 1, "{name}");
        let page = &back.pages[0];
        assert_eq!(page.rows, vec![PersistentId::authored(1)], "{name}");
        assert_eq!(page.components, vec!["Probe".to_owned()], "{name}");
        let read: Probe = from_record(&page.columns[0][0], &mut ByIndex)
            .unwrap_or_else(|e| panic!("{name}: the probe does not read back: {e}"));
        assert_eq!(read, value, "{name} changed the value");
    }
}

/// A file is input: cut short anywhere — a write interrupted, a download
/// truncated — it must be refused, never half-read and never a panic.
#[test]
fn a_damaged_file_is_an_error_not_a_panic() {
    let record = every_kind_scene();
    for (name, encoding) in every_encoding() {
        let bytes = encoding
            .encode(&record)
            .unwrap_or_else(|e| panic!("{name} does not encode: {e}"));
        for cut in 0..bytes.len() {
            // A text cut that only drops trailing whitespace is still the
            // whole document.
            if name == "text" && bytes[cut..].iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let prefix = &bytes[..cut];
            let outcome = catch_unwind(AssertUnwindSafe(|| encoding.decode(prefix)));
            match outcome {
                Ok(Err(_)) => {}
                Ok(Ok(_)) => panic!(
                    "{name}: a file cut at {cut} of {} bytes was accepted",
                    bytes.len()
                ),
                Err(_) => panic!(
                    "{name}: a file cut at {cut} of {} bytes panicked",
                    bytes.len()
                ),
            }
        }
    }
}

/// A count read from a file is a claim, not a fact. A damaged byte can claim
/// billions of entries; the decoder must check the claim against what is left
/// of the input before it allocates for it, and refuse — not abort, not panic.
#[test]
fn a_huge_declared_count_is_refused_not_allocated() {
    let record = every_kind_scene();
    for (name, encoding) in every_encoding() {
        let bytes = encoding
            .encode(&record)
            .unwrap_or_else(|e| panic!("{name} does not encode: {e}"));

        // Every byte in turn turned into a varint continuation, and a maximal
        // varint spliced in at every position.
        for at in 0..bytes.len() {
            let mut flipped = bytes.clone();
            flipped[at] = 0xFF;
            let outcome = catch_unwind(AssertUnwindSafe(|| encoding.decode(&flipped)));
            assert!(
                outcome.is_ok(),
                "{name}: byte {at} set to 0xFF made the decoder panic"
            );

            let mut spliced = bytes.clone();
            spliced.splice(at..at, [0xFF; 9].into_iter().chain([0x01]));
            let outcome = catch_unwind(AssertUnwindSafe(|| encoding.decode(&spliced)));
            assert!(
                outcome.is_ok(),
                "{name}: a huge varint spliced at {at} made the decoder panic"
            );
        }
    }

    for garbage in [&[0xFF_u8; 64][..], &[], &[0xDD, 0xFF, 0xFF, 0xFF, 0xFF]] {
        for (name, encoding) in every_encoding() {
            let outcome = catch_unwind(AssertUnwindSafe(|| encoding.decode(garbage)));
            assert!(
                matches!(outcome, Ok(Err(_))),
                "{name}: {garbage:?} must be refused without a panic"
            );
        }
    }
}
