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

//! `PhysicsFlow` — read-only projection of the physics domain.
//!
//! `Flow::project` publishes a small [`PhysicsView`] (statistics) consumed by
//! telemetry and the editor. Per CLAD, a Flow never mutates the World.
//!
//! The **ECS → physics-provider sync** (registering `RigidBody`s / `Collider`s
//! with the [`PhysicsProvider`], updating handles, cleaning up orphans) is a
//! maintenance invariant, so it lives in the `physics_provider_sync`
//! `DataSystem` below (`PreExtract` phase, before the physics lane steps), not
//! in the Flow.
//!
//! Distance-based *gameplay* gating (detaching a `RigidBody` when far from the
//! camera) is **not** done here: it changes the simulation, so it is
//! developer-authored (opt-in), never an automatic engine default. See
//! `.agent/rules.md` — *adapt the HOW, never the WHAT*.
//!
//! The matching `physics_world_writeback` `DataSystem`
//! ([`crate::ecs::systems::physics_world_writeback`], `Maintenance` phase) runs
//! after the lane's `provider.step(dt)` and pulls the new transforms, kinematic
//! results, and collision events from the provider back into the World.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::math::Vec3;
use khora_core::physics::{
    ColliderDesc, ColliderHandle, PhysicsProvider, RigidBodyDesc, RigidBodyHandle,
};
use khora_core::Runtime;

use crate::ecs::{
    ActiveEvents, Camera, Collider, DataSystemRegistration, GlobalTransform, Parent,
    PhysicsMaterial, RigidBody, SemanticDomain, TickPhase, World,
};
use crate::flow::{Flow, Selection};
use crate::register_flow;

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

/// Slot type written into [`OutputDeck`](khora_core::lane::OutputDeck)
/// by `StandardPhysicsLane` after `provider.step(dt)` completes.
///
/// Drained in `Maintenance` by the `physics_world_writeback` DataSystem,
/// which uses its presence to decide whether the simulation actually
/// advanced this frame (and therefore whether to pull fresh transforms
/// out of the provider). When the agent skips its lane (paused agent,
/// budget exhaustion …) no `PhysicsStepResult` lands in the deck and the
/// writeback no-ops.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhysicsStepResult {
    /// The simulation timestep that was advanced.
    pub dt: f32,
}

/// Read-only physics presentation Flow.
#[derive(Default)]
pub struct PhysicsFlow;

impl Flow for PhysicsFlow {
    type View = PhysicsView;

    const DOMAIN: SemanticDomain = SemanticDomain::Physics;
    const NAME: &'static str = "physics";

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        let active_bodies = world.query::<&RigidBody>().count();
        PhysicsView {
            active_bodies,
            // No automatic AGDF gameplay gating — nothing is stashed.
            stashed_bodies: 0,
            camera_anchor: active_camera_position(world),
        }
    }
}

register_flow!(PhysicsFlow);

/// `DataSystem` (`PreExtract`) — syncs the ECS physics state into the
/// [`PhysicsProvider`] backend before the physics lane steps it. This is the
/// maintenance work that used to live in `PhysicsFlow::adapt`; moving it out
/// keeps the Flow a read-only projector.
fn physics_provider_sync(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(provider_arc) = runtime
        .backends
        .get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()
    else {
        return;
    };
    let provider_arc = provider_arc.clone();
    let mut guard = match provider_arc.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!("physics_provider_sync: provider mutex poisoned: {}", e);
            return;
        }
    };
    sync_to_provider(world, guard.as_mut());
}

inventory::submit! {
    DataSystemRegistration {
        name: "physics_provider_sync",
        phase: TickPhase::PreExtract,
        run: physics_provider_sync,
        order_hint: 0,
        runs_after: &[],
    }
}

fn active_camera_position(world: &World) -> Option<Vec3> {
    for (camera, transform) in world.query::<(&Camera, &GlobalTransform)>() {
        if camera.is_active {
            return Some(transform.0.translation());
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
// ECS → Physics provider sync (was StandardPhysicsLane::sync_to_world)
// ─────────────────────────────────────────────────────────────────────

/// Registers / updates every `RigidBody` and `Collider` in `world` with
/// `provider`, and cleans up orphaned handles. Mutates entity component
/// fields in place (`rb.handle`, `collider.handle`) so the matching
/// `physics_world_writeback` DataSystem can find them later.
fn sync_to_provider(world: &mut World, provider: &mut dyn PhysicsProvider) {
    let mut active_bodies = HashSet::new();
    let mut active_colliders = HashSet::new();

    let rb_map = sync_rigid_bodies(world, provider, &mut active_bodies);
    sync_colliders(world, provider, &mut active_colliders, &rb_map);
    cleanup_orphans(provider, &active_bodies, &active_colliders);
}

fn sync_rigid_bodies(
    world: &mut World,
    provider: &mut dyn PhysicsProvider,
    active_bodies: &mut HashSet<RigidBodyHandle>,
) -> HashMap<EntityId, RigidBodyHandle> {
    let mut rb_map = HashMap::new();
    let query = world.query_mut::<(EntityId, &GlobalTransform, &mut RigidBody)>();

    for (entity_id, transform, rb) in query {
        let current_pos = transform.0.translation();
        let current_rot = transform.0.rotation();

        let desc = RigidBodyDesc {
            position: current_pos,
            rotation: current_rot,
            body_type: rb.body_type,
            linear_velocity: rb.linear_velocity,
            angular_velocity: rb.angular_velocity,
            mass: rb.mass,
            ccd_enabled: rb.ccd_enabled,
        };

        let handle = if let Some(handle) = rb.handle {
            // Teleport detection.
            let (phys_pos, phys_rot) = provider.get_body_transform(handle);
            if (phys_pos - current_pos).length_squared() > 0.0001
                || phys_rot.dot(current_rot).abs() < 0.9999
            {
                provider.set_body_transform(handle, current_pos, current_rot);
            }
            provider.update_body_properties(handle, desc);
            handle
        } else {
            let h = provider.add_body(desc);
            rb.handle = Some(h);
            h
        };

        rb_map.insert(entity_id, handle);
        active_bodies.insert(handle);
    }
    rb_map
}

fn sync_colliders(
    world: &mut World,
    provider: &mut dyn PhysicsProvider,
    active_colliders: &mut HashSet<ColliderHandle>,
    rb_map: &HashMap<EntityId, RigidBodyHandle>,
) {
    let mut parent_map = HashMap::new();
    for (id, parent) in world.query::<(EntityId, &Parent)>() {
        parent_map.insert(id, parent.0);
    }

    let mut parent_transforms = HashMap::new();
    for (id, gt) in world.query::<(EntityId, &GlobalTransform)>() {
        parent_transforms.insert(id, *gt);
    }

    let mut active_events = HashSet::new();
    for (id, _) in world.query::<(EntityId, &ActiveEvents)>() {
        active_events.insert(id);
    }

    let mut materials = HashMap::new();
    for (id, mat) in world.query::<(EntityId, &PhysicsMaterial)>() {
        materials.insert(id, *mat);
    }

    let query = world.query_mut::<(EntityId, &mut Collider, &GlobalTransform)>();
    for (entity_id, collider, transform) in query {
        let is_active = active_events.contains(&entity_id);
        let material = materials.get(&entity_id).cloned().unwrap_or_default();

        let desc = build_collider_desc(
            entity_id,
            transform,
            collider,
            &parent_map,
            &parent_transforms,
            is_active,
            &material,
            rb_map,
        );

        let handle = if let Some(handle) = collider.handle {
            provider.update_collider_properties(handle, desc);
            handle
        } else {
            let h = provider.add_collider(desc);
            collider.handle = Some(h);
            h
        };

        active_colliders.insert(handle);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_collider_desc(
    entity_id: EntityId,
    transform: &GlobalTransform,
    collider: &Collider,
    parent_map: &HashMap<EntityId, EntityId>,
    parent_transforms: &HashMap<EntityId, GlobalTransform>,
    active_events: bool,
    material: &PhysicsMaterial,
    rb_map: &HashMap<EntityId, RigidBodyHandle>,
) -> ColliderDesc {
    let (parent_handle, parent_id) = find_parent_body(entity_id, parent_map, rb_map);
    let mut pos = transform.0.translation();
    let mut rot = transform.0.rotation();

    if let Some(p_id) = parent_id {
        if p_id != entity_id {
            if let Some(p_global) = parent_transforms.get(&p_id) {
                if let Some(inv_p) = p_global.0.inverse() {
                    let local = inv_p.0 * transform.0 .0;
                    let local_t = khora_core::math::AffineTransform(local);
                    pos = local_t.translation();
                    rot = local_t.rotation();
                }
            }
        }
    }

    ColliderDesc {
        parent_body: parent_handle,
        position: pos,
        rotation: rot,
        shape: collider.shape.clone(),
        active_events,
        friction: material.friction,
        restitution: material.restitution,
    }
}

fn find_parent_body(
    entity_id: EntityId,
    parent_map: &HashMap<EntityId, EntityId>,
    rb_map: &HashMap<EntityId, RigidBodyHandle>,
) -> (Option<RigidBodyHandle>, Option<EntityId>) {
    let mut curr = entity_id;
    loop {
        if let Some(h) = rb_map.get(&curr) {
            return (Some(*h), Some(curr));
        }
        if let Some(p) = parent_map.get(&curr) {
            curr = *p;
        } else {
            break;
        }
    }
    (None, None)
}

fn cleanup_orphans(
    provider: &mut dyn PhysicsProvider,
    active_bodies: &HashSet<RigidBodyHandle>,
    active_colliders: &HashSet<ColliderHandle>,
) {
    for h in provider.get_all_bodies() {
        if !active_bodies.contains(&h) {
            provider.remove_body(h);
        }
    }
    for h in provider.get_all_colliders() {
        if !active_colliders.contains(&h) {
            provider.remove_collider(h);
        }
    }
}
