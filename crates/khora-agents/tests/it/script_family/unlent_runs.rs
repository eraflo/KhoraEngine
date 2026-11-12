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

//! A script family the DCC has not lent anything yet runs its level at the
//! level's own price on the frame's inputs. That price must buy progress: a
//! turn cut for want of fuel resumes, and finishes.

use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script::ScriptRuntime;

use super::fixtures::{bus_with, run_script_agent, runtime_with, script_agent};
use crate::behavior_session::{compile, Project};

/// `Heavy`: a loop of three hundred a turn, the same every turn.
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

/// **Never lent a budget, a behaviour heavier than the first guess still
/// finishes its turn.** The level's own price prices a turn not yet measured
/// at a guess; one `Heavy` takes more than that (about two thousand
/// instructions), so its first turn is cut and resumed. Its turn is the same
/// every frame, so within five seconds of frames it must have finished one.
#[test]
fn never_lent_a_budget_a_behaviour_heavier_than_the_guess_finishes_its_turn() {
    let mut scripts = ScriptRuntime::new();
    let (module, _) = HEAVY[0];
    scripts.add_program(module, compile(HEAVY, module));
    let shared = Arc::new(Mutex::new(scripts));
    let runtime = Arc::new(runtime_with(&shared));
    let mut agent = script_agent();
    let bus = bus_with(heavies(1));

    let mut completed = 0;
    let mut spent = Vec::new();
    for _ in 0..300 {
        run_script_agent(&mut agent, &runtime, &bus, &mut OutputDeck::new());
        let scripts = shared.lock().expect("uncontended in test");
        completed += scripts.last_report().completed;
        spent.push(scripts.last_report().spent);
    }

    assert!(
        completed > 0,
        "300 frames at the level's own price finished no turn; \
         fuel spent per frame, the last 100: {:?}",
        &spent[200..]
    );
}
