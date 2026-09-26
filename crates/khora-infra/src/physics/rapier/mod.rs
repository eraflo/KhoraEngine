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

//! Rapier implementation of the physics provider.

mod conversions;
mod debug;
mod events;

use khora_core::physics::{ColliderHandle, RigidBodyHandle, Slot};

mod world;

pub use world::RapierPhysicsWorld;

// --- Internal Helpers ---

/// Both directions carry Rapier's generation, which they used to drop.
///
/// `from_raw_parts(index, 0)` resolved a handle to whatever now sits in the
/// slot: a stale handle moved somebody else's body instead of failing, and only
/// once Rapier had recycled that slot — so never in a short session and
/// reliably in a long one.
fn to_rapier_rb_handle(handle: RigidBodyHandle) -> rapier3d::dynamics::RigidBodyHandle {
    let slot = handle.slot();
    rapier3d::dynamics::RigidBodyHandle::from_raw_parts(slot.index, slot.generation)
}

pub(super) fn from_rapier_rb_handle(
    handle: rapier3d::dynamics::RigidBodyHandle,
) -> RigidBodyHandle {
    let (index, generation) = handle.into_raw_parts();
    RigidBodyHandle(Slot { index, generation }.pack())
}

fn to_rapier_cl_handle(handle: ColliderHandle) -> rapier3d::geometry::ColliderHandle {
    let slot = handle.slot();
    rapier3d::geometry::ColliderHandle::from_raw_parts(slot.index, slot.generation)
}

pub(super) fn from_rapier_cl_handle(handle: rapier3d::geometry::ColliderHandle) -> ColliderHandle {
    let (index, generation) = handle.into_raw_parts();
    ColliderHandle(Slot { index, generation }.pack())
}

/// Set when the low 64 bits hold an entity.
///
/// A flag rather than treating zero as absent: `EntityId { index: 0,
/// generation: 0 }` is the first entity a fresh `World` hands out, and it packs
/// to zero. Reading that back as "no owner" made the first entity in a scene
/// the one a collision could never name — silently, and only that one.
const OWNED: u128 = 1 << 64;

/// Packs an entity into the `u128` Rapier carries on every collider.
///
/// The identity travels **with the collider**, so there is no index to keep in
/// step with the world and nothing to invalidate when an entity dies: the
/// collider dies with it.
fn stamp_owner(entity: Option<khora_core::ecs::entity::EntityId>) -> u128 {
    match entity {
        Some(entity) => OWNED | ((entity.generation as u128) << 32) | entity.index as u128,
        None => 0,
    }
}

/// Reads back what [`stamp_owner`] wrote.
pub(super) fn stamped_owner(user_data: u128) -> Option<khora_core::ecs::entity::EntityId> {
    if user_data & OWNED == 0 {
        return None;
    }
    Some(khora_core::ecs::entity::EntityId {
        index: user_data as u32,
        generation: (user_data >> 32) as u32,
    })
}

#[cfg(test)]
mod owner_tests {
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
}
