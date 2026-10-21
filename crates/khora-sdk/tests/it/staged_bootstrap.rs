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

//! The engine bootstrap taken apart into stages — `begin`, `setup`,
//! `register`, one `initialize_agent` per agent, `finish` — leaves the engine
//! as the one-shot `EngineCore::bootstrap` does, and each stage does its own
//! part of the work.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_sdk::khora_core::agent::{Agent, EngineMode, SharedEngineMode};
use khora_sdk::khora_core::EngineContext;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineBootstrap, EngineCore, GameWorld, InputEvent,
    PhaseProvider, Runtime, WindowConfig,
};

/// What a [`Staged`] app counts: its `setup` calls, and its agent's
/// initializations.
#[derive(Clone, Default)]
struct Counters {
    setups: Arc<AtomicUsize>,
    agent_inits: Arc<AtomicUsize>,
}

/// An app that counts its setups and registers one agent counting its own
/// initializations.
struct Staged {
    counters: Counters,
}

fn staged_mode() -> EngineMode {
    EngineMode::Custom("staged".to_owned())
}

/// The app's agent: counts its `on_initialize` calls.
struct CountingInit {
    inits: Arc<AtomicUsize>,
}

impl Agent for CountingInit {
    fn id(&self) -> AgentId {
        AgentId::Asset
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::Asset,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn on_initialize(&mut self, _ctx: &mut EngineContext<'_>) {
        self.inits.fetch_add(1, Ordering::SeqCst);
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl AgentProvider for Staged {
    fn register_agents(&self, dcc: &DccService, _runtime: &mut Runtime) {
        dcc.register_agent(
            Arc::new(Mutex::new(CountingInit {
                inits: Arc::clone(&self.counters.agent_inits),
            })),
            0.5,
        );
    }
}
impl PhaseProvider for Staged {}
impl EngineApp for Staged {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Staged {
            counters: Counters::default(),
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {
        self.counters.setups.fetch_add(1, Ordering::SeqCst);
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        staged_mode()
    }
}

/// A [`Staged`] app and the counters it writes to.
fn staged() -> (Staged, Counters) {
    let app = Staged::new();
    let counters = app.counters.clone();
    (app, counters)
}

/// Runs every stage of the staged bootstrap into a fresh engine.
fn bootstrap_in_stages(app: Staged) -> EngineCore<Staged> {
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    boot.setup();
    boot.register();
    for index in 0..boot.agent_count() {
        assert!(
            boot.initialize_agent(index),
            "index {index} names a registered agent"
        );
    }
    let mut core = EngineCore::<Staged>::new();
    boot.finish(&mut core);
    core
}

/// Ticks until the DCC runs in the app's mode, or panics after a few seconds.
fn tick_until_the_dcc_agrees(engine: &mut EngineCore<Staged>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.tick();
        if engine.dcc().map(|dcc| dcc.get_context().mode) == Some(staged_mode()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for the DCC to run in the app's mode");
}

#[test]
fn bootstrapping_in_stages_leaves_the_engine_as_one_bootstrap_does() {
    let (one_shot_app, one_shot) = staged();
    let mut reference = EngineCore::<Staged>::new();
    reference.bootstrap(one_shot_app, Runtime::new());

    let (staged_app, in_stages) = staged();
    let mut core = bootstrap_in_stages(staged_app);

    assert!(core.app_mut().is_some(), "the app is held by the engine");
    assert!(core.game_world_mut().is_some(), "the game world exists");
    let dcc = core.dcc().expect("the DCC is held by the engine");
    assert_eq!(
        dcc.agent_count(),
        reference.dcc().expect("the reference DCC").agent_count(),
        "the same agents are registered"
    );
    let mode = core
        .runtime()
        .resources
        .get::<SharedEngineMode>()
        .cloned()
        .expect("the runtime the engine holds carries the engine's own entries");
    assert_eq!(*mode.read().expect("the mode lock"), staged_mode());

    assert_eq!(one_shot.setups.load(Ordering::SeqCst), 1);
    assert_eq!(
        in_stages.setups.load(Ordering::SeqCst),
        1,
        "setup ran exactly once"
    );
    assert_eq!(one_shot.agent_inits.load(Ordering::SeqCst), 1);
    assert_eq!(
        in_stages.agent_inits.load(Ordering::SeqCst),
        1,
        "the app's agent was initialized exactly once"
    );
}

#[test]
fn an_engine_bootstrapped_in_stages_runs_and_its_dcc_starts() {
    let (app, _) = staged();
    let mut core = bootstrap_in_stages(app);
    tick_until_the_dcc_agrees(&mut core);
}

#[test]
fn each_stage_does_its_own_part_of_the_bootstrap() {
    let (app, counters) = staged();
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        0,
        "begin does not run the app's setup"
    );

    boot.setup();
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        1,
        "setup runs the app's setup"
    );

    boot.register();
    assert!(
        boot.agent_count() >= 2,
        "the app's agent and the built-in agents are registered, got {}",
        boot.agent_count()
    );
    assert_eq!(
        counters.agent_inits.load(Ordering::SeqCst),
        0,
        "registering initializes no agent"
    );

    for index in 0..boot.agent_count() {
        assert!(boot.initialize_agent(index));
    }
    assert_eq!(
        counters.agent_inits.load(Ordering::SeqCst),
        1,
        "the app's agent is initialized by its initialize_agent call"
    );

    let mut core = EngineCore::<Staged>::new();
    boot.finish(&mut core);
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        1,
        "finish does not run setup again"
    );
    assert_eq!(
        counters.agent_inits.load(Ordering::SeqCst),
        1,
        "finish initializes no agent again"
    );
}

#[test]
fn the_staged_bootstrap_registers_as_many_agents_as_bootstrap() {
    let (one_shot_app, _) = staged();
    let mut reference = EngineCore::<Staged>::new();
    reference.bootstrap(one_shot_app, Runtime::new());

    let (app, _) = staged();
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    boot.setup();
    boot.register();
    assert_eq!(
        boot.agent_count(),
        reference.dcc().expect("the reference DCC").agent_count()
    );
}

#[test]
fn initialize_agent_past_the_last_agent_returns_false_and_initializes_nothing() {
    let (app, counters) = staged();
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    boot.setup();
    boot.register();

    assert!(!boot.initialize_agent(boot.agent_count()));
    assert_eq!(
        counters.agent_inits.load(Ordering::SeqCst),
        0,
        "no agent was initialized"
    );
}
