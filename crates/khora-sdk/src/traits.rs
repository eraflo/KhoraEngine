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
//! behavior, what it installs in the runtime, custom phases, and application
//! logic.

use khora_core::agent::ExecutionPhase;
use khora_core::platform::KhoraWindow;

use crate::GameWorld;
use crate::InputEvent;
use crate::WindowConfig;

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

    /// Polls for window events and returns them as an iterator of raw event types.
    fn request_redraw(&self);

    /// Returns the current inner size of the window in physical pixels.
    fn inner_size(&self) -> (u32, u32);

    /// Returns the current DPI scaling factor for the window.
    fn scale_factor(&self) -> f64;

    /// Returns a reference to the window as a `dyn KhoraWindow` for use by the engine and agents.
    fn as_khora_window(&self) -> &dyn KhoraWindow;

    /// Translates a raw window event (e.g., winit::event::Event) into an engine-agnostic `InputEvent` that can be forwarded to game logic.
    fn translate_event(&self, raw_event: &dyn std::any::Any) -> Option<InputEvent>;

    /// Clones a long-lived handle to the raw native window as an opaque
    /// `Arc<dyn Any>`. Used by the winit runner to insert the underlying
    /// `Arc<winit::window::Window>` into the service registry so that
    /// editor hooks (e.g., overlay `begin_frame`) can retrieve it without
    /// the SDK leaking winit types into application code.
    fn clone_raw_window_arc(&self) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}

// ─────────────────────────────────────────────────────────────────────
// PhaseProvider — inject custom execution phases
// ─────────────────────────────────────────────────────────────────────

/// Allows the application to add custom execution phases to the scheduler.
///
/// By default, the engine runs the built-in phases in their order:
/// INIT → OBSERVE → TRANSFORM → MUTATE → OUTPUT → FINALIZE.
///
/// An application can add phases of its own, which run after OUTPUT and
/// before FINALIZE in the order it declares them, and remove phases. Removing
/// the only phase an agent runs in refuses the boot: the agent would never
/// execute.
pub trait PhaseProvider {
    /// The application's own phases, in the order they run, after OUTPUT and
    /// before FINALIZE.
    fn custom_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }

    /// The phases the scheduler does not run.
    fn removed_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// EngineApp — composite trait
// ─────────────────────────────────────────────────────────────────────

/// The single generic bound for the engine's application type.
///
/// An application must implement `PhaseProvider`. It installs in the runtime
/// what it needs from the engine in [`provide`](EngineApp::provide); a new
/// subsystem of its own is a family, registered with `register_agent!` like
/// the engine's, never a hand-written agent.
///
/// The engine also requires these inherent methods on the app type:
/// - `window_config() -> WindowConfig`
/// - `new() -> Self`
/// - `setup(&mut self, world: &mut GameWorld)`
/// - `update(&mut self, world: &mut GameWorld, inputs: &[InputEvent])`
/// - `on_shutdown(&mut self)`
///
/// # Examples
///
/// A minimal application. `setup` populates the world once; `update` runs every
/// frame. The optional hooks (`on_shutdown`, `before_frame`, …) keep their
/// no-op defaults.
///
/// ```rust,no_run
/// use khora_sdk::prelude::*;
/// use khora_sdk::{EngineApp, GameWorld, PhaseProvider, Runtime, WindowConfig};
///
/// struct MyGame {
///     frame: u64,
/// }
///
/// impl EngineApp for MyGame {
///     fn window_config() -> WindowConfig {
///         WindowConfig::default()
///     }
///     fn new() -> Self {
///         MyGame { frame: 0 }
///     }
///     fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
///         world.spawn_camera(ecs::Camera::new_perspective(
///             std::f32::consts::FRAC_PI_4,
///             0.1,
///             1000.0,
///         ));
///     }
///     fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {
///         self.frame += 1;
///     }
/// }
///
/// impl PhaseProvider for MyGame {}
/// ```
pub trait EngineApp: PhaseProvider + Send + Sync + 'static {
    /// Returns the window configuration for the application.
    fn window_config() -> WindowConfig
    where
        Self: Sized;

    /// Creates a new instance of the application.
    fn new() -> Self
    where
        Self: Sized;

    /// Called once during engine initialization to set up the game world.
    fn setup(&mut self, world: &mut GameWorld, runtime: &khora_core::Runtime);

    /// Optional: called once at boot, after [`setup`](Self::setup) and before
    /// the engine's families are built, to install in the runtime what the
    /// application needs there (its own resources, a shared service). What it
    /// inserts is in the runtime every family boots with. It never registers
    /// an agent.
    ///
    /// Default: install nothing.
    fn provide(&self, _runtime: &mut khora_core::Runtime) {}

    /// Called every frame to update game logic.
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]);

    /// Called during shutdown to clean up application resources.
    fn on_shutdown(&mut self) {}

    /// Optional: intercept a raw window event before the engine translates it into
    /// an [`InputEvent`]. Return `true` if the event was consumed (e.g., by an
    /// egui overlay) and should NOT be forwarded to game logic.
    ///
    /// Default: do not intercept.
    fn intercept_window_event(
        &mut self,
        _event: &dyn std::any::Any,
        _window: &dyn KhoraWindow,
    ) -> bool {
        false
    }

    /// Optional: called once per frame BEFORE [`update`](Self::update).
    /// Use to begin a UI overlay frame (e.g., `egui::Context::begin_frame`) and
    /// render shell chrome (menus, docks, panels) that produces UI commands.
    ///
    /// It is the application's place before the engine fixes the frame: the
    /// families it switches on or off here
    /// ([`Activation`](khora_core::agent::Activation)) are in force for this
    /// whole frame. A switch requested later counts from the next frame.
    fn before_frame(
        &mut self,
        _world: &mut GameWorld,
        _runtime: &khora_core::Runtime,
        _window: &dyn KhoraWindow,
    ) {
    }

    /// Optional: called after the renderer's `begin_frame` and before the
    /// scheduler dispatches agents. Use to switch the renderer to an offscreen
    /// viewport target (e.g., `set_render_to_viewport(true)`).
    fn before_agents(&mut self, _world: &mut GameWorld, _runtime: &khora_core::Runtime) {}

    /// Optional: called after agent execution and `submit_frame_graph`, but
    /// BEFORE the renderer's `end_frame`. Use to render gizmos to the offscreen
    /// viewport, switch back to the swapchain (`set_render_to_viewport(false)`),
    /// and present a UI overlay (`render_overlay`).
    fn after_agents(&mut self, _world: &mut GameWorld, _runtime: &khora_core::Runtime) {}

    /// Optional: called once, at the boundary of the first frame where the
    /// startup is over — every ticket the app waited on through the runtime's
    /// [`Readiness`](crate::Readiness) landed and no hold is kept, or the
    /// wait timed out (the late ones reported to `Problems`). Use to let a
    /// loading screen go: one put up in [`setup`](Self::setup) and painted by
    /// the frame hooks.
    fn startup_ready(&mut self) {}
}
