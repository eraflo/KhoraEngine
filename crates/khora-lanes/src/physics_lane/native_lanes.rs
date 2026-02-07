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

use khora_core::ecs::entity::EntityId;
use khora_core::physics::{
    ContactManifold, DynamicTree, ImpulseSolver, NarrowPhase, VelocityState,
};
use khora_data::ecs::{Collider, CollisionPair, CollisionPairs, GlobalTransform, RigidBody, World};
use std::collections::HashMap;
use std::sync::RwLock;

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
    pub fn step(&self, world: &mut World) {
        // 1. Sync ECS components to the Dynamic Tree
        self.sync_tree(world);

        // 2. Clear old pairs and generate new ones
        self.generate_pairs(world);
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

    fn generate_pairs(&self, world: &mut World) {
        let tree = self.tree.read().unwrap();
        let mut all_pairs = Vec::new();

        tree.query_pairs(|&a, &b| {
            all_pairs.push(CollisionPair {
                entity_a: a,
                entity_b: b,
            });
        });

        // Store pairs in a singleton CollisionPairs component
        // We find the first entity with CollisionPairs or spawn one.
        let mut found = false;
        {
            let query = world.query_mut::<&mut CollisionPairs>();
            for pairs_comp in query {
                pairs_comp.pairs = all_pairs.clone();
                found = true;
                break;
            }
        }

        if !found {
            world.spawn(CollisionPairs { pairs: all_pairs });
        }
    }
}

/// The Solver Lane resolves collisions and updates velocities/positions.
/// It uses the Sequential Impulse method for stable constraint resolution.
pub struct NativeSolverLane {
    solver: SequentialImpulseSolver,
}

impl NativeSolverLane {
    /// Creates a new `NativeSolverLane`.
    pub fn new() -> Self {
        Self {
            solver: SequentialImpulseSolver::new(),
        }
    }

    /// Executes the solver step: integrates velocities, resolves collisions, and integrates positions.
    pub fn step(&self, world: &mut World, dt: f32) {
        // 1. Integrate Forces (Gravity, etc.)
        self.integrate_velocities(world, dt);

        // 2. Resolve Constraints (Collisions)
        self.solver.solve_collisions(world);

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
    fn solve_collisions(&self, world: &mut World) {
        // 1. Gather all potential collision pairs from the broad-phase output.
        let candidates = {
            let mut pairs = Vec::new();
            let query = world.query::<&CollisionPairs>();
            for p in query {
                pairs.extend_from_slice(&p.pairs);
            }
            pairs
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
