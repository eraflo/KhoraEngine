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
//! `World::relayout` rebuilds every row by field name: a field of the same name
//! and kind keeps its value, a new or retyped one takes its default, a vanished
//! one is dropped, and the report names both, and every value a live row did
//! not keep, by entity, as it was and as the row holds it now.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ErgonType, ScriptValue};

use super::runtime_components::{decl, declare, field, layout, patch, read, slot, write};
use crate::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, LossCause, LostValue, PackedLayout,
    RegisterError, RelayoutReport, SemanticDomain, Transform, World,
};

fn str_value(text: &str) -> ScriptValue {
    ScriptValue::Str(text.to_owned())
}

/// The value `entity`'s row lost in `field`: it held `was`, it holds `now`.
fn lost(
    entity: EntityId,
    field: &str,
    was: ScriptValue,
    now: Option<ScriptValue>,
    cause: LossCause,
) -> LostValue {
    LostValue {
        entity,
        field: field.to_owned(),
        was,
        now,
        cause,
    }
}

/// `report`'s lost values, by entity: the order of pages is not the contract.
fn lost_by_entity(report: &RelayoutReport) -> Vec<LostValue> {
    let mut lost = report.lost.clone();
    lost.sort_by_key(|value| (value.entity.index, value.field.clone()));
    lost
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

    assert_eq!(report.component, "Mover");
    assert_eq!(report.dropped, vec!["speed".to_owned()]);
    assert_eq!(report.defaulted, vec!["velocity".to_owned()]);
    assert_eq!(
        lost_by_entity(&report),
        vec![
            lost(
                alone,
                "speed",
                ScriptValue::Float(2.5),
                None,
                LossCause::FieldDropped
            ),
            lost(
                paired,
                "speed",
                ScriptValue::Float(4.0),
                None,
                LossCause::FieldDropped
            ),
        ],
        "each value of the dropped field, by entity"
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

    assert!(report.dropped.is_empty(), "{report:?}");
    assert_eq!(report.defaulted, vec!["speed".to_owned()]);
    assert_eq!(
        lost_by_entity(&report),
        vec![
            lost(
                alone,
                "speed",
                ScriptValue::Float(2.5),
                Some(ScriptValue::Int(3)),
                LossCause::Retyped
            ),
            lost(
                paired,
                "speed",
                ScriptValue::Float(4.0),
                Some(ScriptValue::Int(3)),
                LossCause::Retyped
            ),
        ],
        "each value the retype voided, with the default the row took"
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
            component: "Mover".to_owned(),
            dropped: Vec::new(),
            defaulted: vec!["armor".to_owned()],
            lost: Vec::new(),
        },
        "a new field loses no value"
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

/// **The int → float widening a reload keeps**: each row's value, converted.
///
/// The other direction stays a retype — see
/// `relayout_of_a_retyped_field_takes_its_default`.
#[test]
fn relayout_keeps_an_int_widened_to_a_float() {
    let (mut world, mover, alone, paired) = movers();

    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Float, ScriptValue::Float(0.5)),
                field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
                field("label", FieldKind::Value, str_value("none")),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report,
        RelayoutReport {
            component: "Mover".to_owned(),
            ..RelayoutReport::default()
        },
        "an exact widening reports nothing"
    );
    for (entity, hp, speed) in [(alone, 7.0, 2.5), (paired, 9.0, 4.0)] {
        assert_eq!(
            read(&world, entity, mover, "hp"),
            Some(ScriptValue::Float(hp)),
            "the int is kept, as the float it names"
        );
        assert_eq!(
            read(&world, entity, mover, "speed"),
            Some(ScriptValue::Float(speed))
        );
    }
}

/// **An `int` a `float` cannot hold exactly is reported**, with the entity, the
/// value it held and the default its row took, as a scene load treats the same
/// value; the other entity's row, exact, is kept and not reported.
#[test]
fn relayout_reports_an_int_a_float_cannot_hold_exactly() {
    let (mut world, mover, alone, paired) = movers();
    // 2^24 + 1: the first int an f32 rounds.
    let inexact = 16_777_217;
    write(&mut world, alone, mover, "hp", ScriptValue::Int(inexact)).expect("an int fits `hp`");

    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Float, ScriptValue::Float(0.5)),
                field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
                field("label", FieldKind::Value, str_value("none")),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report.lost,
        vec![lost(
            alone,
            "hp",
            ScriptValue::Int(inexact),
            Some(ScriptValue::Float(0.5)),
            LossCause::Inexact
        )],
        "{report:?}"
    );
    assert!(report.dropped.is_empty() && report.defaulted.is_empty());
    assert_eq!(
        read(&world, alone, mover, "hp"),
        Some(ScriptValue::Float(0.5)),
        "the row took its default, as a load gives it"
    );
    assert_eq!(
        read(&world, paired, mover, "hp"),
        Some(ScriptValue::Float(9.0))
    );
}

/// **A number a component stops keeping is reported**: a runtime component
/// holds a NaN, and becomes one a scene keeps, whose numbers are finite.
#[test]
fn a_provenance_change_reports_a_number_the_component_stops_keeping() {
    let mut world = World::new();
    let mut ember = decl(
        "Ember",
        SemanticDomain::Script,
        vec![field("heat", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    ember.provenance = ComponentProvenance::Runtime;
    let ember = world
        .register_runtime_component(ember)
        .expect("Ember registers");
    let entity = world.spawn(());
    world
        .add_runtime_component(
            entity,
            ember,
            &patch(&[("heat", ScriptValue::Float(f32::NAN))]),
        )
        .expect("a runtime component keeps a NaN");

    let report = world
        .set_provenance(ember, ComponentProvenance::Authored, false)
        .expect("Ember takes the provenance");

    assert_eq!(report.lost.len(), 1, "{report:?}");
    let value = &report.lost[0];
    assert_eq!(
        (value.entity, value.field.as_str(), value.cause),
        (entity, "heat", LossCause::Refused)
    );
    assert!(matches!(value.was, ScriptValue::Float(v) if v.is_nan()));
    assert_eq!(value.now, Some(ScriptValue::Float(1.0)));
}

/// **The largest `int` is one a `float` cannot hold exactly**: `i64::MAX`
/// rounds to 2^63, so its row is reported and takes the default; `i64::MIN`,
/// -2^63 exactly, is kept.
#[test]
fn relayout_reports_the_largest_int_as_one_a_float_cannot_hold_exactly() {
    let (mut world, mover, alone, paired) = movers();
    write(&mut world, alone, mover, "hp", ScriptValue::Int(i64::MAX)).expect("an int fits `hp`");
    write(&mut world, paired, mover, "hp", ScriptValue::Int(i64::MIN)).expect("an int fits `hp`");

    let report = world
        .relayout(
            mover,
            layout(vec![
                field("hp", FieldKind::Float, ScriptValue::Float(0.5)),
                field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
                field("label", FieldKind::Value, str_value("none")),
            ]),
        )
        .expect("Mover is relaid out");

    assert_eq!(
        report.lost,
        vec![lost(
            alone,
            "hp",
            ScriptValue::Int(i64::MAX),
            Some(ScriptValue::Float(0.5)),
            LossCause::Inexact
        )],
        "{report:?}"
    );
    assert_eq!(
        read(&world, alone, mover, "hp"),
        Some(ScriptValue::Float(0.5))
    );
    assert_eq!(
        read(&world, paired, mover, "hp"),
        Some(ScriptValue::Float(i64::MIN as f32))
    );
}

/// **An `int` in an optional is kept as a load keeps it**: a scene read into
/// a `float?` field keeps the `int?` value `3` as `3.0`, so relaying the field
/// out from `int?` to `float?` keeps it too.
#[test]
fn relayout_keeps_an_optional_int_a_float_optional_holds_as_a_load_does() {
    let optional = |ty| ErgonType::Optional(Box::new(ty));
    let mut world = World::new();
    let mut reach = decl(
        "Reach",
        SemanticDomain::Script,
        vec![field("reach", FieldKind::Value, ScriptValue::Null)],
    );
    reach.layout = PackedLayout::typed(
        vec![field("reach", FieldKind::Value, ScriptValue::Null)],
        vec![optional(ErgonType::Int)],
    )
    .expect("an `int?` layout");
    let reach = world
        .register_runtime_component(reach)
        .expect("Reach registers");
    let entity = world.spawn(());
    world
        .add_runtime_component(entity, reach, &patch(&[("reach", ScriptValue::Int(3))]))
        .expect("Reach attaches");
    let relaid = PackedLayout::typed(
        vec![field("reach", FieldKind::Value, ScriptValue::Null)],
        vec![optional(ErgonType::Float)],
    )
    .expect("a `float?` layout");
    let loaded = relaid.fit(0, &ScriptValue::Int(3)).map(|(value, _)| value);
    assert_eq!(loaded, Some(ScriptValue::Float(3.0)), "a load keeps it");

    let report = world.relayout(reach, relaid).expect("Reach is relaid out");

    assert_eq!(
        read(&world, entity, reach, "reach"),
        loaded,
        "a load and a live edit agree: {report:?}"
    );
    assert!(report.lost_nothing(), "{report:?}");
}

/// **A value its field's new type refuses is lost as retyped**: a field
/// declared with no type held text, and its declaration becomes an `int?`;
/// the text is no finite-number refusal.
#[test]
fn relayout_names_a_value_its_new_type_refuses_as_retyped() {
    let (mut world, mover, alone, paired) = movers();
    let relaid = PackedLayout::typed(
        vec![
            field("hp", FieldKind::Int, ScriptValue::Int(100)),
            field("speed", FieldKind::Float, ScriptValue::Float(1.0)),
            field("label", FieldKind::Value, ScriptValue::Null),
        ],
        vec![
            ErgonType::Int,
            ErgonType::Float,
            ErgonType::Optional(Box::new(ErgonType::Int)),
        ],
    )
    .expect("a typed layout");

    let report = world.relayout(mover, relaid).expect("Mover is relaid out");

    let mut label: Vec<(EntityId, LossCause)> = report
        .lost
        .iter()
        .filter(|value| value.field == "label")
        .map(|value| (value.entity, value.cause))
        .collect();
    label.sort_by_key(|(entity, _)| entity.index);
    let mut expected = vec![(alone, LossCause::Retyped), (paired, LossCause::Retyped)];
    expected.sort_by_key(|(entity, _)| entity.index);
    assert_eq!(label, expected, "{report:?}");
}

/// **An `int` inside an optional or an array that a `float` cannot hold
/// exactly is refused**, as a top-level one is: relaid out from `int?` and
/// `int[]` to `float?` and `float[]`, a value holding 2^24 + 1 is refused
/// whole, the field keeps its default, and the value is lost as `Inexact`.
#[test]
fn relayout_reports_a_nested_int_a_float_cannot_hold_exactly() {
    let inexact = 16_777_217;
    let optional = |ty| ErgonType::Optional(Box::new(ty));
    let array = |ty| ErgonType::Array(Box::new(ty));
    let fields = || {
        vec![
            field("reach", FieldKind::Value, ScriptValue::Null),
            field("hits", FieldKind::Value, ScriptValue::Array(Vec::new())),
        ]
    };
    let mut world = World::new();
    let mut reach = decl("Reach", SemanticDomain::Script, fields());
    reach.layout = PackedLayout::typed(
        fields(),
        vec![optional(ErgonType::Int), array(ErgonType::Int)],
    )
    .expect("an `int?` and `int[]` layout");
    let reach = world
        .register_runtime_component(reach)
        .expect("Reach registers");
    let entity = world.spawn(());
    let hits = ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(inexact)]);
    world
        .add_runtime_component(
            entity,
            reach,
            &patch(&[("reach", ScriptValue::Int(inexact)), ("hits", hits.clone())]),
        )
        .expect("Reach attaches");
    let relaid = PackedLayout::typed(
        fields(),
        vec![optional(ErgonType::Float), array(ErgonType::Float)],
    )
    .expect("a `float?` and `float[]` layout");

    let report = world.relayout(reach, relaid).expect("Reach is relaid out");

    let no_hits = ScriptValue::Array(Vec::new());
    assert_eq!(
        lost_by_entity(&report),
        vec![
            lost(
                entity,
                "hits",
                hits,
                Some(no_hits.clone()),
                LossCause::Inexact
            ),
            lost(
                entity,
                "reach",
                ScriptValue::Int(inexact),
                Some(ScriptValue::Null),
                LossCause::Inexact
            ),
        ],
        "{report:?}"
    );
    assert_eq!(
        report.defaulted,
        vec!["reach".to_owned(), "hits".to_owned()],
        "the one live row holds both defaults"
    );
    assert_eq!(
        read(&world, entity, reach, "reach"),
        Some(ScriptValue::Null)
    );
    assert_eq!(read(&world, entity, reach, "hits"), Some(no_hits));
}
