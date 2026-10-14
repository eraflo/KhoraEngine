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

//! Behaviors and systems run in one list (Ergon spec 10): by module, then by
//! where each is written in its module — never by name, which only breaks a
//! tie — and a system's singleton is found by a named identity that takes
//! nothing from the numbering of the entities the game creates.

use khora_core::ecs::PersistentId;
use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field};
use crate::ecs::{ComponentKey, FieldKind, ScriptRole, SemanticDomain, Transform, World};

fn declared(world: &mut World, name: &str) -> ComponentKey {
    declare(
        world,
        name,
        SemanticDomain::Script,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    )
}

/// Registers the behavior `name`, `order`-th of `module`.
fn behavior(world: &mut World, module: &str, order: u32, name: &str) -> ComponentKey {
    let key = declared(world, name);
    world.set_script_role(
        key,
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order,
        }),
    );
    key
}

/// Registers the system `name`, `order`-th of `module`.
fn system(world: &mut World, module: &str, order: u32, name: &str) -> ComponentKey {
    let key = declared(world, name);
    world.set_script_role(
        key,
        Some(ScriptRole::System {
            module: module.to_owned(),
            order,
        }),
    );
    key
}

fn listed(world: &World) -> Vec<(String, String)> {
    world
        .behavior_components()
        .iter()
        .map(|listed| (listed.module.to_owned(), listed.name.to_owned()))
        .collect()
}

/// **Behaviors and systems are one list, by module then by source order**:
/// registered out of order, with names that sort the other way, they are
/// listed as their modules write them.
#[test]
fn behaviors_and_systems_are_listed_by_module_then_source_order() {
    let mut world = World::new();
    behavior(&mut world, "game/a.erg", 2, "Alpha");
    system(&mut world, "game/b.erg", 0, "Able");
    system(&mut world, "game/a.erg", 0, "Zulu");
    behavior(&mut world, "game/a.erg", 1, "Mike");
    behavior(&mut world, "game/b.erg", 1, "Aaron");

    assert_eq!(
        listed(&world),
        [
            ("game/a.erg", "Zulu"),
            ("game/a.erg", "Mike"),
            ("game/a.erg", "Alpha"),
            ("game/b.erg", "Able"),
            ("game/b.erg", "Aaron"),
        ]
        .map(|(module, name)| (module.to_owned(), name.to_owned()))
        .to_vec()
    );
}

/// **The name breaks a tie** — two of one module recorded at the same place,
/// as an edit moving one before the other leaves them until both are
/// registered again.
#[test]
fn the_name_breaks_a_tie_between_behaviors_and_systems_of_one_place() {
    let mut world = World::new();
    system(&mut world, "game/a.erg", 0, "Zeta");
    behavior(&mut world, "game/a.erg", 0, "Beta");
    system(&mut world, "game/a.erg", 0, "Alpha");

    assert_eq!(
        listed(&world)
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>(),
        vec!["Alpha", "Beta", "Zeta"]
    );
}

/// **A system's state companion is not listed**: it is what the engine keeps
/// of the system, not a program to run.
#[test]
fn a_systems_state_companion_is_not_listed() {
    let mut world = World::new();
    let clock = system(&mut world, "game/clock.erg", 0, "Clock");
    let state = declared(&mut world, "Clock#state");
    world.set_script_role(
        state,
        Some(ScriptRole::BehaviorState {
            behavior: "Clock".to_owned(),
        }),
    );

    assert_eq!(
        world
            .behavior_components()
            .iter()
            .map(|listed| listed.key)
            .collect::<Vec<_>>(),
        vec![clock]
    );
}

/// **A named identity takes nothing from the created numbering**: an entity
/// given one, the next entity the world numbers is the first created
/// identity, as in a world without it — and the named entity is found by it.
#[test]
fn a_named_identity_does_not_advance_the_created_numbering() {
    let mut fresh = World::new();
    let first = fresh.spawn(Transform::identity());
    let expected = fresh.persistent_id(first).expect("alive");

    let mut world = World::new();
    let singleton = world.spawn(Transform::identity());
    let named = PersistentId::named("system:Clock");
    world.set_persistent_id(singleton, named);
    let other = world.spawn(Transform::identity());

    assert_eq!(world.entity_with_id(named), Some(singleton));
    assert_eq!(world.persistent_id(singleton), Some(named));
    assert_eq!(
        world.persistent_id(other),
        Some(expected),
        "the created numbering starts where a world without the named entity starts"
    );
}
