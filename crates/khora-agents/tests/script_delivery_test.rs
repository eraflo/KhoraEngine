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

//! What the engine puts in a queue reaches the agent that drains it.
//!
//! The queues themselves are tested where they live. What is tested here is the
//! join: that the scripting agent, running under **its own declaration** and
//! nothing wider, can actually reach them. That join is where the original bug
//! was — the agent could not reach what its own hot-reload pump filled, and the
//! symptom was silence, not a failure. A unit test of either end alone would
//! have stayed green through all of it.

use std::sync::Arc;

use khora_agents::script_agent::ScriptingAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{engine_event_channel, ScriptEvent};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script_hot_reload::reload_channel;
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;

/// Runs one frame of the agent against `runtime`, stamped with exactly the
/// permit the scheduler would stamp — a test that granted more would be testing
/// a context the engine never builds.
/// One entity running one behavior, so the agent gets past its "no scripts in
/// this scene" exit. The module is never compiled, so the lane runs and reports
/// nothing — which is all these tests need it to do.
fn a_scene_with_one_script() -> ScriptView {
    ScriptView {
        delta_seconds: 0.0,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: "ai/guard.erg".to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: EntityId {
                index: 1,
                generation: 0,
            },
            program: 0,
            authored: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
        }],
    }
}

fn run_one_frame(agent: &mut ScriptingAgent, runtime: &Arc<Runtime>) {
    let mut bus = LaneBus::new();
    bus.publish(a_scene_with_one_script());
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
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

#[test]
fn a_queued_reload_reaches_the_agent() {
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: "ai/guard.erg".to_owned(),
        program: Program::default(),
    });

    let mut runtime = Runtime::default();
    runtime.resources.insert(reloads.clone());
    let runtime = Arc::new(runtime);

    run_one_frame(&mut ScriptingAgent::default(), &runtime);

    assert!(
        reloads.is_empty(),
        "the agent drained the queue — an undeclared reach would have left it full"
    );
}

#[test]
fn a_queued_engine_event_reaches_the_agent() {
    let events = engine_event_channel();
    events.send(ScriptEvent {
        target: EntityId {
            index: 1,
            generation: 0,
        },
        name: "Damaged".to_owned(),
        args: Vec::new(),
    });

    let mut runtime = Runtime::default();
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);

    run_one_frame(&mut ScriptingAgent::default(), &runtime);

    assert!(events.is_empty(), "the agent took the event");
}

/// **The channel is the buffer; the agent's inbox is not.** A scene with no
/// scripts must leave an engine-raised event where it is — bounded, counted,
/// and still there next frame — rather than move it into a plain `Vec` that
/// nothing will ever drain.
#[test]
fn an_engine_event_waits_in_the_channel_while_no_script_runs() {
    let events = engine_event_channel();
    events.send(ScriptEvent {
        target: EntityId {
            index: 1,
            generation: 0,
        },
        name: "Damaged".to_owned(),
        args: Vec::new(),
    });

    let mut runtime = Runtime::default();
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);

    // No view published: the flow has not run, or the scene holds no scripts.
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let mut agent = ScriptingAgent::default();
    let permit = agent.contention();
    let mut ctx = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(&runtime),
        &bus,
        &mut deck,
        &permit,
        Some(agent.id()),
    );
    agent.execute(&mut ctx);

    assert_eq!(
        events.len(),
        1,
        "still queued, for a frame that can deliver it"
    );
}

/// **The reach must come from the declaration, not from the runtime holding
/// the queue.** Stamped with an empty permit — what an agent that forgot to
/// declare would get — the same drain finds nothing and the queue stays full.
/// Without this, an over-broad permit elsewhere would make the tests above pass
/// for the wrong reason.
#[test]
fn an_agent_that_declared_nothing_reaches_neither() {
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: "ai/guard.erg".to_owned(),
        program: Program::default(),
    });
    let events = engine_event_channel();
    events.send(ScriptEvent {
        target: EntityId {
            index: 1,
            generation: 0,
        },
        name: "Damaged".to_owned(),
        args: Vec::new(),
    });

    let mut runtime = Runtime::default();
    runtime.resources.insert(reloads.clone());
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);

    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let nothing = khora_core::agent::Contention::none();
    let mut agent = ScriptingAgent::default();
    let mut ctx = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(&runtime),
        &bus,
        &mut deck,
        &nothing,
        Some(agent.id()),
    );
    agent.execute(&mut ctx);

    assert_eq!(reloads.len(), 1, "refused, so still queued");
    assert_eq!(events.len(), 1, "refused, so still queued");
}

/// A packed build installs no watcher and may raise no engine events, so
/// neither queue is in the runtime. That is the shipping path, not a failure.
#[test]
fn an_agent_whose_queues_are_absent_still_runs() {
    run_one_frame(
        &mut ScriptingAgent::default(),
        &Arc::new(Runtime::default()),
    );
}
