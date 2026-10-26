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

//! A phase a game removes leaves no agent stranded in it: an agent with no
//! phase left to run in refuses the boot, naming itself and the phase.

use khora_sdk::khora_core::agent::ExecutionPhase;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// A game that removes the phase the shadow agent runs in.
struct NoObserve;

impl AgentProvider for NoObserve {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for NoObserve {
    fn removed_phases(&self) -> Vec<ExecutionPhase> {
        vec![ExecutionPhase::OBSERVE]
    }
}
impl EngineApp for NoObserve {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        NoObserve
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **An agent whose every phase was removed refuses the boot.** The shadow
/// agent runs only in `OBSERVE`; a game removing `OBSERVE` would keep it
/// registered and budgeted, yet it would never draw a shadow.
#[test]
#[should_panic(expected = "ShadowRenderer")]
fn removing_the_only_phase_of_an_agent_refuses_the_boot() {
    let mut engine = EngineCore::<NoObserve>::new();
    engine.bootstrap(NoObserve, Runtime::new());
}
