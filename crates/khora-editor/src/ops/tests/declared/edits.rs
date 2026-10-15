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

//! Editing a declared component from its card: a field written into the
//! row, all or nothing; the component added at its defaults, or removed —
//! a behavior with its state.

use super::*;

/// A lamp holding `Lantern` at its defaults, and the world it is in.
fn lamp() -> (GameWorld, EntityId) {
    let mut world = GameWorld::new();
    let key = lantern(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    attach(&mut world, lamp, key);
    (world, lamp)
}

/// **Every kind of field is written by its edit**: an int, a float, a bool,
/// a string, a `Vec3`, an entity, an optional given a value and given
/// `null` again, an array, a nested struct — each lands in the row, and the
/// other fields keep theirs.
#[test]
fn a_declared_field_edit_writes_every_kind_of_field_into_the_row() {
    let (mut world, lamp) = lamp();
    let other = world.spawn((Transform::identity(), Name::new("Post")));
    let cases = [
        ("count", ScriptValue::Int(12)),
        ("glow", ScriptValue::Float(4.25)),
        ("lit", ScriptValue::Bool(false)),
        ("label", ScriptValue::Str("beacon".to_owned())),
        ("offset", ScriptValue::Vec3(Vec3::new(1.0, -2.0, 3.0))),
        ("anchor", ScriptValue::Entity(other)),
        ("target", ScriptValue::Entity(other)),
        ("target", ScriptValue::Null),
        (
            "steps",
            ScriptValue::Array(vec![ScriptValue::Float(0.5), ScriptValue::Float(1.5)]),
        ),
        ("loot", loot("crown", 900)),
    ];

    for (field, value) in cases {
        let before = row(&world, lamp, "Lantern").expect("the lamp holds Lantern");
        apply(
            &mut world,
            vec![set_field(lamp, "Lantern", field, value.clone())],
        );

        let after = row(&world, lamp, "Lantern").expect("the lamp holds Lantern");
        let expected: Vec<(String, ScriptValue)> = before
            .into_iter()
            .map(|(name, old)| {
                let new = if name == field { value.clone() } else { old };
                (name, new)
            })
            .collect();
        assert_eq!(after, expected, "`{field}` set to {value:?}");
    }
}

/// **A value the field refuses leaves the row as it was** — text for a
/// float, a non-finite float, an element of another type in an array, a
/// struct of another shape, a field the component does not have — and the
/// entity keeps the component.
#[test]
fn a_refused_declared_field_edit_leaves_the_row_unchanged() {
    let (mut world, lamp) = lamp();
    let before = row(&world, lamp, "Lantern").expect("the lamp holds Lantern");

    for (field, value) in [
        ("glow", ScriptValue::Str("bright".to_owned())),
        ("glow", ScriptValue::Float(f32::NAN)),
        ("count", ScriptValue::Bool(true)),
        (
            "steps",
            ScriptValue::Array(vec![ScriptValue::Str("one".to_owned())]),
        ),
        (
            "loot",
            ScriptValue::Struct(vec![("item".to_owned(), ScriptValue::Int(1))]),
        ),
        ("missing", ScriptValue::Int(1)),
    ] {
        apply(
            &mut world,
            vec![set_field(lamp, "Lantern", field, value.clone())],
        );
        assert_eq!(
            row(&world, lamp, "Lantern").as_ref(),
            Some(&before),
            "`{field}` refuses {value:?}: nothing written"
        );
    }
}

/// **Adding a declared component attaches it at its defaults.**
#[test]
fn adding_a_declared_component_attaches_it_at_its_defaults() {
    let mut world = GameWorld::new();
    lantern(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));

    apply(
        &mut world,
        vec![PropertyEdit::AddDeclared {
            entity: lamp,
            component: "Lantern".to_owned(),
        }],
    );

    assert_eq!(
        row(&world, lamp, "Lantern"),
        Some(
            lantern_fields()
                .into_iter()
                .map(|(name, _, default)| (name.to_owned(), default))
                .collect()
        )
    );
}

/// **Adding a behavior attaches its authored fields**, at their defaults.
#[test]
fn adding_a_behavior_attaches_it_at_its_defaults() {
    let mut world = GameWorld::new();
    guard(&mut world);
    let sentry = world.spawn((Transform::identity(), Name::new("Sentry")));

    apply(
        &mut world,
        vec![PropertyEdit::AddDeclared {
            entity: sentry,
            component: "Guard".to_owned(),
        }],
    );

    assert_eq!(
        row(&world, sentry, "Guard"),
        Some(vec![("speed".to_owned(), ScriptValue::Float(1.5))])
    );
}

/// **Removing a declared component takes it off** and leaves the entity's
/// other components.
#[test]
fn removing_a_declared_component_takes_it_off_and_leaves_the_rest() {
    let (mut world, lamp) = lamp();

    apply(
        &mut world,
        vec![PropertyEdit::RemoveDeclared {
            entity: lamp,
            component: "Lantern".to_owned(),
        }],
    );

    assert_eq!(row(&world, lamp, "Lantern"), None);
    assert!(world.get_component::<Transform>(lamp).is_some());
    assert_eq!(
        world.get_component::<Name>(lamp).map(|name| name.as_str()),
        Some("Lamp")
    );
}

/// **Removing a behavior takes its state along**: what the engine kept of
/// the instance belongs to it.
#[test]
fn removing_a_behavior_takes_its_state_along() {
    let mut world = GameWorld::new();
    let (behavior, companion) = guard(&mut world);
    let sentry = world.spawn((Transform::identity(), Name::new("Sentry")));
    attach(&mut world, sentry, behavior);
    attach(&mut world, sentry, companion);

    apply(
        &mut world,
        vec![PropertyEdit::RemoveDeclared {
            entity: sentry,
            component: "Guard".to_owned(),
        }],
    );

    assert_eq!(row(&world, sentry, "Guard"), None);
    assert_eq!(
        row(&world, sentry, "Guard#state"),
        None,
        "its state goes too"
    );
}

/// **An `int` written to a `Duration?` field is kept as the float it
/// names**, as a script's write is and as a plain `Duration` field keeps
/// it: the row holds a value of its type, which its card can edit again.
#[test]
fn an_int_written_to_an_optional_duration_is_kept_as_a_float() {
    let mut world = GameWorld::new();
    declare(
        &mut world,
        Declaration {
            name: "Fuse",
            module: Some(COOLDOWN_MODULE),
            domain: SemanticDomain::Script,
            role: None,
            provenance: ComponentProvenance::Authored,
            fields: vec![
                ("wait", ErgonType::Duration, ScriptValue::Float(1.0)),
                (
                    "delay",
                    ErgonType::Optional(Box::new(ErgonType::Duration)),
                    ScriptValue::Null,
                ),
            ],
        },
    );
    let fuse = world.spawn((Transform::identity(), Name::new("Fuse")));
    let key = world
        .inner_world()
        .components()
        .key_named("Fuse")
        .expect("Fuse is declared");
    attach(&mut world, fuse, key);

    apply(
        &mut world,
        vec![
            set_field(fuse, "Fuse", "wait", ScriptValue::Int(3)),
            set_field(fuse, "Fuse", "delay", ScriptValue::Int(3)),
        ],
    );

    assert_eq!(
        field_of(&world, fuse, "Fuse", "wait"),
        Some(ScriptValue::Float(3.0))
    );
    assert_eq!(
        field_of(&world, fuse, "Fuse", "delay"),
        Some(ScriptValue::Float(3.0)),
        "a `Duration?` holds a duration, not an int"
    );
}
