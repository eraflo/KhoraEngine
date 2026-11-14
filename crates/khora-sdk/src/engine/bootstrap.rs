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

//! Assembling the engine: services, agents, lanes and the scheduler, once,
//! before the first frame.

use khora_agents::lane_agent::LaneAgent;
use khora_control::{DccConfig, DccService};
use khora_core::agent::ExecutionPhase;
use khora_core::event::Channel;
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use khora_telemetry::TelemetryService;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::phase_order::phase_order;
use super::scheduler_boot::{build_scheduler, refuse_stranded_agents, register_lane_agents};
use super::EngineCore;
use crate::traits::EngineApp;
use crate::{GameWorld, InputEvent};

/// The DCC's telemetry receiver, handed to [`DccService::start`].
type EventReceiver = crossbeam_channel::Receiver<TelemetryEvent>;

/// [`EngineCore::bootstrap`] taken apart into its stages.
///
/// Called in order — [`begin`](Self::begin), [`setup`](Self::setup),
/// [`register`](Self::register), then [`finish`](Self::finish) — it leaves the
/// engine as one `bootstrap` call does.
pub struct EngineBootstrap<A: EngineApp> {
    app: A,
    /// The runtime while it is still filled; `None` once shared.
    runtime: Option<Runtime>,
    /// The runtime once shared with the agents.
    shared: Option<Arc<Runtime>>,
    game_world: GameWorld,
    dcc: DccService,
    dcc_rx: EventReceiver,
    telemetry: TelemetryService,
    input_events: Channel<InputEvent>,
    /// One agent per collected family registration, kept typed so the engine
    /// can release their lanes when it stops.
    lane_agents: Vec<Arc<Mutex<LaneAgent>>>,
    /// The order the scheduler runs the phases in, the lane agents checked
    /// against it.
    phase_order: Vec<ExecutionPhase>,
}

impl<A: EngineApp> EngineBootstrap<A> {
    /// Starts the bootstrap of `app` over `runtime`: the DCC, telemetry and the
    /// engine's own runtime entries, everything `app.setup` may read.
    pub fn begin(app: A, runtime: Runtime) -> Self {
        Self::begin_with(app, runtime, khora_core::platform::input_channel())
    }

    fn begin_with(app: A, mut runtime: Runtime, input_events: Channel<InputEvent>) -> Self {
        let (dcc, dcc_rx, telemetry, game_world) = populate(&mut runtime, &input_events);
        identify(&mut runtime, A::game_identity());
        super::refusals::install(&mut runtime, &A::game_identity(), A::keeps_user_files());
        let phase_order = phase_order(&app.custom_phases(), &app.removed_phases());
        Self {
            app,
            runtime: Some(runtime),
            shared: None,
            game_world,
            dcc,
            dcc_rx,
            telemetry,
            input_events,
            lane_agents: Vec::new(),
            phase_order,
        }
    }

    /// Runs the application's [`EngineApp::setup`] over the game world.
    ///
    /// Does nothing once [`register`](Self::register) has run.
    pub fn setup(&mut self) {
        if let Some(runtime) = self.runtime.as_ref() {
            // The runtime is now fully populated, so the app can read any
            // engine resource here. Mutation goes through `provide`.
            self.app.setup(&mut self.game_world, runtime);
        }
    }

    /// Lets the application install what it needs in the runtime, then
    /// registers the agent of every family linked into the binary.
    ///
    /// Does nothing the second time.
    ///
    /// # Panics
    ///
    /// Panics when the engine cannot boot its families: their registrations
    /// are refused, another agent already took a family's id, an agent runs in
    /// a phase the app removed, or a `DataSystem` or `Flow` serves no family.
    pub fn register(&mut self) {
        let Some(mut runtime) = self.runtime.take() else {
            return;
        };
        self.app.provide(&mut runtime);
        let shared = Arc::new(runtime);
        self.lane_agents = register_lane_agents(&self.dcc, &shared, &self.phase_order);
        refuse_stranded_agents(&self.dcc, &self.phase_order);
        super::families::admit_families(&self.dcc, &shared);
        self.shared = Some(shared);
    }

    /// Starts the DCC, builds the scheduler and hands everything to `core`.
    ///
    /// # Panics
    ///
    /// Panics when the registered render pass slots form a cycle.
    pub fn finish(self, core: &mut EngineCore<A>) {
        let Self {
            app,
            runtime,
            shared,
            game_world,
            mut dcc,
            dcc_rx,
            telemetry,
            input_events,
            lane_agents,
            phase_order,
        } = self;
        let runtime_arc = match (shared, runtime) {
            (Some(shared), _) => shared,
            // `register` never ran: share the runtime as it is.
            (None, Some(runtime)) => Arc::new(runtime),
            (None, None) => Arc::new(Runtime::new()),
        };
        // A journal named at boot records every decision, from the first.
        core.journal = super::decision_journal::record_path(std::env::var_os(
            super::decision_journal::RECORD_VAR,
        ));
        if let Some(path) = &core.journal {
            log::info!("Decision journal: recording to {}", path.display());
            dcc.start_decision_recording();
        }
        // What the DCC learnt of this machine on an earlier run seeds it.
        core.profiles = super::profiles::Profiles::begin(&runtime_arc, &dcc, A::keeps_user_files());
        // Started once the agents are registered, so GORNA never asks one that
        // is not ready.
        dcc.start(dcc_rx);
        let scheduler = build_scheduler(&phase_order, &mut dcc);

        core.app = Some(app);
        core.game_world = Some(game_world);
        core.telemetry = Some(telemetry);
        core.dcc = Some(dcc);
        core.scheduler = Some(scheduler);
        core.runtime = runtime_arc;
        core.input_events = input_events;
        core.lane_agents = lane_agents;
        // The pass slots are registered at link time, so a cycle among them
        // is a build defect no frame could recover from.
        core.passes = match khora_data::render::registered_pass_order() {
            Ok(order) => order,
            Err(refusal) => panic!("the engine cannot order its render passes: {refusal}"),
        };
    }
}

impl<A: EngineApp> EngineCore<A> {
    /// Boots the engine over `runtime`, as the driver's bootstrap closure filled it.
    ///
    /// Runs `app.setup`, then `app.provide`, and registers every family's
    /// agent: the [`EngineBootstrap`] stages, in order.
    /// See the book: concepts/the-frame.md, "Startup, once".
    ///
    /// # Panics
    ///
    /// Panics when the engine cannot boot, as
    /// [`EngineBootstrap::register`] and [`EngineBootstrap::finish`] say.
    pub fn bootstrap(&mut self, app: A, runtime: Runtime) {
        let mut stages = EngineBootstrap::begin_with(app, runtime, self.input_events.clone());
        stages.setup();
        stages.register();
        stages.finish(self);
    }
}

/// Puts in `runtime`'s resources the machine the engine runs on and the `game`
/// it runs, under which the engine files what it learns.
///
/// The machine names the graphics adapter when a device is installed, the CPU
/// and the OS.
fn identify(runtime: &mut Runtime, game: khora_core::app::GameIdentity) {
    let gpu = (runtime
        .backends
        .get::<Arc<dyn khora_core::renderer::GraphicsDevice>>())
    .and_then(|device| device.adapter_identity());
    let machine = khora_infra::platform::machine::read_machine(gpu);
    log::info!(
        "Machine {} ({}, {} cores, {}); game {} {}",
        machine.key(),
        machine.cpu,
        machine.cores,
        machine.os,
        game.name,
        game.version
    );
    runtime.resources.insert(Arc::new(machine));
    runtime.resources.insert(Arc::new(game));
}

/// Creates the DCC, telemetry and the game world, and fills `runtime` with
/// every engine entry `app.setup` may read.
fn populate(
    runtime: &mut Runtime,
    input_events: &Channel<InputEvent>,
) -> (DccService, EventReceiver, TelemetryService, GameWorld) {
    let (dcc, dcc_rx, telemetry) = start_dcc_and_telemetry(runtime);
    super::defaults::install_defaults(runtime);
    super::runtime_entries::install_engine_resources(runtime, input_events);
    super::runtime_entries::install_scripting_and_loading(runtime);
    let mut game_world = create_game_world(runtime);
    land_boot_work(runtime, &mut game_world);
    (dcc, dcc_rx, telemetry, game_world)
}

/// Creates the DCC and telemetry, registers their monitors and the host's, and
/// exposes their observable handles in `runtime`'s resources.
fn start_dcc_and_telemetry(runtime: &mut Runtime) -> (DccService, EventReceiver, TelemetryService) {
    let (dcc, dcc_rx) = DccService::new(DccConfig::default());
    let telemetry =
        TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());

    // The tracking allocator's stats reach the DCC through telemetry, as
    // memory pressure and allocation churn.
    telemetry.monitor_registry().register(std::sync::Arc::new(
        khora_infra::telemetry::memory_monitor::MemoryMonitor::new("System_RAM".to_string()),
    ));
    // The hardware's health (heat, battery, CPU load), read by the DCC's own
    // thread: the sensors are slow to read, and the telemetry service runs in
    // the frame.
    dcc.set_hardware_monitor(std::sync::Arc::new(
        khora_infra::platform::sysinfo_impl::SysinfoMonitor::new(),
    ));
    // The monitors the host built — the renderer's video memory — handed
    // over in the runtime, since the telemetry service did not exist yet.
    if let Some(khora_core::telemetry::monitoring::HostMonitors(monitors)) =
        runtime
            .resources
            .remove::<khora_core::telemetry::monitoring::HostMonitors>()
    {
        for monitor in monitors {
            telemetry.monitor_registry().register(monitor);
        }
    }

    // Apps such as the editor read live engine state through these handles:
    // cheap clones of shared structures, safe to expose before `app.setup`.
    runtime
        .resources
        .insert(telemetry.monitor_registry().clone());
    runtime.resources.insert(dcc.agent_registry().clone());
    // Live DCC context: shared `Arc<RwLock<Context>>` updated by the
    // DCC cold thread, read by observers each frame.
    runtime.resources.insert(dcc.context_handle());
    // The DCC's last decision: what it lent each agent, and why.
    runtime.resources.insert(dcc.decision_watch());

    (dcc, dcc_rx, telemetry)
}

/// Creates the game world, given the scene loads hub and the shipped
/// prefabs where `runtime` holds them.
fn create_game_world(runtime: &mut Runtime) -> GameWorld {
    // The prefabs are read from the app's asset service, so a game saved over
    // a level holding prefab instances compares against the level expanded.
    let mut game_world = GameWorld::new();
    if let Some(loads) = runtime
        .resources
        .get::<Arc<khora_io::scene_loads::SceneLoadHub>>()
    {
        game_world.set_scene_loads(Arc::clone(loads));
    }
    if let Some(assets) = runtime
        .services
        .get::<Arc<Mutex<khora_io::asset::AssetService>>>()
        .cloned()
    {
        game_world.set_prefabs(Arc::new(khora_io::serialization::AssetPrefabs::new(
            assets.clone(),
        )));
        crate::prefabs::install(runtime, assets);
    }
    game_world
}

/// Compiles the scripts filed before the boot, lands what the host staged,
/// and registers into `game_world` what the mounted scripts declare.
fn land_boot_work(runtime: &mut Runtime, game_world: &mut GameWorld) {
    // Nothing runs yet, so the mounts filed before the boot compile now, what
    // the host staged lands, and their declarations register before `setup`;
    // the first frame boundary finds none pending.
    // See the book: concepts/the-frame.md, "Startup, once".
    khora_io::script::compile_hub::compile_filed_now(runtime);
    if let Some(reserve) = runtime.resources.get::<khora_core::lane::Reserve>() {
        reserve.land_from_host();
    }
    khora_io::script::declarations::register_pending(game_world.inner_world_mut(), runtime);
}
