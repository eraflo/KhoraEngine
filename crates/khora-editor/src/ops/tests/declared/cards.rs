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

//! A declared component's card: read from the World's registry — its layout
//! and its row — with every field typed, in declaration order, beside its
//! default; never a JSON card.

use khora_sdk::editor_ui::DeclaredCardKind;
use khora_sdk::khora_core::script::{FieldHints, FieldRange};

use super::*;

/// **A declared component shows as a typed card**: titled by its name, in
/// its domain, naming the module that declares it, its fields in
/// declaration order — each with its Ergon type, the value the entity holds
/// and its default.
#[test]
fn a_declared_component_shows_as_a_card_with_its_typed_fields_in_declaration_order() {
    let mut world = GameWorld::new();
    let key = lantern(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    attach(&mut world, lamp, key);
    set(&mut world, lamp, key, "glow", ScriptValue::Float(2.5));
    set(&mut world, lamp, key, "loot", loot("gem", 40));

    let inspected = inspect(&world, lamp, PlayMode::Editing);
    let card = card_of(&inspected, "Lantern");

    assert_eq!(card.domain, SemanticDomain::Render);
    assert_eq!(card.module.as_deref(), Some(LANTERN_MODULE));
    assert_eq!(card.kind, DeclaredCardKind::Component);
    assert!(!card.read_only, "an authored component is edited");
    assert!(!card.orphaned, "a module declares it");

    let declared = lantern_fields();
    assert_eq!(
        card.fields
            .iter()
            .map(|field| (field.name.as_str(), field.ty.clone(), field.default.clone()))
            .collect::<Vec<_>>(),
        declared
            .iter()
            .map(|(name, ty, default)| (*name, ty.clone(), default.clone()))
            .collect::<Vec<_>>(),
        "every field, in declaration order, typed, beside its default"
    );
    let value = |name: &str| {
        card.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value.clone())
            .unwrap_or_else(|| panic!("the card shows `{name}`"))
    };
    assert_eq!(value("glow"), ScriptValue::Float(2.5), "the entity's value");
    assert_eq!(value("loot"), loot("gem", 40), "a nested struct, whole");
    assert_eq!(
        value("count"),
        ScriptValue::Int(3),
        "an unset field: its default"
    );
}

/// **A declared component has no JSON card** — and the entity's Rust
/// components keep theirs.
#[test]
fn a_declared_component_is_a_card_beside_the_rust_ones() {
    let mut world = GameWorld::new();
    let key = lantern(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    attach(&mut world, lamp, key);

    let inspected = inspect(&world, lamp, PlayMode::Editing);

    assert!(card(&inspected, "Lantern").is_some(), "a typed card");
    let json: Vec<&str> = inspected
        .components_json
        .iter()
        .map(|component| component.type_name.as_str())
        .collect();
    assert!(json.contains(&"Transform"), "{json:?}");
    assert!(!json.contains(&"Lantern"), "{json:?}");
}

/// **A behavior shows as a behavior's card** naming the module that
/// declares it; an entity without declared components shows none.
#[test]
fn a_behavior_shows_as_a_behavior_card_naming_its_module() {
    let mut world = GameWorld::new();
    let (behavior, _) = guard(&mut world);
    let sentry = world.spawn((Transform::identity(), Name::new("Sentry")));
    attach(&mut world, sentry, behavior);
    let rock = world.spawn((Transform::identity(), Name::new("Rock")));

    let inspected = inspect(&world, sentry, PlayMode::Editing);
    let card = card_of(&inspected, "Guard");
    assert_eq!(card.kind, DeclaredCardKind::Behavior);
    assert_eq!(card.module.as_deref(), Some(GUARD_MODULE));
    assert!(!card.read_only, "a behavior's fields are authored");
    assert_eq!(
        card.fields
            .iter()
            .map(|field| (field.name.as_str(), field.value.clone()))
            .collect::<Vec<_>>(),
        vec![("speed", ScriptValue::Float(1.5))]
    );

    assert!(inspect(&world, rock, PlayMode::Editing).declared.is_empty());
}

/// **A `Duration` and an `Angle` field keep their type to the card**, so
/// the card can show `s` and `deg` — the value the float they are stored as.
#[test]
fn a_duration_and_an_angle_field_keep_their_type_on_the_card() {
    let mut world = GameWorld::new();
    let key = cooldown(&mut world);
    let turret = world.spawn((Transform::identity(), Name::new("Turret")));
    attach(&mut world, turret, key);
    set(&mut world, turret, key, "wait", ScriptValue::Float(0.5));

    let inspected = inspect(&world, turret, PlayMode::Editing);
    let card = card_of(&inspected, "Cooldown");

    assert_eq!(
        card.fields
            .iter()
            .map(|field| (field.name.as_str(), field.ty.clone(), field.value.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("wait", ErgonType::Duration, ScriptValue::Float(0.5)),
            ("turn", ErgonType::Angle, ScriptValue::Float(90.0)),
        ]
    );
}

/// **Each field carries the hints its module gives it to the card** — its
/// range, tooltip and header, as the registry holds them — and a field
/// given none carries none.
#[test]
fn a_card_carries_each_fields_hints_and_none_where_none_are_given() {
    let mut world = GameWorld::new();
    let key = lantern(&mut world);
    let glow = FieldHints {
        range: Some(FieldRange { min: 0.0, max: 5.0 }),
        tooltip: Some("How bright it burns".to_owned()),
        header: Some("Light".to_owned()),
    };
    let count = FieldHints {
        tooltip: Some("Wicks left".to_owned()),
        ..FieldHints::default()
    };
    world.inner_world_mut().set_field_hints(
        key,
        vec![
            ("glow".to_owned(), glow.clone()),
            ("count".to_owned(), count.clone()),
        ],
    );
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    attach(&mut world, lamp, key);

    let inspected = inspect(&world, lamp, PlayMode::Editing);
    let card = card_of(&inspected, "Lantern");
    let hints = |name: &str| {
        card.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.hints.clone())
            .unwrap_or_else(|| panic!("the card shows `{name}`"))
    };

    assert_eq!(hints("glow"), glow);
    assert_eq!(hints("count"), count);
    for plain in ["lit", "label", "offset", "steps", "loot"] {
        assert_eq!(
            hints(plain),
            FieldHints::default(),
            "`{plain}` is given none"
        );
    }
}

/// **A behavior's card carries its fields' hints too.**
#[test]
fn a_behavior_card_carries_its_fields_hints() {
    let mut world = GameWorld::new();
    let (behavior, _) = guard(&mut world);
    let speed = FieldHints {
        range: Some(FieldRange { min: 0.5, max: 4.0 }),
        ..FieldHints::default()
    };
    world
        .inner_world_mut()
        .set_field_hints(behavior, vec![("speed".to_owned(), speed.clone())]);
    let sentry = world.spawn((Transform::identity(), Name::new("Sentry")));
    attach(&mut world, sentry, behavior);

    let inspected = inspect(&world, sentry, PlayMode::Editing);
    let card = card_of(&inspected, "Guard");

    assert_eq!(
        card.fields
            .iter()
            .map(|field| (field.name.as_str(), field.hints.clone()))
            .collect::<Vec<_>>(),
        vec![("speed", speed)]
    );
}
