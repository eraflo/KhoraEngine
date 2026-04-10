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

use khora_control::{DccConfig, DccService, EngineMode};
use khora_core::ServiceRegistry;
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
    /// This method consumes the provided `services` Arc which was
    /// populated by the windowing driver's bootstrap closure.
    pub fn bootstrap(
        &mut self,
        mut app: A,
        services: Arc<ServiceRegistry>,
    ) {
        // Create DCC + telemetry
        let (mut dcc, dcc_rx) = DccService::new(DccConfig::default());
        let telemetry =
            TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());
        dcc.start(dcc_rx);

        // Create the game world
        let mut game_world = GameWorld::new();

        // Call app setup
        app.setup(&mut game_world, &services);

        // Register agents via the app's AgentProvider trait
        // Unwrap the Arc to get mutable access to the services
        let mut services = Arc::try_unwrap(services)
            .unwrap_or_else(|_| panic!("services still referenced after setup"));
        app.register_agents(&dcc, &mut services);
        let services_arc = Arc::new(services);

        // Register built-in agents (always present)
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::render_agent::RenderAgent::new())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::shadow_agent::ShadowAgent::new())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::physics_agent::PhysicsAgent::new())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::ui_agent::UiAgent::new())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::audio_agent::AudioAgent::new())),
            1.0,
        );

        // Initialize agents with a minimal context
        {
            let mut init_services = ServiceRegistry::new();
            // Forward services that agents may need during init
            init_services.insert(Arc::clone(&services_arc));

            let mut init_ctx = khora_core::EngineContext {
                world: None,
                services: Arc::new(init_services),
            };
            dcc.initialize_agents(&mut init_ctx);
        }

        // Build scheduler
        let agent_ids = vec![
            khora_core::control::gorna::AgentId::Renderer,
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
            scheduler.insert_after(
                khora_core::agent::ExecutionPhase::OUTPUT,
                phase,
            );
        }

        let budget_channel = scheduler.budget_channel().clone();
        dcc.connect_budget_channel(budget_channel);

        let _ = dcc
            .event_sender()
            .send(khora_core::telemetry::TelemetryEvent::PhaseChange("boot".to_string()));

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
    pub fn tick_with_services(&mut self, frame_services_arc: Arc<ServiceRegistry>) {
        let Some(telemetry) = self.telemetry.as_mut() else {
            return;
        };

        // Start simulation event if first frame
        if !self.simulation_started {
            if let Some(dcc) = &self.dcc {
                let _ = dcc
                    .event_sender()
                    .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                        "simulation".to_string(),
                    ));
            }
            self.simulation_started = true;
        }

        let _should_log = telemetry.tick();

        let Some(app) = self.app.as_mut() else { return };

        // Drain input events
        let inputs: Vec<InputEvent> = self.input_events.drain(..).collect();

        // Application update (game logic)
        if let Some(gw) = self.game_world.as_mut() {
            app.update(gw, &inputs);

            // ECS maintenance + agent execution
            gw.tick_maintenance();

            self.scheduler.as_mut().map(|s| {
                s.run_frame(gw.inner_world_mut(), frame_services_arc);
            });
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
