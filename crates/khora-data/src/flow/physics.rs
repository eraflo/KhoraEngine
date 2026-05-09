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
//! The Flow drives **two** kinds of work in `Flow::adapt`:
//!
//! 1. **AGDF relevance gating** — entities that drift outside the active
//!    camera's "physics scope" have their `RigidBody` detached via CRPECS;
//!    entities that drift back inside have it restored from a stashed copy
//!    (with hysteresis to prevent thrashing).
//! 2. **ECS → Physics-provider sync** — registers `RigidBody`s and
//!    `Collider`s with the [`PhysicsProvider`], updates their existing
//!    handles, and cleans up orphaned handles. This was previously done
//!    inside `StandardPhysicsLane::sync_to_world` — moving it here
//!    respects the CLAD rule "lanes must not query the World directly".
//!    Component-handle field updates (`rb.handle = Some(...)`) are
//!    performed in the same pass via `&mut World`.
//!
//! `Flow::project` publishes a small statistics view consumed by telemetry
//! and the editor.
//!
//! The matching `physics_world_writeback` `DataSystem` (in
//! [`crate::ecs::systems::physics_world_writeback`], `Maintenance` phase)
//! runs after the lane's `provider.step(dt)` and pulls the new transforms,
//! kinematic results, and collision events from the provider back into
//! the World.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::control::gorna::ResourceBudget;
use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::physics::{
    ColliderDesc, ColliderHandle, PhysicsProvider, RigidBodyDesc, RigidBodyHandle,
};
use khora_core::Runtime;

use crate::ecs::{
    ActiveEvents, Camera, Collider, GlobalTransform, Parent, PhysicsMaterial, RigidBody,
    SemanticDomain, World,
};
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
        runtime: &Runtime,
    ) {
        // Pass A — AGDF: detach / reattach RigidBody by relevance.
        self.adapt_agdf(world);

        // Pass B — sync ECS → physics provider so the lane's
        // `provider.step(dt)` sees the current entity state. This was
        // previously `StandardPhysicsLane::sync_to_world`.
        if let Some(provider_arc) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()
        {
            let provider_arc = provider_arc.clone();
            let mut guard = match provider_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    log::error!("PhysicsFlow: provider mutex poisoned: {}", e);
                    return;
                }
            };
            sync_to_provider(world, guard.as_mut());
        }
    }

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        let active_bodies = world.query::<&RigidBody>().count();
        PhysicsView {
            active_bodies,
            stashed_bodies: self.stash.len(),
            camera_anchor: active_camera_position(world),
        }
    }
}

register_flow!(PhysicsFlow);

impl PhysicsFlow {
    /// AGDF relevance gating: detach RigidBody from entities outside the
    /// active camera's scope, restore from stash on re-entry.
    fn adapt_agdf(&mut self, world: &mut World) {
        let Some(anchor) = active_camera_position(world) else {
            return;
        };

        // Pass 1 — detach.
        let mut to_detach: Vec<(EntityId, RigidBody)> = Vec::new();
        for (entity, transform, rb) in world.query::<(EntityId, &GlobalTransform, &RigidBody)>() {
            if (transform.0.translation() - anchor).length() > DETACH_RADIUS {
                to_detach.push((entity, rb.clone()));
            }
        }
        for (entity, snapshot) in to_detach {
            self.stash.insert(entity, snapshot);
            let _ = world.remove_component::<RigidBody>(entity);
        }

        // Pass 2 — reattach.
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
                    self.stash.insert(entity, snapshot);
                }
            }
        }
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
