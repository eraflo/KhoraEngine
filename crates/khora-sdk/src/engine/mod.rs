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

use khora_control::{DccService, EngineMode};
use khora_core::event::Channel;
use khora_core::Runtime;
use khora_telemetry::TelemetryService;
use std::sync::{Arc, RwLock};

use crate::traits::EngineApp;
use crate::{GameWorld, InputEvent};

mod bootstrap;
mod frame;

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
    /// This frame's OS input, and the frames before it that nobody read.
    ///
    /// A clone of what sits in `runtime.resources`, so a `DataSystem` or an
    /// agent that declares it reads the same stream the app hook does — each
    /// from its own position, none taking events from the others. It replaced
    /// an unbounded `VecDeque` that only this struct could reach.
    input_events: Channel<InputEvent>,
    /// The engine mode last told to the DCC.
    forwarded_mode: Option<EngineMode>,
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
            input_events: khora_core::platform::input_channel(),
            forwarded_mode: None,
        }
    }

    /// Queues an input event to be processed on the next tick.
    pub fn feed_input(&mut self, event: InputEvent) {
        self.input_events.send(event);
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

    /// Applies a developer [`EngineHint`](khora_core::agent::gorna::EngineHint)
    /// biasing GORNA arbitration (`Cap` a per-frame budget, `Prioritize` an
    /// agent) without changing game semantics. No-op if the DCC isn't running.
    /// Thread-safe; takes effect on the next arbitration tick.
    pub fn set_engine_hint(&self, hint: khora_core::agent::gorna::EngineHint) {
        if let Some(dcc) = &self.dcc {
            dcc.set_hint(hint);
        }
    }

    /// Clears all developer hints for an agent, restoring engine defaults.
    pub fn clear_agent_hints(&self, agent_id: khora_core::agent::gorna::AgentId) {
        if let Some(dcc) = &self.dcc {
            dcc.clear_agent_hints(agent_id);
        }
    }

    /// Read-only snapshot of the accumulated per-agent hints (glass-box), or an
    /// empty map if the DCC isn't running.
    pub fn engine_hints(
        &self,
    ) -> std::collections::HashMap<
        khora_core::agent::gorna::AgentId,
        khora_core::agent::gorna::AgentHints,
    > {
        self.dcc.as_ref().map(|d| d.hints()).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    /// The handle the winit runner inserts as a resource is the one games and
    /// the editor name through the public `PRIMARY_VIEWPORT`.
    #[test]
    fn the_runner_viewport_is_the_public_primary_viewport() {
        assert_eq!(super::PRIMARY_VIEWPORT, crate::PRIMARY_VIEWPORT);
    }
}
