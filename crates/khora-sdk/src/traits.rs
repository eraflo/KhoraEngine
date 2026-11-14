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

//! The traits an application implements to run on the engine: its lifecycle,
//! its phases and its window backend.

use khora_core::agent::ExecutionPhase;
use khora_core::platform::KhoraWindow;

use crate::GameWorld;
use crate::InputEvent;
use crate::WindowConfig;

pub use super::window_provider::WindowProvider;

// ─────────────────────────────────────────────────────────────────────
// PhaseProvider — inject custom execution phases
// ─────────────────────────────────────────────────────────────────────

/// The application's changes to the scheduler's execution phases.
///
/// The built-in phases run INIT → OBSERVE → TRANSFORM → MUTATE → OUTPUT →
/// FINALIZE. Added phases run after OUTPUT and before FINALIZE, in the order
/// declared. Removing the only phase an agent runs in refuses the boot.
pub trait PhaseProvider {
    /// Returns the application's own phases, in the order they run.
    fn custom_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }

    /// Returns the phases the scheduler does not run.
    fn removed_phases(&self) -> Vec<ExecutionPhase> {
        Vec::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// EngineApp — composite trait
// ─────────────────────────────────────────────────────────────────────

/// An application the engine runs: its window, its setup and its per-frame update.
///
/// A subsystem of its own is a family registered with `register_agent!`, never
/// a hand-written agent. See the book: reference/sdk.md, "The application traits".
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

    /// Returns the game this application is, under which the engine files what it learns.
    ///
    /// Read once at boot and put in the runtime's resources as an
    /// `Arc<GameIdentity>`. Default: named by its window's title
    /// ([`window_config`](Self::window_config)), at the engine's version
    /// ([`ENGINE_VERSION`](crate::ENGINE_VERSION)).
    fn game_identity() -> khora_core::app::GameIdentity
    where
        Self: Sized,
    {
        khora_core::app::GameIdentity {
            name: Self::window_config().title,
            version: crate::ENGINE_VERSION.to_owned(),
        }
    }

    /// Returns whether the engine keeps its files in the user's folders across runs.
    ///
    /// Those files are this machine's profile for the game and the quarantined
    /// files. An app that keeps none touches no user folder, unless
    /// `KHORA_PROFILES` is set, which keeps a profile. Default: none kept.
    fn keeps_user_files() -> bool
    where
        Self: Sized,
    {
        false
    }

    /// Creates a new instance of the application.
    fn new() -> Self
    where
        Self: Sized;

    /// Sets up the game world, once, during engine initialization.
    fn setup(&mut self, world: &mut GameWorld, runtime: &khora_core::Runtime);

    /// Installs in the runtime what the application needs there: its resources, a shared service.
    ///
    /// Called once, after [`setup`](Self::setup) and before the engine's families
    /// are built, so every family boots with it. It never registers an agent.
    /// Default: install nothing.
    fn provide(&self, _runtime: &mut khora_core::Runtime) {}

    /// Runs the game logic, once per frame, with the frame's input events.
    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]);

    /// Releases the application's resources at shutdown.
    fn on_shutdown(&mut self) {}

    /// Returns whether a raw window event is consumed before it becomes an [`InputEvent`].
    ///
    /// A consumed event, such as one an egui overlay took, never reaches game
    /// logic. Default: none consumed.
    fn intercept_window_event(
        &mut self,
        _event: &dyn std::any::Any,
        _window: &dyn KhoraWindow,
    ) -> bool {
        false
    }

    /// Runs once per frame before [`update`](Self::update), before the engine fixes the frame.
    ///
    /// The place to begin a UI overlay frame. Families switched on or off here
    /// ([`Activation`](khora_core::agent::Activation)) are in force for this
    /// whole frame; a switch requested later counts from the next frame.
    fn before_frame(
        &mut self,
        _world: &mut GameWorld,
        _runtime: &khora_core::Runtime,
        _window: &dyn KhoraWindow,
    ) {
    }

    /// Runs after the renderer begins the frame and before the families run.
    ///
    /// The place to point the renderer at an offscreen viewport target.
    fn before_agents(&mut self, _world: &mut GameWorld, _runtime: &khora_core::Runtime) {}

    /// Runs after the families' frame graph is submitted and before the renderer ends the frame.
    ///
    /// The place to draw gizmos, return to the swapchain and present a UI overlay.
    fn after_agents(&mut self, _world: &mut GameWorld, _runtime: &khora_core::Runtime) {}

    /// Runs once, at the boundary of the first frame that finds startup over.
    ///
    /// Startup is over when every ticket waited on through the runtime's
    /// [`Readiness`](crate::Readiness) landed and no hold is kept, or the wait
    /// timed out, the late ones reported to `Problems`. The place to drop a
    /// loading screen.
    fn startup_ready(&mut self) {}
}
