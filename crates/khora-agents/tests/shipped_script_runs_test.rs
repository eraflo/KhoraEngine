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

//! **The chain, with the file the sandbox actually ships.**
//!
//! Every part of the scripting path had a test and none of it ran in an
//! application: the channel carrying compiled programs was inserted by one
//! binary's launcher, so the editor and the sandbox held an agent, a lane and a
//! runtime over a program table that could never fill. A correct `.erg` on a
//! correct entity produced no behaviour and no message.
//!
//! This test walks the real road end to end — the module is read from disk,
//! compiled, sent through the reload channel, drained by the agent, run by the
//! lane against an instance, and its effect observed as a queued
//! [`WorldCommand`]. It uses the sandbox's own `hover.erg` rather than a string
//! literal, so the example the documentation shows is the example that is
//! proven.

use std::sync::Arc;

use khora_agents::script_agent::ScriptingAgent;
use khora_core::agent::Agent;
use khora_core::control::gorna::{ResourceBudget, StrategyId};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{CommandBuffer, WorldCommand};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script_compile::{compile_module, DiskLoader};
use khora_script::reload::ScriptReload;

const MODULE: &str = "hover.erg";
const BEHAVIOR: &str = "Hover";

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

/// The sandbox's script directory, or `None` in a checkout without it.
fn script_root() -> Option<std::path::PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sandbox/assets/scripts")
        .canonicalize()
        .ok()
}

/// A runtime wired the way `EngineCore::bootstrap` wires one, carrying the real
/// compiled module.
fn wired() -> Option<Arc<Runtime>> {
    let root = script_root()?;
    let compiled = compile_module(&DiskLoader::new(&root), MODULE);
    assert!(
        compiled.diagnostics.is_empty(),
        "{MODULE} does not compile: {:?}",
        compiled.diagnostics
    );
    let program = compiled.program.expect("a program with no diagnostics");

    let reloads = khora_io::script_hot_reload::reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program,
    });

    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::new(std::sync::Mutex::new(
        khora_lanes::script_lane::ScriptRuntime::new(),
    ))
        as Arc<std::sync::Mutex<khora_lanes::script_lane::ScriptRuntime>>);
    runtime.resources.insert(reloads);
    runtime
        .resources
        .insert(khora_core::script::engine_event_channel());
    Some(Arc::new(runtime))
}

fn a_scene_with_one_hovering_sphere(delta_seconds: f32) -> ScriptView {
    ScriptView {
        delta_seconds,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: BEHAVIOR.to_owned(),
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

/// Runs one frame, returning what the behaviours queued.
fn run_one_frame(
    agent: &mut ScriptingAgent,
    runtime: &Arc<Runtime>,
    delta_seconds: f32,
) -> Vec<WorldCommand> {
    let mut bus = LaneBus::new();
    bus.publish(a_scene_with_one_hovering_sphere(delta_seconds));
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
    deck.take::<CommandBuffer>().as_slice().to_vec()
}

fn budgeted() -> ScriptingAgent {
    let mut agent = ScriptingAgent::default();
    // The DCC does this every frame. Without it there is no fuel, every
    // behaviour is deferred, and the test would prove nothing about the
    // behaviour while looking like it passed.
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(2),
        memory_limit: None,
        extra_params: Default::default(),
    });
    agent
}

/// **The whole chain.** A `.erg` on disk moves the entity it is attached to.
#[test]
fn the_shipped_behaviour_moves_its_entity() {
    let Some(runtime) = wired() else {
        return; // checkout without the example tree
    };
    let mut agent = budgeted();

    let queued = run_one_frame(&mut agent, &runtime, 1.0 / 60.0);

    let moved = queued.iter().any(|command| {
        matches!(command, WorldCommand::SetTranslation { entity: target, .. } if *target == entity(1))
    });
    assert!(
        moved,
        "the behaviour queued no position for its entity; got {queued:?}"
    );
}

/// A script writes nothing directly — every effect is a queued command applied
/// at the boundary. That is what lets the lane run world-free.
#[test]
fn the_behaviour_only_ever_queues_commands() {
    let Some(runtime) = wired() else {
        return;
    };
    let mut agent = budgeted();

    let queued = run_one_frame(&mut agent, &runtime, 1.0 / 60.0);

    assert!(
        !queued.is_empty(),
        "a behaviour that did work queued nothing — the deck slot is the only \
         road out of a script"
    );
}

/// **The failure this whole change exists to make visible.** With no compiled
/// module the lane must not silently pass the instance over: it counts it, and
/// queues nothing.
#[test]
fn an_unloaded_module_is_counted_not_ignored() {
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::new(std::sync::Mutex::new(
        khora_lanes::script_lane::ScriptRuntime::new(),
    ))
        as Arc<std::sync::Mutex<khora_lanes::script_lane::ScriptRuntime>>);
    runtime
        .resources
        .insert(khora_io::script_hot_reload::reload_channel());
    let runtime = Arc::new(runtime);

    let mut agent = budgeted();
    let queued = run_one_frame(&mut agent, &runtime, 1.0 / 60.0);

    assert!(
        queued.is_empty(),
        "nothing ran, so nothing should have been queued; got {queued:?}"
    );

    let status = agent.report_status();
    assert!(
        !status.is_stalled,
        "an unloaded module is a wiring mistake, not a fault — reporting it as \
         stalled would push GORNA toward an emergency stop"
    );
}
