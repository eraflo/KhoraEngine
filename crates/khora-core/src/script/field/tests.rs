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

//! What a Rust field type is to a script: its Ergon spelling, the trip to a
//! `ScriptValue` and back, and the values a field refuses.
//!
//! The spellings were once decided by parsing the `stringify!` form of a
//! field's type in the mirror generator; they are pinned here, through the
//! trait that replaced the parser, with the same facts.

use std::fmt::Debug;
use std::marker::PhantomData;

use super::{ErgonType, ScriptField};
use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use crate::script::ScriptValue;

/// The Ergon spelling of `T`.
fn spelled<T: ScriptField>() -> String {
    T::ergon().to_string()
}

/// `value` sent to a script and read back.
fn round_trip<T: ScriptField + PartialEq + Debug>(value: T) {
    let script = value.to_script();
    let back = T::from_script(&script)
        .unwrap_or_else(|error| panic!("{value:?} came back refused: {error:?} (as {script:?})"));
    assert_eq!(
        back, value,
        "{value:?} did not survive the trip as {script:?}"
    );
}

// ─── Whether a type is a script field at all ────────────────────────────────

/// Says `false` for any type…
trait NotAScriptField {
    const IS: bool = false;
}
impl<T> NotAScriptField for T {}

/// …and `true`, through the inherent constant, for one that implements
/// `ScriptField`: the inherent item wins when its bound holds.
struct Probe<T>(PhantomData<T>);
impl<T: ScriptField> Probe<T> {
    const IS: bool = true;
}

// ─── Spellings ──────────────────────────────────────────────────────────────

#[test]
fn the_primitives_spell_as_ergon_writes_them() {
    assert_eq!(spelled::<f32>(), "float");
    assert_eq!(spelled::<bool>(), "bool");
    assert_eq!(spelled::<String>(), "string");
    assert_eq!(spelled::<EntityId>(), "Entity");
    assert_eq!(spelled::<Quaternion>(), "Quat");
    assert_eq!(spelled::<LinearRgba>(), "Color");
}

/// Ergon has one integer, 64-bit signed; every narrower width fits it.
#[test]
fn every_integer_that_fits_an_int_is_an_int() {
    for (rust, ergon) in [
        ("i8", i8::ergon()),
        ("i16", i16::ergon()),
        ("i32", i32::ergon()),
        ("i64", i64::ergon()),
        ("u8", u8::ergon()),
        ("u16", u16::ergon()),
        ("u32", u32::ergon()),
    ] {
        assert_eq!(ergon, ErgonType::Int, "{rust}");
        assert_eq!(ergon.to_string(), "int", "{rust}");
    }
}

/// `Vec3` is an engine type, not a `Vec` of `3`.
#[test]
fn the_vectors_are_engine_types_not_collections() {
    assert_eq!(Vec3::ergon(), ErgonType::Engine("Vec3"));
    assert_eq!(spelled::<Vec2>(), "Vec2");
    assert_eq!(spelled::<Vec3>(), "Vec3");
    assert_eq!(spelled::<Vec4>(), "Vec4");
}

/// The wrappers compose: an `Option` is `T?`, a `Vec` is `T[]`, nested as
/// written.
#[test]
fn optionals_and_arrays_spell_their_element() {
    assert_eq!(
        Vec::<EntityId>::ergon(),
        ErgonType::Array(Box::new(ErgonType::Entity))
    );
    assert_eq!(spelled::<Vec<EntityId>>(), "Entity[]");
    assert_eq!(
        Option::<i32>::ergon(),
        ErgonType::Optional(Box::new(ErgonType::Int))
    );
    assert_eq!(spelled::<Option<i32>>(), "int?");
    assert_eq!(spelled::<Vec<Vec<Vec3>>>(), "Vec3[][]");
    assert_eq!(spelled::<Option<Vec<i64>>>(), "int[]?");
    assert_eq!(spelled::<Vec<Option<f32>>>(), "float?[]");
}

/// **A number that would not survive the trip is refused** — by the type not
/// implementing the trait. Ergon's `int` is 64-bit signed and its `float`
/// 32-bit, so a `u64`, a `usize` or an `f64` read back could differ from what
/// was written; a field that lies is worse than one a script cannot see. A
/// wrapper over such a type is refused with it.
#[test]
fn a_width_that_would_truncate_is_not_a_script_field() {
    let refused = [
        ("u64", Probe::<u64>::IS),
        ("usize", Probe::<usize>::IS),
        ("f64", Probe::<f64>::IS),
        ("Option<u64>", Probe::<Option<u64>>::IS),
        ("Vec<f64>", Probe::<Vec<f64>>::IS),
        ("Vec<[u32; 2]>", Probe::<Vec<[u32; 2]>>::IS),
        ("Option<ScriptValue>", Probe::<Option<ScriptValue>>::IS),
    ];
    for (rust, is) in refused {
        assert!(!is, "`{rust}` must not be a script field");
    }

    // And the probe does tell the two apart.
    let accepted = [
        ("f32", Probe::<f32>::IS),
        ("Vec<Option<EntityId>>", Probe::<Vec<Option<EntityId>>>::IS),
    ];
    for (rust, is) in accepted {
        assert!(is, "`{rust}` is a script field");
    }
}

// ─── The trip ───────────────────────────────────────────────────────────────

#[test]
fn script_field_round_trips_every_supported_type() {
    round_trip(1.5f32);
    round_trip(-0.0f32);
    round_trip(f32::MAX);
    round_trip(i64::MIN);
    round_trip(i64::MAX);
    round_trip(i8::MIN);
    round_trip(i16::MAX);
    round_trip(i32::MIN);
    round_trip(u8::MAX);
    round_trip(u16::MAX);
    round_trip(u32::MAX);
    round_trip(true);
    round_trip(false);
    round_trip(String::from("Guard"));
    round_trip(String::new());
    round_trip(EntityId {
        index: 7,
        generation: 3,
    });
    round_trip(Vec2::new(1.0, -2.0));
    round_trip(Vec3::new(1.0, 2.0, 3.0));
    round_trip(Vec4::new(1.0, 2.0, 3.0, 4.0));
    round_trip(Quaternion::new(0.0, 1.0, 0.0, 0.0));
    round_trip(LinearRgba::new(0.25, 0.5, 0.75, 1.0));
    round_trip(Some(5i32));
    round_trip(None::<i32>);
    round_trip(vec![
        EntityId {
            index: 1,
            generation: 1,
        },
        EntityId {
            index: 2,
            generation: 4,
        },
    ]);
    round_trip(Vec::<Vec3>::new());
    round_trip(vec![Some(1.0f32), None]);
    round_trip(Some(vec![String::from("a"), String::from("b")]));
}

/// The script shapes are the ones the rest of the boundary speaks: an integer
/// of any width is an `Int`, the absent optional is `Null`, a list an `Array`.
#[test]
fn a_field_is_sent_in_the_shape_a_script_reads() {
    assert_eq!(7u8.to_script(), ScriptValue::Int(7));
    assert_eq!((-3i16).to_script(), ScriptValue::Int(-3));
    assert_eq!(2.5f32.to_script(), ScriptValue::Float(2.5));
    assert_eq!(
        String::from("hi").to_script(),
        ScriptValue::Str("hi".into())
    );
    assert_eq!(None::<f32>.to_script(), ScriptValue::Null);
    assert_eq!(Some(2.5f32).to_script(), ScriptValue::Float(2.5));
    assert_eq!(
        vec![1u32, 2].to_script(),
        ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(2)])
    );
    assert_eq!(
        Vec3::new(1.0, 2.0, 3.0).to_script(),
        ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0))
    );
}

// ─── Refusals ───────────────────────────────────────────────────────────────

/// A NaN mass is not a mass, and an infinite speed is not a speed.
#[test]
fn a_non_finite_float_is_refused() {
    assert!(f32::from_script(&ScriptValue::Float(f32::NAN)).is_err());
    assert!(f32::from_script(&ScriptValue::Float(f32::INFINITY)).is_err());
    assert!(f32::from_script(&ScriptValue::Float(f32::NEG_INFINITY)).is_err());
    assert_eq!(f32::from_script(&ScriptValue::Float(1.5)), Ok(1.5));
    // Inside a wrapper too.
    assert!(Option::<f32>::from_script(&ScriptValue::Float(f32::NAN)).is_err());
    assert!(Vec::<f32>::from_script(&ScriptValue::Array(vec![
        ScriptValue::Float(1.0),
        ScriptValue::Float(f32::NAN),
    ]))
    .is_err());
}

/// An `int` is 64-bit; a narrower field takes only what it can hold, and
/// says so rather than wrapping.
#[test]
fn an_integer_out_of_range_is_refused() {
    assert!(u8::from_script(&ScriptValue::Int(300)).is_err());
    assert!(u8::from_script(&ScriptValue::Int(-1)).is_err());
    assert_eq!(u8::from_script(&ScriptValue::Int(255)), Ok(255));
    assert_eq!(u8::from_script(&ScriptValue::Int(0)), Ok(0));
    assert!(i8::from_script(&ScriptValue::Int(128)).is_err());
    assert_eq!(i8::from_script(&ScriptValue::Int(-128)), Ok(-128));
    assert!(u32::from_script(&ScriptValue::Int(i64::from(u32::MAX) + 1)).is_err());
    assert!(i32::from_script(&ScriptValue::Int(i64::from(i32::MIN) - 1)).is_err());
    assert!(u16::from_script(&ScriptValue::Int(-5)).is_err());
}

/// A value of another kind is refused, naming both: the field's type and the
/// value's.
#[test]
fn a_value_of_another_kind_is_refused_naming_both() {
    let error = f32::from_script(&ScriptValue::Bool(true)).expect_err("a bool is not a float");
    assert_eq!(error.expected, "float");
    assert_eq!(error.found, "bool");

    assert!(bool::from_script(&ScriptValue::Int(1)).is_err());
    assert!(String::from_script(&ScriptValue::Unit).is_err());
    assert!(Vec3::from_script(&ScriptValue::Vec2(Vec2::ZERO)).is_err());
    assert!(i64::from_script(&ScriptValue::Float(1.0)).is_err());
    assert!(Vec::<i64>::from_script(&ScriptValue::Int(1)).is_err());
    assert!(
        Vec::<i64>::from_script(&ScriptValue::Array(vec![ScriptValue::Str("x".into())])).is_err()
    );
    // `Null` is an optional's absence, never a value of a plain field.
    assert!(f32::from_script(&ScriptValue::Null).is_err());
    assert_eq!(Option::<f32>::from_script(&ScriptValue::Null), Ok(None));
}

/// Ergon's `float` accepts an `int` — `float m = 1;` holds an integer — so a
/// float field takes one as that float. The widening goes one way only: an
/// int field still refuses a float (above).
#[test]
fn a_float_field_takes_an_int_as_that_float() {
    assert_eq!(f32::from_script(&ScriptValue::Int(3)), Ok(3.0));
    assert_eq!(f32::from_script(&ScriptValue::Int(-2)), Ok(-2.0));
}
