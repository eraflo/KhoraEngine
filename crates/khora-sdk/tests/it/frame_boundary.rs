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

//! The frame boundary, as the engine runs it: the `Boundary` DataSystems are
//! the first of the frame, once per frame, after the families on and off are
//! settled and before `PreSimulation` — so a boundary system serving a family
//! switched off before the frame does not run in it.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::lane::OutputDeck;
use khora_sdk::khora_data::ecs::{DataSystemRegistration, Role, SystemContext, TickPhase, World};
use khora_sdk::{
    Activation, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

const AUDIO: khora_sdk::AgentId = khora_sdk::khora_core::audio::AUDIO;

/// A game: every hook left to the engine's defaults.
struct Game;

impl PhaseProvider for Game {}
impl EngineApp for Game {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// Where the probes below write which of them ran: a runtime entry only this
/// file's engines hold.
#[derive(Default)]
struct Steps(Mutex<Vec<&'static str>>);

fn note(runtime: &Runtime, step: &'static str) {
    if let Some(steps) = runtime.resources.get::<Arc<Steps>>() {
        steps.0.lock().unwrap_or_else(|e| e.into_inner()).push(step);
    }
}

fn boundary(_: &mut World, runtime: &Runtime, _: &mut OutputDeck, _: SystemContext) {
    note(runtime, "boundary");
}

fn boundary_for_audio(_: &mut World, runtime: &Runtime, _: &mut OutputDeck, _: SystemContext) {
    note(runtime, "boundary for audio");
}

fn pre_simulation(_: &mut World, runtime: &Runtime, _: &mut OutputDeck, _: SystemContext) {
    note(runtime, "pre-simulation");
}

khora_sdk::inventory::submit! {
    DataSystemRegistration {
        name: "frame_boundary_probe",
        phase: TickPhase::Boundary,
        run: boundary,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Upkeep,
    }
}
khora_sdk::inventory::submit! {
    DataSystemRegistration {
        name: "frame_boundary_probe_for_audio",
        phase: TickPhase::Boundary,
        run: boundary_for_audio,
        order_hint: 1,
        runs_after: &[],
        observes: &[],
        role: Role::Serves(AUDIO),
    }
}
khora_sdk::inventory::submit! {
    DataSystemRegistration {
        name: "frame_boundary_probe_pre_simulation",
        phase: TickPhase::PreSimulation,
        run: pre_simulation,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Upkeep,
    }
}

/// What the probes wrote since the last call.
fn taken(steps: &Steps) -> Vec<&'static str> {
    std::mem::take(&mut *steps.0.lock().unwrap_or_else(|e| e.into_inner()))
}

/// **The boundary is the frame's first DataSystem phase, after the frame
/// settles.** Each frame runs its `Boundary` systems once, before any
/// `PreSimulation` one. The audio family is switched off between two frames:
/// the next frame settles it before its boundary, so the boundary system
/// serving audio does not run in it, while the upkeep one does.
#[test]
fn the_boundary_runs_first_once_the_frame_is_settled() {
    let steps = Arc::new(Steps::default());
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&steps));
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    let _ = taken(&steps);

    engine.tick();
    assert_eq!(
        taken(&steps),
        vec!["boundary", "boundary for audio", "pre-simulation"]
    );

    engine
        .runtime()
        .resources
        .get::<Arc<Activation>>()
        .expect("the bootstrap publishes the engine's activation")
        .deactivate(AUDIO)
        .expect("the audio family is known");
    engine.tick();
    assert_eq!(
        taken(&steps),
        vec!["boundary", "pre-simulation"],
        "the switch is settled before the boundary"
    );
}
