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

//! Transform propagation — `Transform` → `GlobalTransform` for the scene
//! hierarchy. Runs in [`TickPhase::PostSimulation`], after `app.update` has
//! mutated local `Transform`s and before extraction reads `GlobalTransform`.

use std::collections::{HashMap, VecDeque};

use khora_core::{ecs::entity::EntityId, math::Mat4};

use crate::ecs::{
    DataSystemRegistration, GlobalTransform, Parent, TickPhase, Transform, Without, World,
};

/// Propagates local `Transform` changes through the scene hierarchy to
/// compute the final `GlobalTransform` for each entity.
///
/// Performs a Breadth-First Search (BFS) traversal: parent transforms are
/// computed before their children, ensuring correctness in a single pass.
pub fn transform_propagation_system(world: &mut World) {
    // Stage 1: initialize the work queue with all root entities.
    // A root has `Transform` and `GlobalTransform` but no `Parent`.
    let mut queue: VecDeque<EntityId> = VecDeque::new();
    for (id, transform, global_transform, _) in
        world.query::<(EntityId, &Transform, &mut GlobalTransform, Without<Parent>)>()
    {
        global_transform.0 = transform.to_mat4().into();
        queue.push_back(id);
    }

    // Stage 2: build a parent -> children map for efficient traversal.
    let mut children_map: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
    for (child_id, parent) in world.query::<(EntityId, &Parent)>() {
        children_map.entry(parent.0).or_default().push(child_id);
    }

    // Stage 3: BFS through the hierarchy.
    //
    // Defensive against partial hierarchies (entities created mid-frame
    // without `GlobalTransform`, recently reparented nodes whose page
    // migration is in progress, etc.): every fetch is guarded.
    let mut head = 0;
    while let Some(&parent_id) = queue.get(head) {
        head += 1;

        let Some(children) = children_map.get(&parent_id) else {
            continue;
        };
        let Some(parent_global) = world.get::<GlobalTransform>(parent_id) else {
            continue;
        };
        let parent_matrix = parent_global.0;

        for &child_id in children {
            let Some(local_transform) = world.get::<Transform>(child_id) else {
                continue;
            };
            let child_matrix = Mat4::from(parent_matrix) * local_transform.to_mat4();
            // Only enqueue children whose `GlobalTransform` we successfully
            // wrote — that's the invariant the next iteration of this loop
            // relies on. Transform-only children stay out of the queue.
            if let Some(global_transform) = world.get_mut::<GlobalTransform>(child_id) {
                global_transform.0 = child_matrix.into();
                queue.push_back(child_id);
            }
        }
    }
}

/// Wrapper to match the `DataSystemRegistration::run` signature
/// `fn(&mut World, &Runtime)`. Transform propagation needs no runtime
/// containers, so the second arg is unused.
fn transform_propagation_entry(world: &mut World, _runtime: &khora_core::Runtime) {
    transform_propagation_system(world);
}

inventory::submit! {
    DataSystemRegistration {
        name: "transform_propagation",
        phase: TickPhase::PostSimulation,
        run: transform_propagation_entry,
        order_hint: 0,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Children, GlobalTransform, Parent, SemanticDomain, Transform, World};
    use khora_core::math::{Mat4, Vec3, EPSILON};

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
        let mut world = World::default();

        world.register_component::<Parent>(SemanticDomain::Spatial);
        world.register_component::<Children>(SemanticDomain::Spatial);
        world.register_component::<Transform>(SemanticDomain::Spatial);
        world.register_component::<GlobalTransform>(SemanticDomain::Spatial);

        let parent_transform = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            ..Default::default()
        };
        let parent_id = world.spawn((parent_transform, GlobalTransform::identity()));

        let child_transform = Transform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            ..Default::default()
        };
        let child_id = world.spawn((
            child_transform,
            GlobalTransform::identity(),
            Parent(parent_id),
        ));

        transform_propagation_system(&mut world);

        let child_global_transform = world
            .get::<GlobalTransform>(child_id)
            .expect("Child should have a GlobalTransform component");

        let expected_matrix = Mat4::from_translation(Vec3::new(10.0, 2.0, 0.0));
        assert_matrix_approx_eq(child_global_transform.0.into(), expected_matrix);
    }
}
