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

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::{Contention, ExecutionPhase};
use khora_sdk::khora_core::EngineContext;
use khora_sdk::{
    AgentRegistry, EngineApp, EngineCore, ExecutionTiming, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

use crate::protocol::{Hand, Work};

/// A game that removes the phase the shadows family runs in.
struct NoObserve;

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

/// **An agent whose every phase was removed refuses the boot.** The shadows
/// family runs only in `OBSERVE`; a game removing `OBSERVE` would keep its
/// agent registered and budgeted, yet it would never draw a shadow. The boot
/// names the family.
#[test]
#[should_panic(expected = "the family `shadows`")]
fn removing_the_only_phase_of_an_agent_refuses_the_boot() {
    let mut engine = EngineCore::<NoObserve>::new();
    engine.bootstrap(NoObserve, Runtime::new());
}

/// A game that removes `MUTATE`, a phase no family runs in, and whose
/// `provide` puts an agent running only there into the agent registry the
/// engine publishes in the runtime.
struct StrandedInMutate;

impl PhaseProvider for StrandedInMutate {
    fn removed_phases(&self) -> Vec<ExecutionPhase> {
        vec![ExecutionPhase::MUTATE]
    }
}
impl EngineApp for StrandedInMutate {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        StrandedInMutate
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn provide(&self, runtime: &mut Runtime) {
        let timing = ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::MUTATE],
            default_phase: ExecutionPhase::MUTATE,
            ..ExecutionTiming::default()
        };
        let agent = Hand::new(
            AgentId::new("weather"),
            timing,
            Work::new(Contention::none(), |_: &mut EngineContext<'_>| {}),
        )
        .shared();
        runtime
            .resources
            .get::<Arc<Mutex<AgentRegistry>>>()
            .expect("the engine publishes its agent registry before `provide`")
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .register(agent, 1.0);
    }
}

/// **An app's agent in a removed phase refuses the boot.** No family runs in
/// `MUTATE`, so removing it strands no family; but an agent the app put in
/// the registry through `provide` runs only there: it would be registered and
/// budgeted, yet never run. The boot refuses it as it refuses a family.
#[test]
#[should_panic(expected = "which the app removed")]
fn an_app_agent_in_a_removed_phase_refuses_to_boot() {
    let mut engine = EngineCore::<StrandedInMutate>::new();
    engine.bootstrap(StrandedInMutate, Runtime::new());
}
