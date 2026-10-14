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

//! The behaviors a world knows, from its components alone.
//!
//! A behavior is a declared component whose role says so; the module that
//! runs it is in the role. Its state companion, a declared component with no
//! role and a Rust component are components, not behaviors.

use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field};
use crate::ecs::{BehaviorComponent, ComponentKey, FieldKind, ScriptRole, SemanticDomain, World};

/// Registers the declared component `name` with one `int` field.
fn declared(world: &mut World, name: &str) -> ComponentKey {
    declare(
        world,
        name,
        SemanticDomain::Script,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    )
}

/// Registers the behavior `name` that `module` declares.
fn behavior(world: &mut World, module: &str, name: &str) -> ComponentKey {
    let key = declared(world, name);
    world.set_script_role(
        key,
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order: 0,
        }),
    );
    key
}

/// Registers the state companion of behavior `behavior`.
fn companion(world: &mut World, behavior: &str) -> ComponentKey {
    let key = declared(world, &format!("{behavior}#state"));
    world.set_script_role(
        key,
        Some(ScriptRole::BehaviorState {
            behavior: behavior.to_owned(),
        }),
    );
    key
}

/// Every behavior is listed with its key, module and name, ordered by
/// module and then by name — not by registration order, which a hash map
/// would scramble anyway.
#[test]
fn every_behavior_is_listed_sorted_by_module_then_name() {
    let mut world = World::new();
    let alpha = behavior(&mut world, "b/x.erg", "Alpha");
    let zeta = behavior(&mut world, "a/y.erg", "Zeta");
    let beta = behavior(&mut world, "a/y.erg", "Beta");

    assert_eq!(
        world.behavior_components(),
        vec![
            BehaviorComponent {
                key: beta,
                module: "a/y.erg",
                name: "Beta",
            },
            BehaviorComponent {
                key: zeta,
                module: "a/y.erg",
                name: "Zeta",
            },
            BehaviorComponent {
                key: alpha,
                module: "b/x.erg",
                name: "Alpha",
            },
        ]
    );
}

/// A behavior's state companion is what the engine writes of it, not a
/// behavior of its own: listing it would run the behavior twice.
#[test]
fn a_state_companion_is_not_a_behavior() {
    let mut world = World::new();
    let guard = behavior(&mut world, "ai/guard.erg", "Guard");
    companion(&mut world, "Guard");

    let behaviors = world.behavior_components();

    assert_eq!(
        behaviors.iter().map(|b| b.key).collect::<Vec<_>>(),
        vec![guard]
    );
}

/// A declared component with no role is data a script reads and writes, not
/// a program to run.
#[test]
fn a_declared_component_with_no_role_is_not_a_behavior() {
    let mut world = World::new();
    declared(&mut world, "Health");
    let guard = behavior(&mut world, "ai/guard.erg", "Guard");

    let behaviors = world.behavior_components();

    assert_eq!(
        behaviors.iter().map(|b| b.name).collect::<Vec<_>>(),
        vec!["Guard"]
    );
    assert_eq!(behaviors[0].key, guard);
}

/// A world of Rust components only — `Script` among them — has no behavior
/// component.
#[test]
fn a_world_of_rust_components_lists_no_behavior() {
    let world = World::new();
    assert!(
        !world.components().is_empty(),
        "the world registers Rust components"
    );

    assert!(world.behavior_components().is_empty());
}

/// The role is read when asked, not remembered: a component whose module
/// stopped declaring it a behavior is no longer one.
#[test]
fn a_component_whose_role_is_cleared_is_no_longer_a_behavior() {
    let mut world = World::new();
    let guard = behavior(&mut world, "ai/guard.erg", "Guard");
    world.set_script_role(guard, None);

    assert!(world.behavior_components().is_empty());
}
