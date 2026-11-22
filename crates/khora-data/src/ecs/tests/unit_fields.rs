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

//! `Duration` and `Angle` fields keep their unit in the field's Ergon type, for
//! the inspector, and store the value as a float, so what was saved as a
//! `float` still reads.

use khora_core::script::{ErgonType, ScriptValue};

use super::runtime_components::{decl, field};
use crate::ecs::{FieldKind, PackedField, PackedLayout, SemanticDomain, Transform, World};

/// A layout of `fields`, each of its Ergon type, each stored as the kind its
/// type is.
fn typed(fields: &[(&str, ErgonType, ScriptValue)]) -> PackedLayout {
    PackedLayout::typed(
        fields
            .iter()
            .map(|(name, ty, default)| PackedField {
                name: (*name).to_owned(),
                kind: FieldKind::of(ty),
                default: default.clone(),
            })
            .collect(),
        fields.iter().map(|(_, ty, _)| ty.clone()).collect(),
    )
    .expect("a valid layout")
}

/// **A duration and an angle are stored as floats**: the kind their storage
/// is, whatever unit their type says.
#[test]
fn a_duration_and_an_angle_are_stored_as_floats() {
    assert_eq!(FieldKind::of(&ErgonType::Duration), FieldKind::Float);
    assert_eq!(FieldKind::of(&ErgonType::Angle), FieldKind::Float);
}

/// **A layout keeps the unit of its fields**: a `Duration` field is typed
/// `Duration`, an `Angle` field `Angle`, and both take a float.
#[test]
fn a_layout_keeps_the_unit_of_a_duration_and_an_angle_field() {
    let layout = typed(&[
        ("wait", ErgonType::Duration, ScriptValue::Float(2.0)),
        ("turn", ErgonType::Angle, ScriptValue::Float(90.0)),
    ]);

    assert_eq!(layout.ty(0), Some(&ErgonType::Duration));
    assert_eq!(layout.ty(1), Some(&ErgonType::Angle));
    assert_eq!(layout.fields()[0].kind, FieldKind::Float);
    assert_eq!(layout.fields()[1].kind, FieldKind::Float);
    assert_eq!(
        layout.admit(0, &ScriptValue::Float(0.5)),
        Some(ScriptValue::Float(0.5))
    );
    assert_eq!(layout.admit(1, &ScriptValue::Str("left".to_owned())), None);
}

/// **A `float` field becoming a `Duration` keeps its values**: the unit is how
/// the value is read, not what is stored.
#[test]
fn a_float_field_becoming_a_duration_keeps_its_values() {
    let mut world = World::new();
    let mut old = decl(
        "Cooldown",
        SemanticDomain::Script,
        vec![field("wait", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    old.layout = typed(&[("wait", ErgonType::Float, ScriptValue::Float(1.0))]);
    let key = world
        .register_runtime_component(old)
        .expect("Cooldown registers");
    let entity = world.spawn(Transform::default());
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![("wait".to_owned(), ScriptValue::Float(2.5))]),
        )
        .expect("Cooldown attaches");

    let report = world
        .relayout(
            key,
            typed(&[("wait", ErgonType::Duration, ScriptValue::Float(1.0))]),
        )
        .expect("Cooldown is relaid out");

    assert!(
        report.defaulted.is_empty(),
        "a float read as a duration is the same value: {:?}",
        report.defaulted
    );
    assert_eq!(
        world
            .declared_row(entity, key)
            .expect("the entity holds Cooldown"),
        vec![("wait".to_owned(), ScriptValue::Float(2.5))]
    );
}

/// A struct type `Timer { t }` whose one field is of type `t`.
fn timer_of(t: ErgonType) -> ErgonType {
    ErgonType::Struct {
        name: "Timer".to_owned(),
        fields: Some(vec![("t".to_owned(), t)]),
    }
}

/// **A `float` inside a struct field becoming a `Duration` keeps its values**,
/// as a top-level one does: the struct's bytes are the same.
#[test]
fn a_float_inside_a_struct_field_becoming_a_duration_keeps_its_values() {
    let mut world = World::new();
    let timer = |t: f32| ScriptValue::Struct(vec![("t".to_owned(), ScriptValue::Float(t))]);
    let mut old = decl(
        "Fuse",
        SemanticDomain::Script,
        vec![field("timer", FieldKind::Value, timer(0.0))],
    );
    old.layout = typed(&[("timer", timer_of(ErgonType::Float), timer(0.0))]);
    let key = world
        .register_runtime_component(old)
        .expect("Fuse registers");
    let entity = world.spawn(Transform::default());
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![("timer".to_owned(), timer(3.5))]),
        )
        .expect("Fuse attaches");

    let report = world
        .relayout(
            key,
            typed(&[("timer", timer_of(ErgonType::Duration), timer(0.0))]),
        )
        .expect("Fuse is relaid out");

    assert!(
        report.defaulted.is_empty(),
        "a struct's float read as a duration is the same value: {:?}",
        report.defaulted
    );
    assert_eq!(
        world
            .declared_row(entity, key)
            .expect("the entity holds Fuse"),
        vec![("timer".to_owned(), timer(3.5))]
    );
}

/// **A value read from a file fits a field strictly**: an `int` becomes a
/// `float` only where it converts exactly, and says it was widened.
#[test]
fn fit_widens_only_an_exact_int_into_a_float_field() {
    let layout = typed(&[
        ("speed", ErgonType::Float, ScriptValue::Float(0.0)),
        ("count", ErgonType::Int, ScriptValue::Int(0)),
    ]);

    assert_eq!(
        layout.fit(0, &ScriptValue::Int(3)),
        Some((ScriptValue::Float(3.0), true))
    );
    assert_eq!(
        layout.fit(0, &ScriptValue::Float(0.5)),
        Some((ScriptValue::Float(0.5), false))
    );
    assert_eq!(
        layout.fit(1, &ScriptValue::Int(3)),
        Some((ScriptValue::Int(3), false))
    );
    // 2^24 + 1 has no exact `f32`.
    assert_eq!(layout.fit(0, &ScriptValue::Int(16_777_217)), None);
    assert_eq!(layout.fit(1, &ScriptValue::Float(0.5)), None);
    assert_eq!(layout.fit(2, &ScriptValue::Int(3)), None);
}

/// **Two layouts are the same when their fields and their types are**: a
/// `float` field and a `Duration` one store alike, yet differ.
#[test]
fn same_as_compares_the_fields_and_their_types() {
    let layout = typed(&[("wait", ErgonType::Float, ScriptValue::Float(1.0))]);
    let timed = typed(&[("wait", ErgonType::Duration, ScriptValue::Float(1.0))]);
    let renamed = typed(&[("delay", ErgonType::Float, ScriptValue::Float(1.0))]);

    assert!(layout.same_as(&layout.clone()));
    assert!(!layout.same_as(&timed));
    assert!(!layout.same_as(&renamed));
}

/// **A script's write rounds an `int` a float cannot hold exactly, at any
/// depth**, where a file's value is refused: 2^24 + 1 written into a
/// `Duration`, a `float?`, a `float[]` or a struct's `float` is kept as its
/// nearest float, 2^24.
#[test]
fn admit_rounds_an_int_a_float_cannot_hold_exactly_at_any_depth() {
    let inexact = ScriptValue::Int(16_777_217);
    let rounded = ScriptValue::Float(16_777_216.0);
    let loot = |ty| ErgonType::Struct {
        name: "Loot".to_owned(),
        fields: Some(vec![("n".to_owned(), ty)]),
    };
    let layout = typed(&[
        ("wait", ErgonType::Duration, ScriptValue::Float(0.0)),
        (
            "spare",
            ErgonType::Optional(Box::new(ErgonType::Float)),
            ScriptValue::Null,
        ),
        (
            "clips",
            ErgonType::Array(Box::new(ErgonType::Float)),
            ScriptValue::Array(Vec::new()),
        ),
        ("loot", loot(ErgonType::Float), ScriptValue::Null),
    ]);
    let in_loot = |value| ScriptValue::Struct(vec![("n".to_owned(), value)]);

    let written = [
        (inexact.clone(), rounded.clone()),
        (inexact.clone(), rounded.clone()),
        (
            ScriptValue::Array(vec![inexact.clone()]),
            ScriptValue::Array(vec![rounded.clone()]),
        ),
        (in_loot(inexact), in_loot(rounded)),
    ];

    for (slot, (value, kept)) in written.into_iter().enumerate() {
        assert_eq!(layout.admit(slot, &value), Some(kept), "slot {slot}");
        assert_eq!(
            layout.fit(slot, &value),
            None,
            "slot {slot}: a file's is refused"
        );
    }
}
