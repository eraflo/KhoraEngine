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

//! What the engine puts in a queue reaches the scripting world, and the
//! behaviors it runs.
//!
//! The queues themselves are tested where they live. What is tested here is the
//! join: that the `script_intake` data system takes from the very queues the
//! engine fills into the very scripting world the agent's lane runs against,
//! and that the agent, under its own declaration, leaves them to it. That join
//! is where the original bug was — what the hot-reload pump filled never
//! reached the scripts, and the symptom was silence, not a failure. A unit
//! test of either end alone would have stayed green through all of it.

use std::sync::{Arc, Mutex};

use khora_agents::script_agent::ScriptAgent;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{engine_event_channel, ScriptEvent};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script::hot_reload::reload_channel;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};

use crate::script_intake::run_intake;

const MODULE: &str = "ai/guard.erg";

/// One entity running one behavior, so the agent gets past its "no scripts in
/// this scene" exit.
fn a_scene_with_one_script() -> ScriptView {
    ScriptView {
        left_scope: Vec::new(),
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: the_guard(),
            program: 0,
            arrival: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
            row: Vec::new(),
        }],
    }
}

fn the_guard() -> EntityId {
    EntityId {
        index: 1,
        generation: 0,
    }
}

fn damaged() -> ScriptEvent {
    ScriptEvent {
        target: the_guard(),
        name: "Damaged".to_owned(),
        args: Vec::new(),
    }
}

/// Runs one frame of the agent against `runtime`, stamped with exactly the
/// permit the scheduler would stamp — a test that granted more would be testing
/// a context the engine never builds.
fn run_one_frame(agent: &mut ScriptAgent, runtime: &Arc<Runtime>) {
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

/// A `Runtime` holding the scripting world, as `EngineBuilder` registers it,
/// and a handle to it.
fn a_runtime_with_scripting() -> (Runtime, Arc<Mutex<ScriptRuntime>>) {
    let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&scripts));
    (runtime, scripts)
}

/// **A queued reload reaches the scripting world**, taken by the intake from
/// the channel the pump fills.
#[test]
fn a_queued_reload_reaches_the_scripting_world_through_the_intake() {
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: Some(Program::default()),
    });
    let (mut runtime, scripts) = a_runtime_with_scripting();
    runtime.resources.insert(reloads.clone());

    run_intake(&runtime);

    assert!(reloads.is_empty(), "the intake drained the queue");
    assert!(
        scripts
            .lock()
            .expect("the scripting world")
            .program(MODULE)
            .is_some(),
        "and installed what it held"
    );
}

/// **A queued engine event reaches the mail**, taken by the intake from the
/// channel the engine raises into.
#[test]
fn a_queued_engine_event_reaches_the_mail_through_the_intake() {
    let events = engine_event_channel();
    events.send(damaged());
    let (mut runtime, scripts) = a_runtime_with_scripting();
    runtime.resources.insert(events.clone());

    run_intake(&runtime);

    assert!(events.is_empty(), "the intake took the event");
    assert_eq!(
        scripts.lock().expect("the scripting world").pending_len(),
        1,
        "into the scripting world's mail"
    );
}

/// **An engine event taken in is delivered that frame.** The intake runs
/// before the agent, so the guard's handler hears the hit in the same frame
/// the engine's channel held it.
#[test]
fn an_engine_event_taken_in_is_delivered_that_frame() {
    const GUARD: &str = "event Damaged;
    behavior Guard {
        int hits = 0;
        on Damaged() { hits += 1; }
    }";
    let compiled = khora_script::compile_module(
        &khora_script::MemoryLoader::new().with(MODULE, GUARD),
        MODULE,
    );
    let program = compiled.program.expect("the guard compiles");
    let events = engine_event_channel();
    let (mut runtime, scripts) = a_runtime_with_scripting();
    scripts
        .lock()
        .expect("the scripting world")
        .add_program(MODULE, program);
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);
    let mut agent = ScriptAgent::default();
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(50),
        memory_limit: None,
        extra_params: Default::default(),
    });
    // A first frame spawns the guard.
    run_intake(&runtime);
    run_one_frame(&mut agent, &runtime);

    events.send(damaged());
    run_intake(&runtime);
    run_one_frame(&mut agent, &runtime);

    let scripts = scripts.lock().expect("the scripting world");
    let slot = scripts
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .and_then(|layout| layout.slot_of("hits"))
        .expect("the guard's `hits`");
    assert_eq!(
        scripts
            .peek(the_guard(), "Guard")
            .and_then(|instance| instance.fields.get(slot).cloned()),
        Some(Persisted::Scalar(Value::Int(1))),
        "the guard heard the hit the frame it was taken in"
    );
}

/// **The agent leaves both queues to the intake.** Under its own permit, a
/// frame of the agent over a scene with a script drains neither the reload
/// nor the event channel: what the engine put there waits for the next
/// frame's boundary.
#[test]
fn the_agents_frame_leaves_both_queues_to_the_intake() {
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: Some(Program::default()),
    });
    let events = engine_event_channel();
    events.send(damaged());
    let (mut runtime, _) = a_runtime_with_scripting();
    runtime.resources.insert(reloads.clone());
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);

    run_one_frame(&mut ScriptAgent::default(), &runtime);

    assert_eq!(reloads.len(), 1, "the reload is the intake's to take");
    assert_eq!(events.len(), 1, "the event is the intake's to take");
}

/// **The channel is the buffer; the agent's inbox is not.** A scene with no
/// scripts must leave an engine-raised event where it is — bounded, counted,
/// and still there next frame — rather than move it into a plain `Vec` that
/// nothing will ever drain.
#[test]
fn an_engine_event_waits_in_the_channel_while_no_script_runs() {
    let events = engine_event_channel();
    events.send(damaged());
    let (mut runtime, _) = a_runtime_with_scripting();
    runtime.resources.insert(events.clone());
    let runtime = Arc::new(runtime);

    // No view published: the flow has not run, or the scene holds no scripts.
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let mut agent = ScriptAgent::default();
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

/// A packed build installs no watcher and may raise no engine events, so
/// neither queue is in the runtime. That is the shipping path, not a failure.
#[test]
fn an_agent_whose_queues_are_absent_still_runs() {
    let (runtime, _) = a_runtime_with_scripting();
    let runtime = Arc::new(runtime);
    run_intake(&runtime);
    run_one_frame(&mut ScriptAgent::default(), &runtime);
}
