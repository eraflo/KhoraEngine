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
//! writes one slot of its own, and its agent is named `ecs`, a name no
//! built-in agent takes. Its third lane runs only on a device with storage buffers, so
//! it is on the menu only when the engine boots over such a device.

use crate::protocol::{offer_of, report_of};
use khora_sdk::khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use std::sync::{Arc, Mutex};

use khora_agents::registered_agent::RegisteredAgent;
use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_sdk::khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::lane::family::{LaneFamily, Semantics};
use khora_sdk::khora_core::lane::guard::{InputRefusal, OutputRefusal, SinkKind};
use khora_sdk::khora_core::lane::io::{
    Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink,
};
use khora_sdk::khora_core::lane::level::{Impact, Level};
use khora_sdk::khora_core::lane::LaneError;
use khora_sdk::khora_core::lane::{Lane, LaneReport};
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::Activation;
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
    const NAME: &'static str = "shutdown probe";
    const AGENT: AgentId = AgentId::new("ecs");
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
const STORAGE_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "storage")];

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

/// The probe's behaviour for a device with storage buffers.
#[derive(Default)]
pub struct StorageProbeLane;

impl Lane for StorageProbeLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        STORAGE_LEVELS
    }

    fn supports(&self, caps: &DeviceCaps) -> bool {
        caps.storage_buffers
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
        note_shutdown("storage", gpu);
    }
}

khora_sdk::inventory::submit! {
    RegisteredAgent(khora_sdk::khora_core::register_agent! {
        family: ProbeFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [],
        fixed_step: None,
        lanes: [FirstProbeLane, SecondProbeLane, StorageProbeLane],
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

/// The agents `engine` registers in `phase`.
fn agents_in(engine: &EngineCore<Game>, phase: ExecutionPhase) -> Vec<AgentId> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    registry
        .iter()
        .filter_map(|agent| {
            let agent = agent.lock().ok()?;
            agent
                .execution_timing()
                .allowed_phases
                .contains(&phase)
                .then(|| agent.id())
        })
        .collect()
}

/// **One `LaneAgent` per collected registration.** The probe family's agent
/// is registered with the DCC beside the built-in agents, in the phase its
/// registration names.
#[test]
fn bootstrap_registers_one_lane_agent_per_collected_registration() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let output = agents_in(&engine, ExecutionPhase::OUTPUT);
    assert!(
        output.contains(&AgentId::new("ecs")),
        "the probe family's agent runs in OUTPUT: {output:?}"
    );
    assert!(
        output.contains(&AgentId::new("renderer")),
        "beside the agents not migrated yet: {output:?}"
    );
}

/// **The script family boots like any other.** Its registration is
/// collected with the others and its agent is a `LaneAgent`, running as
/// layers at its place — no hand-written script agent registered beside the
/// families. It runs in `TRANSFORM`, and it is the one agent named `script`.
#[test]
fn the_script_family_boots_as_a_lane_agent_from_its_registration() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let transform = agents_in(&engine, ExecutionPhase::TRANSFORM);
    let named_script = transform
        .iter()
        .filter(|id| **id == khora_sdk::khora_core::script::SCRIPT)
        .count();
    assert_eq!(
        named_script, 1,
        "one script agent, in TRANSFORM: {transform:?}"
    );
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    let agent = registry
        .get_by_id(khora_sdk::khora_core::script::SCRIPT)
        .expect("the script agent is registered");
    let layered = agent.lock().is_ok_and(|agent| agent.place().is_some());
    assert!(
        layered,
        "the script agent is the family's `LaneAgent`, running as layers; origin {:?}",
        registry.origin(khora_sdk::khora_core::script::SCRIPT)
    );
}

/// **A linked family is known to the engine's activation**, and starts on:
/// the probe's agent can be switched off by name, as any family registered
/// through `register_agent!` can.
#[test]
fn a_linked_family_is_known_to_the_engines_activation() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    let activation = engine
        .runtime()
        .resources
        .get::<Arc<Activation>>()
        .cloned()
        .expect("the bootstrap publishes the engine's activation");

    assert!(activation.is_active(AgentId::new("ecs")), "on at boot");
    assert_eq!(activation.deactivate(AgentId::new("ecs")), Ok(()));
    activation.settle();
    assert!(!activation.is_active(AgentId::new("ecs")));
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

// ─── The device's capabilities ──────────────────────────────────────────────

/// The strategies the probe family's agent offers in `engine`.
fn probe_menu(engine: &EngineCore<Game>) -> Vec<StrategyId> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    let probe = registry
        .iter()
        .filter(|agent| {
            agent.lock().is_ok_and(|agent| {
                agent
                    .execution_timing()
                    .allowed_phases
                    .contains(&ExecutionPhase::OUTPUT)
            })
        })
        .find(|agent| {
            agent
                .lock()
                .is_ok_and(|agent| agent.id() == AgentId::new("ecs"))
        })
        .expect("the probe family's agent is registered");
    let mut probe = probe.lock().expect("the probe agent's lock");
    probe
        .offer(&crate::protocol::request())
        .rows
        .iter()
        .map(|row| row.id)
        .collect()
}

/// **Boot reads the capabilities from the installed device.** Over a device
/// that has storage buffers, the probe's storage lane stays on its family's
/// menu; over a device that has none, or with no device at all, it leaves it.
#[test]
fn boot_reads_the_device_caps_from_the_installed_device() {
    let with_storage = Arc::new(QueueLog::with_caps(DeviceCaps {
        storage_buffers: true,
        ..DeviceCaps::default()
    }));
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(with_storage as Arc<dyn GraphicsDevice>);
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    let menu = probe_menu(&engine);
    assert!(
        menu.contains(&StrategyId::HighPerformance),
        "a device with storage buffers keeps the storage lane: {menu:?}"
    );

    let without_storage = Arc::new(QueueLog::default());
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(without_storage as Arc<dyn GraphicsDevice>);
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    let menu = probe_menu(&engine);
    assert!(
        !menu.contains(&StrategyId::HighPerformance),
        "a device without storage buffers drops the storage lane: {menu:?}"
    );

    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    let menu = probe_menu(&engine);
    assert!(
        !menu.contains(&StrategyId::HighPerformance),
        "without a device the default caps stand: {menu:?}"
    );
    assert!(
        menu.contains(&StrategyId::LowPower) && menu.contains(&StrategyId::Balanced),
        "the lanes that run anywhere stay: {menu:?}"
    );
}

// ─── A family whose agent id is taken ───────────────────────────────────────

/// A game agent registered under the probe family's id.
struct Squatter;

impl khora_sdk::khora_core::agent::Agent for Squatter {
    fn id(&self) -> AgentId {
        AgentId::new("ecs")
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(AgentId::new("ecs"), request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(AgentId::new("ecs"), StrategyId::Balanced)
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

// ─── Where each agent comes from ────────────────────────────────────────────

/// **Boot records where each agent comes from.** A family's agent comes from
/// the module its registration was written in — the probe's, this file — and
/// the engine's own agents from `khora-agents`, so a tool can list each under
/// its crate.
#[test]
fn boot_records_each_agents_origin() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    assert_eq!(
        registry.origin(AgentId::new("ecs")),
        Some(module_path!()),
        "the probe family was registered in this module"
    );
    for engine_agent in ["renderer", "script", "audio", "physics", "ui"] {
        let origin = registry.origin(AgentId::new(engine_agent));
        assert!(
            origin.is_some_and(|module| module.starts_with("khora_agents::")),
            "`{engine_agent}` comes from khora-agents: {origin:?}"
        );
    }
}

// ─── A game agent under an engine agent's name ──────────────────────────────

/// A game agent that names itself `script`, the name of the engine's own
/// script runner.
struct GameScript;

impl khora_sdk::khora_core::agent::Agent for GameScript {
    fn id(&self) -> AgentId {
        AgentId::new("script")
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(AgentId::new("script"), request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(AgentId::new("script"), StrategyId::Balanced)
    }
    fn execute(&mut self, _context: &mut khora_sdk::khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A game registering its own agent under the name `script`.
struct GameNamingItsAgentScript;

impl AgentProvider for GameNamingItsAgentScript {
    fn register_agents(&self, dcc: &DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(GameScript)), 1.0);
    }
}
impl PhaseProvider for GameNamingItsAgentScript {}
impl EngineApp for GameNamingItsAgentScript {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        GameNamingItsAgentScript
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **The engine's own agents never boot without themselves.** A name is now
/// free text, so a game naming its agent `script` (or `renderer`) takes the
/// name before the engine registers its own: the registry keeps the first,
/// and the engine's script runner would be silently left out — no script
/// would ever run. Boot refuses that, as it refuses a family whose name is
/// taken.
#[test]
fn a_game_agent_under_an_engine_agents_name_refuses_to_boot() {
    let booted = std::panic::catch_unwind(|| {
        let mut engine = EngineCore::<GameNamingItsAgentScript>::new();
        engine.bootstrap(GameNamingItsAgentScript, Runtime::new());
        let dcc = engine.dcc().expect("bootstrapped");
        let registry = dcc.agent_registry().lock().expect("the registry lock");
        let holder = registry
            .get_by_id(AgentId::new("script"))
            .expect("an agent named `script` is registered");
        let is_the_games = holder
            .lock()
            .is_ok_and(|agent| agent.as_any().is::<GameScript>());
        (is_the_games, registry.origin(AgentId::new("script")))
    });

    assert!(
        booted.is_err(),
        "the engine booted with its script runner refused by the registry; \
         `script` is the game's agent ({:?}) yet its origin reads {:?}",
        booted.as_ref().map(|(is_the_games, _)| *is_the_games),
        booted.as_ref().map(|(_, origin)| *origin),
    );
}
