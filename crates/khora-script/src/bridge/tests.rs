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

//! Bridge tests.
//!
//! The round trips are generated from the same table the bridge is, so a row
//! added there is a row tested here — a test that has to be remembered is a test
//! that will not be.

use super::*;
use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

/// A distinguishable value per row, so a conversion that swapped two variants
/// would be caught rather than passing on a zero that matches anything.
macro_rules! define_samples {
    ($($variant:ident : $rust:ty ;)*) => {
        fn samples() -> Vec<ScriptValue> {
            vec![
                ScriptValue::Bool(true),
                ScriptValue::Int(-7),
                ScriptValue::Float(2.5),
                ScriptValue::Entity(EntityId { index: 3, generation: 2 }),
                ScriptValue::Vec2(Vec2::new(1.0, 2.0)),
                ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
                ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
                ScriptValue::Quat(Quaternion::new(0.0, 0.0, 0.0, 1.0)),
                ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 1.0)),
            ]
        }

        /// One name per row, so the count assertion below reads as a list.
        const ROWS: &[&str] = &[$(stringify!($variant),)*];
    };
}
khora_core::script_value_table!(define_samples);

// ─── The round trips ────────────────────────────────────────────────────────

/// **The guarantee.** Every regular value survives boundary → register →
/// boundary unchanged.
#[test]
fn every_regular_value_survives_a_register() {
    let mut arena = Arena::new();

    for original in samples() {
        let register = to_register(&original, &mut arena).expect("has a register form");
        let back = from_register(register, &[], &arena).expect("has a boundary form");
        assert_eq!(back, original, "{original:?} did not survive a register");
    }
}

/// And boundary → store → boundary.
#[test]
fn every_regular_value_survives_a_saved_slot() {
    for original in samples() {
        let stored = to_persisted(&original).expect("has a stored form");
        let back = from_persisted(&stored)
            .expect("has a boundary form")
            .expect("a written slot");
        assert_eq!(back, original, "{original:?} did not survive a save");
    }
}

/// The samples and the table must stay in step: a row added without a sample
/// would silently test nothing.
#[test]
fn there_is_one_sample_per_row() {
    assert_eq!(samples().len(), ROWS.len(), "rows: {ROWS:?}");
}

// ─── Text, the irregular one that matters ───────────────────────────────────

/// **What the old conversion could not do.** A boundary string has no index in
/// the running program, so it has to be allocated — which is why the version
/// without an arena had to refuse it, and why an event could not carry one.
#[test]
fn text_reaches_a_register_through_the_arena() {
    let mut arena = Arena::new();
    let original = ScriptValue::Str("boss".to_owned());

    let register = to_register(&original, &mut arena).expect("allocated");
    assert!(
        matches!(register, Value::Str(StrRef::Arena(_))),
        "in the arena, not pretending to be a program literal"
    );

    let back = from_register(register, &[], &arena).expect("resolved");
    assert_eq!(back, original);
}

/// Coming back it is **copied**, not referenced — the arena is freed at the end
/// of the frame and the boundary value outlives it.
#[test]
fn text_leaving_a_register_is_owned() {
    let mut arena = Arena::new();
    let register = to_register(&ScriptValue::Str("hit".to_owned()), &mut arena).expect("allocated");
    let back = from_register(register, &[], &arena).expect("resolved");

    arena.reset();

    assert_eq!(back, ScriptValue::Str("hit".to_owned()), "still readable");
}

/// A string a store holds is owned, and reads back as such.
#[test]
fn text_survives_a_saved_slot() {
    let original = ScriptValue::Str("Boss".to_owned());
    let stored = to_persisted(&original).expect("stored");

    assert!(matches!(stored, Persisted::Owned(Object::Str(_))));
    assert_eq!(from_persisted(&stored).expect("read"), Some(original));
}

// ─── What the table says cannot travel ──────────────────────────────────────

/// Refused **by name**. A silent drop is what put a guard at the origin after a
/// reload with nothing said.
#[test]
fn a_list_is_refused_rather_than_dropped() {
    let mut arena = Arena::new();
    let list = ScriptValue::Array(vec![ScriptValue::Int(1)]);

    assert_eq!(
        to_register(&list, &mut arena),
        Err(Unrepresentable::kind("list", "a register"))
    );
    assert_eq!(
        to_persisted(&list),
        Err(Unrepresentable::kind("list", "a saved field"))
    );
}

#[test]
fn a_struct_is_refused_rather_than_dropped() {
    let mut arena = Arena::new();
    let fields = ScriptValue::Struct(vec![("current".to_owned(), ScriptValue::Int(50))]);

    assert!(to_register(&fields, &mut arena).is_err());
    assert!(to_persisted(&fields).is_err());
}

/// **`null` does not cross.** A handler declares `int amount`, never
/// `int? amount`, so an event carrying this would arrive as something no
/// parameter can be.
#[test]
fn null_has_no_boundary_form() {
    let arena = Arena::new();

    assert_eq!(
        from_register(Value::Null, &[], &arena),
        Err(Unrepresentable::kind("null", "the engine"))
    );
}

// ─── Written, unwritten, and the difference ─────────────────────────────────

/// **The distinction the whole save path rests on.** An unwritten slot is not a
/// value that happens to be empty: recording it would shadow the default the
/// initialiser is meant to produce.
#[test]
fn an_unwritten_slot_reads_as_nothing() {
    assert_eq!(from_persisted(&Persisted::Scalar(Value::Unit)), Ok(None));
}

/// And a spent `after` reads the same way — it is bookkeeping, not scene data.
#[test]
fn a_spent_countdown_reads_as_nothing() {
    assert_eq!(from_persisted(&Persisted::Scalar(Value::Null)), Ok(None));
}

/// `Unit` at the boundary *is* a value, and stays one — only the store's `Unit`
/// means "unwritten". The asymmetry is deliberate and easy to reintroduce
/// backwards.
#[test]
fn unit_travels_toward_the_vm_but_not_back_from_a_slot() {
    let mut arena = Arena::new();

    assert_eq!(to_register(&ScriptValue::Unit, &mut arena), Ok(Value::Unit));
    assert_eq!(from_persisted(&Persisted::Scalar(Value::Unit)), Ok(None));
}
