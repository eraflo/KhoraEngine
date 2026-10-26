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

//! The engine boots one `LaneAgent` per collected registration, in the modes
//! the registration names, and releases every lane when it stops — while the
//! device is still there.
//!
//! The probe family below is submitted to the collection of this whole test
//! binary, so every engine booted here carries it. It reads nothing and
//! writes one slot of its own, and its agent id (`Ecs`) is one no built-in
//! agent takes.

use std::sync::{Arc, Mutex};

use khora_agents::registered_agent::RegisteredAgent;
use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::{AgentImportance, EngineMode, ExecutionPhase};
use khora_sdk::khora_core::gpu::GpuSetup;
use khora_sdk::khora_core::lane::contract::{Lane, LaneReport};
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::lane::family::{LaneFamily, Semantics};
use khora_sdk::khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind};
use khora_sdk::khora_core::lane::io::{
    Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink,
};
use khora_sdk::khora_core::lane::level::{Impact, Level};
use khora_sdk::khora_core::lane::{LaneError, LaneKind};
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

use crate::render_doubles::QueueLog;

// ─── The probe family ───────────────────────────────────────────────────────

/// The probe's one deck slot.
#[derive(Debug, Default)]
pub struct ProbeSlot;

/// The probe family's outputs.
pub struct ProbeOutputs {
    slot: ProbeSlot,
}

impl<'f> LaneOutputs<'f> for ProbeOutputs {
    fn declare(d: &mut Declaration) {
        d.write::<ProbeSlot>("slot", SinkKind::Deck);
    }

    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(Self { slot: ProbeSlot })
    }

    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        sink.put("slot", self.slot)
    }
}

/// The probe lanes read nothing.
pub struct NoInputs;

impl<'f> LaneInputs<'f> for NoInputs {
    fn declare(_d: &mut Declaration) {}

    fn fetch(_src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(NoInputs)
    }
}

/// A family no built-in agent negotiates for.
pub struct ProbeFamily;

impl LaneFamily for ProbeFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "shutdown probe";
    const AGENT: AgentId = AgentId::Ecs;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const fn level(id: StrategyId, name: &'static str) -> Level {
    Level {
        id,
        name,
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

const FIRST_LEVELS: &[Level] = &[level(StrategyId::LowPower, "first")];
const SECOND_LEVELS: &[Level] = &[level(StrategyId::Balanced, "second")];

/// Every probe lane shutdown so far, in every engine of this binary: the
/// lane, and the address of the device it was handed (`None` when headless).
static SHUTDOWNS: Mutex<Vec<(&'static str, Option<usize>)>> = Mutex::new(Vec::new());

/// The address `device` is known by in [`SHUTDOWNS`].
fn address(device: &dyn GraphicsDevice) -> usize {
    device as *const dyn GraphicsDevice as *const () as usize
}

fn note_shutdown(lane: &'static str, gpu: &GpuSetup<'_>) {
    SHUTDOWNS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((lane, gpu.device().map(address)));
}

/// The probe's first behaviour.
#[derive(Default)]
pub struct FirstProbeLane;

impl Lane for FirstProbeLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        FIRST_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.slot = ProbeSlot;
        Ok(LaneReport::default())
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        note_shutdown("first", gpu);
    }
}

/// The probe's second behaviour.
#[derive(Default)]
pub struct SecondProbeLane;

impl Lane for SecondProbeLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        SECOND_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.slot = ProbeSlot;
        Ok(LaneReport::default())
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        note_shutdown("second", gpu);
    }
}

khora_sdk::inventory::submit! {
    RegisteredAgent(khora_sdk::khora_core::register_agent! {
        family: ProbeFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [],
        modes: [EngineMode::Playing],
        fixed_step: None,
        lanes: [FirstProbeLane, SecondProbeLane],
    })
}

// ─── The app ────────────────────────────────────────────────────────────────

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

/// The agents `engine` runs in `phase` and `mode`.
fn agents_in(engine: &EngineCore<Game>, phase: ExecutionPhase, mode: &EngineMode) -> Vec<AgentId> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    registry
        .collect_for_phase(phase, mode)
        .into_iter()
        .filter_map(|(agent, ..)| agent.lock().ok().map(|agent| agent.id()))
        .collect()
}

/// **One `LaneAgent` per collected registration.** The probe family's agent
/// is registered with the DCC beside the built-in agents, in the phase its
/// registration names.
#[test]
fn bootstrap_registers_one_lane_agent_per_collected_registration() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let output = agents_in(&engine, ExecutionPhase::OUTPUT, &EngineMode::Playing);
    assert!(
        output.contains(&AgentId::Ecs),
        "the probe family's agent runs in OUTPUT: {output:?}"
    );
    assert!(
        output.contains(&AgentId::Renderer),
        "beside the agents not migrated yet: {output:?}"
    );
}

/// A registration's modes are the modes its agent runs in: the probe runs
/// while the game is played, not in a tool's mode.
#[test]
fn a_lane_agent_runs_only_in_its_registrations_modes() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let tool = EngineMode::Custom("tool".to_owned());
    assert!(
        agents_in(&engine, ExecutionPhase::OUTPUT, &EngineMode::Playing).contains(&AgentId::Ecs)
    );
    assert!(
        !agents_in(&engine, ExecutionPhase::OUTPUT, &tool).contains(&AgentId::Ecs),
        "registered for Playing only"
    );
}

/// **Stopping the engine releases every lane, before the device goes.** Each
/// of the probe's two lanes — the one it ran and the one it never did — is
/// shut down exactly once, and handed the engine's own graphics device.
#[test]
fn engine_shutdown_releases_every_lane() {
    let device = Arc::new(QueueLog::default());
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
    let ours = address(device.as_ref());
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);

    engine.shutdown();

    let shutdowns = SHUTDOWNS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let on_our_device: Vec<&'static str> = shutdowns
        .iter()
        .filter(|(_, device)| *device == Some(ours))
        .map(|(lane, _)| *lane)
        .collect();
    let mut sorted = on_our_device.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["first", "second"],
        "each lane once, with this engine's device: {shutdowns:?}"
    );
}

// ─── A family whose agent id is taken ───────────────────────────────────────

/// A game agent registered under the probe family's id.
struct Squatter;

impl khora_sdk::khora_core::agent::Agent for Squatter {
    fn id(&self) -> AgentId {
        AgentId::Ecs
    }
    fn negotiate(
        &mut self,
        _request: khora_sdk::khora_core::agent::gorna::NegotiationRequest,
    ) -> khora_sdk::khora_core::agent::gorna::NegotiationResponse {
        khora_sdk::khora_core::agent::gorna::NegotiationResponse {
            strategies: Vec::new(),
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _budget: khora_sdk::khora_core::agent::gorna::ResourceBudget) {}
    fn report_status(&self) -> khora_sdk::AgentStatus {
        khora_sdk::AgentStatus {
            agent_id: AgentId::Ecs,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, _context: &mut khora_sdk::khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A game registering its own agent under the probe family's id.
struct SquattingGame;

impl AgentProvider for SquattingGame {
    fn register_agents(&self, dcc: &DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(Squatter)), 1.0);
    }
}
impl PhaseProvider for SquattingGame {}
impl EngineApp for SquattingGame {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        SquattingGame
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **A checked family never boots without its agent.** The registry keeps one
/// agent per id, so a family whose id another agent already holds would
/// never run. Boot refuses that, as it refuses two families under one id,
/// rather than starting with the family silently left out.
#[test]
fn a_family_whose_agent_id_is_taken_refuses_to_boot() {
    let booted = std::panic::catch_unwind(|| {
        let mut engine = EngineCore::<SquattingGame>::new();
        engine.bootstrap(SquattingGame, Runtime::new());
    });

    assert!(
        booted.is_err(),
        "the engine booted with the probe family's agent refused by the registry"
    );
}
