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

//! Physics Lane
//!
//! The physics lane is responsible for synchronizing the physics world with the ECS world.

mod native_lanes;
mod physics_debug_lane;

pub use native_lanes::*;
pub use physics_debug_lane::*;

use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::physics::{ColliderDesc, PhysicsProvider, RigidBodyDesc};
use khora_data::ecs::{Collider, GlobalTransform, Parent, RigidBody, Transform, World};

/// The standard physics lane for industrial-grade simulation.
#[derive(Debug, Default)]
pub struct StandardPhysicsLane;

impl StandardPhysicsLane {
    /// Creates a new `StandardPhysicsLane`.
    pub fn new() -> Self {
        Self
    }

    /// Synchronizes components from ECS to the physics provider.
    fn sync_to_world(&self, world: &mut World, provider: &mut dyn PhysicsProvider) {
        let mut active_bodies = HashSet::new();
        let mut active_colliders = HashSet::new();

        // 1. Sync RigidBodies
        let rb_map = self.sync_rigid_bodies(world, provider, &mut active_bodies);

        // 2. Sync Colliders (requires hierarchy search)
        self.sync_colliders(world, provider, &mut active_colliders, &rb_map);

        // 3. Cleanup Orphaned Handles
        self.cleanup_orphans(provider, &active_bodies, &active_colliders);
    }

    fn sync_rigid_bodies(
        &self,
        world: &mut World,
        provider: &mut dyn PhysicsProvider,
        active_bodies: &mut HashSet<khora_core::physics::RigidBodyHandle>,
    ) -> HashMap<EntityId, khora_core::physics::RigidBodyHandle> {
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
                // Teleport detection
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
        &self,
        world: &mut World,
        provider: &mut dyn PhysicsProvider,
        active_colliders: &mut HashSet<khora_core::physics::ColliderHandle>,
        rb_map: &HashMap<EntityId, khora_core::physics::RigidBodyHandle>,
    ) {
        // Collect a hierarchy map for efficient parent body search.
        let mut parent_map = HashMap::new();
        for (id, parent) in world.query::<(EntityId, &Parent)>() {
            parent_map.insert(id, parent.0);
        }

        // Pre-collect components that might be on OTHER entities than the one carrying the collider.
        // Specifically, we need the GlobalTransform of any entity that is a parent of a collider.
        let mut parent_transforms = HashMap::new();
        for (id, gt) in world.query::<(EntityId, &GlobalTransform)>() {
            // Optimization: We only really need these for entities that are in the rb_map
            // or are parents of colliders. For simplicity and correctness with the SoA query,
            // we collect all, but a future optimization could filter this.
            parent_transforms.insert(id, *gt);
        }

        // Collect optional physics state for entities with colliders.
        let mut active_events = HashSet::new();
        for (id, _) in world.query::<(EntityId, &khora_data::ecs::ActiveEvents)>() {
            active_events.insert(id);
        }

        let mut materials = HashMap::new();
        for (id, mat) in world.query::<(EntityId, &khora_data::ecs::PhysicsMaterial)>() {
            materials.insert(id, *mat);
        }

        let query = world.query_mut::<(EntityId, &mut Collider, &GlobalTransform)>();
        for (entity_id, collider, transform) in query {
            let is_active = active_events.contains(&entity_id);
            let material = materials.get(&entity_id).cloned().unwrap_or_default();

            let desc = self.build_collider_desc(
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
        &self,
        entity_id: EntityId,
        transform: &GlobalTransform,
        collider: &Collider,
        parent_map: &HashMap<EntityId, EntityId>,
        parent_transforms: &HashMap<EntityId, GlobalTransform>,
        active_events: bool,
        material: &khora_data::ecs::PhysicsMaterial,
        rb_map: &HashMap<EntityId, khora_core::physics::RigidBodyHandle>,
    ) -> ColliderDesc {
        let (parent_handle, parent_id) = self.find_parent_body(entity_id, parent_map, rb_map);
        let mut pos = transform.0.translation();
        let mut rot = transform.0.rotation();

        // If attached to a parent body, we must use the relative transform
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
        &self,
        entity_id: EntityId,
        parent_map: &HashMap<EntityId, EntityId>,
        rb_map: &HashMap<EntityId, khora_core::physics::RigidBodyHandle>,
    ) -> (
        Option<khora_core::physics::RigidBodyHandle>,
        Option<EntityId>,
    ) {
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
        &self,
        provider: &mut dyn PhysicsProvider,
        active_bodies: &HashSet<khora_core::physics::RigidBodyHandle>,
        active_colliders: &HashSet<khora_core::physics::ColliderHandle>,
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

    /// Synchronizes components from the physics provider back to ECS.
    fn sync_from_world(&self, world: &mut World, provider: &dyn PhysicsProvider) {
        let query = world.query_mut::<(&mut Transform, &mut RigidBody)>();
        for (transform, rb) in query {
            if let Some(handle) = rb.handle {
                let (pos, rot) = provider.get_body_transform(handle);
                // Update transform
                transform.translation = pos;
                transform.rotation = rot;
            }
        }
    }

    fn resolve_characters(&self, world: &mut World, provider: &dyn PhysicsProvider) {
        let mut results = Vec::new();
        {
            let query = world.query_mut::<(
                EntityId,
                &mut khora_data::ecs::KinematicCharacterController,
                &Collider,
            )>();
            for (id, kcc, collider) in query {
                if let Some(h) = collider.handle {
                    let options = khora_core::physics::CharacterControllerOptions {
                        autostep_height: kcc.autostep_height,
                        autostep_min_width: kcc.autostep_min_width,
                        autostep_enabled: kcc.autostep_enabled,
                        max_slope_climb_angle: kcc.max_slope_climb_angle,
                        min_slope_slide_angle: kcc.min_slope_slide_angle,
                        offset: kcc.offset,
                    };
                    let (m, g) = provider.move_character(h, kcc.desired_translation, &options);
                    results.push((id, m, g));
                }
            }
        }

        for (id, m, g) in results {
            if let Some(kcc) = world.get_mut::<khora_data::ecs::KinematicCharacterController>(id) {
                kcc.is_grounded = g;
                kcc.desired_translation = khora_core::math::Vec3::ZERO;
            }
            if let Some(transform) = world.get_mut::<Transform>(id) {
                transform.translation = transform.translation + m;
            }
        }
    }

    fn dispatch_events(&self, world: &mut World, provider: &dyn PhysicsProvider) {
        let events = provider.get_collision_events();
        let query = world.query_mut::<(EntityId, &mut khora_data::ecs::CollisionEvents)>();
        for (_, buffer) in query {
            if events.is_empty() {
                buffer.events.clear();
            } else {
                buffer.events = events.clone();
            }
        }
    }
}

impl khora_core::lane::Lane for StandardPhysicsLane {
    fn strategy_name(&self) -> &'static str {
        "StandardPhysics"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Physics
    }

    fn execute(&self, ctx: &mut khora_core::lane::LaneContext) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Slot};

        let dt = ctx.get::<khora_core::lane::PhysicsDeltaTime>()
            .ok_or(LaneError::missing("PhysicsDeltaTime"))?.0;
        let world = ctx.get::<Slot<World>>()
            .ok_or(LaneError::missing("Slot<World>"))?.get();
        let provider = ctx.get::<Slot<dyn PhysicsProvider>>()
            .ok_or(LaneError::missing("Slot<dyn PhysicsProvider>"))?.get();

        self.step(world, provider, dt);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl StandardPhysicsLane {
    /// Executes the full physics step: sync, simulate, writeback, characters, events.
    pub fn step(&self, world: &mut World, provider: &mut dyn PhysicsProvider, dt: f32) {
        // 1. Sync ECS -> Physics World
        self.sync_to_world(world, provider);

        // 2. Simulate
        provider.step(dt);

        // 3. Sync Physics World -> ECS (Transforms)
        self.sync_from_world(world, provider);

        // 4. Kinematic Character Movement
        self.resolve_characters(world, provider);

        // 5. Collision Events
        self.dispatch_events(world, provider);
    }
}
