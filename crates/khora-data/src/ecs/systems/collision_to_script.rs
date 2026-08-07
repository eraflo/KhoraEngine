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
//! the events that had a handler waiting. The check is the `Script` component,
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

use crate::ecs::{DataSystemRegistration, Script, TickPhase, World};

/// What a behavior declares to hear that something started touching it.
pub const TOUCHED: &str = "Touched";

/// And to hear that it stopped.
pub const SEPARATED: &str = "Separated";

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

    // Collected once rather than queried per contact: a frame of scenery
    // grinding together is hundreds of contacts, and each `query` is a walk.
    let scripted: Vec<_> = world
        .query::<(khora_core::ecs::entity::EntityId, &Script)>()
        .map(|(entity, _)| entity)
        .collect();

    for contact in contacts {
        let name = match contact.kind {
            CollisionKind::Started => TOUCHED,
            CollisionKind::Stopped => SEPARATED,
        };
        for (subject, other) in [(contact.a, contact.b), (contact.b, contact.a)] {
            if !scripted.contains(&subject) {
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
    use khora_core::ecs::entity::EntityId;
    use khora_core::physics::collision_channel;
    use khora_core::script::engine_event_channel;

    /// A world where `scripted` runs a behavior and `plain` does not.
    fn a_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let scripted = world.spawn(Script {
            module: "ai/guard.erg".to_owned(),
            behavior: "Guard".to_owned(),
            ..Default::default()
        });
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
        let one = world.spawn(Script {
            module: "ai/guard.erg".to_owned(),
            behavior: "Guard".to_owned(),
            ..Default::default()
        });
        let other = world.spawn(Script {
            module: "ai/guard.erg".to_owned(),
            behavior: "Guard".to_owned(),
            ..Default::default()
        });
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

    /// Physics without scripting is an ordinary configuration, not a failure.
    #[test]
    fn a_runtime_with_no_script_channel_runs_without_complaint() {
        let (mut world, _, _) = a_world();
        let mut runtime = Runtime::default();
        runtime.resources.insert(collision_channel());

        run(&mut world, &runtime);
    }
}
