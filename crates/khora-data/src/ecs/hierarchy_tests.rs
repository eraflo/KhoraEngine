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

//! Hierarchy invariant tests.
//!
//! The hierarchy is stored twice — `Parent` on the child, `Children` on the
//! parent — because both directions are walked every frame. Two copies of one
//! fact is two chances to disagree, so these lock the cases where they used to.

use khora_core::ecs::entity::EntityId;

use super::{Children, Parent, Transform, World};

fn parent_of(world: &World, child: EntityId) -> Option<EntityId> {
    world.get::<Parent>(child).map(|p| p.0)
}

fn children_of(world: &World, parent: EntityId) -> Option<Vec<EntityId>> {
    world.get::<Children>(parent).map(|c| c.0.clone())
}

fn world_with(count: usize) -> (World, Vec<EntityId>) {
    let mut world = World::new();
    let entities = (0..count)
        .map(|_| world.spawn(Transform::identity()))
        .collect();
    (world, entities)
}

#[test]
fn parenting_writes_both_halves_of_the_edge() {
    let (mut world, e) = world_with(2);

    assert!(world.set_parent(e[1], Some(e[0])));
    assert_eq!(parent_of(&world, e[1]), Some(e[0]));
    assert_eq!(children_of(&world, e[0]), Some(vec![e[1]]));
}

/// The hazard: re-parenting used to leave the child listed under its old parent
/// as well as its new one, so the old one kept claiming it.
#[test]
fn reparenting_removes_the_child_from_its_former_parent() {
    let (mut world, e) = world_with(3);
    let (child, first, second) = (e[0], e[1], e[2]);

    assert!(world.set_parent(child, Some(first)));
    assert!(world.set_parent(child, Some(second)));

    assert_eq!(parent_of(&world, child), Some(second));
    assert_eq!(
        children_of(&world, first),
        Some(Vec::new()),
        "the former parent no longer claims it"
    );
    assert_eq!(children_of(&world, second), Some(vec![child]));
}

#[test]
fn detaching_clears_both_halves() {
    let (mut world, e) = world_with(2);
    let (child, parent) = (e[0], e[1]);

    assert!(world.set_parent(child, Some(parent)));
    assert!(world.set_parent(child, None));

    assert!(parent_of(&world, child).is_none());
    assert_eq!(children_of(&world, parent), Some(Vec::new()));
}

/// Detaching drops only `Parent`. An entity with no parent is still in the
/// scene, so its other Spatial components have to survive.
#[test]
fn detaching_keeps_the_entitys_other_components() {
    let (mut world, e) = world_with(2);

    assert!(world.set_parent(e[0], Some(e[1])));
    assert!(world.set_parent(e[0], None));

    assert!(world.get::<Transform>(e[0]).is_some());
}

#[test]
fn parenting_an_entity_to_itself_is_refused() {
    let (mut world, e) = world_with(1);
    assert!(!world.set_parent(e[0], Some(e[0])));
    assert!(parent_of(&world, e[0]).is_none());
}

/// A cycle is refused rather than created because every consumer walks the
/// hierarchy — transform propagation, the scene tree, recursive despawn — and
/// each would hang on the loop instead of reporting it.
#[test]
fn parenting_to_a_descendant_is_refused() {
    let (mut world, e) = world_with(3);
    let (root, child, grandchild) = (e[0], e[1], e[2]);

    assert!(world.set_parent(child, Some(root)));
    assert!(world.set_parent(grandchild, Some(child)));

    assert!(
        !world.set_parent(root, Some(grandchild)),
        "that edge would close a loop"
    );
    assert!(parent_of(&world, root).is_none());
}

#[test]
fn parenting_a_dead_entity_is_refused() {
    let (mut world, e) = world_with(2);
    world.despawn(e[0]);

    assert!(!world.set_parent(e[0], Some(e[1])));
    assert!(!world.set_parent(e[1], Some(e[0])));
}

// ─── Recursive despawn ──────────────────────────────────────────────────────

/// Destroying a parent alone would leave its children holding a `Parent` that
/// names a recycled index — the ABA problem, one level down.
#[test]
fn despawning_takes_the_whole_subtree() {
    let (mut world, e) = world_with(3);
    let (root, child, grandchild) = (e[0], e[1], e[2]);

    assert!(world.set_parent(child, Some(root)));
    assert!(world.set_parent(grandchild, Some(child)));

    assert_eq!(world.despawn_subtree(root), 3);
    for entity in [root, child, grandchild] {
        assert!(!world.contains(entity), "{entity:?} survived");
    }
}

/// The subtree goes and nothing above it does — and the surviving parent must
/// not keep an id pointing at nothing.
#[test]
fn despawning_a_child_leaves_the_parent_and_forgets_the_child() {
    let (mut world, e) = world_with(2);
    let (parent, child) = (e[0], e[1]);

    assert!(world.set_parent(child, Some(parent)));
    assert_eq!(world.despawn_subtree(child), 1);

    assert!(world.contains(parent));
    assert_eq!(
        children_of(&world, parent),
        Some(Vec::new()),
        "no dangling id in the parent's list"
    );
}

#[test]
fn despawning_a_lone_entity_takes_only_it() {
    let (mut world, e) = world_with(2);
    assert_eq!(world.despawn_subtree(e[0]), 1);
    assert!(world.contains(e[1]));
}

#[test]
fn despawning_something_already_gone_removes_nothing() {
    let (mut world, e) = world_with(1);
    assert_eq!(world.despawn_subtree(e[0]), 1);
    assert_eq!(world.despawn_subtree(e[0]), 0);
}

// ─── Liveness ───────────────────────────────────────────────────────────────

/// The generation half of `contains` is the point: an index alone is recycled,
/// so a handle kept across frames can end up naming a different entity.
#[test]
fn a_recycled_index_does_not_answer_for_the_handle_that_named_it() {
    let (mut world, e) = world_with(1);
    let old = e[0];
    assert!(world.contains(old));

    world.despawn(old);
    assert!(!world.contains(old));

    let fresh = world.spawn(Transform::identity());
    assert_eq!(fresh.index, old.index, "the index was recycled");
    assert!(world.contains(fresh));
    assert!(!world.contains(old), "the old handle still names nobody");
}

#[test]
fn an_index_past_the_end_is_not_live() {
    let world = World::new();
    assert!(!world.contains(EntityId {
        index: 999,
        generation: 1,
    }));
}
