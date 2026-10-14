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

//! A contact the engine reports reaches a handler a script declared.
//!
//! Every piece of this was tested where it lives and every piece passed while
//! the whole did nothing: the queue was written, the drain was written, and
//! nothing ever filled the queue. What is checked here is the join — that
//! `on Touched(Entity other)` fires, with the other entity in hand.

use std::sync::Arc;

use khora_agents::script_agent::ScriptAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{engine_event_channel, ScriptEvent, ScriptValue, WorldCommand};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script::hot_reload::reload_channel;
use khora_script::reload::ScriptReload;

const MODULE: &str = "ai/guard.erg";

/// A guard that despawns whatever touches it.
///
/// The command it queues lands on the deck, which is the only thing a test
/// outside the lane can observe — and it observes **both** halves at once: that
/// the handler fired, and that the entity it was handed is the right one. A
/// behavior that merely incremented a field would prove the first and say
/// nothing about the second.
const GUARD: &str = r#"
behavior Guard {
    on Touched(Entity other) {
        other.Despawn();
    }
}
"#;

fn compile(source: &str) -> khora_script::vm::Program {
    let parsed = khora_script::parse(khora_script::lex(source).tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = khora_script::check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = khora_script::compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

fn a_scene_with_a_guard() -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: 0.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(1),
            program: 0,
            arrival: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
            row: Vec::new(),
        }],
    }
}

/// Runs one frame of the agent, returning the commands its behaviors queued.
fn run_one_frame(agent: &mut ScriptAgent, runtime: &Arc<Runtime>) -> Vec<WorldCommand> {
    let mut bus = LaneBus::new();
    bus.publish(a_scene_with_a_guard());
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    {
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);
    }
    deck.take::<khora_core::script::CommandBuffer>()
        .as_slice()
        .to_vec()
}

/// **The whole chain, from a physics contact to a behavior's handler.**
///
/// The event is put on the channel the way `collision_to_script` puts it there.
/// Delivery is next frame by design, so the guard runs twice: once to be
/// spawned, once to hear about the contact.
#[test]
fn a_touched_event_reaches_the_behavior_that_declared_the_handler() {
    let events = engine_event_channel();
    // The program reaches the agent the way every program does — through the
    // reload channel the hot-reload pump fills. There is no other road in, and
    // a test that invented one would be testing a road production does not use.
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: compile(GUARD),
    });
    let mut runtime_with_reload = Runtime::default();
    // The scripting world, as `EngineBuilder` registers it. The agent
    // holds a handle to it, not the thing.
    runtime_with_reload
        .services
        .insert(std::sync::Arc::new(std::sync::Mutex::new(
            khora_lanes::script_lane::ScriptRuntime::new(),
        ))
            as std::sync::Arc<
                std::sync::Mutex<khora_lanes::script_lane::ScriptRuntime>,
            >);
    runtime_with_reload.resources.insert(events.clone());
    runtime_with_reload.resources.insert(reloads);
    let runtime = Arc::new(runtime_with_reload);

    let mut agent = ScriptAgent::default();
    // The DCC does this every frame; without it the agent has no fuel and
    // defers everything, which is a budget of nothing rather than a bug.
    agent.apply_budget(khora_core::agent::gorna::ResourceBudget {
        strategy_id: khora_core::agent::gorna::StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(1),
        memory_limit: None,
        extra_params: Default::default(),
    });

    // Frame one: the program is applied and the guard spawns. Nothing has
    // touched it, so it queues nothing.
    let quiet = run_one_frame(&mut agent, &runtime);
    assert!(quiet.is_empty(), "nothing touched it yet; got {quiet:?}");

    // What `collision_to_script` sends when physics reports a contact.
    events.send(ScriptEvent {
        target: entity(1),
        name: khora_data::ecs::systems::collision_to_script::TOUCHED.to_owned(),
        args: vec![ScriptValue::Entity(entity(2))],
    });

    let queued = run_one_frame(&mut agent, &runtime);

    assert!(
        queued.iter().any(|command| matches!(
            command,
            WorldCommand::Despawn { entity: target } if *target == entity(2)
        )),
        "the handler fired and was handed the entity that touched it; got {queued:?}"
    );
    assert!(
        events.is_empty(),
        "the agent took the event rather than leaving it queued"
    );
}

// ─── A behavior attached by its component ───────────────────────────────────

/// A guard attached as a component: it counts what touched and left it, and
/// despawns what touched it — so the handler's argument is checked too.
const COMPONENT_GUARD: crate::behavior_session::Project = &[(
    MODULE,
    "behavior Guard {
         int touched = 0;
         int separated = 0;
         on Touched(Entity other) { touched += 1; other.Despawn(); }
         on Separated(Entity other) { separated += 1; }
     }",
)];

/// Puts `contact` through the engine's `Maintenance` systems — where
/// `collision_to_script` turns a contact into events — and hands what they
/// raised to `session`, for its next frame.
fn report_contact(
    session: &mut crate::behavior_session::Session,
    contact: khora_core::physics::Collision,
) {
    let collisions = khora_core::physics::collision_channel();
    let raised = engine_event_channel();
    let mut runtime = Runtime::default();
    runtime.resources.insert(collisions.clone());
    runtime.resources.insert(raised.clone());
    collisions.send(contact);
    khora_control::substrate::run_data_systems(
        &mut session.world,
        &runtime,
        &mut OutputDeck::new(),
        khora_data::ecs::TickPhase::Maintenance,
    );
    for event in raised.drain() {
        session.send(event);
    }
}

/// **From a physics contact to the handler of a behavior attached by its
/// component** — no hand-built view: the contact goes through
/// `collision_to_script`, the flow projects the guard from its component,
/// the lane runs `on Touched` with the other entity in hand, and the field
/// it wrote is the `World`'s.
#[test]
fn a_contact_reaches_the_handler_of_a_behavior_attached_by_its_component() {
    use crate::behavior_session::{attach, int, Session};
    use khora_core::physics::{Collision, CollisionKind};

    let mut session = Session::new(khora_data::ecs::World::new(), COMPONENT_GUARD);
    let guard = session.world.spawn(khora_data::ecs::Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    let visitor = session.world.spawn(khora_data::ecs::Transform::identity());
    session.frame(0.016);

    report_contact(
        &mut session,
        Collision {
            kind: CollisionKind::Started,
            a: visitor,
            b: guard,
        },
    );
    session.frame(0.016);

    assert_eq!(int(&session.world, guard, "Guard", "touched"), Some(1));
    assert!(
        !session.world.contains(visitor),
        "the handler was handed the entity that touched it"
    );

    report_contact(
        &mut session,
        Collision {
            kind: CollisionKind::Stopped,
            a: guard,
            b: visitor,
        },
    );
    session.frame(0.016);

    assert_eq!(int(&session.world, guard, "Guard", "separated"), Some(1));
    assert_eq!(
        int(&session.world, guard, "Guard", "touched"),
        Some(1),
        "a contact is heard once"
    );
}
