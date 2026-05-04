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
//! The engine owns: DCC, scheduler, telemetry, service registry, frame loop.
//! The app owns: window, renderer, agents, phases, game logic.

use khora_control::{substrate, DccConfig, DccService, EngineMode};
use khora_core::lane::{ClearColor, ColorTarget, DepthTarget};
use khora_core::renderer::traits::RenderSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::ServiceRegistry;
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
    services: Arc<ServiceRegistry>,
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
            })),
            services: Arc::new(ServiceRegistry::new()),
            input_events: VecDeque::new(),
            simulation_started: false,
        }
    }

    /// Bootstraps the engine: creates DCC, telemetry, scheduler,
    /// registers agents, calls `app.setup()`, and initializes agents.
    ///
    /// This method takes ownership of the `services` registry populated
    /// by the windowing driver's bootstrap closure.  It wraps the registry
    /// in an `Arc` internally once all built-in services have been inserted.
    pub fn bootstrap(&mut self, mut app: A, mut services: ServiceRegistry) {
        // Create DCC + telemetry
        let (mut dcc, dcc_rx) = DccService::new(DccConfig::default());
        let telemetry =
            TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());

        // ── Expose observable handles via ServiceRegistry ────────────────
        // Apps (e.g. the editor) read live engine state (monitors, agent
        // list, DCC context) through these handles. They're cheap clones of
        // internal Arc-shared structures, so doing so before `app.setup` is
        // safe.
        services.insert(telemetry.monitor_registry().clone());
        services.insert(dcc.agent_registry().clone());
        // Live DCC context: shared `Arc<RwLock<Context>>` updated by the
        // DCC cold thread, read by observers each frame.
        services.insert(dcc.context_handle());

        // Create the game world
        let mut game_world = GameWorld::new();

        // Call app setup — pass a temporary Arc view so the API is unchanged.
        // We own `services` exclusively here; no other Arc clone exists yet.
        {
            let services_ref = Arc::new(std::mem::take(&mut services));
            app.setup(&mut game_world, &services_ref);
            services = Arc::try_unwrap(services_ref).unwrap_or_else(|_| {
                panic!(
                    "app.setup() stored a clone of the ServiceRegistry Arc. \
                     Services must not be cloned inside setup() — cache individual \
                     services via Arc<T>, not the whole registry."
                )
            });
        }

        // Register agents via the app's AgentProvider trait.
        app.register_agents(&dcc, &mut services);

        // ── Data-layer GPU services ──────────────────────────────────────────
        // GpuCache: engine-wide shared GPU mesh store. All agents read from it.
        // ProjectionRegistry: runs sync_all() once per frame in tick_with_services()
        // before the scheduler dispatches agents.
        let gpu_cache = khora_data::GpuCache::new();
        let proj_registry = khora_data::ProjectionRegistry::new(gpu_cache.clone());
        services.insert(gpu_cache);
        services.insert(proj_registry);

        // ── Frame graph ──────────────────────────────────────────────────────
        // Per-frame collection of render passes recorded by agents during the
        // OUTPUT phase. `tick_with_services()` drains it after the scheduler
        // completes and submits the recorded command buffers.
        let frame_graph: SharedFrameGraph = Arc::new(Mutex::new(FrameGraph::new()));
        services.insert(frame_graph);

        // ── Scene-extraction data containers ─────────────────────────────────
        // RenderFlow + UiFlow publish their per-frame views directly into
        // the LaneBus during the Substrate Pass — no shared service needed.

        // EcsMaintenance — owned by ServiceRegistry so the `ecs_maintenance`
        // DataSystem (Maintenance phase) can fetch and tick it each frame.
        services.insert(Arc::new(Mutex::new(khora_data::ecs::EcsMaintenance::new())));

        // PhysicsQueryService: on-demand raycast/debug queries, no GORNA required.
        if let Some(provider) = services
            .get::<std::sync::Arc<std::sync::Mutex<Box<dyn khora_core::physics::PhysicsProvider>>>>(
            )
        {
            services.insert(khora_agents::PhysicsQueryService::new(provider.clone()));
        }

        let services_arc = Arc::new(services);

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

        // Initialize agents with the full service registry so on_initialize()
        // can find Arc<dyn GraphicsDevice>, Arc<Mutex<Box<dyn RenderSystem>>>,
        // GpuCache, etc. via a flat TypeId lookup.
        // ServiceRegistry has no nested delegation — agents must receive the
        // real registry directly, not a wrapper that only contains Arc<ServiceRegistry>.
        {
            let init_bus = khora_core::lane::LaneBus::new();
            let mut init_deck = khora_core::lane::OutputDeck::new();
            let mut init_ctx = khora_core::EngineContext {
                world: None,
                services: Arc::clone(&services_arc),
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
            khora_core::control::gorna::AgentId::Physics,
            khora_core::control::gorna::AgentId::Ui,
            khora_core::control::gorna::AgentId::Audio,
        ];

        let registry = dcc.agent_registry().clone();
        let mut scheduler =
            khora_control::ExecutionScheduler::new(registry, self.context.clone(), &agent_ids);

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
        self.services = services_arc;
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
        // Build default frame services and delegate.
        let mut frame_services = ServiceRegistry::new();
        frame_services.insert(Arc::clone(&self.services));
        frame_services.insert(PRIMARY_VIEWPORT);
        let frame_services_arc = Arc::new(frame_services);
        self.tick_with_services(frame_services_arc);
    }

    /// Executes one frame with a custom frame services registry.
    ///
    /// Used by the winit runner to inject a `FrameContext` into the
    /// frame services for cross-agent synchronization.
    ///
    /// This is a convenience wrapper that calls the staged methods in order
    /// without invoking any [`EngineApp`] lifecycle hooks. Drivers that need
    /// to interleave hooks (e.g., the editor's overlay/shell) should call the
    /// staged methods directly via the winit runner.
    pub fn tick_with_services(&mut self, frame_services_arc: Arc<ServiceRegistry>) {
        let inputs = self.drain_inputs();
        self.run_app_update(&inputs);
        let presents = self.begin_render_frame(&frame_services_arc);
        self.run_scheduler(&frame_services_arc);
        self.end_render_frame(presents);

        // Substrate Pass — end-of-tick maintenance (compaction, deferred
        // cleanup, idempotent best-effort work). Runs after every agent and
        // after the I/O boundary so the world is in its final post-frame state.
        if let Some(gw) = self.game_world.as_mut() {
            substrate::run_data_systems(
                gw.inner_world_mut(),
                &self.services,
                TickPhase::Maintenance,
            );
        }
    }

    /// Stage 1 — drain queued input events. Also marks simulation started
    /// (emits the `"simulation"` phase change on the first call) and ticks
    /// the telemetry service.
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
        self.input_events.drain(..).collect()
    }

    /// Stage 2 — run `app.update`, ECS maintenance, mesh sync, and scene/UI
    /// extractions. Called between [`drain_inputs`](Self::drain_inputs) and
    /// [`begin_render_frame`](Self::begin_render_frame).
    pub fn run_app_update(&mut self, inputs: &[InputEvent]) {
        let (Some(app), Some(gw)) = (self.app.as_mut(), self.game_world.as_mut()) else {
            return;
        };
        let services = &self.services;

        // Substrate Pass — pre-simulation invariants (input-driven mutations,
        // scene events that must be visible to agents).
        substrate::run_data_systems(gw.inner_world_mut(), services, TickPhase::PreSimulation);

        app.update(gw, inputs);

        // Substrate Pass — post-simulation invariants (hierarchy fix-ups
        // such as transform_propagation, run after app.update mutates Transforms
        // but before extraction reads GlobalTransform).
        substrate::run_data_systems(gw.inner_world_mut(), services, TickPhase::PostSimulation);

        // Substrate Pass — pre-extract. Runs:
        //  - `gpu_mesh_sync` (CPU→GPU mesh upload, replaces former proj.sync_all)
        //  - any other PreExtract DataSystem registered by users.
        // RenderFlow + UiFlow then run inside the scheduler's Substrate Pass
        // and publish their views into the LaneBus.
        substrate::run_data_systems(gw.inner_world_mut(), &self.services, TickPhase::PreExtract);
    }

    /// Stage 3 — acquire the swapchain via `RenderSystem::begin_frame` and
    /// populate the per-frame [`FrameContext`] with `ColorTarget`,
    /// `DepthTarget`, and `ClearColor`.
    ///
    /// Returns `true` when a renderer is present and `begin_frame` succeeded
    /// (driver should later call [`present_frame`](Self::present_frame)).
    /// Returns `false` only when no renderer is registered or the swapchain
    /// could not be acquired.
    ///
    /// Note: the `RenderSystem::render_to_viewport` flag still controls
    /// **where** color/depth targets point (offscreen viewport vs swapchain),
    /// but `begin_frame` is always invoked so that the swapchain texture is
    /// available for editor overlay passes that paint on top of an
    /// offscreen-rendered scene.
    pub fn begin_render_frame(&mut self, frame_services_arc: &Arc<ServiceRegistry>) -> bool {
        let render_system = self
            .services
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        let fctx = frame_services_arc
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
                log::error!("EngineCore: begin_frame failed: {}", e);
                false
            }
        }
    }

    /// Stage 4 — dispatch the scheduler so all registered agents execute
    /// their phases for this frame.
    pub fn run_scheduler(&mut self, frame_services_arc: &Arc<ServiceRegistry>) {
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        if let Some(s) = self.scheduler.as_mut() {
            s.run_frame(gw.inner_world_mut(), frame_services_arc.clone());
        }
    }

    /// Stage 5a — submit recorded passes from the [`FrameGraph`] to the GPU.
    /// When `presents` is `false` (render-to-viewport mode), the frame graph
    /// is discarded instead.
    ///
    /// Drivers that need to interleave their own rendering between agent
    /// submission and the final present (e.g., the editor's `render_overlay`
    /// pass that must paint on top of the 3D scene) should call this method
    /// between [`run_scheduler`](Self::run_scheduler) and
    /// [`present_frame`](Self::present_frame).
    pub fn submit_passes(&mut self, presents: bool) {
        let device = self
            .services
            .get::<Arc<dyn GraphicsDevice>>()
            .map(|arc| (*arc).clone());
        let frame_graph = self
            .services
            .get::<SharedFrameGraph>()
            .map(|arc| (*arc).clone());

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
            .services
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        if let Some(rs) = &render_system {
            if let Ok(mut guard) = rs.lock() {
                if let Err(e) = guard.end_frame() {
                    log::error!("EngineCore: end_frame failed: {}", e);
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

    /// Stores the services Arc. Used by the winit runner after bootstrap.
    pub fn set_services(&mut self, services: Arc<ServiceRegistry>) {
        self.services = services;
    }

    /// Returns a reference to the service registry.
    pub fn services(&self) -> &Arc<ServiceRegistry> {
        &self.services
    }

    /// Returns a mutable reference to the game world, if initialized.
    pub fn game_world_mut(&mut self) -> Option<&mut GameWorld> {
        self.game_world.as_mut()
    }

    /// Returns the DCC service, if initialized.
    pub fn dcc(&self) -> Option<&DccService> {
        self.dcc.as_ref()
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
