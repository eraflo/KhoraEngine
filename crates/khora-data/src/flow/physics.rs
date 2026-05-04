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

//! `PhysicsFlow` — first realisation of AGDF in the engine.
//!
//! `Flow::adapt` performs **structural mutations** on the World based on
//! contextual relevance: entities that drift outside the active camera's
//! "physics scope" have their `RigidBody` (and the rest of their Physics
//! domain) detached via CRPECS; entities that drift back inside have it
//! restored from a stashed copy. Hysteresis (separate `enter` / `exit`
//! thresholds) prevents thrashing at the boundary.
//!
//! The accompanying `PhysicsView` reports per-frame statistics — number of
//! active bodies, number of stashed bodies, and the camera anchor used —
//! for telemetry and editor introspection.

use std::collections::HashMap;

use khora_core::control::gorna::ResourceBudget;
use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::ServiceRegistry;

use crate::ecs::{Camera, GlobalTransform, RigidBody, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;

/// Distance beyond which an entity's physics is detached.
const DETACH_RADIUS: f32 = 50.0;

/// Distance below which a previously-detached entity has its physics
/// restored. Smaller than [`DETACH_RADIUS`] to provide hysteresis.
const REATTACH_RADIUS: f32 = 30.0;

/// View published into the [`LaneBus`](khora_core::lane::LaneBus) by
/// `PhysicsFlow`. Carries per-frame physics statistics for downstream
/// consumers (telemetry, editor panels).
#[derive(Debug, Default, Clone)]
pub struct PhysicsView {
    /// Number of entities currently carrying an active `RigidBody`.
    pub active_bodies: usize,
    /// Number of entities whose `RigidBody` has been stashed by AGDF.
    pub stashed_bodies: usize,
    /// World-space camera position used as the relevance anchor.
    pub camera_anchor: Option<Vec3>,
}

/// AGDF-aware physics presentation Flow.
#[derive(Default)]
pub struct PhysicsFlow {
    /// Stashed `RigidBody` components, keyed by entity. Restored when the
    /// entity comes back inside the reattach radius.
    stash: HashMap<EntityId, RigidBody>,
}

impl Flow for PhysicsFlow {
    type View = PhysicsView;

    const DOMAIN: SemanticDomain = SemanticDomain::Physics;
    const NAME: &'static str = "physics";

    fn adapt(
        &mut self,
        world: &mut World,
        _sel: &Selection,
        _budget: &ResourceBudget,
        _services: &ServiceRegistry,
    ) {
        let Some(anchor) = active_camera_position(world) else {
            return;
        };

        // Pass 1 — detach: snapshot live RigidBodies whose entity drifted
        //          outside DETACH_RADIUS, stash them, then drop their
        //          Physics-domain components in a second pass (we cannot
        //          mutate while holding the query iterator).
        let mut to_detach: Vec<(EntityId, RigidBody)> = Vec::new();
        for (entity, transform, rb) in world.query::<(EntityId, &GlobalTransform, &RigidBody)>() {
            if (transform.0.translation() - anchor).length() > DETACH_RADIUS {
                to_detach.push((entity, rb.clone()));
            }
        }
        for (entity, snapshot) in to_detach {
            self.stash.insert(entity, snapshot);
            // Surgical single-component removal — keeps Transform /
            // GlobalTransform / Name etc. intact. Was previously a domain
            // wipe (data loss) before `World::remove_component` existed.
            let _ = world.remove_component::<RigidBody>(entity);
        }

        // Pass 2 — reattach: any stashed entity that drifted back inside
        //          REATTACH_RADIUS gets its RigidBody restored.
        let restorable: Vec<EntityId> = self
            .stash
            .keys()
            .copied()
            .filter(|e| {
                world
                    .get::<GlobalTransform>(*e)
                    .map(|t| (t.0.translation() - anchor).length() < REATTACH_RADIUS)
                    .unwrap_or(false)
            })
            .collect();
        for entity in restorable {
            if let Some(rb) = self.stash.remove(&entity) {
                let snapshot = rb.clone();
                if let Err(e) = world.add_component(entity, rb) {
                    log::warn!(
                        "PhysicsFlow: failed to reattach RigidBody to {:?}: {:?}",
                        entity,
                        e
                    );
                    // Re-stash so we don't lose the snapshot.
                    self.stash.insert(entity, snapshot);
                }
            }
        }
    }

    fn project(&self, world: &World, _sel: &Selection, _services: &ServiceRegistry) -> Self::View {
        let active_bodies = world.query::<&RigidBody>().count();
        PhysicsView {
            active_bodies,
            stashed_bodies: self.stash.len(),
            camera_anchor: active_camera_position(world),
        }
    }
}

register_flow!(PhysicsFlow);

fn active_camera_position(world: &World) -> Option<Vec3> {
    for (camera, transform) in world.query::<(&Camera, &GlobalTransform)>() {
        if camera.is_active {
            return Some(transform.0.translation());
        }
    }
    None
}
