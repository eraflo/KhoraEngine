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

mod collider_aliases;
mod conversions;
mod debug;
mod events;

use khora_core::physics::{ColliderHandle, RigidBodyHandle, SlotId};

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
    RigidBodyHandle(SlotId { index, generation }.pack())
}

fn to_rapier_cl_handle(handle: ColliderHandle) -> rapier3d::geometry::ColliderHandle {
    let slot = handle.slot();
    rapier3d::geometry::ColliderHandle::from_raw_parts(slot.index, slot.generation)
}

pub(super) fn from_rapier_cl_handle(handle: rapier3d::geometry::ColliderHandle) -> ColliderHandle {
    let (index, generation) = handle.into_raw_parts();
    ColliderHandle(SlotId { index, generation }.pack())
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
mod tests;
