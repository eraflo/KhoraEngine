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

//! A declared component changes domain (Ergon spec 08).
//!
//! An author edits `[Domain(Script)]` to `[Domain(Physics)]` and reloads: every
//! row of the component moves to its entity's page of the new domain, its
//! values intact, beside the entity's other components of that domain — and
//! the domain is how a script names a domain, `SemanticDomain::name`.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ComponentName, ScriptValue};

use super::runtime_components::{declare, field, patch, read};
use crate::ecs::{
    Component, ComponentKey, FieldKind, PageIndex, RegisterError, SemanticDomain, Transform, World,
};

/// A Physics-domain Rust component, for a page the moved rows join.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mass(f32);
impl Component for Mass {}

/// A Script-domain Rust component, left behind by a move out of Script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tick(u32);
impl Component for Tick {}

const ALL: [SemanticDomain; SemanticDomain::COUNT] = [
    SemanticDomain::Spatial,
    SemanticDomain::Render,
    SemanticDomain::Audio,
    SemanticDomain::Physics,
    SemanticDomain::Ui,
    SemanticDomain::Script,
];

fn str_value(text: &str) -> ScriptValue {
    ScriptValue::Str(text.to_owned())
}

/// `entity`'s location in `domain`.
fn location(world: &World, entity: EntityId, domain: SemanticDomain) -> Option<PageIndex> {
    let (id, metadata) = world.entities.get(entity.index as usize)?;
    if *id != entity {
        return None;
    }
    metadata.as_ref()?.locations.get(&domain).copied()
}

/// Whether `entity`'s page in `domain` holds a column of `key`.
fn page_holds(world: &World, entity: EntityId, domain: SemanticDomain, key: ComponentKey) -> bool {
    location(world, entity, domain).is_some_and(|at| {
        world.storage.pages[at.page_id as usize]
            .columns
            .contains_key(&key)
    })
}

/// A world where `Wake` (Script domain: `strength`, `label`) is held by an
/// entity that holds nothing else and by one that also holds a `Tick` (Script)
/// and a `Mass` (Physics).
fn wakes() -> (World, ComponentKey, EntityId, EntityId) {
    let mut world = World::new();
    world.register_component::<Mass>(SemanticDomain::Physics);
    world.register_component::<Tick>(SemanticDomain::Script);
    let wake = declare(
        &mut world,
        "Wake",
        SemanticDomain::Script,
        vec![
            field("strength", FieldKind::Int, ScriptValue::Int(1)),
            field("label", FieldKind::Value, str_value("none")),
        ],
    );

    let alone = world.spawn(());
    world
        .add_runtime_component(
            alone,
            wake,
            &patch(&[
                ("strength", ScriptValue::Int(7)),
                ("label", str_value("alone")),
            ]),
        )
        .expect("Wake attaches");

    let crowded = world.spawn((Transform::identity(), Mass(2.5), Tick(3)));
    world
        .add_runtime_component(
            crowded,
            wake,
            &patch(&[
                ("strength", ScriptValue::Int(9)),
                ("label", str_value("crowded")),
            ]),
        )
        .expect("Wake attaches");

    (world, wake, alone, crowded)
}

// --- names ---

/// **A domain is named as Ergon spells it in `[Domain(…)]`**, and the name
/// leads back to the domain.
#[test]
fn a_domain_is_named_as_a_script_spells_it() {
    assert_eq!(
        ALL.map(SemanticDomain::name),
        ["Spatial", "Render", "Audio", "Physics", "Ui", "Script"]
    );
    for domain in ALL {
        assert_eq!(SemanticDomain::from_name(domain.name()), Some(domain));
    }
}

#[test]
fn an_unknown_domain_name_is_none() {
    for name in ["Phisics", "", "Physics ", "Gameplay"] {
        assert_eq!(SemanticDomain::from_name(name), None, "{name:?}");
    }
}

// --- moves ---

/// **The rows move, the values stay.** After `Script` → `Physics`, the
/// component is a Physics component: each holder's Physics page holds its row,
/// with the values it had, and a Script page it left no longer does.
#[test]
fn a_domain_move_keeps_every_value() {
    let (mut world, wake, alone, crowded) = wakes();

    world
        .move_component_domain(wake, SemanticDomain::Physics)
        .expect("Wake moves");

    assert_eq!(
        world
            .components()
            .vtable(wake)
            .expect("still registered")
            .domain,
        SemanticDomain::Physics
    );
    assert_eq!(world.components().key_named("Wake"), Some(wake), "same key");
    for (entity, strength, label) in [(alone, 7, "alone"), (crowded, 9, "crowded")] {
        assert_eq!(
            read(&world, entity, wake, "strength"),
            Some(ScriptValue::Int(strength))
        );
        assert_eq!(
            read(&world, entity, wake, "label"),
            Some(str_value(label)),
            "a variable-size field moves with its row"
        );
        assert!(
            page_holds(&world, entity, SemanticDomain::Physics, wake),
            "the entity's Physics page holds Wake"
        );
    }
    // `alone` holds Wake and nothing else, so its Script and Physics rows are
    // distinct pages: the one it left no longer holds Wake. `crowded` is not
    // checked this way — a bundle spawned across several domains shares one
    // page today, so its Script and Physics locations are the same page.
    assert!(
        !page_holds(&world, alone, SemanticDomain::Script, wake),
        "the Script page it left no longer holds Wake"
    );
}

/// The moved row joins the entity's existing page of the new domain, beside
/// the components already there; what stays in the old domain is untouched.
#[test]
fn a_domain_move_joins_the_entitys_other_components() {
    let (mut world, wake, _, crowded) = wakes();
    world
        .move_component_domain(wake, SemanticDomain::Physics)
        .expect("Wake moves");

    let after = location(&world, crowded, SemanticDomain::Physics).expect("a Physics row");
    let page = &world.storage.pages[after.page_id as usize];
    assert!(page.columns.contains_key(&ComponentKey::of::<Mass>()));
    assert!(page.columns.contains_key(&wake));
    assert_eq!(world.get::<Mass>(crowded), Some(&Mass(2.5)));
    assert_eq!(world.get::<Tick>(crowded), Some(&Tick(3)), "Tick stays");
    assert_eq!(
        world.get::<Transform>(crowded),
        Some(&Transform::identity())
    );
}

/// The rows are found where they now live: by `row`, by a write, and by the
/// projection scripts read.
#[test]
fn a_moved_component_is_read_and_written_in_its_new_domain() {
    let (mut world, wake, alone, crowded) = wakes();
    world
        .move_component_domain(wake, SemanticDomain::Physics)
        .expect("Wake moves");
    // The clock moves past the setup, so the write below carries a later
    // tick than the rows it finds.
    world.end_run();

    let physics = world.domain_epoch(SemanticDomain::Physics);
    let slot = super::runtime_components::slot(&world, wake, "strength").expect("a slot");
    world
        .row_mut(alone, wake)
        .expect("the row is found in Physics")
        .set_field(slot, &ScriptValue::Int(70))
        .expect("written");
    assert!(world.domain_epoch(SemanticDomain::Physics) > physics);
    assert_eq!(
        read(&world, alone, wake, "strength"),
        Some(ScriptValue::Int(70))
    );

    let projected = world.project_components(&[ComponentName::new("Wake")]);
    let wakes = projected.component("Wake").expect("projected");
    assert_eq!(wakes.holders(), 2);
    assert!(wakes.has(crowded));
}

/// A move changes where data lives for both domains: each one's epoch says so.
#[test]
fn a_domain_move_moves_both_domain_epochs() {
    let (mut world, wake, _, _) = wakes();
    // The clock moves past the setup, so the change below carries a later
    // tick than the rows it finds.
    world.end_run();
    let script = world.domain_epoch(SemanticDomain::Script);
    let physics = world.domain_epoch(SemanticDomain::Physics);

    world
        .move_component_domain(wake, SemanticDomain::Physics)
        .expect("Wake moves");

    assert!(world.domain_epoch(SemanticDomain::Script) > script);
    assert!(world.domain_epoch(SemanticDomain::Physics) > physics);
}

/// A row added after the move lands in the new domain.
#[test]
fn a_row_added_after_a_move_lands_in_the_new_domain() {
    let (mut world, wake, _, _) = wakes();
    world
        .move_component_domain(wake, SemanticDomain::Ui)
        .expect("Wake moves");

    let late = world.spawn(Transform::identity());
    world
        .add_runtime_component(late, wake, &patch(&[("strength", ScriptValue::Int(4))]))
        .expect("Wake attaches");
    assert!(page_holds(&world, late, SemanticDomain::Ui, wake));
    assert_eq!(
        read(&world, late, wake, "strength"),
        Some(ScriptValue::Int(4))
    );
}

/// Moving to the domain it is in changes nothing.
#[test]
fn a_move_to_the_same_domain_keeps_everything() {
    let (mut world, wake, alone, crowded) = wakes();

    world
        .move_component_domain(wake, SemanticDomain::Script)
        .expect("a move to its own domain is not an error");

    assert_eq!(
        world.components().vtable(wake).expect("registered").domain,
        SemanticDomain::Script
    );
    for (entity, strength) in [(alone, 7), (crowded, 9)] {
        assert_eq!(
            read(&world, entity, wake, "strength"),
            Some(ScriptValue::Int(strength))
        );
        assert!(page_holds(&world, entity, SemanticDomain::Script, wake));
    }
}

/// A Rust component's domain is its type's; an unknown key is no component.
#[test]
fn only_a_declared_component_changes_domain() {
    let (mut world, _, _, crowded) = wakes();

    assert_eq!(
        world.move_component_domain(ComponentKey::of::<Mass>(), SemanticDomain::Script),
        Err(RegisterError::NotRuntime {
            name: "Mass".to_owned()
        })
    );
    assert_eq!(world.get::<Mass>(crowded), Some(&Mass(2.5)));

    let unknown = ComponentKey::named("NobodyDeclaredThis");
    assert_eq!(
        world.move_component_domain(unknown, SemanticDomain::Script),
        Err(RegisterError::Unknown(unknown))
    );
}
