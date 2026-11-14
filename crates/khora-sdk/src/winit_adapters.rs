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

//! Winit integration: [`run_winit`] and the runner that drives the engine from
//! winit's event loop.

use std::any::Any;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use khora_core::platform::KhoraWindow;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::RenderSystem;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use crate::engine::{EngineCore, PRIMARY_VIEWPORT};
use crate::traits::{EngineApp, WindowProvider};

pub use crate::winit_window_provider::WinitWindowProvider;

// ─────────────────────────────────────────────────────────────────────
// WinitAppRunner — bridges winit ApplicationHandler with EngineCore
// ─────────────────────────────────────────────────────────────────────

/// One-shot bootstrap closure used to wire engine backends, services, and
/// resources into the [`khora_core::Runtime`] at window-creation time.
type BootstrapFn = Box<dyn FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any)>;

/// The winit `ApplicationHandler` that runs an [`EngineCore`] for the app `A`.
pub struct WinitAppRunner<W: WindowProvider, A: EngineApp> {
    window: Option<W>,
    engine: EngineCore<A>,
    renderer: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    bootstrap: Option<BootstrapFn>,
    /// The engine's frame context, fetched from the runtime once.
    frame_context: Option<Arc<FrameContext>>,
}

impl<W: WindowProvider, A: EngineApp> WinitAppRunner<W, A> {
    /// Creates a runner that calls `bootstrap` once its window exists.
    pub fn new(
        bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any) + 'static,
    ) -> Self {
        Self {
            window: None,
            engine: EngineCore::new(),
            renderer: None,
            bootstrap: Some(Box::new(bootstrap)),
            frame_context: None,
        }
    }

    /// Runs one frame: the engine's stages, with the app's hooks between them.
    ///
    /// See the book: concepts/the-frame.md, "The six-stage descent".
    fn run_frame(&mut self) {
        // One `FrameContext` serves every frame: each frame's blackboard
        // writes replace the previous frame's entries of the same type.
        let runtime_arc = Arc::clone(self.engine.runtime());
        if self.frame_context.is_none() {
            self.frame_context = runtime_arc.resources.get::<Arc<FrameContext>>().cloned();
        }

        let inputs = self.engine.drain_inputs();

        if let Some(window) = self.window.as_ref() {
            let kwin = window.as_khora_window();
            self.engine.with_app_and_world(|app, world| {
                app.before_frame(world, &runtime_arc, kwin);
            });
        }

        self.engine.run_app_update(&inputs);

        let presents = self.engine.begin_render_frame(&runtime_arc);

        self.engine.with_app_and_world(|app, world| {
            app.before_agents(world, &runtime_arc);
        });

        self.engine.run_scheduler(&runtime_arc);

        // Submitted before `after_agents`, so an overlay drawn there paints
        // over the 3D scene instead of under it.
        self.engine.submit_passes(presents);

        self.engine.with_app_and_world(|app, world| {
            app.after_agents(world, &runtime_arc);
        });

        self.engine.present_frame(presents);

        // The Maintenance-phase DataSystems (write-backs, ECS compaction)
        // run only here.
        self.engine.run_maintenance();

        // Report the frame's busy time to the DCC: the frame ends here.
        self.engine.close_frame();
    }
}

impl<W: WindowProvider, A: EngineApp> ApplicationHandler for WinitAppRunner<W, A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Khora Engine: Initializing...");

        let window_config = A::window_config();
        let window = W::create(event_loop as &dyn Any, &window_config);

        // Windowing-tier entries go in first: the bootstrap closure may read them.
        let mut runtime = khora_core::Runtime::new();

        // Scripting's resources, before the application's closure: an app
        // mounts its scripts there.
        crate::scripts::install(&mut runtime);

        runtime.resources.insert(PRIMARY_VIEWPORT);

        // One FrameContext for the engine's life: its blackboard is reused across frames.
        let fctx = Arc::new(FrameContext::new());
        runtime.resources.insert(Arc::clone(&fctx));
        self.frame_context = Some(fctx);

        // The raw window goes in the resources, so the app's hooks can hand
        // `Arc<winit::window::Window>` to an overlay each frame.
        let raw_window_arc = window.clone_raw_window_arc();
        if let Ok(winit_arc) = raw_window_arc.downcast::<winit::window::Window>() {
            runtime.resources.insert(winit_arc);
        }

        // The closure gets the event loop opaquely, to build windowing-aware
        // resources (an egui overlay) without the SDK knowing. Heavy work is
        // filed with the engine's families and waited on through `Readiness`.
        let Some(bootstrap) = self.bootstrap.take() else {
            log::error!("WinitAppRunner: no bootstrap to run");
            return;
        };
        start_engine(&mut self.engine, runtime, |runtime| {
            bootstrap(window.as_khora_window(), runtime, event_loop as &dyn Any)
        });

        // Cache renderer for resize handling.
        if let Some(rs) = self
            .engine
            .runtime()
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
        {
            self.renderer = Some(rs.clone());
        }

        let raw_window = window.clone_raw_window_arc();
        self.window = Some(window);

        // The first frame is painted while the window is still hidden, then
        // the window is shown: what appears is that frame — a tool's loading
        // film, say — never an empty window.
        self.run_frame();
        if let Ok(winit_window) = raw_window.downcast::<winit::window::Window>() {
            winit_window.set_visible(true);
            // The taskbar reads the window's icon when the window appears and
            // may get nothing back yet: setting the title again makes it read
            // the icon.
            winit_window.set_title(&winit_window.title());
            winit_window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // An event the app consumed (an egui overlay's) never becomes game
        // input; close, resize and redraw are acted on regardless.
        let app = self.engine.app_mut();
        let consumed_by_app = if let (Some(app), Some(window)) = (app, &self.window) {
            app.intercept_window_event(&event as &dyn Any, window.as_khora_window())
        } else {
            false
        };

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Shutdown requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_ref() {
                    log::info!("Window resized: {}x{}", size.width, size.height);
                    if let Ok(mut r) = renderer.lock() {
                        r.resize(size.width, size.height);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.run_frame();
            }
            _ => {
                if consumed_by_app {
                    return;
                }
                if let Some(window) = &self.window {
                    if let Some(input_event) = window.translate_event(&event) {
                        self.engine.feed_input(input_event);
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl<W: WindowProvider, A: EngineApp> Drop for WinitAppRunner<W, A> {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

/// Builds the application, runs `first` over `runtime`, then bootstraps `engine` with both.
///
/// The app is built first so a tool's logger, installed by its `new`, catches
/// what `first` logs.
pub(crate) fn start_engine<A: EngineApp>(
    engine: &mut EngineCore<A>,
    mut runtime: khora_core::Runtime,
    first: impl FnOnce(&mut khora_core::Runtime),
) {
    let app = A::new();
    first(&mut runtime);
    engine.bootstrap(app, runtime);
}

// ─────────────────────────────────────────────────────────────────────
// run_winit — entry point for winit-based applications
// ─────────────────────────────────────────────────────────────────────

/// Runs the app `A` in a window opened through `W`, until the window closes.
///
/// `bootstrap` runs once, before the engine boots, with the window, the runtime
/// to install backends into, and the active event loop as `&dyn Any` (downcast
/// to `winit::event_loop::ActiveEventLoop`). It blocks, so it is the last thing
/// `main` does. See the book: reference/sdk.md, "`run_winit` — the entry point".
///
/// # Errors
///
/// When the winit event loop cannot be created, or ends in an error.
///
/// # Panics
///
/// With [`WinitWindowProvider`], when the platform refuses to open the window;
/// when the engine cannot boot, as [`EngineCore::bootstrap`](crate::EngineCore::bootstrap) says; and when a frame's passes cannot be submitted, as
/// [`EngineCore::submit_passes`](crate::EngineCore::submit_passes) says.
///
/// # Examples
///
/// ```rust,no_run
/// use khora_sdk::prelude::*;
/// use khora_sdk::{
///     run_winit, EngineApp, GameWorld, PhaseProvider, Runtime, WindowConfig,
/// };
/// use khora_sdk::winit_adapters::WinitWindowProvider;
///
/// struct MyGame;
///
/// impl EngineApp for MyGame {
///     fn window_config() -> WindowConfig { WindowConfig::default() }
///     fn new() -> Self { MyGame }
///     fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
///     fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
/// }
/// impl PhaseProvider for MyGame {}
///
/// fn main() -> anyhow::Result<()> {
///     run_winit::<WinitWindowProvider, MyGame>(|_window, runtime, _event_loop| {
///         // Insert your renderer, physics, audio, and UI backends into
///         // `runtime.backends` / `runtime.resources` here. See the `sandbox`
///         // example for a full backend wiring.
///         let _ = runtime;
///     })
/// }
/// ```
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any) + 'static,
) -> Result<()> {
    log::info!("Khora Engine: Starting...");

    let event_loop = EventLoop::new()?;
    let mut runner = WinitAppRunner::<W, A>::new(bootstrap);
    event_loop.run_app(&mut runner)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::start_engine;
    use crate::{
        EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
    };

    thread_local! {
        /// The steps of the start, in the order they ran on this thread.
        static ORDER: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn note(step: &'static str) {
        ORDER.with(|order| order.borrow_mut().push(step));
    }

    /// What `first` leaves in the runtime, for the engine to boot with.
    struct FilledByFirst;

    /// An app that notes when it is built and when it is set up.
    struct Ordered;

    impl PhaseProvider for Ordered {}
    impl EngineApp for Ordered {
        fn window_config() -> WindowConfig {
            WindowConfig::default()
        }
        fn new() -> Self {
            note("new");
            Ordered
        }
        fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {
            note("setup");
        }
        fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    }

    /// **The app is built before `first`, and its `setup` runs last.**
    ///
    /// A tool's logger, installed by its `new`, then catches what `first` logs;
    /// the engine boots with what `first` put in the runtime.
    #[test]
    fn the_app_is_built_before_first() {
        ORDER.with(|order| order.borrow_mut().clear());
        let mut engine = EngineCore::<Ordered>::new();

        start_engine(&mut engine, Runtime::new(), |runtime| {
            note("first");
            runtime.resources.insert(std::sync::Arc::new(FilledByFirst));
        });

        assert_eq!(
            ORDER.with(|order| order.borrow().clone()),
            vec!["new", "first", "setup"],
            "the app is built, then `first` runs, then the engine boots"
        );
        assert!(
            engine
                .runtime()
                .resources
                .get::<std::sync::Arc<FilledByFirst>>()
                .is_some(),
            "the engine boots with the runtime `first` filled"
        );
        engine.shutdown();
    }
}
