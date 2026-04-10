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

//! Injection traits for the engine framework.
//!
//! The engine is a generic framework — the application (e.g. editor, game launcher)
//! provides concrete implementations of these traits to inject platform-specific
//! behavior, agents, custom phases, and application logic.

use khora_control::DccService;
use khora_core::agent::ExecutionPhase;
use khora_core::platform::KhoraWindow;

use crate::GameWorld;
use crate::WindowConfig;
use crate::InputEvent;

// ─────────────────────────────────────────────────────────────────────
// WindowProvider — abstracts the platform window backend
// ─────────────────────────────────────────────────────────────────────

/// Provides a platform window for the engine to render into.
///
/// The engine doesn't know about winit, SDL, or any specific windowing library.
/// This trait abstracts window creation, event polling, and surface queries.
pub trait WindowProvider: 'static {
    /// Creates the window.
    ///
    /// The `native_loop` parameter is an opaque handle to the native event loop.
    /// The implementation must downcast it to the correct type.
    /// For winit: `native_loop.downcast_ref::<winit::event_loop::ActiveEventLoop>()`.
    fn create(native_loop: &dyn std::any::Any, config: &WindowConfig) -> Self
    where
        Self: Sized;

    fn request_redraw(&self);
    fn inner_size(&self) -> (u32, u32);
    fn scale_factor(&self) -> f64;
    fn as_khora_window(&self) -> &dyn KhoraWindow;
    fn translate_event(&self, raw_event: &dyn std::any::Any) -> Option<InputEvent>;
}

// ─────────────────────────────────────────────────────────────────────
// AgentProvider — inject agents into the DCC
// ─────────────────────────────────────────────────────────────────────

/// Allows the application to register custom agents with the DCC.
///
/// This is where mode-specific agents (like the editor's UiAgent) are registered.
/// The engine calls this method once during initialization, after the DCC is created.
pub trait AgentProvider {
    /// Register agents with the DCC service.
    ///
    /// Use `dcc.register_agent(agent, priority)` for agents active in all modes.
    /// Use `dcc.register_agent_for_mode(agent, priority, modes)` for mode-specific agents.
    ///
    /// The `services` registry provides access to engine services that agents may need.
    fn register_agents(&self, dcc: &DccService, services: &mut khora_core::ServiceRegistry);
}

// ─────────────────────────────────────────────────────────────────────
// PhaseProvider — inject custom execution phases
// ─────────────────────────────────────────────────────────────────────

/// Allows the application to add custom execution phases to the scheduler.
///
/// By default, the engine uses the standard phase order:
/// OBSERVE → INPUT → TRANSFORM → SIMULATE → OUTPUT → PRESENT
///
/// Applications can insert custom phases (e.g. editor-specific phases).
pub trait PhaseProvider {
    /// Returns additional phases to insert into the scheduler's phase order.
    fn custom_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }

    /// Returns phases to remove from the default order.
    fn removed_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// EngineApp — composite trait
// ─────────────────────────────────────────────────────────────────────

/// The single generic bound for the engine's application type.
///
/// An application must implement `AgentProvider` and `PhaseProvider`.
/// `AgentProvider::register_agents` is where the app registers its custom agents
/// with the DCC — game logic belongs in custom agents using `ExecutionTiming`
/// and `ExecutionPhase`, not in a free-form `update()` method.
///
/// The engine also requires these inherent methods on the app type:
/// - `window_config() -> WindowConfig`
/// - `new() -> Self`
/// - `setup(&mut self, world: &mut GameWorld)`
/// - `update(&mut self, world: &mut GameWorld, inputs: &[InputEvent])`
/// - `on_shutdown(&mut self)`
pub trait EngineApp: AgentProvider + PhaseProvider + Send + Sync {
    /// Returns the window configuration for the application.
    fn window_config() -> WindowConfig
    where
        Self: Sized;

    /// Creates a new instance of the application.
    fn new() -> Self
    where
        Self: Sized;

    /// Called once during engine initialization to set up the game world.
    fn setup(&mut self, world: &mut GameWorld, services: &khora_core::ServiceRegistry);

    /// Called every frame to update game logic.
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]);

    /// Called during shutdown to clean up application resources.
    fn on_shutdown(&mut self) {}
}
