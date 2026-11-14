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

//! **An envelope lent before the scripts arrived is not kept for good.** The
//! DCC's first epoch prices the script family on a scene no module has
//! reached yet: nothing to run, a price of nothing, an envelope to match.
//! Once behaviours arrive, the price the lane quotes on its inputs drifts
//! from the one the envelope was lent on, the agent quotes every level again,
//! and no level fits: it reports over budget, which is what makes the DCC
//! open an epoch that prices the scripts again (`Overruns`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::gorna::protocol::{Envelope, Epoch};
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::cost::PerDevice;
use khora_core::lane::OutputDeck;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script::ScriptRuntime;

use super::fixtures::{bus_with, run_script_agent, runtime_with, script_agent};
use crate::families::script::behavior_session::{compile, Project};

/// `Heavy`: a loop of three hundred a turn.
const HEAVY: Project = &[(
    "game/heavy.erg",
    "behavior Heavy { int ticks = 0;
     void Update(float dt) { int n = 0; while (n < 300) { n += 1; } ticks += 1; } }",
)];

/// A view of `count` `Heavy`s in 1/60-s frames.
fn heavies(count: u32) -> ScriptView {
    ScriptView {
        left_scope: Vec::new(),
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 1.0 / 60.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: "game/heavy.erg".to_owned(),
            behavior: "Heavy".to_owned(),
        }],
        instances: (0..count)
            .map(|index| ScriptInstance {
                entity: EntityId {
                    index,
                    generation: 0,
                },
                program: 0,
                arrival: None,
                translation: Default::default(),
                rotation: Default::default(),
                scale: Default::default(),
                row: Vec::new(),
            })
            .collect(),
    }
}

/// An envelope of `time` on the host, and room for anything in memory.
fn on_the_host(time: Duration) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: time,
            ..PerDevice::default()
        },
        memory: PerDevice {
            cpu: u64::MAX,
            gpu: u64::MAX,
            npu: u64::MAX,
            other: u64::MAX,
        },
    }
}

/// **An envelope priced on an empty scene is reported short once behaviours
/// arrive.** Epoch 1 lends a microsecond, what an empty scene costs, and the
/// first frame runs on it; then four `Heavy`s arrive, each turn hundreds of
/// instructions. Within a few frames the agent reports over budget.
#[test]
fn an_envelope_priced_on_an_empty_scene_is_reported_short_once_behaviours_arrive() {
    let mut scripts = ScriptRuntime::new();
    let (module, _) = HEAVY[0];
    scripts.add_program(module, compile(HEAVY, module));
    let shared = Arc::new(Mutex::new(scripts));
    let runtime = Arc::new(runtime_with(&shared));
    let mut agent = script_agent();
    agent.apply(
        Epoch(1),
        Some(&on_the_host(Duration::from_micros(1))),
        None,
        None,
    );

    run_script_agent(
        &mut agent,
        &runtime,
        &bus_with(heavies(0)),
        &mut OutputDeck::new(),
    );
    assert!(
        !agent.report().over_budget,
        "set-up: an empty scene fits its envelope: {:?}",
        agent.report()
    );

    let mut short = false;
    for _ in 0..6 {
        run_script_agent(
            &mut agent,
            &runtime,
            &bus_with(heavies(4)),
            &mut OutputDeck::new(),
        );
        short |= agent.report().over_budget;
    }

    assert!(
        short,
        "four heavy behaviours do not fit a microsecond: {:?}",
        agent.report()
    );
}
