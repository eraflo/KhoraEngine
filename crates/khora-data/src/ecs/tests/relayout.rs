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

//! A run-time component's fields change while rows of it exist.
//!
//! When a script is reloaded, the component it declares may gain, lose, rename
//! or retype fields. `World::relayout` rebuilds every row of it field by field,
//! by name: a field of the same name and kind keeps its value, a new or retyped
//! one takes its default, a vanished one is dropped — and the report names
//! both.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field, layout, patch, read, slot};
use crate::ecs::{
    ComponentKey, FieldKind, RegisterError, RelayoutReport, SemanticDomain, Transform, World,
};

fn str_value(text: &str) -> ScriptValue {
    ScriptValue::Str(text.to_owned())
}

/// A world with two entities holding `Mover` — in two different pages, so the
/// relayout has more than one column to rebuild.
fn movers() -> (World, ComponentKey, EntityId, EntityId) {
    let mut world = World::new();
    let mover = declare(
        &mut world,
        "Mover",
        SemanticDomain::Script,
        vec![
            field("hp", FieldKind::Int, ScriptValue::Int(100)),
            field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
            field("label", FieldKind::Value, str_value("none")),
        ],
    );
    let other = declare(
        &mut world,
        "Other",
        SemanticDomain::Script,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );

    let alone = world.spawn(());
    world
        .add_runtime_component(
            alone,
            mover,
            &patch(&[
                ("hp", ScriptValue::Int(7)),
                ("speed", ScriptValue::Float(2.5)),
                ("label", str_value("alone")),
            ]),
        )
        .expect("Mover attaches");

    let paired = world.spawn(());
    world
        .add_runtime_component(paired, other, &ScriptValue::Unit)
        .expect("Other attaches");
    world
        .add_runtime_component(
            paired,
            mover,
            &patch(&[
                ("hp", ScriptValue::Int(9)),
                ("speed", ScriptValue::Float(4.0)),
                ("label", str_value("paired")),
            ]),
        )
        .expect("Mover attaches");

    (world, mover, alone, paired)
}

#[test]
fn relayout_keeps_fields_by_name() {
    let (mut world, mover, alone, paired) = movers();

    // `speed` is renamed `velocity`.
    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Int, ScriptValue::Int(100)),
                field("velocity", FieldKind::Float, ScriptValue::Float(1.0)),
                field("label", FieldKind::Value, str_value("none")),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report,
        RelayoutReport {
            dropped: vec!["speed".to_owned()],
            defaulted: vec!["velocity".to_owned()],
        }
    );
    assert_eq!(slot(&world, mover, "speed"), None, "the old name is gone");
    for (entity, hp, label) in [(alone, 7, "alone"), (paired, 9, "paired")] {
        assert_eq!(
            read(&world, entity, mover, "hp"),
            Some(ScriptValue::Int(hp))
        );
        assert_eq!(
            read(&world, entity, mover, "label"),
            Some(str_value(label)),
            "a Value field survives too"
        );
        assert_eq!(
            read(&world, entity, mover, "velocity"),
            Some(ScriptValue::Float(1.0)),
            "the renamed field takes its default"
        );
    }
    assert!(
        world.row(paired, mover).is_some() && world.components().key_named("Mover") == Some(mover),
        "the component keeps its id and its name"
    );
}

#[test]
fn relayout_of_a_retyped_field_takes_its_default() {
    let (mut world, mover, alone, paired) = movers();

    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Int, ScriptValue::Int(100)),
                field("speed", FieldKind::Int, ScriptValue::Int(3)),
                field("label", FieldKind::Value, str_value("none")),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report,
        RelayoutReport {
            dropped: Vec::new(),
            defaulted: vec!["speed".to_owned()],
        }
    );
    for (entity, hp) in [(alone, 7), (paired, 9)] {
        assert_eq!(
            read(&world, entity, mover, "speed"),
            Some(ScriptValue::Int(3)),
            "the retyped field takes its default"
        );
        assert_eq!(
            read(&world, entity, mover, "hp"),
            Some(ScriptValue::Int(hp))
        );
    }
}

#[test]
fn relayout_with_a_new_field_gives_every_row_its_default() {
    let (mut world, mover, alone, paired) = movers();

    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Int, ScriptValue::Int(100)),
                field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
                field("label", FieldKind::Value, str_value("none")),
                field("armor", FieldKind::Int, ScriptValue::Int(5)),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report,
        RelayoutReport {
            dropped: Vec::new(),
            defaulted: vec!["armor".to_owned()],
        }
    );
    for (entity, speed) in [(alone, 2.5), (paired, 4.0)] {
        assert_eq!(
            read(&world, entity, mover, "armor"),
            Some(ScriptValue::Int(5))
        );
        assert_eq!(
            read(&world, entity, mover, "speed"),
            Some(ScriptValue::Float(speed))
        );
    }

    // A row added after the relayout is laid out the new way.
    let late = world.spawn(());
    world
        .add_runtime_component(late, mover, &patch(&[("armor", ScriptValue::Int(8))]))
        .expect("Mover attaches");
    assert_eq!(
        read(&world, late, mover, "armor"),
        Some(ScriptValue::Int(8))
    );
    assert_eq!(read(&world, late, mover, "hp"), Some(ScriptValue::Int(100)));
}

#[test]
fn relayout_of_a_rust_component_is_refused() {
    let mut world = World::new();
    let transform = ComponentKey::of::<Transform>();
    assert!(
        world.components().vtable(transform).is_some(),
        "Transform is registered"
    );

    assert_eq!(
        world.relayout(
            transform,
            layout(vec![field("x", FieldKind::Float, ScriptValue::Float(0.0))]),
        ),
        Err(RegisterError::NotRuntime {
            name: "Transform".to_owned()
        }),
    );
    assert!(
        world
            .components()
            .vtable(transform)
            .expect("Transform's vtable")
            .columns
            .packed()
            .is_none(),
        "Transform is still a Rust component"
    );
}
