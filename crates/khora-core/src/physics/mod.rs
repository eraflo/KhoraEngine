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

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::math::{LinearRgba, Quat, Vec3};

/// Opaque handle to a rigid body in the physics engine.
///
/// Carries the backend's slot **and its generation**. A backend that recycles
/// slots — Rapier does — would otherwise resolve a handle to whatever now
/// occupies the slot the caller meant, silently: a stale handle would move
/// somebody else's body rather than fail. See [`Slot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub struct RigidBodyHandle(pub u64);

/// Opaque handle to a collider in the physics engine.
///
/// Carries the slot and its generation, for the reason [`RigidBodyHandle`]
/// gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub struct ColliderHandle(pub u64);

/// A backend slot and the generation that says which occupant is meant.
///
/// Both handle types are one of these packed into a `u64`. Packed rather than
/// two fields because a handle is stored on components and crosses the
/// serialization boundary, and one number stays one number there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Which slot in the backend's arena.
    pub index: u32,
    /// How many times that slot has been reused before this occupant.
    pub generation: u32,
}

impl Slot {
    /// Packs into the `u64` a handle carries.
    pub const fn pack(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }

    /// Unpacks what [`pack`](Self::pack) wrote.
    pub const fn unpack(packed: u64) -> Self {
        Self {
            index: packed as u32,
            generation: (packed >> 32) as u32,
        }
    }
}

impl RigidBodyHandle {
    /// The slot and generation this addresses.
    pub const fn slot(self) -> Slot {
        Slot::unpack(self.0)
    }
}

impl ColliderHandle {
    /// The slot and generation this addresses.
    pub const fn slot(self) -> Slot {
        Slot::unpack(self.0)
    }
}

/// Two entities beginning or ending contact.
///
/// The engine's form of a collision, as distinct from [`CollisionEvent`] which
/// is the backend's: that one names two colliders, this one names two entities,
/// and the translation happens where the provider is in hand rather than being
/// left to whoever consumes it.
///
/// **A transition, not a state.** `Started` and `Stopped` say what changed;
/// "who is touching whom right now" is a different question that this does not
/// answer, and that a relation between entities would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collision {
    /// Whether contact began or ended.
    pub kind: CollisionKind,
    /// One of the two. Which is which carries no meaning — a contact is
    /// symmetric, and a consumer that cares about one entity checks both.
    pub a: crate::ecs::entity::EntityId,
    /// The other.
    pub b: crate::ecs::entity::EntityId,
}

/// Whether a [`Collision`] began or ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionKind {
    /// The two started touching.
    Started,
    /// They stopped.
    Stopped,
}

impl crate::event::Supersedes for Collision {
    // Nothing coalesces. Two entities that touch, separate and touch again
    // within one frame did that twice, and a consumer counting hits is
    // counting hits.
}

/// How many collisions are kept for readers that have not caught up.
///
/// A heavy frame is hundreds of contacts; this is several frames of one. The
/// case that reaches it is a consumer that stopped reading, where the oldest
/// contact is also the least worth delivering.
pub const COLLISION_BACKLOG: usize = 4096;

/// A channel for the contacts the physics lane reports.
pub fn collision_channel() -> crate::event::Channel<Collision> {
    crate::event::Channel::bounded(COLLISION_BACKLOG, crate::event::WhenFull::DropOldest)
}

/// What one physics step reported, on its way from the lane to the channel.
///
/// A deck slot, because that is the sanctioned road out of a lane: a lane that
/// wrote the shared channel directly would be a lane reaching engine state, and
/// the scheduler would have no way to know it had.
#[derive(Debug, Clone, Default)]
pub struct ContactBatch {
    /// The contacts, in the order the backend reported them.
    pub contacts: Vec<Collision>,
}

/// Defines the type of a rigid body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum BodyType {
    /// Responds to forces and collisions.
    Dynamic,
    /// Fixed in place, does not move.
    Static,
    /// Controlled by the user, not by forces.
    Kinematic,
}

/// Description for creating a rigid body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigidBodyDesc {
    /// Initial position.
    pub position: Vec3,
    /// Initial rotation.
    pub rotation: Quat,
    /// Body type.
    pub body_type: BodyType,
    /// Linear velocity.
    pub linear_velocity: Vec3,
    /// Angular velocity.
    pub angular_velocity: Vec3,
    /// Mass of the body in kilograms.
    pub mass: f32,
    /// Whether to enable Continuous Collision Detection (CCD).
    pub ccd_enabled: bool,
}

/// Description for creating a collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderDesc {
    /// The entity this collider belongs to.
    ///
    /// Stamped into the backend so a contact can name **who** touched whom.
    /// Without it a collision arrives as two backend handles and the only way
    /// back to entities is to scan every collider component comparing handles —
    /// linear per contact, and ambiguous the moment a slot is recycled.
    ///
    /// `None` for a collider the ECS does not own, which nothing creates today
    /// and a tool or a query volume might.
    pub owner: Option<crate::ecs::entity::EntityId>,
    /// Parent rigid body to attach to (if any).
    pub parent_body: Option<RigidBodyHandle>,
    /// Relative or absolute position.
    pub position: Vec3,
    /// Relative or absolute rotation.
    pub rotation: Quat,
    /// Shape definition.
    pub shape: ColliderShape,
    /// Whether to enable collision events for this collider.
    pub active_events: bool,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
}

/// Supported collider shapes.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum ColliderShape {
    /// Box with half-extents.
    Box(Vec3),
    /// Sphere with radius.
    Sphere(f32),
    /// Capsule with half-height and radius.
    Capsule(f32, f32),
}

impl ColliderShape {
    /// Computes the axis-aligned bounding box (AABB) for this shape in local space.
    pub fn compute_aabb(&self) -> crate::math::Aabb {
        match self {
            ColliderShape::Box(half_extents) => crate::math::Aabb::from_half_extents(*half_extents),
            ColliderShape::Sphere(radius) => {
                crate::math::Aabb::from_half_extents(Vec3::new(*radius, *radius, *radius))
            }
            ColliderShape::Capsule(half_height, radius) => {
                let r = Vec3::new(*radius, *radius, *radius);
                let h = Vec3::new(0.0, *half_height, 0.0);
                crate::math::Aabb::from_half_extents(r + h)
            }
        }
    }
}

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

/// Options for resolving kinematic character movement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct CharacterControllerOptions {
    /// Max height of obstacles the character can step over.
    pub autostep_height: f32,
    /// Min width of obstacles for autostepping.
    pub autostep_min_width: f32,
    /// Whether autostepping is enabled.
    pub autostep_enabled: bool,
    /// Max angle for climbing slopes.
    pub max_slope_climb_angle: f32,
    /// Min angle for sliding down slopes.
    pub min_slope_slide_angle: f32,
    /// Distance to maintain from obstacles.
    pub offset: f32,
}

/// Events representing collision start/end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub enum CollisionEvent {
    /// Collision between two colliders started.
    Started(ColliderHandle, ColliderHandle),
    /// Collision between two colliders stopped.
    Stopped(ColliderHandle, ColliderHandle),
}

/// A ray in 3D space.
///
/// Re-exported from [`crate::math`], where it lives alongside the intersection
/// tests: physics is only one of its callers — editor picking and gizmo
/// manipulation cast the same rays and must not reinvent the math.
pub use crate::math::Ray;

/// Information about a raycast hit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub struct RaycastHit {
    /// The collider that was hit.
    pub collider: ColliderHandle,
    /// Distance from ray origin to hit point.
    pub distance: f32,
    /// Normal vector at the hit point.
    pub normal: Vec3,
    /// Exact position of the hit.
    pub position: Vec3,
}

/// A simple line for debug rendering.
#[derive(Debug, Clone, Copy)]
pub struct DebugLine {
    /// Start point.
    pub start: Vec3,
    /// End point.
    pub end: Vec3,
    /// Color.
    pub color: LinearRgba,
}

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
            Slot {
                index: 7,
                generation: 0,
            }
            .pack(),
        );
        let recycled = ColliderHandle(
            Slot {
                index: 7,
                generation: 1,
            }
            .pack(),
        );

        assert_ne!(first, recycled, "same slot, different occupant");
    }

    #[test]
    fn a_handle_round_trips_its_slot() {
        let slot = Slot {
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
        let slot = Slot {
            index: 42,
            generation: 0,
        };

        assert_eq!(slot.pack(), 42);
    }
}
