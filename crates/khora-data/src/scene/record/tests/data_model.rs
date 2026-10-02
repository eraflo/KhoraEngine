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

//! The shape a value takes as a record.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Marker;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Meters(f32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Pair(u8, i16);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Shape {
    Point,
    Circle(f32),
    Segment(f32, f32),
    Rect { w: f32, h: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Everything {
    unit: (),
    flag: bool,
    small: u8,
    signed: i16,
    wide: u64,
    single: f32,
    double: f64,
    letter: char,
    text: String,
    absent: Option<u32>,
    present: Option<i32>,
    list: Vec<u16>,
    tuple: (u32, bool),
    array: [i8; 2],
    names: BTreeMap<String, u32>,
    keyed: BTreeMap<u32, bool>,
    marker: Marker,
    meters: Meters,
    pair: Pair,
    shapes: Vec<Shape>,
}

fn everything() -> Everything {
    Everything {
        unit: (),
        flag: true,
        small: 200,
        signed: -300,
        wide: u64::MAX,
        single: 1.5,
        double: -2.25,
        letter: 'λ',
        text: "hi".into(),
        absent: None,
        present: Some(-4),
        list: vec![1, 2],
        tuple: (7, false),
        array: [-1, 1],
        names: BTreeMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)]),
        keyed: BTreeMap::from([(3, true)]),
        marker: Marker,
        meters: Meters(3.0),
        pair: Pair(1, -2),
        shapes: vec![
            Shape::Point,
            Shape::Circle(1.0),
            Shape::Segment(0.0, 2.0),
            Shape::Rect { w: 3.0, h: 4.0 },
        ],
    }
}

/// A record is serde's data model written down, names included: that is what
/// lets a later read match by name instead of by position. Integers widen to
/// the two 64-bit kinds, an `f32` keeps its own width so it reads back
/// bit-exact, tuples and arrays are sequences, and each struct and enum keeps
/// its serde name and its field and variant names.
#[test]
fn the_serializer_writes_serdes_data_model_by_name() {
    let expected = structure(
        "Everything",
        vec![
            ("unit", Record::Unit),
            ("flag", Record::Bool(true)),
            ("small", Record::U64(200)),
            ("signed", Record::I64(-300)),
            ("wide", Record::U64(u64::MAX)),
            ("single", Record::F32(1.5)),
            ("double", Record::F64(-2.25)),
            ("letter", Record::Char('λ')),
            ("text", text("hi")),
            ("absent", Record::None),
            ("present", Record::Some(Box::new(Record::I64(-4)))),
            ("list", Record::Seq(vec![Record::U64(1), Record::U64(2)])),
            (
                "tuple",
                Record::Seq(vec![Record::U64(7), Record::Bool(false)]),
            ),
            ("array", Record::Seq(vec![Record::I64(-1), Record::I64(1)])),
            (
                "names",
                Record::Map(vec![
                    (text("a"), Record::U64(1)),
                    (text("b"), Record::U64(2)),
                ]),
            ),
            (
                "keyed",
                Record::Map(vec![(Record::U64(3), Record::Bool(true))]),
            ),
            (
                "marker",
                Record::UnitStruct {
                    name: "Marker".into(),
                },
            ),
            (
                "meters",
                Record::Newtype {
                    name: "Meters".into(),
                    value: Box::new(Record::F32(3.0)),
                },
            ),
            (
                "pair",
                Record::TupleStruct {
                    name: "Pair".into(),
                    fields: vec![Record::U64(1), Record::I64(-2)],
                },
            ),
            (
                "shapes",
                Record::Seq(vec![
                    variant("Shape", "Point", VariantPayload::Unit),
                    variant(
                        "Shape",
                        "Circle",
                        VariantPayload::Newtype(Box::new(Record::F32(1.0))),
                    ),
                    variant(
                        "Shape",
                        "Segment",
                        VariantPayload::Tuple(vec![Record::F32(0.0), Record::F32(2.0)]),
                    ),
                    variant(
                        "Shape",
                        "Rect",
                        struct_payload(vec![("w", Record::F32(3.0)), ("h", Record::F32(4.0))]),
                    ),
                ]),
            ),
        ],
    );

    let written = to_record(&everything(), &mut IdTable::default()).expect("writes");
    assert_eq!(written, expected);
}

/// Every shape of the data model reads back as the value that wrote it.
#[test]
fn every_shape_of_the_data_model_reads_back() {
    let value = everything();
    assert_eq!(round_trip(&value), value);
    let () = round_trip(&());
    assert_eq!(round_trip(&'x'), 'x');
    assert_eq!(round_trip(&i64::MIN), i64::MIN);
    assert_eq!(round_trip(&u64::MAX), u64::MAX);
    assert_eq!(
        round_trip(&f32::MIN_POSITIVE).to_bits(),
        f32::MIN_POSITIVE.to_bits()
    );
    assert_eq!(round_trip(&0.1_f64).to_bits(), 0.1_f64.to_bits());
}

/// A byte string is kept as bytes, not spread into a list of numbers.
#[test]
fn a_byte_string_is_kept_as_bytes() {
    struct Raw;
    impl Serialize for Raw {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(&[0, 1, 255])
        }
    }

    assert_eq!(
        to_record(&Raw, &mut IdTable::default()),
        Ok(Record::Bytes(vec![0, 1, 255]))
    );
}

/// A record is a machine format: a type that writes itself differently for
/// humans — a UUID as text, say — must be told it is not writing for one.
#[test]
fn the_serializer_is_not_human_readable() {
    struct Probe;
    impl Serialize for Probe {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let human_readable = serializer.is_human_readable();
            serializer.serialize_bool(human_readable)
        }
    }

    assert_eq!(
        to_record(&Probe, &mut IdTable::default()),
        Ok(Record::Bool(false))
    );
}
