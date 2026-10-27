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

//! **The physics step is a game setting**, never a GORNA lever. The game's
//! `physics.step` reaches the engine as the `PhysicsSettings` runtime entry the
//! application installs before the bootstrap; the engine boots its physics
//! agent at that step, the scheduler cuts gameplay time by it, the lane steps
//! the provider by it, and no budget changes it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_sdk::khora_core::agent::gorna::{
    AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
};
use khora_sdk::khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::lane::{LaneBus, OutputDeck};
use khora_sdk::khora_core::math::{Quat, Ray, Vec3};
use khora_sdk::khora_core::physics::{
    CharacterControllerOptions, ColliderDesc, ColliderHandle, CollisionEvent, PhysicsProvider,
    PhysicsSettings, RaycastHit, RigidBodyDesc, RigidBodyHandle,
};
use khora_sdk::khora_core::{EngineContext, WorldAccess};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// 1/128 s: exact in binary, and none of the steps the former agent picked.
const A_128TH: Duration = Duration::from_nanos(7_812_500);

/// The provider as the engine registers it.
type SharedProvider = Arc<Mutex<Box<dyn PhysicsProvider>>>;

// ─── A provider recording its steps ─────────────────────────────────────────

/// A provider that simulates nothing and records the time each step advanced.
struct RecordingProvider {
    steps: Arc<Mutex<Vec<f32>>>,
}

/// A recording provider, shared as the engine registers it, and its steps.
fn recording_provider() -> (SharedProvider, Arc<Mutex<Vec<f32>>>) {
    let steps = Arc::new(Mutex::new(Vec::new()));
    let provider: Box<dyn PhysicsProvider> = Box::new(RecordingProvider {
        steps: Arc::clone(&steps),
    });
    (Arc::new(Mutex::new(provider)), steps)
}

impl PhysicsProvider for RecordingProvider {
    fn step(&mut self, dt: f32) {
        self.steps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(dt);
    }
    fn set_gravity(&mut self, _gravity: Vec3) {}
    fn add_body(&mut self, _desc: RigidBodyDesc) -> RigidBodyHandle {
        RigidBodyHandle(0)
    }
    fn remove_body(&mut self, _handle: RigidBodyHandle) {}
    fn add_collider(&mut self, _desc: ColliderDesc) -> ColliderHandle {
        ColliderHandle(0)
    }
    fn remove_collider(&mut self, _handle: ColliderHandle) {}
    fn get_body_transform(&self, _handle: RigidBodyHandle) -> (Vec3, Quat) {
        (Vec3::ZERO, Quat::IDENTITY)
    }
    fn get_body_velocity(&self, _handle: RigidBodyHandle) -> (Vec3, Vec3) {
        (Vec3::ZERO, Vec3::ZERO)
    }
    fn set_body_transform(&mut self, _handle: RigidBodyHandle, _pos: Vec3, _rot: Quat) {}
    fn get_all_bodies(&self) -> Vec<RigidBodyHandle> {
        Vec::new()
    }
    fn get_all_colliders(&self) -> Vec<ColliderHandle> {
        Vec::new()
    }
    fn update_body_properties(&mut self, _handle: RigidBodyHandle, _desc: RigidBodyDesc) {}
    fn update_collider_properties(&mut self, _handle: ColliderHandle, _desc: ColliderDesc) {}
    fn get_debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>) {
        (Vec::new(), Vec::new())
    }
    fn cast_ray(&self, _ray: &Ray, _max_toi: f32, _solid: bool) -> Option<RaycastHit> {
        None
    }
    fn take_collision_events(&self) -> Vec<CollisionEvent> {
        Vec::new()
    }
    fn entity_of(&self, _collider: ColliderHandle) -> Option<EntityId> {
        None
    }
    fn move_character(
        &self,
        _collider: ColliderHandle,
        desired_translation: Vec3,
        _options: &CharacterControllerOptions,
    ) -> (Vec3, bool) {
        (desired_translation, false)
    }
}

// ─── The game ───────────────────────────────────────────────────────────────

/// A game registering no agent of its own.
struct Game;

impl AgentProvider for Game {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
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

/// The engine booted over `runtime`.
fn booted(runtime: Runtime) -> EngineCore<Game> {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    engine
}

/// A runtime holding the game's `provider`, and its `settings` when it has
/// any.
fn game_runtime(provider: &SharedProvider, settings: Option<PhysicsSettings>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(provider));
    if let Some(settings) = settings {
        runtime.resources.insert(settings);
    }
    runtime
}

/// The physics agent the engine registered with its DCC.
fn physics_agent_of(engine: &EngineCore<Game>) -> Arc<Mutex<dyn Agent>> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let found = registry
        .iter()
        .find(|agent| {
            agent
                .lock()
                .map(|agent| agent.id() == AgentId::Physics)
                .unwrap_or(false)
        })
        .cloned();
    found.expect("the engine registered a physics agent")
}

/// The step the agent declares to the scheduler.
fn declared_step(agent: &Arc<Mutex<dyn Agent>>) -> Option<Duration> {
    agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .execution_timing()
        .fixed_timestep
}

fn unconstrained() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

/// Every strategy GORNA could name to the agent: the ones it offers, and the
/// three the former agent turned into steps of 1/30, 1/60 and 1/120 s.
fn every_strategy(agent: &Arc<Mutex<dyn Agent>>) -> Vec<StrategyId> {
    let offered: Vec<StrategyId> = agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .negotiate(unconstrained())
        .strategies
        .iter()
        .map(|option| option.id)
        .collect();
    offered
        .into_iter()
        .chain([
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance,
            StrategyId::Custom(3),
        ])
        .collect()
}

fn apply(agent: &Arc<Mutex<dyn Agent>>, strategy_id: StrategyId) {
    agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .apply_budget(ResourceBudget {
            strategy_id,
            time_limit: Duration::from_millis(2),
            memory_limit: None,
            extra_params: HashMap::new(),
        });
}

/// One run of `agent` over `engine`'s runtime, as the scheduler runs one
/// sub-step of a world-free agent.
fn run_once(engine: &EngineCore<Game>, agent: &Arc<Mutex<dyn Agent>>) -> OutputDeck {
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let mut agent = agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(engine.runtime()),
        &bus,
        &mut deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
    drop(context);
    deck
}

// ─── The setting ────────────────────────────────────────────────────────────

/// **The physics step comes from the game setting.** A game whose
/// `physics.step` is 1/128 s boots a physics agent declaring a step of
/// 1/128 s — and whatever level GORNA applies, the step stays.
#[test]
fn physics_step_comes_from_the_game_setting() {
    let (provider, _steps) = recording_provider();
    let engine = booted(game_runtime(
        &provider,
        Some(PhysicsSettings { step: A_128TH }),
    ));
    let agent = physics_agent_of(&engine);

    assert_eq!(
        declared_step(&agent),
        Some(A_128TH),
        "the agent runs at the game's physics.step"
    );
    for strategy in every_strategy(&agent) {
        apply(&agent, strategy);
        assert_eq!(
            declared_step(&agent),
            Some(A_128TH),
            "GORNA's {strategy:?} left the game's step alone"
        );
    }
}

/// The lane steps the game's own provider by the game's step: the time the
/// scheduler cuts by is the time the simulation advances.
#[test]
fn the_physics_agent_steps_the_games_provider_by_the_game_setting() {
    let (provider, steps) = recording_provider();
    let engine = booted(game_runtime(
        &provider,
        Some(PhysicsSettings { step: A_128TH }),
    ));
    let agent = physics_agent_of(&engine);

    run_once(&engine, &agent);

    let recorded = steps
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(
        recorded,
        vec![A_128TH.as_secs_f32()],
        "one step of the game's size, on the game's provider"
    );
}

/// A game that sets no physics step runs at the default setting's step.
#[test]
fn a_game_without_a_physics_setting_runs_at_the_default_step() {
    let (provider, _steps) = recording_provider();
    let engine = booted(game_runtime(&provider, None));
    let agent = physics_agent_of(&engine);

    let default = PhysicsSettings::default().step;
    assert!(
        !default.is_zero(),
        "the default setting has a step: {default:?}"
    );
    assert_eq!(declared_step(&agent), Some(default));
}

/// **A game that brings no provider still boots.** The physics family locks a
/// provider, and boot refuses a family whose entry the runtime lacks; so the
/// engine installs one of its own when the application installs none, as it
/// does a mix bus for audio.
#[test]
fn a_game_without_a_physics_provider_boots_with_the_engines_own() {
    let engine = booted(Runtime::new());

    assert!(
        engine.runtime().look_up::<SharedProvider>().is_some(),
        "the engine installed a physics provider"
    );
    let agent = physics_agent_of(&engine);
    assert_eq!(declared_step(&agent), Some(PhysicsSettings::default().step));
}

/// **The engine's frame steps the provider by the game's step.** The first
/// frame advances exactly one gameplay tick (1/60 s); at a `physics.step` of
/// 1/128 s that tick holds two whole steps, so the frame steps the game's
/// provider twice, each by 1/128 s — the setting reaches the provider through
/// the scheduler's cut, not only through one hand-made run.
#[test]
fn an_engine_frame_steps_the_provider_by_the_game_setting_as_often_as_the_tick_holds() {
    let (provider, steps) = recording_provider();
    let mut engine = booted(game_runtime(
        &provider,
        Some(PhysicsSettings { step: A_128TH }),
    ));

    engine.tick();

    let recorded = steps
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(
        recorded,
        vec![A_128TH.as_secs_f32(); 2],
        "one tick of 1/60 s holds two steps of 1/128 s"
    );
}
