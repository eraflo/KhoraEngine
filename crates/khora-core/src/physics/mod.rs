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

//! # Physics Abstractions
//!
//! Universal traits and types for physics simulation providers.

use crate::math::{Quat, Vec3};

mod collision;
mod debug;
mod desc;
mod handle;
mod query;

pub use collision::collision_channel;
pub use collision::Collision;
pub use collision::CollisionEvent;
pub use collision::CollisionKind;
pub use collision::ContactBatch;
pub use collision::COLLISION_BACKLOG;
pub use debug::DebugLine;
pub use desc::BodyType;
pub use desc::ColliderDesc;
pub use desc::ColliderShape;
pub use desc::RigidBodyDesc;
pub use handle::ColliderHandle;
pub use handle::RigidBodyHandle;
pub use handle::SlotId;
pub use query::CharacterControllerOptions;
pub use query::RaycastHit;

/// Interface contract for any physics engine implementation (e.g., Rapier).
pub trait PhysicsProvider: Send + Sync {
    /// Advances the simulation by `dt` seconds.
    fn step(&mut self, dt: f32);

    /// Sets the global gravity vector.
    fn set_gravity(&mut self, gravity: Vec3);

    /// Adds a rigid body to the simulation.
    fn add_body(&mut self, desc: RigidBodyDesc) -> RigidBodyHandle;

    /// Removes a rigid body from the simulation.
    fn remove_body(&mut self, handle: RigidBodyHandle);

    /// Adds a collider to the simulation.
    fn add_collider(&mut self, desc: ColliderDesc) -> ColliderHandle;

    /// Removes a collider from the simulation.
    fn remove_collider(&mut self, handle: ColliderHandle);

    /// Synchronizes the position and rotation of a rigid body.
    fn get_body_transform(&self, handle: RigidBodyHandle) -> (Vec3, Quat);

    /// The linear and angular velocity a body currently has.
    ///
    /// Read back rather than assumed: `RigidBody::initial_velocity` is what its
    /// author declared, applied once when the body is created. What it is doing
    /// now is the solver's, and until this existed nothing could ask.
    fn get_body_velocity(&self, handle: RigidBodyHandle) -> (Vec3, Vec3);

    /// Manually sets the position and rotation of a rigid body.
    fn set_body_transform(&mut self, handle: RigidBodyHandle, pos: Vec3, rot: Quat);

    /// Returns a list of all active rigid body handles.
    fn get_all_bodies(&self) -> Vec<RigidBodyHandle>;

    /// Returns a list of all active collider handles.
    fn get_all_colliders(&self) -> Vec<ColliderHandle>;

    /// Updates the properties of an existing rigid body.
    fn update_body_properties(&mut self, handle: RigidBodyHandle, desc: RigidBodyDesc);

    /// Updates the properties of an existing collider.
    fn update_collider_properties(&mut self, handle: ColliderHandle, desc: ColliderDesc);

    /// Returns debug rendering lines from the physics engine.
    fn get_debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>);

    /// Casts a ray into the physics world and returns the closest hit.
    fn cast_ray(&self, ray: &Ray, max_toi: f32, solid: bool) -> Option<RaycastHit>;

    /// Takes the collision events accumulated since the last call.
    ///
    /// **Destructive, and across steps, not "during the last step"** as this
    /// said before — the backend's buffer is not cleared by stepping, so what
    /// comes back is everything since the previous call and the caller is the
    /// only one who will ever see it. A second caller gets nothing.
    fn take_collision_events(&self) -> Vec<CollisionEvent>;

    /// The entity a collider belongs to, if the ECS created it.
    ///
    /// A contact names two colliders; gameplay asks about two entities. The
    /// backend is asked because it is where the answer already is — the owner
    /// was stamped on the collider when it was made, so this survives slot
    /// recycling and needs no index to keep in step with the world.
    fn entity_of(&self, collider: ColliderHandle) -> Option<crate::ecs::entity::EntityId>;

    /// Resolves movement for a kinematic character controller.
    /// Returns the actual translation applied and whether the character is grounded.
    fn move_character(
        &self,
        collider: ColliderHandle,
        desired_translation: Vec3,
        options: &CharacterControllerOptions,
    ) -> (Vec3, bool);
}

/// A ray in 3D space.
///
/// Re-exported from [`crate::math`], where it lives alongside the intersection
/// tests: physics is only one of its callers — editor picking and gizmo
/// manipulation cast the same rays and must not reinvent the math.
pub use crate::math::Ray;

#[cfg(test)]
mod handle_tests {
    use super::*;

    /// **The bug the generation exists to stop.** A backend that recycles slots
    /// hands the same index to a new occupant; a handle that carried only the
    /// index would address that new occupant instead of failing, and would do
    /// so silently — a stale handle moving somebody else's body.
    #[test]
    fn two_occupants_of_one_slot_are_different_handles() {
        let first = ColliderHandle(
            SlotId {
                index: 7,
                generation: 0,
            }
            .pack(),
        );
        let recycled = ColliderHandle(
            SlotId {
                index: 7,
                generation: 1,
            }
            .pack(),
        );

        assert_ne!(first, recycled, "same slot, different occupant");
    }

    #[test]
    fn a_handle_round_trips_its_slot() {
        let slot = SlotId {
            index: u32::MAX,
            generation: 3,
        };

        assert_eq!(RigidBodyHandle(slot.pack()).slot(), slot);
        assert_eq!(ColliderHandle(slot.pack()).slot(), slot);
    }

    /// The index occupies the low half, so a generation of zero packs to the
    /// index itself — which is what every handle written before this change
    /// held, and why they keep meaning what they meant.
    #[test]
    fn a_first_generation_handle_is_its_index() {
        let slot = SlotId {
            index: 42,
            generation: 0,
        };

        assert_eq!(slot.pack(), 42);
    }
}
