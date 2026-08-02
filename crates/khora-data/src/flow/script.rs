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

//! What the script lane is allowed to see.
//!
//! The read half of the script bridge; [`script_commands`] is the write half. A
//! lane may not query the `World` (`RULES.md` §3), so everything a behavior can
//! read this frame has to arrive through this projection. That is a constraint
//! worth taking literally: what is not here is not reachable, which is how the
//! engine surface stays a deliberate list rather than whatever happened to be in
//! scope.
//!
//! # Programs are listed once, instances many times
//!
//! A thousand guards run one `Guard` behavior. Copying `"ai/guard.erg"` and
//! `"Guard"` into a thousand instances would allocate two strings per entity per
//! frame — enough on its own to miss the frame budget the whole language exists
//! to respect. The distinct programs are deduplicated into
//! [`ScriptView::programs`] and each instance carries an index.
//!
//! [`script_commands`]: crate::ecs::systems::script_commands

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::Runtime;

use crate::ecs::{Script, SemanticDomain, Transform, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;

/// A distinct behavior appearing in the scene.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptProgram {
    /// The module, relative to the script root.
    pub module: String,
    /// The behavior's name within it.
    pub behavior: String,
}

/// One entity running one behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptInstance {
    /// The entity the behavior is attached to.
    pub entity: EntityId,

    /// Index into [`ScriptView::programs`].
    pub program: u32,

    /// Where the entity is, this frame.
    ///
    /// Carried rather than looked up because the lane has no `World` to look it
    /// up in — and reading its own position is the single most common thing a
    /// behavior does.
    pub translation: Vec3,
    /// How the entity is oriented, this frame.
    pub rotation: Quaternion,
    /// The entity's scale, this frame.
    pub scale: Vec3,
}

/// Everything the script lane may read this frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScriptView {
    /// The distinct behaviors in the scene, referenced by index.
    pub programs: Vec<ScriptProgram>,
    /// Every entity running one, in a stable order.
    pub instances: Vec<ScriptInstance>,
}

impl ScriptView {
    /// The program an instance runs.
    pub fn program_of(&self, instance: &ScriptInstance) -> Option<&ScriptProgram> {
        self.programs.get(instance.program as usize)
    }

    /// How many behaviors will run this frame.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether nothing will run.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}

/// Projects the scene's scripts into a [`ScriptView`].
#[derive(Default)]
pub struct ScriptFlow;

impl Flow for ScriptFlow {
    type View = ScriptView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "script";

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        let mut view = ScriptView::default();

        for entity in world.iter_entities() {
            let Some(script) = world.get::<Script>(entity) else {
                continue;
            };

            let program = ScriptProgram {
                module: script.module.clone(),
                behavior: script.behavior.clone(),
            };
            // Linear rather than hashed: a scene has a handful of distinct
            // behaviors and thousands of instances, so the scan is over the
            // short list and a map's hashing would cost more than it saves.
            let index = match view.programs.iter().position(|known| *known == program) {
                Some(index) => index,
                None => {
                    view.programs.push(program);
                    view.programs.len() - 1
                }
            };

            let transform = world.get::<Transform>(entity).copied();
            view.instances.push(ScriptInstance {
                entity,
                program: index as u32,
                // An entity with a behavior but no `Transform` is legitimate —
                // a game-state manager has nowhere to be. Identity keeps the
                // lane branch-free rather than making every read an `Option`.
                translation: transform.map_or(Vec3::ZERO, |t| t.translation),
                rotation: transform.map_or(Quaternion::IDENTITY, |t| t.rotation),
                scale: transform.map_or(Vec3::ONE, |t| t.scale),
            });
        }
        view
    }

    // Deliberately uncached, like `PhysicsFlow`. The projection reads every
    // entity's `Transform`, which changes every frame the game is not paused —
    // a cache key folding the Spatial epoch would miss on essentially every
    // frame while still costing the hash.
}

register_flow!(ScriptFlow);

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::script::ScriptValue;

    fn project(world: &World) -> ScriptView {
        ScriptFlow.project(world, &Selection::new(), &Runtime::default())
    }

    #[test]
    fn an_entity_without_a_script_is_not_projected() {
        let mut world = World::new();
        world.spawn(Transform::identity());

        assert!(project(&world).is_empty());
    }

    #[test]
    fn a_scripted_entity_arrives_with_its_program_and_place() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            Script::new("ai/guard.erg", "Guard"),
        ));

        let view = project(&world);
        assert_eq!(view.len(), 1);

        let instance = &view.instances[0];
        assert_eq!(instance.entity, entity);
        assert_eq!(instance.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            view.program_of(instance).map(|p| p.behavior.as_str()),
            Some("Guard")
        );
    }

    /// **The reason programs are a separate list.** A thousand guards must not
    /// mean a thousand copies of `"ai/guard.erg"`.
    #[test]
    fn many_instances_of_one_behavior_share_a_single_program() {
        let mut world = World::new();
        for _ in 0..50 {
            world.spawn((Transform::identity(), Script::new("ai/guard.erg", "Guard")));
        }

        let view = project(&world);
        assert_eq!(view.len(), 50);
        assert_eq!(view.programs.len(), 1, "one program, fifty instances");
        assert!(view.instances.iter().all(|i| i.program == 0));
    }

    #[test]
    fn distinct_behaviors_get_distinct_programs() {
        let mut world = World::new();
        world.spawn((Transform::identity(), Script::new("ai/guard.erg", "Guard")));
        world.spawn((Transform::identity(), Script::new("ai/guard.erg", "Sentry")));
        world.spawn((
            Transform::identity(),
            Script::new("loot/chest.erg", "Chest"),
        ));

        let view = project(&world);
        assert_eq!(view.programs.len(), 3);
    }

    /// Two behaviors of the same name in different modules are two behaviors —
    /// deduplicating on the name alone would run one where the other was meant.
    #[test]
    fn the_module_is_part_of_a_programs_identity() {
        let mut world = World::new();
        world.spawn((Transform::identity(), Script::new("ai/guard.erg", "Guard")));
        world.spawn((
            Transform::identity(),
            Script::new("town/guard.erg", "Guard"),
        ));

        assert_eq!(project(&world).programs.len(), 2);
    }

    /// A game-state manager has nowhere to be, and must still run.
    #[test]
    fn an_entity_with_no_transform_still_runs() {
        let mut world = World::new();
        world.spawn(Script::new("game/rules.erg", "Rules"));

        let view = project(&world);
        assert_eq!(view.len(), 1);
        assert_eq!(view.instances[0].translation, Vec3::ZERO);
        assert_eq!(view.instances[0].scale, Vec3::ONE);
    }

    /// The view is what the lane sees, so it must reflect an authored field
    /// change without the lane consulting the `World` itself.
    #[test]
    fn the_projection_follows_the_world() {
        let mut world = World::new();
        let entity = world.spawn((Transform::identity(), Script::new("ai/guard.erg", "Guard")));

        if let Some(script) = world.get_mut::<Script>(entity) {
            *script =
                Script::new("ai/guard.erg", "Chase").with_field("speed", ScriptValue::Float(5.0));
        }

        let view = project(&world);
        assert_eq!(
            view.program_of(&view.instances[0])
                .map(|p| p.behavior.as_str()),
            Some("Chase")
        );
    }
}
