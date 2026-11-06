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

//! **The script family reckons its fuel each frame.** What the DCC lends is a
//! time, kept as it was lent and handed to the lane as its budget; the fuel
//! that time buys is reckoned in every run, at the rate and overhead the lane
//! has measured by then — never a fuel frozen when the allocation came in,
//! which a rate measured since would leave wrong for the rest of the epoch.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::agent::LaneAgent;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::ScriptRuntime;

use super::behavior_session::{compile, Project};
use crate::script_family::fixtures::{publish_script_views, script_agent};

/// `Spin`: an `Update` that never finishes, so every frame spends all the
/// fuel it is handed.
const SPIN: Project = &[(
    "game/spin.erg",
    "behavior Spin { int n = 0; void Update(float dt) { while (n >= 0) { n += 1; } } }",
)];

/// One `Spin`, in 1/60-s frames.
fn scene() -> ScriptView {
    ScriptView {
        left_scope: Vec::new(),
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 1.0 / 60.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: "game/spin.erg".to_owned(),
            behavior: "Spin".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: EntityId {
                index: 1,
                generation: 0,
            },
            program: 0,
            arrival: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
            row: Vec::new(),
        }],
    }
}

/// A runtime with the scripting world registered, as the engine wires it,
/// `Spin` loaded.
fn wired() -> (Arc<Runtime>, Arc<Mutex<ScriptRuntime>>) {
    let mut scripts = ScriptRuntime::new();
    let (module, _) = SPIN[0];
    scripts.add_program(module, compile(SPIN, module));
    let shared = Arc::new(Mutex::new(scripts));
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&shared));
    (Arc::new(runtime), shared)
}

/// One frame of the agent over the scene.
fn run_one_frame(agent: &mut LaneAgent, runtime: &Arc<Runtime>) {
    let mut bus = LaneBus::new();
    publish_script_views(&mut bus, scene());
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

/// Has the lane measure `rate` instructions per millisecond and no
/// overhead, over enough runs that nothing earlier is left of either.
fn measure(shared: &Arc<Mutex<ScriptRuntime>>, rate: u64) {
    let mut runtime = shared.lock().expect("uncontended in test");
    let turns = Duration::from_millis(1);
    for _ in 0..300 {
        runtime.observe(rate, turns, turns);
    }
}

/// What the last frame spent.
fn spent(shared: &Arc<Mutex<ScriptRuntime>>) -> u64 {
    shared
        .lock()
        .expect("uncontended in test")
        .last_report()
        .spent
}

/// **A rate measured between two frames changes the fuel of the second.**
/// One millisecond is lent once. Measured at two hundred instructions a
/// millisecond, the frame after spends about two hundred; measured at two
/// thousand before the next, that frame spends about two thousand — under
/// the same allocation.
#[test]
fn a_rate_measured_between_two_frames_changes_the_fuel_of_the_second() {
    let (runtime, shared) = wired();
    let mut agent = script_agent();
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: Duration::from_millis(1),
        memory_limit: None,
        extra_params: Default::default(),
    });

    measure(&shared, 200);
    run_one_frame(&mut agent, &runtime);
    let slow = spent(&shared);

    measure(&shared, 2_000);
    run_one_frame(&mut agent, &runtime);
    let fast = spent(&shared);

    assert!(
        (150..=260).contains(&slow),
        "a millisecond at 200 instr/ms is about 200 fuel: spent {slow}"
    );
    assert!(
        (1_800..=2_100).contains(&fast),
        "the same millisecond at 2000 instr/ms is about 2000 fuel: spent {fast}"
    );
}
