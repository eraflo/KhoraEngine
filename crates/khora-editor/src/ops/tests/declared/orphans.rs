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

//! An orphaned declared component — one no module declares any more, kept
//! with its data — still shows its card, flagged, and can be edited and
//! removed.

use super::*;

/// A cairn holding `Relic`, an orphan, with `power` at 9.
fn cairn() -> (GameWorld, EntityId) {
    let mut world = GameWorld::new();
    let key = relic(&mut world);
    let cairn = world.spawn((Transform::identity(), Name::new("Cairn")));
    attach(&mut world, cairn, key);
    set(&mut world, cairn, key, "power", ScriptValue::Int(9));
    (world, cairn)
}

/// **An orphan's card is shown, flagged** — no module named, its data
/// there, still editable.
#[test]
fn an_orphaned_declared_component_shows_a_flagged_card() {
    let (world, cairn) = cairn();

    let inspected = inspect(&world, cairn, PlayMode::Editing);
    let card = card_of(&inspected, "Relic");

    assert!(card.orphaned);
    assert_eq!(card.module, None);
    assert!(!card.read_only, "an orphan is still the author's data");
    assert_eq!(
        card.fields
            .iter()
            .map(|field| (field.name.as_str(), field.value.clone()))
            .collect::<Vec<_>>(),
        vec![("power", ScriptValue::Int(9))]
    );
}

/// **An orphan's field is edited** like any declared field.
#[test]
fn an_orphaned_declared_components_field_is_edited() {
    let (mut world, cairn) = cairn();

    apply(
        &mut world,
        vec![set_field(cairn, "Relic", "power", ScriptValue::Int(2))],
    );

    assert_eq!(
        field_of(&world, cairn, "Relic", "power"),
        Some(ScriptValue::Int(2))
    );
}

/// **An orphan is removed** like any declared component.
#[test]
fn an_orphaned_declared_component_is_removed() {
    let (mut world, cairn) = cairn();

    apply(
        &mut world,
        vec![PropertyEdit::RemoveDeclared {
            entity: cairn,
            component: "Relic".to_owned(),
        }],
    );

    assert_eq!(row(&world, cairn, "Relic"), None);
}
