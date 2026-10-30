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

use crate::ecs::SystemContext;
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
    for (id, transform, mut global_transform, _) in
        world.query_mut::<(EntityId, &Transform, &mut GlobalTransform, Without<Parent>)>()
    {
        let placed: AffineTransform = transform.to_mat4().into();
        // Written only when it differs: an unmoved scene writes nothing, and
        // the views projected from it stay valid.
        global_transform.set_if_neq(GlobalTransform(match simulated.get(&id) {
            Some(pose) => with_scale_of(pose, &placed),
            None => placed,
        }));
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
            let placed: AffineTransform =
                (Mat4::from(parent_matrix) * local_transform.to_mat4()).into();
            let child_matrix = match simulated.get(&child_id) {
                // The solver placed it in the world; the parent has no say in
                // where — only in how big, which a solver never decides.
                Some(pose) => with_scale_of(pose, &placed),
                None => placed,
            };
            // Only enqueue children whose `GlobalTransform` we successfully
            // wrote — that's the invariant the next iteration of this loop
            // relies on. Transform-only children stay out of the queue.
            if let Some(mut global_transform) = world.get_mut::<GlobalTransform>(child_id) {
                global_transform.set_if_neq(GlobalTransform(child_matrix));
                queue.push_back(child_id);
            }
        }
    }
}

/// A simulated pose, sized as the authored hierarchy sizes the entity.
///
/// A provider reports where a body is and how it is turned — never its size,
/// so the pose alone would draw a body scaled 2 at scale 1 from its first
/// simulated frame. The scale is the one the unsimulated composition gives
/// (`placed`), so a scaled ancestor still sizes a simulated child. A mirror
/// goes on the first axis, where [`AffineTransform::rotation`] — and so the
/// pose the provider was given — leaves it.
fn with_scale_of(pose: &AffineTransform, placed: &AffineTransform) -> AffineTransform {
    let mut scale = placed.scale();
    if Mat4::from(*placed).determinant() < 0.0 {
        scale.x = -scale.x;
    }
    (Mat4::from(*pose) * Mat4::from_scale(scale)).into()
}

/// Wrapper to match the `DataSystemRegistration::run` signature
/// `fn(&mut World, &Runtime, &mut OutputDeck)`. Transform propagation
/// needs neither the runtime containers nor the output deck, so the
/// trailing args are unused.
fn transform_propagation_entry(
    world: &mut World,
    _runtime: &khora_core::Runtime,
    _deck: &mut khora_core::lane::OutputDeck,
    _ctx: SystemContext,
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
        observes: &[],
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

    /// **A simulated body rendered at scale 1.** A solver gives a pose —
    /// translation and rotation — and never a size; the size is the author's.
    /// Taking the simulated pose whole as the world matrix dropped the
    /// `Transform`'s scale from the first simulated frame on.
    #[test]
    fn a_simulated_root_keeps_its_authored_scale() {
        let mut world = World::new();
        let rotation = khora_core::math::Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.5);
        let entity = world.spawn((
            Transform::new(
                Vec3::new(0.0, 10.0, 0.0),
                khora_core::math::Quat::IDENTITY,
                Vec3::new(2.0, 2.0, 2.0),
            ),
            GlobalTransform::identity(),
            SimulatedTransform::from_parts(Vec3::new(0.0, 4.0, 0.0), rotation),
        ));

        transform_propagation_system(&mut world);

        assert_matrix_approx_eq(
            world.get::<GlobalTransform>(entity).unwrap().0.into(),
            Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0))
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(Vec3::new(2.0, 2.0, 2.0)),
        );
    }

    /// A simulated child takes the scale the hierarchy would have given it —
    /// its parent's world scale times its own — while its position stays the
    /// solver's: the parent's scale reaches the size, never the placement.
    #[test]
    fn a_simulated_child_inherits_its_parents_scale_but_not_its_placement() {
        let mut world = World::new();
        let parent = world.spawn((
            Transform::new(
                Vec3::new(100.0, 0.0, 0.0),
                khora_core::math::Quat::IDENTITY,
                Vec3::new(3.0, 3.0, 3.0),
            ),
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

        let global = world.get::<GlobalTransform>(child).unwrap().0;
        assert_matrix_approx_eq(
            global.into(),
            Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
                * Mat4::from_scale(Vec3::new(3.0, 3.0, 3.0)),
        );
    }

    /// A mirror is part of the authored size, not of the pose: a simulated root
    /// authored with scale (−1, 1, 1) is drawn mirrored, as its unsimulated
    /// composition would draw it.
    #[test]
    fn a_simulated_root_keeps_its_authored_mirror() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::new(
                Vec3::new(0.0, 10.0, 0.0),
                khora_core::math::Quat::IDENTITY,
                Vec3::new(-1.0, 1.0, 1.0),
            ),
            GlobalTransform::identity(),
            SimulatedTransform::from_parts(
                Vec3::new(0.0, 4.0, 0.0),
                khora_core::math::Quat::IDENTITY,
            ),
        ));

        transform_propagation_system(&mut world);

        assert_matrix_approx_eq(
            world.get::<GlobalTransform>(entity).unwrap().0.into(),
            Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0))
                * Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
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

    /// A solver moves and turns a body; it does not resize it. The scale the
    /// author gave the body — a mirror included — survives the simulated pose
    /// on its way to `GlobalTransform`, so a mirrored rigid body is still
    /// drawn mirrored (and at its size) once physics owns it.
    #[test]
    fn a_simulated_body_keeps_its_authored_scale_and_mirror() {
        let mut world = World::new();
        let rotation = khora_core::math::Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.5);
        let entity = world.spawn((
            Transform::new(
                Vec3::new(0.0, 10.0, 0.0),
                rotation,
                Vec3::new(-2.0, 2.0, 2.0),
            ),
            GlobalTransform::identity(),
            SimulatedTransform::from_parts(Vec3::new(0.0, 4.0, 0.0), rotation),
        ));

        transform_propagation_system(&mut world);

        assert_matrix_approx_eq(
            world.get::<GlobalTransform>(entity).unwrap().0.into(),
            Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0))
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(Vec3::new(-2.0, 2.0, 2.0)),
        );
    }

    /// The flows keyed on the Spatial domain, run over a static scene.
    mod render_cache {
        use khora_core::asset::{AssetHandle, AssetUUID};
        use khora_core::ecs::entity::EntityId;
        use khora_core::lane::OutputDeck;
        use khora_core::math::Vec3;
        use khora_core::renderer::api::gpu_scene::GpuMesh;
        use khora_core::renderer::api::pipeline::PrimitiveTopology;
        use khora_core::renderer::api::resource::{BufferId, IndexFormat};
        use khora_core::renderer::light::{DirectionalLight, LightType};
        use khora_core::Runtime;

        use super::super::transform_propagation_entry;
        use crate::ecs::{
            AudioListener, AudioSource, Camera, GlobalTransform, HandleComponent, Light, Observer,
            Parent, SystemContext, Transform, World,
        };
        use crate::flow::{AudioFlow, Flow, RenderFlow, Selection, ShadowFlow};

        /// The cache keys of `RenderFlow`, `ShadowFlow` and `AudioFlow`, in
        /// that order.
        type Keys = [Option<u64>; 3];

        /// A scene nothing moves in: a camera, a shadow-casting light, a mesh
        /// with a child mesh, a sound and a listener, each placed by a
        /// `Transform` away from the origin. Returns the world and the root
        /// mesh.
        fn static_scene() -> (World, EntityId) {
            let mut world = World::new();
            let at = |x: f32, y: f32, z: f32| {
                (
                    Transform::from_translation(Vec3::new(x, y, z)),
                    GlobalTransform::identity(),
                )
            };
            let mesh = || HandleComponent {
                handle: AssetHandle::new(GpuMesh {
                    vertex_buffer: BufferId(0),
                    index_buffer: BufferId(0),
                    index_count: 0,
                    index_format: IndexFormat::Uint32,
                    primitive_topology: PrimitiveTopology::TriangleList,
                }),
                uuid: AssetUUID::new(),
            };

            let (transform, global) = at(0.0, 2.0, 10.0);
            world.spawn((
                transform,
                global,
                Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.1, 100.0),
            ));
            let (transform, global) = at(0.0, 10.0, 0.0);
            world.spawn((
                transform,
                global,
                Light::new(LightType::Directional(DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.2).normalize(),
                    shadow_enabled: true,
                    ..DirectionalLight::default()
                })),
            ));
            let (transform, global) = at(3.0, 0.0, -2.0);
            let root = world.spawn((transform, global, mesh()));
            let (transform, global) = at(0.0, 1.0, 0.0);
            world.spawn((transform, global, mesh(), Parent(root)));
            let (transform, global) = at(-4.0, 0.0, 1.0);
            world.spawn((transform, global, AudioSource::default()));
            let (transform, global) = at(0.0, 1.5, 6.0);
            world.spawn((transform, global, AudioListener));
            (world, root)
        }

        /// One frame as the substrate runs it: `transform_propagation` as a
        /// DataSystem, then the three flows, each a run of its own. Returns
        /// the cache keys the flows were asked for.
        fn frame(world: &mut World, runtime: &Runtime) -> Keys {
            let system = Observer::DataSystem("transform_propagation");
            let ctx = SystemContext {
                since: world.observer_last_seen(system),
            };
            let tick = world.begin_run();
            let mut deck = OutputDeck::new();
            transform_propagation_entry(world, runtime, &mut deck, ctx);
            world.note_observer_run(system, tick);
            world.end_run();

            let selection = Selection::new();
            let mut keys = [None; 3];

            let tick = world.begin_run();
            keys[0] = RenderFlow.cache_key(world, runtime);
            let _ = RenderFlow.project(world, &selection, runtime);
            world.note_observer_run(Observer::Flow(RenderFlow::NAME), tick);
            world.end_run();

            let tick = world.begin_run();
            keys[1] = ShadowFlow.cache_key(world, runtime);
            let _ = ShadowFlow.project(world, &selection, runtime);
            world.note_observer_run(Observer::Flow(ShadowFlow::NAME), tick);
            world.end_run();

            let tick = world.begin_run();
            keys[2] = AudioFlow.cache_key(world, runtime);
            let _ = AudioFlow.project(world, &selection, runtime);
            world.note_observer_run(Observer::Flow(AudioFlow::NAME), tick);
            world.end_run();

            keys
        }

        /// **A propagation that changes nothing writes nothing.** Once the
        /// first frame has placed every `GlobalTransform`, a frame in which
        /// no `Transform` moved leaves the Spatial domain as it was, so the
        /// cached views of `RenderFlow`, `ShadowFlow` and `AudioFlow` stay
        /// valid; moving one `Transform` invalidates all three.
        #[test]
        fn transform_propagation_writing_nothing_leaves_the_render_cache_valid() {
            let (mut world, root) = static_scene();
            let runtime = Runtime::default();

            let _settling = frame(&mut world, &runtime);
            let second = frame(&mut world, &runtime);
            let third = frame(&mut world, &runtime);

            for (flow, key) in ["RenderFlow", "ShadowFlow", "AudioFlow"].iter().zip(second) {
                assert!(key.is_some(), "{flow} caches its view");
            }
            assert_eq!(
                third, second,
                "a frame in which nothing moved must leave every cache key as it was"
            );

            world
                .get_mut::<Transform>(root)
                .expect("the root mesh has a Transform")
                .translation = Vec3::new(3.0, 0.5, -2.0);
            let moved = frame(&mut world, &runtime);

            assert_eq!(
                world
                    .get::<GlobalTransform>(root)
                    .map(|g| g.0.translation()),
                Some(Vec3::new(3.0, 0.5, -2.0)),
                "the move was propagated"
            );
            for ((flow, before), after) in ["RenderFlow", "ShadowFlow", "AudioFlow"]
                .iter()
                .zip(third)
                .zip(moved)
            {
                assert_ne!(after, before, "moving a Transform must change {flow}'s key");
            }
        }

        /// **Each real change invalidates the caches that read it, and only
        /// those.** Over the static scene: a light switched off (Render), a
        /// mesh handle swapped by value (Render), a sound's volume (Audio),
        /// and a mesh despawned (Render and Spatial).
        #[test]
        fn each_real_change_invalidates_the_caches_that_read_it() {
            let (mut world, root) = static_scene();
            let runtime = Runtime::default();
            let _settling = frame(&mut world, &runtime);
            let mut last = frame(&mut world, &runtime);
            let mut step = |world: &mut World, what: &str, changed: [bool; 3]| {
                let keys = frame(world, &runtime);
                for (((flow, before), after), changed) in ["RenderFlow", "ShadowFlow", "AudioFlow"]
                    .iter()
                    .zip(last)
                    .zip(keys)
                    .zip(changed)
                {
                    assert_eq!(
                        after != before,
                        changed,
                        "{what}: {flow}'s key {}",
                        if changed { "must change" } else { "must stay" }
                    );
                }
                last = keys;
            };

            let light = world
                .query::<(EntityId, &Light)>()
                .map(|(entity, _)| entity)
                .next()
                .expect("a light");
            world.get_mut::<Light>(light).expect("the light").enabled = false;
            step(&mut world, "a light switched off", [true, true, false]);

            let mut mesh = world
                .get::<HandleComponent<GpuMesh>>(root)
                .expect("a mesh")
                .clone();
            mesh.uuid = AssetUUID::new();
            assert!(world.set_component(root, mesh));
            step(&mut world, "a mesh handle swapped", [true, true, false]);

            let source = world
                .query::<(EntityId, &AudioSource)>()
                .map(|(entity, _)| entity)
                .next()
                .expect("a source");
            world
                .get_mut::<AudioSource>(source)
                .expect("the source")
                .volume = 0.25;
            step(&mut world, "a sound's volume", [false, false, true]);

            step(&mut world, "nothing", [false, false, false]);

            assert!(world.despawn(root));
            step(&mut world, "a mesh despawned", [true, true, true]);
        }
    }
}
