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

//! `hierarchy.rs` claims to be the only writer of `Parent` and `Children`.
//! The script command applier writes any registered component by name.

use khora_core::script::{CommandBuffer, ScriptValue, WorldCommand};

use khora_core::ecs::entity::EntityId;

use crate::ecs::systems::script_commands::apply_all;
use crate::ecs::{Children, Parent, Transform, World};

/// A script removing a child's `Parent` by name leaves the former parent
/// still listing the child: the two halves of the edge disagree.
#[test]
fn a_script_removing_parent_by_name_keeps_both_halves_in_step() {
    let mut world = World::new();
    let parent = world.spawn(Transform::identity());
    let child = world.spawn(Transform::identity());
    assert!(world.set_parent(child, Some(parent)));

    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::RemoveComponent {
        entity: child,
        component: "Parent".into(),
    });
    apply_all(&mut world, &buffer);

    let parent_of = world.get::<Parent>(child).map(|p| p.0);
    let listed = world
        .get::<Children>(parent)
        .is_some_and(|c| c.0.contains(&child));
    assert_eq!(
        (parent_of.is_some(), listed),
        (parent_of.is_some(), parent_of.is_some()),
        "Parent is {parent_of:?} but the parent still lists the child: {listed}"
    );
}

/// A script adding `Parent` by name gives the child a parent that does not
/// list it.
#[test]
fn a_script_adding_parent_by_name_keeps_both_halves_in_step() {
    let mut world = World::new();
    let parent = world.spawn(Transform::identity());
    let child = world.spawn(Transform::identity());

    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::AddComponent {
        entity: child,
        component: "Parent".into(),
        value: ScriptValue::Entity(parent),
    });
    apply_all(&mut world, &buffer);

    let parent_of = world.get::<Parent>(child).map(|p| p.0);
    let listed = world
        .get::<Children>(parent)
        .is_some_and(|c| c.0.contains(&child));
    assert_eq!(
        listed,
        parent_of == Some(parent),
        "Parent is {parent_of:?} but the parent lists the child: {listed}"
    );
}

/// `set_parent` refuses a cycle. A chain deeper than `is_descendant_of`'s
/// bound lets one through.
#[test]
fn set_parent_refuses_a_cycle_through_a_deep_chain() {
    let mut world = World::new();
    let chain: Vec<EntityId> = (0..1100)
        .map(|_| world.spawn(Transform::identity()))
        .collect();
    for pair in chain.windows(2) {
        assert!(world.set_parent(pair[1], Some(pair[0])));
    }
    let (root, leaf) = (chain[0], chain[chain.len() - 1]);
    assert!(
        !world.set_parent(root, Some(leaf)),
        "a cycle through 1100 entities was accepted"
    );
}
