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

use khora_core::{
    ecs::entity::EntityId,
    math::{AffineTransform, Mat4},
};

use crate::ecs::{
    DataSystemRegistration, GlobalTransform, Parent, SimulatedTransform, Teleported, TickPhase,
    Transform, Without, World,
};

/// Propagates local `Transform` changes through the scene hierarchy to
/// compute the final `GlobalTransform` for each entity.
///
/// Performs a Breadth-First Search (BFS) traversal: parent transforms are
/// computed before their children, ensuring correctness in a single pass.
pub fn transform_propagation_system(world: &mut World) {
    // Whose pose the simulation owns this frame.
    //
    // Collected first because the queries below take the world mutably, and
    // consulted at every step because a simulated pose is **taken whole**: it
    // is already a world pose, so composing it with a parent would apply that
    // parent's transform twice. That is precisely what used to happen — the
    // writeback wrote the provider's world pose into the *local* `Transform`
    // and this system then multiplied it by the parent again, so a parented
    // body drifted by its parent's transform every single frame.
    // An entity somebody just moved is **not** in this map, however simulated
    // it is: the author's placement wins the frame they made it, and the
    // physics sync reads the world pose computed here to push it into the
    // provider. Without the exception the simulated pose would win, the sync
    // would push it straight back, and the move would be invisible.
    let simulated: HashMap<EntityId, AffineTransform> = world
        .query::<(EntityId, &SimulatedTransform)>()
        .filter(|(id, _)| world.get::<Teleported>(*id).is_none())
        .map(|(id, pose)| (id, pose.0))
        .collect();

    // Stage 1: initialize the work queue with all root entities.
    // A root has `Transform` and `GlobalTransform` but no `Parent`.
    let mut queue: VecDeque<EntityId> = VecDeque::new();
    for (id, transform, global_transform, _) in
        world.query::<(EntityId, &Transform, &mut GlobalTransform, Without<Parent>)>()
    {
        global_transform.0 = match simulated.get(&id) {
            Some(pose) => *pose,
            None => transform.to_mat4().into(),
        };
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
            let child_matrix = match simulated.get(&child_id) {
                // The solver placed it in the world; the parent has no say.
                Some(pose) => *pose,
                None => (Mat4::from(parent_matrix) * local_transform.to_mat4()).into(),
            };
            // Only enqueue children whose `GlobalTransform` we successfully
            // wrote — that's the invariant the next iteration of this loop
            // relies on. Transform-only children stay out of the queue.
            if let Some(global_transform) = world.get_mut::<GlobalTransform>(child_id) {
                global_transform.0 = child_matrix;
                queue.push_back(child_id);
            }
        }
    }
}

/// Wrapper to match the `DataSystemRegistration::run` signature
/// `fn(&mut World, &Runtime, &mut OutputDeck)`. Transform propagation
/// needs neither the runtime containers nor the output deck, so the
/// trailing args are unused.
fn transform_propagation_entry(
    world: &mut World,
    _runtime: &khora_core::Runtime,
    _deck: &mut khora_core::lane::OutputDeck,
) {
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

    /// **The bug a parented rigid body had, every single frame.**
    ///
    /// A solver places a body in the world. The old writeback wrote that world
    /// pose into the *local* `Transform`, and this system then multiplied it by
    /// the parent again — so the entity drifted by its parent's transform on
    /// every frame, and the physics sync's "teleport detection" fired each time
    /// and snapped it back. A simulated pose is taken whole.
    #[test]
    fn a_simulated_child_ignores_its_parents_transform() {
        let mut world = World::new();
        let parent = world.spawn((
            Transform::from_translation(Vec3::new(100.0, 0.0, 0.0)),
            GlobalTransform::identity(),
        ));
        let child = world.spawn((
            Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
            GlobalTransform::identity(),
            Parent(parent),
            SimulatedTransform::from_parts(
                Vec3::new(1.0, 2.0, 3.0),
                khora_core::math::Quat::IDENTITY,
            ),
        ));

        transform_propagation_system(&mut world);

        let global = world.get::<GlobalTransform>(child).unwrap();
        assert_matrix_approx_eq(
            global.0.into(),
            Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        );
    }

    /// A root the simulation owns takes its pose whole too, rather than being
    /// recomputed from a placement the author has not touched since.
    #[test]
    fn a_simulated_root_takes_the_simulated_pose() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)),
            GlobalTransform::identity(),
            SimulatedTransform::from_parts(
                Vec3::new(0.0, 4.0, 0.0),
                khora_core::math::Quat::IDENTITY,
            ),
        ));

        transform_propagation_system(&mut world);

        assert_matrix_approx_eq(
            world.get::<GlobalTransform>(entity).unwrap().0.into(),
            Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0)),
        );
    }

    /// And an entity nothing simulates still composes with its parent — the
    /// preference is a preference, not a replacement.
    #[test]
    fn an_unsimulated_child_still_composes() {
        let mut world = World::new();
        let parent = world.spawn((
            Transform::from_translation(Vec3::new(100.0, 0.0, 0.0)),
            GlobalTransform::identity(),
        ));
        let child = world.spawn((
            Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
            GlobalTransform::identity(),
            Parent(parent),
        ));

        transform_propagation_system(&mut world);

        assert_matrix_approx_eq(
            world.get::<GlobalTransform>(child).unwrap().0.into(),
            Mat4::from_translation(Vec3::new(105.0, 0.0, 0.0)),
        );
    }
}
