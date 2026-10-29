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

use khora_agents::lane_agent::{set_fixed_step, LaneAgent};
use khora_agents::registered_agent::{lane_agents_in, registered_agents};
use khora_control::{DccConfig, DccService, EngineMode};
use khora_core::agent::{Agent, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::Runtime;
use khora_data::render::{FrameGraph, SharedFrameGraph};
use khora_telemetry::TelemetryService;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::event::Channel;
use khora_core::telemetry::TelemetryEvent;

use super::EngineCore;
use crate::InputEvent;

/// The DCC's telemetry receiver, handed to [`DccService::start`].
type EventReceiver = crossbeam_channel::Receiver<TelemetryEvent>;
use crate::traits::EngineApp;
use crate::GameWorld;

/// [`EngineCore::bootstrap`] taken apart into the stages a tool can spread
/// over several frames while a loading screen plays.
///
/// Called in order — [`begin`](Self::begin), [`setup`](Self::setup),
/// [`register`](Self::register), [`initialize_agent`](Self::initialize_agent)
/// for every index below [`agent_count`](Self::agent_count), then
/// [`finish`](Self::finish) — it leaves the engine as one `bootstrap` call
/// does.
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
        let (dcc, dcc_rx, telemetry, game_world) = populate(&app, &mut runtime, &input_events);
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
    pub fn setup(&mut self) {
        if let Some(runtime) = self.runtime.as_ref() {
            // The runtime is now fully populated, so the app can read any
            // engine resource here. Mutation goes through `register_agents`.
            self.app.setup(&mut self.game_world, runtime);
        }
    }

    /// Registers the application's agents, then the engine's built-in ones.
    pub fn register(&mut self) {
        let Some(mut runtime) = self.runtime.take() else {
            return;
        };
        self.app.register_agents(&self.dcc, &mut runtime);
        let shared = Arc::new(runtime);
        register_builtin_agents(&self.dcc);
        self.lane_agents = register_lane_agents(&self.dcc, &shared, &self.phase_order);
        refuse_stranded_agents(&self.dcc, &self.phase_order);
        self.shared = Some(shared);
    }

    /// The number of registered agents — the indices
    /// [`initialize_agent`](Self::initialize_agent) accepts.
    pub fn agent_count(&self) -> usize {
        self.dcc.agent_count()
    }

    /// The application and, while it is there, the runtime — what a tool's
    /// loading frames are painted with while the bootstrap is under way.
    pub fn app_and_runtime(&mut self) -> (&mut A, Option<&Runtime>) {
        let runtime = match (&self.runtime, &self.shared) {
            (Some(runtime), _) => Some(runtime),
            (None, Some(shared)) => Some(&**shared),
            (None, None) => None,
        };
        (&mut self.app, runtime)
    }

    /// Initializes the registered agent at `index`, in the order the DCC
    /// initializes them all; `false`, and nothing initialized, past the last.
    pub fn initialize_agent(&mut self, index: usize) -> bool {
        // Agents initialize with the full runtime so `on_initialize` finds the
        // graphics device, the render system, the asset store… through the
        // typed runtime containers.
        let Some(runtime) = self.shared.as_ref() else {
            return false;
        };
        let init_bus = khora_core::lane::LaneBus::new();
        let mut init_deck = khora_core::lane::OutputDeck::new();
        let mut init_ctx = khora_core::EngineContext::for_initialisation(
            Arc::clone(runtime),
            &init_bus,
            &mut init_deck,
        );
        self.dcc.initialize_agent(index, &mut init_ctx)
    }

    /// Starts the DCC, builds the scheduler and hands everything to `core`.
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
        // Start the DCC background thread AFTER agents are initialized so
        // GORNA does not run health-checks before agents are ready.
        dcc.start(dcc_rx);
        let scheduler = build_scheduler(&phase_order, &mut dcc, core);

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
    /// Bootstraps the engine: creates DCC, telemetry, scheduler,
    /// registers agents, calls `app.setup()`, and initializes agents.
    ///
    /// This method takes ownership of the `runtime` populated
    /// by the windowing driver's bootstrap closure.  It wraps the runtime
    /// in an `Arc` once all built-in entries have been inserted.
    pub fn bootstrap(&mut self, app: A, runtime: Runtime) {
        let mut stages = EngineBootstrap::begin_with(app, runtime, self.input_events.clone());
        stages.setup();
        stages.register();
        let mut index = 0;
        while stages.initialize_agent(index) {
            index += 1;
        }
        stages.finish(self);
    }
}

/// Creates the DCC, telemetry and the game world, and fills `runtime` with
/// every engine entry `app.setup` may read.
fn populate<A: EngineApp>(
    app: &A,
    runtime: &mut Runtime,
    input_events: &Channel<InputEvent>,
) -> (DccService, EventReceiver, TelemetryService, GameWorld) {
    // Create DCC + telemetry
    let (dcc, dcc_rx) = DccService::new(DccConfig::default());
    let telemetry =
        TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());

    // Register the system-RAM monitor so the tracking allocator's live
    // stats flow through telemetry into the DCC, where they drive memory
    // pressure + allocation-churn signals (it is no longer write-only).
    telemetry.monitor_registry().register(std::sync::Arc::new(
        khora_infra::telemetry::memory_monitor::MemoryMonitor::new("System_RAM".to_string()),
    ));

    // ── Expose observable handles via Resources ─────────────────────
    // Apps (e.g. the editor) read live engine state (monitors, agent
    // list, DCC context) through these handles. They're cheap clones of
    // internal Arc-shared structures, so doing so before `app.setup` is
    // safe.
    runtime
        .resources
        .insert(telemetry.monitor_registry().clone());
    runtime.resources.insert(dcc.agent_registry().clone());
    // Live DCC context: shared `Arc<RwLock<Context>>` updated by the
    // DCC cold thread, read by observers each frame.
    runtime.resources.insert(dcc.context_handle());

    // ── Overlay channels (gizmo + grid) ──────────────────────────────────
    // Shared resources the host application (editor, debug tooling)
    // writes into each frame; the overlay families' lanes read them in
    // the OUTPUT phase. The engine only provides the slots — it never
    // produces the data itself, keeping the editor a pure consumer
    // of engine APIs (no engine-internal `EditorAgent`).
    //
    // Inserted BEFORE `app.setup` (like the observable handles above)
    // so an app can configure them during setup — e.g. the editor
    // enables the grid via `GridConfig` there. They are standalone
    // `Arc<Mutex<Default>>` with no dependency on later-created
    // resources, so exposing them this early is safe.
    // Each overlay starts with its empty View: no gizmo line, the grid and
    // the wireframe off. The editor opts in by setting them again.
    // The audio family mixes into the shared bus every frame. An application
    // that opens an audio device installs the bus it hands the device; without
    // one, the engine keeps a bus of its own, which nothing drains.
    if !runtime
        .resources
        .contains::<Arc<dyn khora_core::audio::AudioMixBus>>()
    {
        let mix_bus: Arc<dyn khora_core::audio::AudioMixBus> =
            Arc::new(khora_infra::DefaultMixBus::new(
                khora_core::audio::StreamInfo {
                    channels: 2,
                    sample_rate: 48_000,
                },
                8192,
            ));
        runtime.resources.insert(mix_bus);
    }

    // The UI family hands the frame's texts to the text renderer. An
    // application that brings none gets the standard one.
    if !runtime
        .backends
        .contains::<Arc<dyn khora_core::renderer::api::text::TextRenderer>>()
    {
        let text: Arc<dyn khora_core::renderer::api::text::TextRenderer> = Arc::new(
            khora_infra::StandardTextRenderer::new(khora_infra::graphics::TEXT_WGSL.to_owned()),
        );
        runtime.backends.insert(text);
    }

    // The physics family steps the provider every fixed sub-step, at the
    // game's `physics.step`. An application that brings neither gets the
    // engine's own: Rapier, at the default step.
    if runtime
        .look_up::<Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>>>()
        .is_none()
    {
        let physics: Box<dyn khora_core::physics::PhysicsProvider> =
            Box::new(khora_infra::physics::rapier::RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));
    }
    if !runtime
        .resources
        .contains::<khora_core::physics::PhysicsSettings>()
    {
        runtime
            .resources
            .insert(khora_core::physics::PhysicsSettings::default());
    }

    let host_views = khora_core::lane::HostViews::new();
    host_views.set(khora_data::render::GizmoView::default());
    host_views.set(khora_data::render::GridView::default());
    host_views.set(khora_data::render::WireframeView::default());
    runtime.resources.insert(Arc::new(host_views));

    // ── Data-layer GPU resources ─────────────────────────────────────
    // AssetStore: the single engine-wide store of projected GPU assets
    // (Assets<GpuMesh> / GpuMaterial / CpuTexture sub-stores, created on
    // demand). The SDK/app fills the CpuTexture sub-store (it owns the
    // AssetService); the data-layer projection (ProjectionRegistry, runs
    // sync_all/sync_materials once per frame in PreExtract) only uploads
    // from it (khora-data must not depend on khora-io).
    //
    // Inserted BEFORE `app.setup` so an app can register assets (e.g.
    // decoded textures) into the store during setup. Neither needs a
    // graphics device at construction, so this is safe this early.
    let asset_store = khora_data::AssetStore::new();
    let proj_registry = khora_data::ProjectionRegistry::new(asset_store.clone());
    runtime.resources.insert(asset_store);
    runtime.resources.insert(proj_registry);

    // IBL bake service — projects the scene environment (procedural sky
    // for now) into the GPU cubes/LUT image-based lighting samples. Baked
    // once by the `ibl_bake` DataSystem on the first frame a device is
    // available; the lit lanes read the result at group 3.
    runtime.resources.insert(khora_data::IblBaker::new());

    // InputMap — engine-wide action / binding map. Inserted BEFORE
    // `app.setup` so apps can bind actions and cache the handle during
    // setup (e.g. the sandbox's PlayerController). The `input_map_update`
    // data system derives it from the channel below, once per frame in
    // `PreSimulation`.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_core::platform::InputMap::new())));

    // The raw stream `InputMap` is derived from, published so the descent
    // can reach it. `InputMap` answers "is jump held"; a consumer that
    // needs the order of two clicks, a double-tap, or the cursor's motion
    // needs the events, which the map's three `HashSet`s cannot express.
    // The engine's own handle is a clone of this one.
    runtime.resources.insert(input_events.clone());

    // Where the physics lane's contacts land, for anything that listens.
    // Inserted whether or not a consumer exists yet: a channel somebody has
    // to create before writing to it is a channel somebody forgets to
    // create, and the dispatch would then drop a frame of contacts with
    // nothing to say about it.
    runtime
        .resources
        .insert(khora_core::physics::collision_channel());

    // Frame graph — per-frame collection of render passes recorded by
    // agents during OUTPUT; `tick_with_services()` drains + submits it.
    let frame_graph: SharedFrameGraph = Arc::new(Mutex::new(FrameGraph::new()));
    runtime.resources.insert(frame_graph);

    // Shader/pipeline composition is owned by the `PipelineSystem` backend
    // (`WgpuPipelineSystem`), injected into `runtime.resources` by the app
    // bootstrap; render lanes resolve their layouts + pipelines through it.

    // EcsMaintenance — fetched + ticked each frame by the `ecs_maintenance`
    // DataSystem (Maintenance phase).
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::ecs::EcsMaintenance::new())));

    // AssetEviction — fetched + ticked each frame by the `asset_eviction`
    // DataSystem (Maintenance phase). Reclaims orphaned GPU meshes/materials
    // (from despawns and inline material edits) that the insert-only
    // projection would otherwise leak, freeing their wgpu resources.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::AssetEviction::new())));

    // Time — the engine's per-frame clock. The scheduler publishes the
    // real frame delta + fixed step + render-interpolation alpha into it
    // each frame; Flows and game `update` read it (replacing hardcoded
    // deltas). Shared behind RwLock so the scheduler (holding Arc<Runtime>)
    // writes while readers borrow `&Runtime`. Inserted before `app.setup`
    // so apps can read it during setup.
    let time: khora_core::time::SharedTime =
        Arc::new(std::sync::RwLock::new(khora_core::time::Time::default()));
    runtime.resources.insert(time);

    // The engine mode — which agents run. The app writes it (an editor
    // switches between editing and playing), the scheduler reads it once
    // per frame, and it starts where the app says.
    let mode: khora_core::agent::SharedEngineMode =
        Arc::new(std::sync::RwLock::new(app.initial_mode()));
    runtime.resources.insert(mode);

    // TransformInterpolation — engine-owned per-entity "previous pose" store
    // the capture pass fills and the render projection blends by the
    // interpolation alpha. A resource, not an ECS component, so it never
    // appears in the editor or scene files (interpolation is render-only).
    let transform_interpolation: khora_core::interpolation::SharedTransformInterpolation = Arc::new(
        std::sync::RwLock::new(khora_core::interpolation::TransformInterpolation::new()),
    );
    runtime.resources.insert(transform_interpolation);

    // UiImageAtlas — `AssetUUID → AtlasRect` mapping for UI images; the GPU
    // atlas itself is allocated by the `image_on_gpu` residency, once an
    // image is wanted.
    runtime
        .resources
        .insert(Arc::new(khora_data::ui::UiImageAtlas::new()));

    // AgentFrameStatus map — per-agent execution metrics measured and
    // written by the scheduler each frame; agents read their own slot in
    // `report_status` instead of holding per-frame counters.
    let agent_frame_status: khora_core::agent::gorna::AgentFrameStatusMap =
        Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
    runtime.resources.insert(agent_frame_status);

    // The scripting world — compiled programs, live behavior instances, the
    // mail behaviors owe each other. Registered here rather than owned by
    // `ScriptAgent` for the same reason the physics provider is: an
    // agent chooses a lane against a budget, and a subsystem's state is not
    // strategy state. It also means anything that legitimately needs to see
    // live behaviors — an inspector, a debugger — can, without going
    // through the agent.
    let script_runtime: Arc<Mutex<khora_lanes::script_lane::ScriptRuntime>> =
        Arc::new(Mutex::new(khora_lanes::script_lane::ScriptRuntime::new()));
    runtime.services.insert(script_runtime);

    // What scripting needs — the engine's `AssetWatch`, the queues that
    // feed the scripting world, the catalog — for **every** application
    // rather than by one binary's launcher. A windowing driver installs
    // them before the application's own bootstrap closure, which may mount
    // its scripts there; this finds them in place and replaces nothing, so
    // no program compiled at startup is dropped. Filling them is the
    // application's job — `khora_sdk::scripts::mount`.
    crate::scripts::install(runtime);

    // Create the game world — reading the shipped prefabs, where the app
    // ships an asset service, so a game saved over a level holding prefab
    // instances compares against the level expanded.
    let mut game_world = GameWorld::new();
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

    // What the scripts mounted above declare, registered before `setup`:
    // they were compiled as they were mounted, so a game attaches its
    // behaviors from its first entities. The frame boundary then finds
    // nothing pending, and registers nothing twice.
    khora_io::script::declarations::register_pending(game_world.inner_world_mut(), runtime);

    (dcc, dcc_rx, telemetry, game_world)
}

/// The engine's built-in agents, always present. Agents implement only the
/// `Agent` trait + `Default`, so construction goes through `Default::default()`.
fn register_builtin_agents(dcc: &DccService) {
    dcc.register_agent(
        Arc::new(Mutex::new(
            khora_agents::render_agent::RenderAgent::default(),
        )),
        1.0,
    );
    // Gameplay. Registered like any other consumer, which is the point of
    // giving scripting an agent at all: the DCC can tell it "you have
    // 0.4ms, hand back control", and Ergon is the only scripting language
    // in reach that can be told that. It runs only while the game does:
    // an editor editing a scene is not playing it.
    dcc.register_agent_for_mode(
        Arc::new(Mutex::new(
            khora_agents::script_agent::ScriptAgent::default(),
        )),
        1.0,
        vec![EngineMode::Playing],
    );
}

/// The order the scheduler runs the phases in: the built-in ones, the app's
/// own phases after `OUTPUT` and before `FINALIZE` in the order it declared
/// them, without the phases it removed. A phase declared twice runs once.
fn phase_order(custom: &[ExecutionPhase], removed: &[ExecutionPhase]) -> Vec<ExecutionPhase> {
    let mut order = ExecutionPhase::DEFAULT_ORDER.to_vec();
    let mut at = order
        .iter()
        .position(|p| *p == ExecutionPhase::OUTPUT)
        .map_or(order.len(), |output| output + 1);
    for phase in custom {
        if !order.contains(phase) {
            order.insert(at, *phase);
            at += 1;
        }
    }
    order.retain(|phase| !removed.contains(phase));
    order
}

/// Refuses the boot when a registered agent runs in no phase of `order`: it
/// would be registered and budgeted, yet never execute.
fn refuse_stranded_agents(dcc: &DccService, order: &[ExecutionPhase]) {
    let registry = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for agent in registry.iter() {
        let agent = agent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let phases = agent.execution_timing().allowed_phases;
        if !phases.iter().any(|phase| order.contains(phase)) {
            panic!(
                "the engine cannot boot: the agent {:?} runs only in {phases:?}, which the app removed",
                agent.id()
            );
        }
    }
}

/// One `LaneAgent` per family registration linked into the binary, each
/// registered with `dcc` for its registration's modes.
///
/// The registrations are checked together first, against the runtime and the
/// device. They are fixed when the program is built, so a refused set is a
/// build defect no frame could recover from: the engine refuses to boot.
fn register_lane_agents(
    dcc: &DccService,
    runtime: &Runtime,
    phase_order: &[ExecutionPhase],
) -> Vec<Arc<Mutex<LaneAgent>>> {
    let registrations = registered_agents();
    // No family offers a storage-buffer lane yet: the caps are read from the
    // device once one does.
    let caps = DeviceCaps::default();
    let mut agents = match lane_agents_in(&registrations, runtime, &caps, phase_order) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the engine cannot boot its lane agents: {refusal}"),
    };
    // The physics step is the game's setting, not the registration's default
    // and never a budget's: the physics family runs at `physics.step`.
    let physics = runtime
        .resources
        .get::<khora_core::physics::PhysicsSettings>()
        .copied()
        .unwrap_or_default();
    for (registration, agent) in registrations.iter().zip(agents.iter_mut()) {
        if registration.agent == khora_core::agent::gorna::AgentId::Physics {
            set_fixed_step(agent, physics.step);
        }
    }
    let taken = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .all_ids();
    if let Some(registration) = registrations
        .iter()
        .find(|registration| taken.contains(&registration.agent))
    {
        panic!(
            "the engine cannot boot its lane agents: the family `{}` negotiates as {:?}, which another agent already is",
            registration.family_name, registration.agent
        );
    }
    registrations
        .iter()
        .zip(agents)
        .map(|(registration, agent)| {
            let agent = Arc::new(Mutex::new(agent));
            dcc.register_agent_for_mode(
                agent.clone() as Arc<Mutex<dyn Agent>>,
                1.0,
                registration.modes.to_vec(),
            );
            agent
        })
        .collect()
}

/// The execution scheduler over `dcc`'s agents, running `phase_order`.
fn build_scheduler<A: EngineApp>(
    phase_order: &[ExecutionPhase],
    dcc: &mut DccService,
    core: &EngineCore<A>,
) -> khora_control::ExecutionScheduler {
    // Every registered agent gets a budget: the built-in ones and the ones the
    // application registered before them.
    let registry = dcc.agent_registry().clone();
    let mut scheduler =
        khora_control::ExecutionScheduler::from_registry(registry, core.context.clone());

    // Connect the read-only observation tunnel: the scheduler publishes
    // per-agent cost samples + per-component access snapshots the DCC's
    // cost model and layout advisor consume.
    scheduler.set_telemetry_sender(dcc.event_sender());

    // Run phases through the parallel wave executor: agents declaring
    // `AgentAccess::Isolated` / `SharedWorld` (no `&mut World`, disjoint
    // outputs) run concurrently within a phase, the rest serially. With
    // the current agents this makes `[Ui, Overlay]` a concurrent wave;
    // pass submission stays deterministic (fixed scene→ui→overlay fold).
    scheduler.set_parallel_execution(true);

    // The app's own phases, in the order the lane agents were checked against.
    scheduler.set_phase_order(phase_order);

    let budget_channel = scheduler.budget_channel().clone();
    dcc.connect_budget_channel(budget_channel);
    scheduler
}

#[cfg(test)]
mod tests {
    use super::phase_order;
    use khora_core::agent::ExecutionPhase;

    const FIRST: ExecutionPhase = ExecutionPhase::custom(6);
    const SECOND: ExecutionPhase = ExecutionPhase::custom(7);

    /// A game's own phases run after `OUTPUT` and before `FINALIZE`, in the
    /// order the game declared them.
    #[test]
    fn a_games_phases_run_in_the_order_it_declared_them() {
        let order = phase_order(&[FIRST, SECOND], &[]);

        assert_eq!(
            order,
            vec![
                ExecutionPhase::INIT,
                ExecutionPhase::OBSERVE,
                ExecutionPhase::TRANSFORM,
                ExecutionPhase::MUTATE,
                ExecutionPhase::OUTPUT,
                FIRST,
                SECOND,
                ExecutionPhase::FINALIZE,
            ]
        );
    }

    /// A removed phase is not in the order, a built-in one or the game's own.
    #[test]
    fn a_removed_phase_leaves_the_order() {
        let order = phase_order(&[FIRST, SECOND], &[ExecutionPhase::OBSERVE, FIRST]);

        assert!(!order.contains(&ExecutionPhase::OBSERVE));
        assert!(!order.contains(&FIRST));
        assert_eq!(
            order,
            vec![
                ExecutionPhase::INIT,
                ExecutionPhase::TRANSFORM,
                ExecutionPhase::MUTATE,
                ExecutionPhase::OUTPUT,
                SECOND,
                ExecutionPhase::FINALIZE,
            ]
        );
    }

    /// A phase declared twice, or a built-in phase declared again, runs once.
    #[test]
    fn a_phase_declared_twice_runs_once() {
        let order = phase_order(&[FIRST, FIRST, ExecutionPhase::OBSERVE], &[]);

        assert_eq!(order.iter().filter(|p| **p == FIRST).count(), 1);
        assert_eq!(
            order
                .iter()
                .filter(|p| **p == ExecutionPhase::OBSERVE)
                .count(),
            1
        );
    }
}
