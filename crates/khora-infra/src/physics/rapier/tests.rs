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

use super::*;
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{ColliderDesc, ColliderShape, PhysicsProvider};

use khora_core::ecs::entity::EntityId;

fn entity(index: u32, generation: u32) -> EntityId {
    EntityId { index, generation }
}

fn a_box(owner: Option<EntityId>) -> ColliderDesc {
    ColliderDesc {
        owner,
        parent_body: None,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        shape: ColliderShape::Sphere(1.0),
        active_events: true,
        friction: 0.5,
        restitution: 0.0,
        is_sensor: false,
        is_character: false,
    }
}

/// **What unblocks a collision anybody can act on.** A contact names two
/// colliders; this is what turns one back into the entity gameplay knows.
#[test]
fn a_collider_remembers_the_entity_that_made_it() {
    let mut world = RapierPhysicsWorld::default();
    let owner = entity(12, 3);

    let handle = world.add_collider(a_box(Some(owner)));

    assert_eq!(world.entity_of(handle), Some(owner));
}

/// The generation is carried, so two entities that reused one index are
/// told apart — the same recycling problem as the slot, one level up.
#[test]
fn a_recycled_entity_index_is_not_mistaken_for_the_old_one() {
    let mut world = RapierPhysicsWorld::default();
    let old = world.add_collider(a_box(Some(entity(4, 0))));
    let new = world.add_collider(a_box(Some(entity(4, 1))));

    assert_eq!(world.entity_of(old), Some(entity(4, 0)));
    assert_eq!(world.entity_of(new), Some(entity(4, 1)));
}

/// **The first entity a `World` hands out is `{ index: 0, generation: 0 }`,
/// which packs to zero.** Treating zero as "no owner" made that one entity
/// the one a collision could never name — and only that one, so a scene
/// whose floor happened to be spawned first lost every contact with it.
#[test]
fn the_zeroth_entity_is_still_an_entity() {
    let mut world = RapierPhysicsWorld::default();
    let first = entity(0, 0);

    let handle = world.add_collider(a_box(Some(first)));

    assert_eq!(world.entity_of(handle), Some(first));
}

/// A collider the ECS did not make — a query volume, a tool — answers
/// nothing rather than answering entity zero.
#[test]
fn a_collider_with_no_owner_names_nobody() {
    let mut world = RapierPhysicsWorld::default();

    let handle = world.add_collider(a_box(None));

    assert_eq!(world.entity_of(handle), None);
}

/// A handle whose collider is gone resolves to nothing, which is the
/// answer: what it named does not exist, so neither does its entity.
#[test]
fn a_removed_colliders_handle_names_nobody() {
    let mut world = RapierPhysicsWorld::default();
    let handle = world.add_collider(a_box(Some(entity(1, 0))));

    world.remove_collider(handle);

    assert_eq!(world.entity_of(handle), None);
}

/// Isolates the provider: an overlapping pair that asked for events should
/// produce one.
///
/// The ball needs a dynamic parent body. Rapier generates no contact
/// between two colliders that both count as fixed, and a collider with no
/// parent counts as fixed — so a test written without one measures that
/// rule rather than the event wiring.
#[test]
fn two_overlapping_colliders_report_a_contact() {
    use khora_core::physics::{BodyType, RigidBodyDesc};

    let mut world = RapierPhysicsWorld::default();

    let mut floor = a_box(Some(entity(0, 0)));
    floor.shape = ColliderShape::Box(Vec3::new(10.0, 0.5, 10.0));
    world.add_collider(floor);

    let body = world.add_body(RigidBodyDesc {
        body_type: BodyType::Dynamic,
        position: Vec3::new(0.0, 0.8, 0.0),
        rotation: Quat::IDENTITY,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
        mass: 1.0,
        ccd_enabled: false,
    });
    let mut ball = a_box(Some(entity(1, 0)));
    ball.parent_body = Some(body);
    world.add_collider(ball);

    world.step(1.0 / 60.0);

    assert!(
        !world.take_collision_events().is_empty(),
        "the overlap should have been reported"
    );
}
