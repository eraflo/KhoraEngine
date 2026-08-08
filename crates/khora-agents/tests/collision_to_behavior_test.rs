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

use khora_agents::script_agent::ScriptingAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{engine_event_channel, ScriptEvent, ScriptValue, WorldCommand};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script_hot_reload::reload_channel;
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
        Despawn(other);
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
        delta_seconds: 0.0,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(1),
            program: 0,
            authored: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
        }],
    }
}

/// Runs one frame of the agent, returning the commands its behaviors queued.
fn run_one_frame(agent: &mut ScriptingAgent, runtime: &Arc<Runtime>) -> Vec<WorldCommand> {
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
/// spawned and initialised, once to hear about the contact.
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

    let mut agent = ScriptingAgent::default();
    // The DCC does this every frame; without it the agent has no fuel and
    // defers everything, which is a budget of nothing rather than a bug.
    agent.apply_budget(khora_core::control::gorna::ResourceBudget {
        strategy_id: khora_core::control::gorna::StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(1),
        memory_limit: None,
        extra_params: Default::default(),
    });

    // Frame one: the program is applied and the guard initialises. Nothing has
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
