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

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ScriptSnapshot, ScriptValue};
use khora_core::Runtime;

use crate::ecs::{Script, ScriptState, SemanticDomain, Transform, World};
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

    /// What the world holds for this instance, **only the first frame it
    /// appears**: the authored fields and, apart, what was observed of it
    /// before — never merged here. The lane composes them.
    pub arrival: Option<ScriptArrival>,

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

/// What an instance brings the first frame it appears.
///
/// The authored starting values and the observed state are handed over apart;
/// the lane decides how they combine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptArrival {
    /// The fields its author set, from its `Script`.
    pub fields: Vec<(String, ScriptValue)>,
    /// What was observed of this behavior before — its `ScriptState` — if the
    /// entity has one recorded by the same behavior.
    pub observed: Option<ScriptSnapshot>,
}

/// Everything the script lane may read this frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScriptView {
    /// The seconds since the previous frame.
    ///
    /// Carried here rather than read by the lane, for the same reason a
    /// transform is: the lane has no `Runtime` to read it from. It is what
    /// `every` and `after` count down.
    pub delta_seconds: f32,
    /// The distinct behaviors in the scene, referenced by index.
    pub programs: Vec<ScriptProgram>,
    /// Every entity running one, in a stable order.
    pub instances: Vec<ScriptInstance>,
    /// What the player is holding, this frame.
    ///
    /// Projected rather than read, for the same reason a transform is: the lane
    /// is `Isolated` and reaches no `Runtime`, so it cannot ask the `InputMap`
    /// anything. A `Flow` is exactly the thing that turns engine state into
    /// something a lane may read, and until this field `InputMap` was published
    /// every frame with no consumer anywhere in the CLAD descent.
    pub input: khora_core::platform::InputSnapshot,
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
pub struct ScriptFlow {
    /// Entities whose authored fields have already been handed over.
    ///
    /// The flow is the only place that can know this: it sees every frame's
    /// entity set, and the lane sees only what it is given. Keeping it here is
    /// what turns "send the fields once" into something the projection can
    /// actually decide.
    seen: HashSet<EntityId>,
    /// Entities appearing for the first time, computed in `select` for
    /// `project` — which is `&self` and cannot work it out itself.
    newcomers: HashSet<EntityId>,
}

impl Flow for ScriptFlow {
    type View = ScriptView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "script";

    fn select(&mut self, world: &World, _runtime: &Runtime) -> Selection {
        let live: HashSet<EntityId> = world
            .iter_entities()
            .filter(|entity| world.get::<Script>(*entity).is_some())
            .collect();

        self.newcomers = live.difference(&self.seen).copied().collect();
        // Replaced rather than extended, so a despawned entity is forgotten and
        // an index that comes back — with a new generation — is a newcomer
        // again. Growing forever would also be a leak in a scene that spawns.
        self.seen = live;

        Selection::new()
    }

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let mut view = ScriptView {
            delta_seconds: runtime
                .resources
                .get::<khora_core::time::SharedTime>()
                .and_then(|time| time.read().ok().map(|time| time.delta_seconds))
                .unwrap_or(0.0),
            // Snapshotted under the lock and released: the lane may run on a
            // pool thread, and handing it anything that borrows the map would
            // hold the lock for the length of a frame's scripting.
            input: runtime
                .resources
                .get::<std::sync::Arc<std::sync::Mutex<khora_core::platform::InputMap>>>()
                .and_then(|map| map.lock().ok().map(|map| map.snapshot()))
                .unwrap_or_default(),
            ..Default::default()
        };

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

            let arrival = self
                .newcomers
                .contains(&entity)
                .then(|| arrival_of(world, entity, script));

            let transform = world.get::<Transform>(entity).copied();
            view.instances.push(ScriptInstance {
                entity,
                program: index as u32,
                arrival,
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

/// What `entity` brings the first frame it appears.
fn arrival_of(world: &World, entity: EntityId, script: &Script) -> ScriptArrival {
    let observed = world
        .get::<ScriptState>(entity)
        .filter(|state| state.behavior == script.behavior)
        .map(|state| state.snapshot.clone());
    ScriptArrival {
        fields: script.fields.clone(),
        observed,
    }
}

register_flow!(ScriptFlow);

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs both stages, the way the registration trampoline does — `select`
    /// is where the flow works out which entities are new, so a test that
    /// skipped it would never see an authored field.
    fn project(world: &World) -> ScriptView {
        let mut flow = ScriptFlow::default();
        let selection = flow.select(world, &Runtime::default());
        flow.project(world, &selection, &Runtime::default())
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

    /// **No merge.** A newcomer hands the lane what its author set and what
    /// was observed of it, side by side: which wins is the lane's decision,
    /// and a value both hold arrives twice rather than once, overwritten.
    #[test]
    fn a_newcomer_arrives_with_authored_fields_and_observed_state_apart() {
        let mut world = World::new();
        let observed = ScriptSnapshot {
            state: Some("Chase".to_owned()),
            ..ScriptSnapshot::default()
        }
        .with_field("health", ScriptValue::Int(40));
        world.spawn((
            Transform::identity(),
            Script::new("ai/guard.erg", "Guard")
                .with_field("health", ScriptValue::Int(100))
                .with_field("speed", ScriptValue::Float(3.0)),
            crate::ecs::ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: observed.clone(),
            },
        ));

        let view = project(&world);

        assert_eq!(
            view.instances[0].arrival,
            Some(ScriptArrival {
                fields: vec![
                    ("health".to_owned(), ScriptValue::Int(100)),
                    ("speed".to_owned(), ScriptValue::Float(3.0)),
                ],
                observed: Some(observed),
            })
        );
    }

    /// An entity that never ran arrives with its authored fields and nothing
    /// observed.
    #[test]
    fn a_newcomer_that_never_ran_arrives_with_nothing_observed() {
        let mut world = World::new();
        world.spawn((
            Transform::identity(),
            Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100)),
        ));

        let view = project(&world);

        assert_eq!(
            view.instances[0].arrival,
            Some(ScriptArrival {
                fields: vec![("health".to_owned(), ScriptValue::Int(100))],
                observed: None,
            })
        );
    }

    /// A state observed of another behavior — the entity's behavior was
    /// swapped since — is not handed over: a `Chase` must not start from a
    /// `Guard`'s fields and state.
    #[test]
    fn a_state_for_another_behavior_is_not_handed_over() {
        let mut world = World::new();
        world.spawn((
            Transform::identity(),
            Script::new("ai/guard.erg", "Chase").with_field("speed", ScriptValue::Float(5.0)),
            crate::ecs::ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(40)),
            },
        ));

        let view = project(&world);

        assert_eq!(
            view.instances[0].arrival,
            Some(ScriptArrival {
                fields: vec![("speed".to_owned(), ScriptValue::Float(5.0))],
                observed: None,
            })
        );
    }

    /// The arrival is delivered once: after the first frame the lane holds the
    /// live state, and sending it again would clone every field of every
    /// entity every frame for nobody.
    #[test]
    fn no_arrival_after_the_first_frame() {
        let mut world = World::new();
        world.spawn((
            Transform::identity(),
            Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100)),
            crate::ecs::ScriptState {
                behavior: "Guard".to_owned(),
                snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(40)),
            },
        ));

        let mut flow = ScriptFlow::default();
        let runtime = Runtime::default();
        let selection = flow.select(&world, &runtime);
        let first = flow.project(&world, &selection, &runtime);
        let selection = flow.select(&world, &runtime);
        let second = flow.project(&world, &selection, &runtime);

        assert!(first.instances[0].arrival.is_some(), "the first frame");
        assert_eq!(second.len(), 1, "the instance still runs");
        assert_eq!(second.instances[0].arrival, None, "but arrives only once");
    }
}
