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

//! Values of every kind, written by position and read back equal.

use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    ScriptValue, SuspendedMachine, TimerRemaining,
};

use super::*;
use crate::ecs::{SerializableTransform, Transform};

/// Every kind serde's data model has — structs, unit, newtype and tuple
/// structs, tuples, arrays, enums of every variant kind, options (a present
/// `None` included), sequences, maps, strings, bytes, numbers at their
/// extremes — nested in one value, comes back equal.
#[test]
fn every_kind_of_value_round_trips() {
    let scene = rich_scene();
    assert_eq!(round_trip(&scene), scene);
}

/// Numbers at both ends of every width, and around the boundaries a varint
/// changes length at.
#[test]
fn every_number_round_trips() {
    for value in [
        0u64,
        1,
        127,
        128,
        255,
        256,
        16_383,
        16_384,
        u32::MAX.into(),
        u64::MAX,
    ] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [
        0i64,
        -1,
        1,
        -64,
        64,
        -65,
        65,
        i32::MIN.into(),
        i64::MIN,
        i64::MAX,
    ] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [i8::MIN, -1, 0, i8::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [i16::MIN, i16::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [u16::MAX, 0] {
        assert_eq!(round_trip(&value), value);
    }
    let low = Numbers {
        a_i8: 0,
        a_i16: -1,
        a_i32: 1,
        a_i64: -300,
        a_u8: 128,
        a_u16: 300,
        a_u32: 70_000,
        a_u64: 1 << 40,
        a_f32: -0.1,
        a_f64: 1e300,
        flag: false,
        letter: '\u{10FFFF}',
    };
    assert_eq!(round_trip(&low), low);
}

/// Floats come back bit for bit: infinities, negative zero, a subnormal, and
/// NaNs with their payload.
#[test]
fn non_finite_floats_round_trip_bit_exact() {
    for value in [
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        f32::from_bits(1),
        f32::NAN,
        f32::from_bits(0x7fc0_0001),
        f32::from_bits(0xffc0_1234),
    ] {
        assert_eq!(round_trip(&value).to_bits(), value.to_bits(), "{value:?}");
    }
    for value in [
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        f64::from_bits(1),
        f64::NAN,
        f64::from_bits(0x7ff8_0000_0000_0001),
    ] {
        assert_eq!(round_trip(&value).to_bits(), value.to_bits(), "{value:?}");
    }
}

/// Strings of any length and content, including empty and long enough to
/// need a multi-byte length.
#[test]
fn strings_round_trip() {
    for value in [
        String::new(),
        "a".to_owned(),
        "héllo wörld ✓ 🦀".to_owned(),
        "x".repeat(1000),
        "\0nul\0inside".to_owned(),
    ] {
        assert_eq!(round_trip(&value), value);
    }
}

/// An option tells its three states apart, nested: absent, present but
/// holding an absent one, and present all the way.
#[test]
fn options_round_trip() {
    for value in [None, Some(None), Some(Some(3u8))] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [None, Some(Shape::Empty), Some(Shape::Circle(1.0))] {
        assert_eq!(round_trip(&value), value);
    }
}

/// Every variant kind of an enum: unit, newtype, tuple, struct.
#[test]
fn every_variant_kind_round_trips() {
    for value in rich_scene().shapes {
        assert_eq!(round_trip(&value), value);
    }
}

/// Collections: empty and full sequences and maps, sequences of sequences.
#[test]
fn collections_round_trip() {
    let empty: Vec<u32> = Vec::new();
    assert_eq!(round_trip(&empty), empty);
    let many: Vec<u32> = (0..1000).collect();
    assert_eq!(round_trip(&many), many);
    let map: BTreeMap<String, Vec<Shape>> = [
        ("none".to_owned(), vec![]),
        ("some".to_owned(), vec![Shape::Empty, Shape::Rect(1.0, 2.0)]),
    ]
    .into_iter()
    .collect();
    assert_eq!(round_trip(&map), map);
    let keyed: HashMap<i32, Option<String>> = [(-1, None), (2, Some("two".to_owned()))]
        .into_iter()
        .collect();
    assert_eq!(round_trip(&keyed), keyed);
}

/// The engine's own persisted types: a component mirror, and a script's
/// values and runtime snapshot — a sequence frozen part-way, a legacy machine,
/// every script value kind.
#[test]
fn engine_types_round_trip() {
    let transform = SerializableTransform::from(Transform::new(
        Vec3::new(1.5, -2.0, 0.1),
        Quaternion::from_axis_angle(Vec3::Y, 0.75),
        Vec3::new(2.0, 2.0, 2.0),
    ));
    assert_round_trips(&transform);

    let values = vec![
        ScriptValue::Unit,
        ScriptValue::Bool(true),
        ScriptValue::Int(-42),
        ScriptValue::Float(0.1),
        ScriptValue::Str("text".into()),
        ScriptValue::Vec2(Vec2::new(1.0, 2.0)),
        ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
        ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ScriptValue::Quat(Quaternion::from_axis_angle(Vec3::X, 0.5)),
        ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 0.4)),
        ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Array(vec![])]),
        ScriptValue::Struct(vec![("inner".into(), ScriptValue::Bool(false))]),
    ];
    assert_eq!(round_trip(&values), values);

    let frozen = ScriptSnapshot {
        fields: vec![("speed".into(), ScriptValue::Float(3.0))],
        state: Some("Patrol".into()),
        state_fields: vec![("waypoint".into(), ScriptValue::Int(2))],
        timers: vec![TimerRemaining {
            repeating: true,
            interval: 0.5,
            state: None,
            ordinal: 0,
            remaining: Some(0.25),
        }],
        pending: Some(PendingSequence {
            fingerprint: 0xfeed_beef_dead_c0de,
            remaining: 1.5,
            machine: SuspendedMachine::Frozen(FrozenMachine {
                body: PendingBody::Update,
                registers: vec![
                    FrozenValue::Int(-7),
                    FrozenValue::Float(1.25),
                    FrozenValue::Literal("hello".into()),
                    FrozenValue::Null,
                ],
                frames: vec![FrozenFrame {
                    function: "on_update".into(),
                    base: 0,
                    return_pc: 0,
                    result: 0,
                }],
                program_counter: 42,
            }),
        }),
    };
    assert_eq!(round_trip(&frozen), frozen);

    let legacy = ScriptSnapshot {
        pending: Some(PendingSequence {
            fingerprint: 1,
            remaining: 0.0,
            machine: SuspendedMachine::Legacy(vec![1, 2, 3, 0xFF]),
        }),
        ..ScriptSnapshot::default()
    };
    assert_eq!(round_trip(&legacy), legacy);
}

/// No name reaches the bytes: a struct's field names and an enum's variant
/// names are left out — what makes the encoding positional.
#[test]
fn no_name_reaches_the_bytes() {
    let bytes = encode(&rich_scene());
    for name in [
        b"by_name".as_slice(),
        b"numbers",
        b"Poly",
        b"Circle",
        b"points",
        b"Scene",
    ] {
        assert!(
            !bytes.windows(name.len()).any(|window| window == name),
            "`{}` is written in the bytes",
            String::from_utf8_lossy(name)
        );
    }
}

/// Writing appends: what `out` already held is kept, and the value follows.
#[test]
fn writing_appends_to_the_buffer() {
    let alone = encode(&Pair(9, "nine".into()));
    let mut out = vec![0xAB, 0xCD];
    to_positional(&Pair(9, "nine".into()), &mut out, &mut Table::default()).expect("writes");
    assert_eq!(&out[..2], &[0xAB, 0xCD]);
    assert_eq!(&out[2..], alone.as_slice());
}
