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

//! ⚠️ **Orphan / experimental** — `NativeBroadphaseLane` and
//! `NativeSolverLane` are NOT wired into any agent. Only
//! [`CollisionPairsResource`] is kept live (registered eagerly in
//! `khora-sdk::engine` so any in-flight wiring of these lanes finds the
//! sink already present).
//!
//! These lanes predate the `LaneBus` / `OutputDeck` substrate and query
//! the `World` directly (a violation of CLAD rule R2 — *Lanes consume
//! Views, never `world.query*`*). Reviving them as the canonical native
//! physics backend is part of the deferred P1.b / P1.c plan, which will
//! either:
//! 1. Migrate them to read a `BroadphaseView` / `NarrowphaseView` from
//!    [`LaneBus`](khora_core::lane::LaneBus) and write a typed slot into
//!    [`OutputDeck`](khora_core::lane::OutputDeck), drained by a
//!    `Maintenance` `DataSystem`, OR
//! 2. Delete this file entirely if Rapier remains the only physics
//!    backend.
//!
//! Until that decision is made, this module is dead-code-friendly: no
//! agent registers either lane, and the type alias above is the only
//! cross-crate consumer.

#![allow(dead_code)]

use khora_core::ecs::entity::EntityId;
use khora_core::physics::{
    ContactManifold, DynamicTree, ImpulseSolver, NarrowPhase, VelocityState,
};
use khora_data::ecs::{Collider, GlobalTransform, RigidBody, World};
use khora_data::physics::{CollisionPair, CollisionPairs};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Shared sink for broadphase output. Lives in
/// [`khora_core::Resources`]; the broadphase lane writes into it, the
/// solver lane reads from it. Replaces the previous singleton ECS
/// component which polluted the editor scene tree.
pub type CollisionPairsResource = Arc<Mutex<CollisionPairs>>;

/// The Broadphase Lane manages spatial partitioning and potential collision pair generation.
/// It maintains a persistent Dynamic AABB Tree to minimize update costs.
pub struct NativeBroadphaseLane {
    /// Spatial hierarchy for efficient overlap queries.
    /// Wrapped in RwLock for thread-safe access if multiple lanes query it.
    tree: RwLock<DynamicTree<EntityId>>,
    /// Mapping from EntityId to tree node handle for efficient updates.
    handles: RwLock<HashMap<EntityId, i32>>,
}

impl Default for NativeBroadphaseLane {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeBroadphaseLane {
    /// Creates a new `NativeBroadphaseLane`.
    pub fn new() -> Self {
        Self {
            tree: RwLock::new(DynamicTree::new()),
            handles: RwLock::new(HashMap::new()),
        }
    }

    /// Executes the broad-phase step: updates the tree and generates collision pairs.
    pub fn step(&self, world: &mut World, pairs_sink: &CollisionPairsResource) {
        // 1. Sync ECS components to the Dynamic Tree
        self.sync_tree(world);

        // 2. Clear old pairs and generate new ones — written into the
        //    shared resource (no ECS mutation).
        self.generate_pairs(pairs_sink);
    }

    fn sync_tree(&self, world: &mut World) {
        let mut tree = self.tree.write().unwrap();
        let mut handles = self.handles.write().unwrap();

        // Track seen entities to remove dead ones
        let mut current_entities = std::collections::HashSet::new();

        // Query entities with Collider and GlobalTransform
        let query = world.query::<(EntityId, &Collider, &GlobalTransform)>();
        for (entity_id, collider, transform) in query {
            let world_aabb = collider.shape.compute_aabb().transform(&transform.0 .0);
            current_entities.insert(entity_id);

            let displacement = world
                .get::<RigidBody>(entity_id)
                .map(|b| b.linear_velocity)
                .unwrap_or(khora_core::math::Vec3::ZERO);

            if let Some(&handle) = handles.get(&entity_id) {
                // Update with displacement prediction if it's a dynamic body
                tree.update(handle, world_aabb, displacement, false);
            } else {
                let handle = tree.insert(world_aabb, entity_id);
                handles.insert(entity_id, handle);
            }
        }

        // Cleanup removed entities
        let mut to_remove = Vec::new();
        for &entity_id in handles.keys() {
            if !current_entities.contains(&entity_id) {
                to_remove.push(entity_id);
            }
        }

        for entity_id in to_remove {
            if let Some(handle) = handles.remove(&entity_id) {
                tree.remove(handle);
            }
        }
    }

    fn generate_pairs(&self, pairs_sink: &CollisionPairsResource) {
        let tree = self.tree.read().unwrap();
        let mut all_pairs = Vec::new();

        tree.query_pairs(|&a, &b| {
            all_pairs.push(CollisionPair {
                entity_a: a,
                entity_b: b,
            });
        });

        // Write into the shared `Arc<Mutex<CollisionPairs>>` Resource.
        // The solver lane reads the same Arc to drive narrow-phase
        // resolution. No ECS mutation happens here — broadphase output
        // is transient scratch, not entity data.
        match pairs_sink.lock() {
            Ok(mut sink) => {
                sink.pairs = all_pairs;
            }
            Err(e) => {
                log::error!(
                    "NativeBroadphaseLane: collision-pairs sink poisoned: {}",
                    e
                );
            }
        }
    }
}

impl khora_core::lane::Lane for NativeBroadphaseLane {
    fn strategy_name(&self) -> &'static str {
        "NativeBroadphase"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Physics
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Slot};

        let world = ctx
            .get::<Slot<World>>()
            .ok_or(LaneError::missing("Slot<World>"))?
            .get();
        let pairs_sink = ctx
            .get::<CollisionPairsResource>()
            .ok_or(LaneError::missing("CollisionPairsResource"))?;

        self.step(world, pairs_sink);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The Solver Lane resolves collisions and updates velocities/positions.
/// It uses the Sequential Impulse method for stable constraint resolution.
pub struct NativeSolverLane {
    solver: SequentialImpulseSolver,
}

impl Default for NativeSolverLane {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSolverLane {
    /// Creates a new `NativeSolverLane`.
    pub fn new() -> Self {
        Self {
            solver: SequentialImpulseSolver::new(),
        }
    }

    /// Executes the solver step: integrates velocities, resolves
    /// collisions (using the shared broadphase pairs sink), and
    /// integrates positions.
    pub fn step(&self, world: &mut World, dt: f32, pairs_sink: &CollisionPairsResource) {
        // 1. Integrate Forces (Gravity, etc.)
        self.integrate_velocities(world, dt);

        // 2. Resolve Constraints (Collisions) — pulls pairs from the
        //    shared resource written by `NativeBroadphaseLane`.
        self.solver.solve_collisions(world, pairs_sink);

        // 3. Integrate Positions
        self.integrate_positions(world, dt);
    }

    fn integrate_velocities(&self, world: &mut World, dt: f32) {
        let gravity = khora_core::math::Vec3::new(0.0, -9.81, 0.0);
        let query = world.query_mut::<&mut RigidBody>();
        for rb in query {
            if rb.body_type == khora_core::physics::BodyType::Dynamic {
                // v = v + a*dt
                rb.linear_velocity = rb.linear_velocity + (gravity * dt);
            }
        }
    }

    fn integrate_positions(&self, world: &mut World, dt: f32) {
        let query = world.query_mut::<(&mut khora_data::ecs::Transform, &RigidBody)>();
        for (transform, rb) in query {
            if rb.body_type == khora_core::physics::BodyType::Dynamic {
                // Integrate translation
                transform.translation = transform.translation + (rb.linear_velocity * dt);

                // Integrate rotation: dq = [w * dt / 2, 1] * q
                let angular_velocity = rb.angular_velocity;
                let w_mag = angular_velocity.length();
                if w_mag > 0.0001 {
                    let axis = angular_velocity / w_mag;
                    let angle = w_mag * dt;
                    let delta_rot = khora_core::math::Quat::from_axis_angle(axis, angle);
                    transform.rotation = delta_rot * transform.rotation;
                    transform.rotation = transform.rotation.normalize();
                }
            }
        }
    }
}

impl khora_core::lane::Lane for NativeSolverLane {
    fn strategy_name(&self) -> &'static str {
        "NativeSolver"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Physics
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Slot};

        let dt = ctx
            .get::<khora_core::lane::PhysicsDeltaTime>()
            .ok_or(LaneError::missing("PhysicsDeltaTime"))?
            .0;
        let world = ctx
            .get::<Slot<World>>()
            .ok_or(LaneError::missing("Slot<World>"))?
            .get();
        let pairs_sink = ctx
            .get::<CollisionPairsResource>()
            .ok_or(LaneError::missing("CollisionPairsResource"))?;

        self.step(world, dt, pairs_sink);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Encapsulates the Impulse-based constraint resolution logic.
/// This solver follows the Sequential Impulse method, which is a popular approach
/// for solving constraints (like collisions) by applying a series of impulses
/// until the system reaches a stable state.
struct SequentialImpulseSolver {
    narrow_phase: NarrowPhase,
    impulse_solver: ImpulseSolver,
}

impl SequentialImpulseSolver {
    fn new() -> Self {
        Self {
            narrow_phase: NarrowPhase::new(),
            impulse_solver: ImpulseSolver::new(),
        }
    }

    /// Iterates over all detected collision pairs and resolves them.
    /// This is the entry point for the collision resolution phase of the lane.
    fn solve_collisions(&self, world: &mut World, pairs_sink: &CollisionPairsResource) {
        // 1. Read potential collision pairs from the shared resource
        //    populated by `NativeBroadphaseLane`. We clone out the
        //    Vec so the lock is not held while we mutate the world.
        let candidates: Vec<CollisionPair> = match pairs_sink.lock() {
            Ok(g) => g.pairs.clone(),
            Err(e) => {
                log::error!(
                    "NativeSolverLane: collision-pairs sink poisoned: {}",
                    e
                );
                return;
            }
        };

        // 2. Process each pair sequentially.
        for pair in candidates {
            self.solve_pair(world, pair.entity_a, pair.entity_b);
        }
    }

    /// Performs narrow-phase detection and resolves a single pair of entities.
    fn solve_pair(&self, world: &mut World, a_id: EntityId, b_id: EntityId) {
        let mut manifold = None;

        // Scope the immutable borrows to allow mutable access to the world later.
        {
            let a_coll = world.get::<Collider>(a_id);
            let a_trans = world.get::<GlobalTransform>(a_id);
            let b_coll = world.get::<Collider>(b_id);
            let b_trans = world.get::<GlobalTransform>(b_id);

            if let (Some(ac), Some(at), Some(bc), Some(bt)) = (a_coll, a_trans, b_coll, b_trans) {
                // Perform intersection test using the NarrowPhase instance.
                manifold = self.narrow_phase.detect(&ac.shape, &at.0, &bc.shape, &bt.0);
            }
        }

        // If a collision was detected, apply the resolution impulse.
        if let Some(m) = manifold {
            // Retrieve both rigid bodies mutably using the safe multi-entity access API.
            let [a_rb_opt, b_rb_opt] = world.get_many_mut::<RigidBody, 2>([a_id, b_id]);

            match (a_rb_opt, b_rb_opt) {
                (Some(rb_a), Some(rb_b)) => {
                    self.apply_impulse(rb_a, rb_b, &m);
                }
                (Some(rb_a), None) => {
                    // Resolve against a static object (which has No RigidBody component).
                    self.apply_impulse_static(rb_a, &m);
                }
                (None, Some(rb_b)) => {
                    // Mirror of the above for symmetry.
                    self.apply_impulse_static(rb_b, &m.inverted());
                }
                _ => {}
            }
        }
    }

    /// Resolves two entities by delegating the math to the core ImpulseSolver.
    fn apply_impulse(&self, a: &mut RigidBody, b: &mut RigidBody, m: &ContactManifold) {
        let state_a = VelocityState {
            linear_velocity: a.linear_velocity,
            angular_velocity: a.angular_velocity,
            mass: a.mass,
            body_type: a.body_type,
        };

        let state_b = VelocityState {
            linear_velocity: b.linear_velocity,
            angular_velocity: b.angular_velocity,
            mass: b.mass,
            body_type: b.body_type,
        };

        let (new_a, new_b) = self.impulse_solver.resolve(state_a, state_b, m);

        a.linear_velocity = new_a.linear_velocity;
        b.linear_velocity = new_b.linear_velocity;
    }

    /// Resolves a dynamic body against a static one.
    fn apply_impulse_static(&self, a: &mut RigidBody, m: &ContactManifold) {
        let state_a = VelocityState {
            linear_velocity: a.linear_velocity,
            angular_velocity: a.angular_velocity,
            mass: a.mass,
            body_type: a.body_type,
        };

        // Represent static as a default state with Static body type.
        let state_static = VelocityState {
            linear_velocity: khora_core::math::Vec3::ZERO,
            angular_velocity: khora_core::math::Vec3::ZERO,
            mass: 0.0,
            body_type: khora_core::physics::BodyType::Static,
        };

        let (new_a, _) = self.impulse_solver.resolve(state_a, state_static, m);
        a.linear_velocity = new_a.linear_velocity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::{AffineTransform, Vec3};

    #[test]
    fn test_sphere_sphere_collision() {
        let sphere_a = Collider::new_sphere(1.0);
        let sphere_b = Collider::new_sphere(1.0);
        let trans_a = AffineTransform::from_translation(Vec3::new(0.0, 0.0, 0.0));
        let trans_b = AffineTransform::from_translation(Vec3::new(1.5, 0.0, 0.0));

        let narrow = NarrowPhase::new();
        let manifold = narrow
            .detect(&sphere_a.shape, &trans_a, &sphere_b.shape, &trans_b)
            .unwrap();
        assert!(manifold.depth > 0.0);
        assert!((manifold.normal.x - 1.0).abs() < 0.001);
        assert!((manifold.depth - 0.5).abs() < 0.001);
    }
}
