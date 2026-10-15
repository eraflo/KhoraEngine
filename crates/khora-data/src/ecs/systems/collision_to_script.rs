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

//! Telling behaviors that something touched them.
//!
//! The first producer the engine has ever had for
//! [`ScriptEvent`](khora_core::script::ScriptEvent). The queue and the drain
//! were written frames apart and both worked; nothing filled the queue, so
//! `on Touched(...)` was a handler a script could declare and never see fire.
//!
//! ```text
//!   Channel<Collision>  ──►  this  ──►  Channel<ScriptEvent>  ──►  agent
//!   (physics, this frame)                                    (next frame)
//! ```
//!
//! # One contact, up to two events
//!
//! A collision names two entities and neither is the subject: a behavior asks
//! "what touched *me*", so each side gets its own event carrying the other.
//!
//! # Only for entities that run a behavior
//!
//! A contact between two pieces of scenery would otherwise put two events in a
//! bounded channel for nobody to handle, and a busy frame of scenery would drop
//! the events that had a handler waiting. The check is a behavior's component,
//! which is what makes an entity one the scripting agent will run at all.
//!
//! # Delivery is next frame
//!
//! Because it is a channel, and because that is the rule everywhere in
//! scripting: an event handled where it was raised opens a cascade with no
//! bound, and a budget that cannot bound the work it pays for is not a budget.

use khora_core::event::Channel;
use khora_core::lane::OutputDeck;
use khora_core::physics::{Collision, CollisionKind};
use khora_core::script::{ScriptEvent, ScriptValue};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

/// What a behavior declares to hear that something started touching it.
pub const TOUCHED: &str = khora_core::script::event::TOUCHED.name;

/// And to hear that it stopped.
pub const SEPARATED: &str = khora_core::script::event::SEPARATED.name;

fn collision_to_script(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(collisions) = runtime.resources.get::<Channel<Collision>>() else {
        return;
    };
    let contacts = collisions.read_for("collision_to_script");
    if contacts.is_empty() {
        return;
    }
    let Some(events) = runtime.resources.get::<Channel<ScriptEvent>>() else {
        // Physics runs, scripting does not. Reading the collisions anyway was
        // deliberate: the cursor moves, so a channel nobody scripts against
        // does not slide its window past a reader that appears later.
        return;
    };

    // An entity runs a behavior when it holds one's component: the keys
    // collected once, each looked up where the entity's row is, per contact
    // side — a frame of scenery grinding together is hundreds of contacts.
    let behaviors: Vec<_> = world
        .behavior_components()
        .into_iter()
        .map(|behavior| behavior.key)
        .collect();
    let scripted = |entity| {
        behaviors
            .iter()
            .any(|key| world.row(entity, *key).is_some())
    };

    for contact in contacts {
        let name = match contact.kind {
            CollisionKind::Started => TOUCHED,
            CollisionKind::Stopped => SEPARATED,
        };
        for (subject, other) in [(contact.a, contact.b), (contact.b, contact.a)] {
            if !scripted(subject) {
                continue;
            }
            events.send(ScriptEvent {
                target: subject,
                name: name.to_owned(),
                args: vec![ScriptValue::Entity(other)],
            });
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "collision_to_script",
        phase: TickPhase::Maintenance,
        run: collision_to_script,
        // After `collision_dispatch` (10) put this frame's contacts on the
        // channel. Running before it would deliver the previous frame's, which
        // is a frame of lag for no reason.
        order_hint: 20,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{
        ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout,
        RuntimeComponentDecl, ScriptRole, SemanticDomain,
    };
    use khora_core::ecs::entity::EntityId;
    use khora_core::physics::collision_channel;
    use khora_core::script::engine_event_channel;

    /// A world where `scripted` runs a behavior and `plain` does not.
    fn a_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let scripted = holding(&mut world, guard);
        let plain = world.spawn(crate::ecs::Transform::default());
        (world, scripted, plain)
    }

    fn wired() -> (Runtime, Channel<Collision>, Channel<ScriptEvent>) {
        let collisions = collision_channel();
        let events = engine_event_channel();
        let mut runtime = Runtime::default();
        runtime.resources.insert(collisions.clone());
        runtime.resources.insert(events.clone());
        (runtime, collisions, events)
    }

    fn run(world: &mut World, runtime: &Runtime) {
        collision_to_script(world, runtime, &mut OutputDeck::new());
    }

    /// **The first producer this queue has ever had.** Before it, `on Touched`
    /// was a handler a script could declare and never see fire.
    #[test]
    fn a_contact_becomes_an_event_for_the_scripted_side() {
        let (mut world, scripted, plain) = a_world();
        let (runtime, collisions, events) = wired();
        collisions.send(Collision {
            kind: CollisionKind::Started,
            a: scripted,
            b: plain,
        });

        run(&mut world, &runtime);

        let raised = events.drain();
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].target, scripted);
        assert_eq!(raised[0].name, TOUCHED);
        assert_eq!(raised[0].args, vec![ScriptValue::Entity(plain)]);
    }

    /// A behavior asks "what touched *me*", so neither side is the subject and
    /// both that run one hear about it — each carrying the other.
    #[test]
    fn a_contact_between_two_scripted_entities_raises_both_ways() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let one = holding(&mut world, guard);
        let other = holding(&mut world, guard);
        let (runtime, collisions, events) = wired();
        collisions.send(Collision {
            kind: CollisionKind::Started,
            a: one,
            b: other,
        });

        run(&mut world, &runtime);

        let raised = events.drain();
        assert_eq!(raised.len(), 2);
        assert!(raised.iter().any(|e| e.target == one));
        assert!(raised.iter().any(|e| e.target == other));
    }

    /// **The bounded channel is why this filters.** Two pieces of scenery
    /// grinding together would otherwise fill it with events nobody handles,
    /// and drop the ones that had a handler waiting.
    #[test]
    fn scenery_touching_scenery_raises_nothing() {
        let mut world = World::new();
        let one = world.spawn(crate::ecs::Transform::default());
        let other = world.spawn(crate::ecs::Transform::default());
        let (runtime, collisions, events) = wired();
        collisions.send(Collision {
            kind: CollisionKind::Started,
            a: one,
            b: other,
        });

        run(&mut world, &runtime);

        assert!(events.is_empty());
    }

    #[test]
    fn separating_raises_its_own_name() {
        let (mut world, scripted, plain) = a_world();
        let (runtime, collisions, events) = wired();
        collisions.send(Collision {
            kind: CollisionKind::Stopped,
            a: scripted,
            b: plain,
        });

        run(&mut world, &runtime);

        assert_eq!(events.drain()[0].name, SEPARATED);
    }

    /// Its cursor is its own, so a second run delivers only what arrived since.
    #[test]
    fn a_contact_is_delivered_once() {
        let (mut world, scripted, plain) = a_world();
        let (runtime, collisions, events) = wired();
        collisions.send(Collision {
            kind: CollisionKind::Started,
            a: scripted,
            b: plain,
        });

        run(&mut world, &runtime);
        run(&mut world, &runtime);

        assert_eq!(events.len(), 1);
    }

    /// Registers the declared component `name`, playing `role` for scripts,
    /// with one `int` field.
    fn declare(world: &mut World, name: &str, role: Option<ScriptRole>) -> ComponentKey {
        let key = world
            .register_runtime_component(RuntimeComponentDecl {
                name: name.to_owned(),
                domain: SemanticDomain::Script,
                provenance: ComponentProvenance::Authored,
                resumable: false,
                layout: PackedLayout::new(vec![PackedField {
                    name: "n".to_owned(),
                    kind: FieldKind::Int,
                    default: ScriptValue::Int(0),
                }])
                .expect("a valid layout"),
            })
            .unwrap_or_else(|e| panic!("`{name}` registers: {e}"));
        world.set_script_role(key, role);
        key
    }

    /// The behavior `Guard`, declared by `ai/guard.erg`.
    fn guard_behavior(world: &mut World) -> ComponentKey {
        declare(
            world,
            "Guard",
            Some(ScriptRole::Behavior {
                module: "ai/guard.erg".to_owned(),
                order: 0,
            }),
        )
    }

    /// An entity holding declared component `key` and nothing scripted else.
    fn holding(world: &mut World, key: ComponentKey) -> EntityId {
        let entity = world.spawn(crate::ecs::Transform::default());
        world
            .add_runtime_component(entity, key, &ScriptValue::Unit)
            .expect("the component attaches");
        entity
    }

    /// One contact of `kind` between `a` and `b`, run through the system;
    /// what it raised.
    fn raised_by(
        world: &mut World,
        kind: CollisionKind,
        a: EntityId,
        b: EntityId,
    ) -> Vec<ScriptEvent> {
        let (runtime, collisions, events) = wired();
        collisions.send(Collision { kind, a, b });
        run(world, &runtime);
        events.drain()
    }

    /// **A behavior is a component.** Adding `Guard` to an entity is what
    /// attaches the behavior, so holding it is what makes
    /// the entity hear that something touched it.
    #[test]
    fn an_entity_holding_a_behavior_component_hears_it_was_touched() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let scripted = holding(&mut world, guard);
        let plain = world.spawn(crate::ecs::Transform::default());

        let raised = raised_by(&mut world, CollisionKind::Started, plain, scripted);

        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].target, scripted);
        assert_eq!(raised[0].name, TOUCHED);
        assert_eq!(raised[0].args, vec![ScriptValue::Entity(plain)]);
    }

    #[test]
    fn an_entity_holding_a_behavior_component_hears_it_separated() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let scripted = holding(&mut world, guard);
        let plain = world.spawn(crate::ecs::Transform::default());

        let raised = raised_by(&mut world, CollisionKind::Stopped, scripted, plain);

        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].target, scripted);
        assert_eq!(raised[0].name, SEPARATED);
        assert_eq!(raised[0].args, vec![ScriptValue::Entity(plain)]);
    }

    /// Any behavior counts, not only the first declared: each side holding
    /// a different one hears about the other.
    #[test]
    fn two_entities_holding_different_behaviors_both_hear_the_contact() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let door = declare(
            &mut world,
            "Door",
            Some(ScriptRole::Behavior {
                module: "props/door.erg".to_owned(),
                order: 0,
            }),
        );
        let one = holding(&mut world, guard);
        let other = holding(&mut world, door);

        let raised = raised_by(&mut world, CollisionKind::Started, one, other);

        assert_eq!(raised.len(), 2, "{raised:?}");
        assert!(raised
            .iter()
            .any(|e| e.target == one && e.args == vec![ScriptValue::Entity(other)]));
        assert!(raised
            .iter()
            .any(|e| e.target == other && e.args == vec![ScriptValue::Entity(one)]));
    }

    /// A declared component with no role is data, not a program: an entity
    /// holding only that runs nothing, so an event for it would only take a
    /// place in the bounded channel.
    #[test]
    fn an_entity_holding_only_a_plain_declared_component_hears_nothing() {
        let mut world = World::new();
        let health = declare(&mut world, "Health", None);
        let holder = holding(&mut world, health);
        let plain = world.spawn(crate::ecs::Transform::default());

        assert!(raised_by(&mut world, CollisionKind::Started, holder, plain).is_empty());
    }

    /// A state companion is what the engine keeps of a behavior, not the
    /// behavior: left alone on an entity (its behavior removed), it runs
    /// nothing.
    #[test]
    fn an_entity_holding_only_a_state_companion_hears_nothing() {
        let mut world = World::new();
        guard_behavior(&mut world);
        let state = declare(
            &mut world,
            "Guard#state",
            Some(ScriptRole::BehaviorState {
                behavior: "Guard".to_owned(),
            }),
        );
        let holder = holding(&mut world, state);
        let plain = world.spawn(crate::ecs::Transform::default());

        assert!(raised_by(&mut world, CollisionKind::Started, holder, plain).is_empty());
    }

    /// An entity whose behavior component was removed stops being scripted:
    /// what is checked is what it holds now, not what it once held.
    #[test]
    fn an_entity_whose_behavior_component_was_removed_hears_nothing() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let holder = holding(&mut world, guard);
        assert!(
            world.remove_component_by_key(holder, guard),
            "the behavior detaches"
        );
        let plain = world.spawn(crate::ecs::Transform::default());

        assert!(raised_by(&mut world, CollisionKind::Started, holder, plain).is_empty());
    }

    /// An entity running two behaviors is one subject: one event per side
    /// of the contact, which the lane hands to each behavior it runs, never
    /// two in the channel.
    #[test]
    fn an_entity_holding_two_behaviors_hears_a_contact_once() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let door = declare(
            &mut world,
            "Door",
            Some(ScriptRole::Behavior {
                module: "props/door.erg".to_owned(),
                order: 0,
            }),
        );
        let both = holding(&mut world, guard);
        world
            .add_runtime_component(both, door, &ScriptValue::Unit)
            .expect("the second behavior attaches");
        let plain = world.spawn(crate::ecs::Transform::default());

        let raised = raised_by(&mut world, CollisionKind::Started, both, plain);

        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].target, both);
    }

    /// An entity despawned before the contact is read runs nothing any more:
    /// its behavior component went with it.
    #[test]
    fn a_despawned_behavior_holder_hears_nothing() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let gone = holding(&mut world, guard);
        let plain = world.spawn(crate::ecs::Transform::default());
        assert!(world.despawn(gone), "the holder despawns");

        assert!(raised_by(&mut world, CollisionKind::Started, gone, plain).is_empty());
    }

    /// A contact naming a despawned entity whose slot now holds a newer
    /// behavior holder is about the dead one: the newcomer is not told.
    #[test]
    fn a_contact_naming_a_recycled_entity_reaches_neither_generation() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        let stale = holding(&mut world, guard);
        assert!(world.despawn(stale), "the holder despawns");
        let newcomer = holding(&mut world, guard);
        assert_eq!(newcomer.index, stale.index, "the slot is reused");
        assert_ne!(newcomer.generation, stale.generation);
        let plain = world.spawn(crate::ecs::Transform::default());

        let raised = raised_by(&mut world, CollisionKind::Started, stale, plain);

        assert!(raised.is_empty(), "{raised:?}");
    }

    /// An orphaned behavior — no script declares it any more, its role and
    /// data kept — is still one the flow projects, so its holder still hears
    /// contacts: who is told follows who runs.
    #[test]
    fn an_orphaned_behavior_holder_still_hears_a_contact() {
        let mut world = World::new();
        let guard = guard_behavior(&mut world);
        world.set_declared_by(guard, Some("ai/guard.erg".to_owned()));
        let holder = holding(&mut world, guard);
        world.set_declared_by(guard, None);
        let plain = world.spawn(crate::ecs::Transform::default());

        let raised = raised_by(&mut world, CollisionKind::Started, holder, plain);

        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].target, holder);
    }

    /// Physics without scripting is an ordinary configuration, not a failure.
    #[test]
    fn a_runtime_with_no_script_channel_runs_without_complaint() {
        let (mut world, _, _) = a_world();
        let mut runtime = Runtime::default();
        runtime.resources.insert(collision_channel());

        run(&mut world, &runtime);
    }
}
