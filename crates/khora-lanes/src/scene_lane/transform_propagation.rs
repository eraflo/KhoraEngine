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

use std::collections::{HashMap, VecDeque};

use khora_core::{ecs::entity::EntityId, math::Mat4};
use khora_data::ecs::{GlobalTransform, Parent, Transform, Without, World};

/// A system that propagates local `Transform` changes through the scene hierarchy
/// to compute the final `GlobalTransform` for each entity.
///
/// This system performs a Breadth-First Search (BFS) traversal of the scene hierarchy.
/// It guarantees that parent transforms are computed before their children, ensuring
/// correctness in a single pass.
///
/// It is designed to run once per frame, before the rendering and physics systems.
pub fn transform_propagation_system(world: &mut World) {
    // Stage 1: Initialize the work queue with all root entities.
    // A root is an entity with a `Transform` and `GlobalTransform` but no `Parent`.
    // Their `GlobalTransform` is calculated directly from their local `Transform`.
    let mut queue: VecDeque<EntityId> = VecDeque::new();
    for (id, transform, global_transform, _) in
        world.query::<(EntityId, &Transform, &mut GlobalTransform, Without<Parent>)>()
    {
        global_transform.0 = transform.to_mat4().into();
        queue.push_back(id);
    }

    // Stage 2: Build a map of parent -> children relationships for efficient traversal.
    // This is done once at the beginning for performance.
    let mut children_map: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
    for (child_id, parent) in world.query::<(EntityId, &Parent)>() {
        children_map.entry(parent.0).or_default().push(child_id);
    }

    // Stage 3: Process the queue in a Breadth-First manner.
    // We use a `head` index to iterate through the queue without complex borrowing issues,
    // as we are both reading from and writing to the queue.
    let mut head = 0;
    while let Some(&parent_id) = queue.get(head) {
        head += 1; // Move to the next item for the next iteration.

        // If this parent has any children...
        if let Some(children) = children_map.get(&parent_id) {
            // Get the parent's global transform, which we know is now up-to-date.
            // We can `unwrap` because we know the entity is valid and has this component.
            let parent_matrix = world.get::<GlobalTransform>(parent_id).unwrap().0;

            for &child_id in children {
                // Read the child's local transform.
                if let Some(local_transform) = world.get::<Transform>(child_id) {
                    let child_matrix = Mat4::from(parent_matrix) * local_transform.to_mat4();

                    // Update the child's `GlobalTransform` component directly.
                    if let Some(global_transform) = world.get_mut::<GlobalTransform>(child_id) {
                        global_transform.0 = child_matrix.into();
                    }

                    // Add the child to the end of the queue. Its own children will be processed
                    // in a future iteration, after all entities at the current depth are done.
                    queue.push_back(child_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::{Mat4, Vec3, EPSILON};
    use khora_data::ecs::{Children, GlobalTransform, Parent, SemanticDomain, Transform, World};

    /// A helper function to compare two `Mat4` matrices for approximate equality.
    fn assert_matrix_approx_eq(a: Mat4, b: Mat4) {
        for i in 0..4 {
            for j in 0..4 {
                let val_a = a.cols[i][j];
                let val_b = b.cols[i][j];
                assert!(
                    (val_a - val_b).abs() < EPSILON,
                    "Matrix mismatch at col {}, row {}: {} != {}",
                    i,
                    j,
                    val_a,
                    val_b
                );
            }
        }
    }

    #[test]
    fn test_transform_propagation_simple_hierarchy() {
        // --- 1. ARRANGE ---
        let mut world = World::default();

        // Register all the components we will use.
        world.register_component::<Parent>(SemanticDomain::Spatial);
        world.register_component::<Children>(SemanticDomain::Spatial);
        world.register_component::<Transform>(SemanticDomain::Spatial);
        world.register_component::<GlobalTransform>(SemanticDomain::Spatial);

        // Create a root entity (the parent).
        // It's translated by 10 units on the X axis.
        let parent_transform = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            ..Default::default()
        };
        let parent_id = world.spawn((parent_transform, GlobalTransform::identity()));

        // Create a child entity.
        // It's translated by 2 units on the Y axis *relative to its parent*.
        let child_transform = Transform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            ..Default::default()
        };
        let child_id = world.spawn((
            child_transform,
            GlobalTransform::identity(),
            Parent(parent_id),
        ));

        // --- 2. ACT ---
        // Run the system we want to test.
        transform_propagation_system(&mut world);

        // --- 3. ASSERT ---
        // Get the final GlobalTransform of the child entity.
        let child_global_transform = world
            .get::<GlobalTransform>(child_id)
            .expect("Child should have a GlobalTransform component");

        // The expected result is the parent's translation combined with the child's.
        let expected_matrix = Mat4::from_translation(Vec3::new(10.0, 2.0, 0.0));

        assert_matrix_approx_eq(child_global_transform.0.into(), expected_matrix);
    }
}
