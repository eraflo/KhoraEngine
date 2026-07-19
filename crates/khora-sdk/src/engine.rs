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

//! The Khora Engine core — generic engine runtime with injection points.
//!
//! This module contains `EngineCore`, the winit-agnostic engine core.
//! Windowing and event-loop integration lives in `winit_adapters.rs`.
//!
//! The engine owns: DCC, scheduler, telemetry, runtime containers, frame loop.
//! The app owns: window, renderer, agents, phases, game logic.

use khora_control::{substrate, DccConfig, DccService, EngineMode};
use khora_core::lane::{ClearColor, ColorTarget, DepthTarget};
use khora_core::renderer::traits::RenderSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::ecs::TickPhase;
use khora_data::render::{submit_frame_graph, FrameGraph, SharedFrameGraph};
use khora_telemetry::TelemetryService;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::traits::EngineApp;
use crate::GameWorld;
use crate::InputEvent;

/// Well-known viewport handle for the primary 3D viewport.
pub const PRIMARY_VIEWPORT: khora_core::ui::editor::viewport_texture::ViewportTextureHandle =
    khora_core::ui::editor::viewport_texture::ViewportTextureHandle(0);

// ─────────────────────────────────────────────────────────────────────
// EngineCore — winit-agnostic engine runtime
// ─────────────────────────────────────────────────────────────────────

/// The core engine state, independent of any windowing backend.
///
/// Created by the windowing driver (e.g. `WinitAppRunner`), then
/// driven frame-by-frame via [`EngineCore::tick`].
pub struct EngineCore<A: EngineApp> {
    app: Option<A>,
    game_world: Option<GameWorld>,
    telemetry: Option<TelemetryService>,
    dcc: Option<DccService>,
    scheduler: Option<khora_control::ExecutionScheduler>,
    context: Arc<RwLock<khora_control::Context>>,
    runtime: Arc<Runtime>,
    input_events: VecDeque<InputEvent>,
    simulation_started: bool,
}

impl<A: EngineApp> EngineCore<A> {
    /// Creates a new, uninitialized engine core.
    pub fn new() -> Self {
        Self {
            app: None,
            game_world: None,
            telemetry: None,
            dcc: None,
            scheduler: None,
            context: Arc::new(RwLock::new(khora_control::Context {
                hardware: khora_control::HardwareState::default(),
                mode: EngineMode::Playing,
                global_budget_multiplier: 1.0,
                memory_pressure: 0.0,
            })),
            runtime: Arc::new(Runtime::new()),
            input_events: VecDeque::new(),
            simulation_started: false,
        }
    }

    /// Bootstraps the engine: creates DCC, telemetry, scheduler,
    /// registers agents, calls `app.setup()`, and initializes agents.
    ///
    /// This method takes ownership of the `runtime` populated
    /// by the windowing driver's bootstrap closure.  It wraps the runtime
    /// in an `Arc` once all built-in entries have been inserted.
    pub fn bootstrap(&mut self, mut app: A, mut runtime: Runtime) {
        // Create DCC + telemetry
        let (mut dcc, dcc_rx) = DccService::new(DccConfig::default());
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
        // writes into each frame; the `OverlayAgent` lanes read them in
        // the OUTPUT phase. The engine only provides the slots — it never
        // produces the data itself, keeping the editor a pure consumer
        // of engine APIs (no engine-internal `EditorAgent`).
        //
        // Inserted BEFORE `app.setup` (like the observable handles above)
        // so an app can configure them during setup — e.g. the editor
        // enables the grid via `GridConfig` there. They are standalone
        // `Arc<Mutex<Default>>` with no dependency on later-created
        // resources, so exposing them this early is safe.
        let gizmo_frame: khora_lanes::render_lane::SharedGizmoFrame =
            Arc::new(Mutex::new(khora_data::render::GizmoFrame::default()));
        runtime.resources.insert(gizmo_frame);
        // Editor grid — disabled by default; the editor opts in.
        let grid_config: khora_lanes::render_lane::SharedGridConfig =
            Arc::new(Mutex::new(khora_data::render::GridConfig::default()));
        runtime.resources.insert(grid_config);

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
        // setup (e.g. the sandbox's PlayerController). The engine ticks it
        // each frame from `drain_inputs`.
        runtime
            .resources
            .insert(Arc::new(Mutex::new(khora_core::platform::InputMap::new())));

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

        // TransformInterpolation — engine-owned per-entity "previous pose" store
        // the capture pass fills and the render projection blends by the
        // interpolation alpha. A resource, not an ECS component, so it never
        // appears in the editor or scene files (interpolation is render-only).
        let transform_interpolation: khora_core::interpolation::SharedTransformInterpolation =
            Arc::new(std::sync::RwLock::new(
                khora_core::interpolation::TransformInterpolation::new(),
            ));
        runtime.resources.insert(transform_interpolation);

        // UiImageAtlas — `AssetUUID → AtlasRect` mapping for UI images; the GPU
        // atlas itself is allocated lazily by `UiAgent::on_initialize`.
        runtime
            .resources
            .insert(Arc::new(khora_data::ui::UiImageAtlas::new()));

        // AgentFrameStatus map — per-agent execution metrics measured and
        // written by the scheduler each frame; agents read their own slot in
        // `report_status` instead of holding per-frame counters.
        let agent_frame_status: khora_core::control::gorna::AgentFrameStatusMap =
            Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        runtime.resources.insert(agent_frame_status);

        // Create the game world
        let mut game_world = GameWorld::new();

        // Call app setup. The runtime is now fully populated, so the app can
        // read any engine resource here. Mutation goes through `register_agents`.
        app.setup(&mut game_world, &runtime);

        // Register agents via the app's AgentProvider trait.
        app.register_agents(&dcc, &mut runtime);

        let runtime_arc = Arc::new(runtime);

        // Register built-in agents (always present). Agents implement only the
        // `Agent` trait + `Default`, so construction goes through `Default::default()`.
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::render_agent::RenderAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::shadow_agent::ShadowAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::overlay_agent::OverlayAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::physics_agent::PhysicsAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::ui_agent::UiAgent::default())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::audio_agent::AudioAgent::default())),
            1.0,
        );

        // Initialize agents with the full runtime so on_initialize() can
        // find Arc<dyn GraphicsDevice>, Arc<Mutex<Box<dyn RenderSystem>>>,
        // GpuCache, etc. via the typed runtime containers.
        {
            let init_bus = khora_core::lane::LaneBus::new();
            let mut init_deck = khora_core::lane::OutputDeck::new();
            let mut init_ctx = khora_core::EngineContext {
                world: khora_core::WorldAccess::None,
                runtime: Arc::clone(&runtime_arc),
                bus: &init_bus,
                deck: &mut init_deck,
            };
            dcc.initialize_agents(&mut init_ctx);
        }
        // Start the DCC background thread AFTER agents are initialized
        // so GORNA does not run health-checks before agents are ready.
        dcc.start(dcc_rx);

        // Build scheduler
        let agent_ids = vec![
            khora_core::control::gorna::AgentId::Renderer,
            khora_core::control::gorna::AgentId::ShadowRenderer,
            khora_core::control::gorna::AgentId::Overlay,
            khora_core::control::gorna::AgentId::Physics,
            khora_core::control::gorna::AgentId::Ui,
            khora_core::control::gorna::AgentId::Audio,
        ];

        let registry = dcc.agent_registry().clone();
        let mut scheduler =
            khora_control::ExecutionScheduler::new(registry, self.context.clone(), &agent_ids);

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

        // Inject custom phases from the app
        let custom_phases = app.custom_phases();
        for phase in custom_phases {
            scheduler.insert_after(khora_core::agent::ExecutionPhase::OUTPUT, phase);
        }

        let budget_channel = scheduler.budget_channel().clone();
        dcc.connect_budget_channel(budget_channel);

        let _ = dcc
            .event_sender()
            .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                "boot".to_string(),
            ));

        // Store everything
        self.app = Some(app);
        self.game_world = Some(game_world);
        self.telemetry = Some(telemetry);
        self.dcc = Some(dcc);
        self.scheduler = Some(scheduler);
        self.runtime = runtime_arc;
    }

    /// Queues an input event to be processed on the next tick.
    pub fn feed_input(&mut self, event: InputEvent) {
        self.input_events.push_back(event);
    }

    /// Executes one frame: app update, ECS maintenance, scheduler.
    ///
    /// This method is called by the windowing driver (e.g. winit)
    /// each time a redraw is requested.
    pub fn tick(&mut self) {
        // Default tick path: reuse the engine-level Runtime as-is. The
        // winit driver injects per-frame resources (`FrameContext`,
        // viewport handle) by calling `tick_with_runtime` directly.
        let frame_runtime_arc = Arc::clone(&self.runtime);
        self.tick_with_runtime(frame_runtime_arc);
    }

    /// Executes one frame with a custom per-frame [`Runtime`] overlay.
    ///
    /// Used by the winit runner to inject a `FrameContext` into the
    /// per-frame Resources for cross-agent synchronization.
    ///
    /// This is a convenience wrapper that calls the staged methods in order
    /// without invoking any [`EngineApp`] lifecycle hooks. Drivers that need
    /// to interleave hooks (e.g., the editor's overlay/shell) should call the
    /// staged methods directly via the winit runner.
    pub fn tick_with_runtime(&mut self, frame_runtime_arc: Arc<Runtime>) {
        let inputs = self.drain_inputs();
        self.run_app_update(&inputs);
        let presents = self.begin_render_frame(&frame_runtime_arc);
        self.run_scheduler(&frame_runtime_arc);
        self.end_render_frame(presents);
        self.run_maintenance();
    }

    /// Stage 6 — runs the end-of-tick `Maintenance`-phase `DataSystem`s
    /// against the scheduler's per-frame [`OutputDeck`].
    ///
    /// The engine is subsystem-agnostic here: it threads the deck through
    /// the substrate dispatcher and lets each `DataSystem` drain whatever
    /// typed slot belongs to its domain (audio writeback, physics
    /// writeback, future subsystems …). No subsystem-specific code lives
    /// in this method.
    pub fn run_maintenance(&mut self) {
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        let world = gw.inner_world_mut();

        if let Some(scheduler) = self.scheduler.as_mut() {
            substrate::run_data_systems(
                world,
                &self.runtime,
                scheduler.deck_mut(),
                TickPhase::Maintenance,
            );
        } else {
            // No scheduler installed — pass a transient empty deck so
            // `DataSystem`s that read it harmlessly observe an empty slot.
            let mut deck = khora_core::lane::OutputDeck::new();
            substrate::run_data_systems(world, &self.runtime, &mut deck, TickPhase::Maintenance);
        }
    }

    /// Stage 1 — drain queued input events. Also marks simulation started
    /// (emits the `"simulation"` phase change on the first call), ticks
    /// the telemetry service, and feeds the events into the engine-wide
    /// [`khora_core::platform::InputMap`] so app code can query actions
    /// (`is_pressed`, `just_pressed`) from the same frame's inputs.
    pub fn drain_inputs(&mut self) -> Vec<InputEvent> {
        if !self.simulation_started {
            if let Some(dcc) = &self.dcc {
                let _ =
                    dcc.event_sender()
                        .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                            "simulation".to_string(),
                        ));
            }
            self.simulation_started = true;
        }
        if let Some(telemetry) = self.telemetry.as_mut() {
            let _ = telemetry.tick();
        }
        let drained: Vec<InputEvent> = self.input_events.drain(..).collect();

        // Tick the InputMap with this frame's events. Lock is short — only
        // held for the duration of `update`. App code that needs the map
        // (e.g. `runtime.resources.get::<Arc<Mutex<InputMap>>>()`) sees the
        // updated state on its next lock.
        if let Some(map_arc) = self
            .runtime
            .resources
            .get::<Arc<Mutex<khora_core::platform::InputMap>>>()
        {
            if let Ok(mut map) = map_arc.lock() {
                map.update(&drained);
            }
        }

        drained
    }

    /// Stage 2 — run `app.update`, ECS maintenance, mesh sync, and scene/UI
    /// extractions. Called between [`drain_inputs`](Self::drain_inputs) and
    /// [`begin_render_frame`](Self::begin_render_frame).
    pub fn run_app_update(&mut self, inputs: &[InputEvent]) {
        let (Some(app), Some(gw)) = (self.app.as_mut(), self.game_world.as_mut()) else {
            return;
        };
        let runtime = &self.runtime;

        // Pre-scheduler phases run before the per-frame `OutputDeck` is
        // built. We pass a transient empty deck so `DataSystem`s that
        // happen to read it observe an empty slot — they simply no-op.
        let mut deck = khora_core::lane::OutputDeck::new();

        // Substrate Pass — pre-simulation invariants (input-driven mutations,
        // scene events that must be visible to agents).
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PreSimulation,
        );

        app.update(gw, inputs);

        // Substrate Pass — post-simulation invariants (hierarchy fix-ups
        // such as transform_propagation, run after app.update mutates Transforms
        // but before extraction reads GlobalTransform).
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PostSimulation,
        );

        // Substrate Pass — pre-extract. Runs:
        //  - `gpu_mesh_sync` (CPU→GPU mesh upload, replaces former proj.sync_all)
        //  - any other PreExtract DataSystem registered by users.
        // RenderFlow + UiFlow then run inside the scheduler's Substrate Pass
        // and publish their views into the LaneBus.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            &self.runtime,
            &mut deck,
            TickPhase::PreExtract,
        );
    }

    /// Stage 3 — acquire the swapchain via `RenderSystem::begin_frame` and
    /// populate the per-frame [`FrameContext`] with `ColorTarget`,
    /// `DepthTarget`, and `ClearColor`.
    ///
    /// Returns `true` when a renderer is present and `begin_frame` succeeded
    /// (driver should later call [`present_frame`](Self::present_frame)).
    /// Returns `false` only when no renderer is registered or the swapchain
    /// could not be acquired.
    pub fn begin_render_frame(&mut self, frame_runtime_arc: &Arc<Runtime>) -> bool {
        let render_system = self
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        let fctx = frame_runtime_arc
            .resources
            .get::<Arc<khora_core::renderer::api::core::FrameContext>>()
            .map(|arc| (*arc).clone());

        let Some(rs) = &render_system else {
            return false;
        };
        let Ok(mut guard) = rs.lock() else {
            return false;
        };
        match guard.begin_frame() {
            Ok(targets) => {
                if let Some(fctx) = &fctx {
                    fctx.insert(ColorTarget(targets.color));
                    if let Some(d) = targets.depth {
                        fctx.insert(DepthTarget(d));
                    }
                    fctx.insert(ClearColor(khora_core::math::LinearRgba::new(
                        0.1, 0.1, 0.15, 1.0,
                    )));
                }
                true
            }
            Err(e) => {
                // Fatal device errors (lost / out-of-memory) are surfaced loudly
                // so the host can decide to tear down; transient acquisition
                // skips (minimized window, surface reconfigure, timeout) are
                // expected and logged at debug to avoid per-frame spam.
                if e.is_fatal() {
                    log::error!("EngineCore: begin_frame fatal error: {}", e);
                } else {
                    log::debug!("EngineCore: begin_frame skipped this frame: {}", e);
                }
                false
            }
        }
    }

    /// Stage 4 — dispatch the scheduler so all registered agents execute
    /// their phases for this frame.
    pub fn run_scheduler(&mut self, frame_runtime_arc: &Arc<Runtime>) {
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        if let Some(s) = self.scheduler.as_mut() {
            s.run_frame(gw.inner_world_mut(), frame_runtime_arc.clone());
        }
    }

    /// Stage 5a — submit recorded passes from the [`FrameGraph`] to the GPU.
    /// When `presents` is `false` (render-to-viewport mode), the frame graph
    /// is discarded instead.
    pub fn submit_passes(&mut self, presents: bool) {
        let device = self
            .runtime
            .backends
            .get::<Arc<dyn GraphicsDevice>>()
            .map(|arc| (*arc).clone());
        let frame_graph = self
            .runtime
            .resources
            .get::<SharedFrameGraph>()
            .map(|arc| (*arc).clone());

        // Fold the agents' recorded passes (buffered in the scheduler's deck by
        // Render/Ui/Overlay instead of locking the shared graph) into the
        // FrameGraph in a fixed layer order — scene, then UI, then overlay
        // compositing. This reproduces the previous add_pass insertion order,
        // so the topological submit order is unchanged.
        if let (Some(graph), Some(scheduler)) = (&frame_graph, self.scheduler.as_mut()) {
            use khora_data::render::{OverlayPassSlot, ScenePassSlot, UiPassSlot};
            let deck = scheduler.deck_mut();
            let scene = deck.take::<ScenePassSlot>().0;
            let ui = deck.take::<UiPassSlot>().0;
            let overlay = deck.take::<OverlayPassSlot>().0;
            if scene.is_some() || ui.is_some() || overlay.is_some() {
                if let Ok(mut fg) = graph.lock() {
                    for pass in [scene, ui, overlay].into_iter().flatten() {
                        fg.add_pass(pass.descriptor, pass.command_buffer);
                    }
                } else {
                    log::error!("submit_passes: FrameGraph mutex poisoned, dropping frame passes");
                }
            }
        }

        if presents {
            if let (Some(graph), Some(device)) = (&frame_graph, &device) {
                submit_frame_graph(graph, device.as_ref());
            }
        } else if let Some(graph) = &frame_graph {
            graph.lock().expect("FrameGraph mutex poisoned").clear();
        }
    }

    /// Stage 5b — call `RenderSystem::end_frame` to present the swapchain.
    /// No-op when `presents` is `false`.
    pub fn present_frame(&mut self, presents: bool) {
        if !presents {
            return;
        }
        let render_system = self
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        if let Some(rs) = &render_system {
            if let Ok(mut guard) = rs.lock() {
                if let Err(e) = guard.end_frame() {
                    if e.is_fatal() {
                        log::error!("EngineCore: end_frame fatal error: {}", e);
                    } else {
                        log::debug!("EngineCore: end_frame skipped this frame: {}", e);
                    }
                }
            }
        }
    }

    /// Convenience: runs both [`submit_passes`](Self::submit_passes) and
    /// [`present_frame`](Self::present_frame) in order. Used by the default
    /// `tick` path; drivers that need to interleave hooks should call the
    /// staged methods directly.
    pub fn end_render_frame(&mut self, presents: bool) {
        self.submit_passes(presents);
        self.present_frame(presents);
    }

    /// Mutable accessor for the application instance. Used by the winit
    /// runner to invoke [`EngineApp`] lifecycle hooks between staged frame
    /// methods.
    pub fn app_mut(&mut self) -> Option<&mut A> {
        self.app.as_mut()
    }

    /// Invokes a closure with mutable access to BOTH the application and the
    /// game world simultaneously. Used by the winit runner to call lifecycle
    /// hooks that need to read/write components (e.g., gizmo collection)
    /// without re-borrowing `EngineCore` twice.
    ///
    /// The closure is skipped silently if either is uninitialized.
    pub fn with_app_and_world<F>(&mut self, f: F)
    where
        F: FnOnce(&mut A, &mut GameWorld),
    {
        if let (Some(app), Some(world)) = (self.app.as_mut(), self.game_world.as_mut()) {
            f(app, world);
        }
    }

    /// Stores the runtime Arc. Used by the winit runner after bootstrap.
    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = runtime;
    }

    /// Returns a reference to the engine-level runtime.
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// Returns a mutable reference to the game world, if initialized.
    pub fn game_world_mut(&mut self) -> Option<&mut GameWorld> {
        self.game_world.as_mut()
    }

    /// Returns the DCC service, if initialized.
    pub fn dcc(&self) -> Option<&DccService> {
        self.dcc.as_ref()
    }

    /// Applies a developer [`EngineHint`](khora_core::control::gorna::EngineHint)
    /// biasing GORNA arbitration (`Cap` a per-frame budget, `Prioritize` an
    /// agent) without changing game semantics. No-op if the DCC isn't running.
    /// Thread-safe; takes effect on the next arbitration tick.
    pub fn set_engine_hint(&self, hint: khora_core::control::gorna::EngineHint) {
        if let Some(dcc) = &self.dcc {
            dcc.set_hint(hint);
        }
    }

    /// Clears all developer hints for an agent, restoring engine defaults.
    pub fn clear_agent_hints(&self, agent_id: khora_core::control::gorna::AgentId) {
        if let Some(dcc) = &self.dcc {
            dcc.clear_agent_hints(agent_id);
        }
    }

    /// Read-only snapshot of the accumulated per-agent hints (glass-box), or an
    /// empty map if the DCC isn't running.
    pub fn engine_hints(
        &self,
    ) -> std::collections::HashMap<
        khora_core::control::gorna::AgentId,
        khora_core::control::gorna::AgentHints,
    > {
        self.dcc
            .as_ref()
            .map(|d| d.hints())
            .unwrap_or_default()
    }

    /// Shuts down the engine, calling `app.on_shutdown()`.
    ///
    /// Note: renderer shutdown is the responsibility of the application,
    /// since the renderer was created and registered by the app's bootstrap closure.
    pub fn shutdown(&mut self) {
        if let Some(app) = self.app.as_mut() {
            app.on_shutdown();
        }
        log::info!("Engine shutdown complete.");
    }
}

impl<A: EngineApp> Default for EngineCore<A> {
    fn default() -> Self {
        Self::new()
    }
}
