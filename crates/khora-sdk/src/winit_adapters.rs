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

//! Winit-specific integration for the Khora Engine.
//!
//! This module provides the `run_winit` function that bridges winit's
//! `ApplicationHandler` with the winit-agnostic `EngineCore`.

use std::any::Any;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use khora_core::platform::KhoraWindow;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::RenderSystem;
use khora_infra::platform::winit::WinitWindow;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use crate::engine::{
    BootStep, EngineBootstrap, EngineCore, StagedBoot, Startup, StartupProgress, StartupStep,
    PRIMARY_VIEWPORT,
};
use crate::traits::{EngineApp, WindowProvider};
use crate::{InputEvent, WindowConfig};

// ─────────────────────────────────────────────────────────────────────
// WinitWindowProvider — concrete window provider
// ─────────────────────────────────────────────────────────────────────

/// A window provider backed by winit.
pub struct WinitWindowProvider {
    window: WinitWindow,
}

impl WindowProvider for WinitWindowProvider {
    fn create(native_loop: &dyn Any, config: &WindowConfig) -> Self
    where
        Self: Sized,
    {
        let event_loop = native_loop
            .downcast_ref::<ActiveEventLoop>()
            .expect("WindowProvider::create called with wrong native_loop type");

        let mut builder = khora_infra::platform::winit::WinitWindowBuilder::new()
            .with_title(&config.title)
            .with_dimensions(config.width, config.height);

        if let Some(icon) = &config.icon {
            builder = builder.with_icon_rgba(icon.rgba.clone(), icon.width, icon.height);
        }

        let window = builder.build(event_loop).expect("Failed to create window");
        Self { window }
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn inner_size(&self) -> (u32, u32) {
        self.window.inner_size()
    }

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn as_khora_window(&self) -> &dyn KhoraWindow {
        &self.window
    }

    fn clone_raw_window_arc(&self) -> Arc<dyn Any + Send + Sync> {
        self.window.clone_winit_arc()
    }

    fn translate_event(&self, raw_event: &dyn Any) -> Option<InputEvent> {
        if let Some(winit_event) = raw_event.downcast_ref::<WindowEvent>() {
            khora_infra::platform::winit::input::translate_winit_input(winit_event)
        } else {
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// WinitAppRunner — bridges winit ApplicationHandler with EngineCore
// ─────────────────────────────────────────────────────────────────────

/// One-shot bootstrap closure used to wire engine backends, services, and
/// resources into the [`khora_core::Runtime`] at window-creation time.
type BootstrapFn = Box<dyn FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any) + Send>;

/// Winit-specific application runner.
pub struct WinitAppRunner<W: WindowProvider, A: EngineApp> {
    window: Option<W>,
    engine: EngineCore<A>,
    renderer: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    bootstrap: Option<BootstrapFn>,
    /// A staged startup not begun yet (see [`run_winit_staged`]).
    staged: Option<StagedBoot>,
    /// A staged startup under way: one step per frame, loading frames between.
    booting: Option<Booting<A>>,
    tokio_runtime: Option<tokio::runtime::Runtime>,
    /// Per-frame context, recreated each frame.
    frame_context: Option<Arc<FrameContext>>,
}

/// What a staged startup works on: the application and the runtime until the
/// engine bootstrap takes them, then that bootstrap.
struct BootCtx<A: EngineApp> {
    app: Option<A>,
    runtime: Option<khora_core::Runtime>,
    stages: Option<EngineBootstrap<A>>,
}

impl<A: EngineApp> BootCtx<A> {
    /// The application and, while there is one, the runtime.
    fn app_and_runtime(&mut self) -> Option<(&mut A, Option<&khora_core::Runtime>)> {
        if let Some(stages) = self.stages.as_mut() {
            return Some(stages.app_and_runtime());
        }
        let runtime = self.runtime.as_ref();
        self.app.as_mut().map(|app| (app, runtime))
    }
}

/// A staged startup under way.
struct Booting<A: EngineApp> {
    /// The app's steps, then the engine bootstrap's `begin`, `setup` and
    /// `register`.
    steps: Startup<BootCtx<A>>,
    ctx: BootCtx<A>,
    /// The next agent to initialize, once the steps are done.
    next_agent: usize,
}

impl<A: EngineApp> Booting<A> {
    fn progress(&self) -> StartupProgress {
        let agents = self.ctx.stages.as_ref().map_or(0, |s| s.agent_count());
        let steps = self.steps_progress();
        StartupProgress {
            done: steps.done + self.next_agent,
            total: steps.total + agents,
        }
    }

    fn steps_progress(&self) -> StartupProgress {
        self.steps.progress()
    }
}

impl<W: WindowProvider, A: EngineApp> WinitAppRunner<W, A> {
    /// Creates a new winit app runner with the given bootstrap closure.
    pub fn new(
        bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any) + Send + 'static,
    ) -> Self {
        Self {
            window: None,
            engine: EngineCore::new(),
            renderer: None,
            bootstrap: Some(Box::new(bootstrap)),
            staged: None,
            booting: None,
            tokio_runtime: None,
            frame_context: None,
        }
    }

    /// Creates a runner whose startup is staged: `boot.first` as the window
    /// opens, then a step per frame while the application paints its loading
    /// frames.
    pub fn new_staged(boot: StagedBoot) -> Self {
        Self {
            window: None,
            engine: EngineCore::new(),
            renderer: None,
            bootstrap: None,
            staged: Some(boot),
            booting: None,
            tokio_runtime: None,
            frame_context: None,
        }
    }

    /// The renderer the runtime holds, kept for resizes and loading frames.
    fn cache_renderer(&mut self, runtime: &khora_core::Runtime) {
        if let Some(rs) = runtime.backends.get::<Arc<Mutex<Box<dyn RenderSystem>>>>() {
            self.renderer = Some(rs.clone());
        }
    }

    /// Presents one frame painted by the application's loading hook alone.
    fn present_loading_frame(&mut self) {
        let Some(booting) = self.booting.as_mut() else {
            return;
        };
        let progress = booting.progress();
        let Some(renderer) = self.renderer.clone() else {
            return;
        };
        if let Ok(mut rs) = renderer.lock() {
            rs.set_render_to_viewport(false);
            if let Err(e) = rs.begin_frame() {
                // A surface not ready (minimised, resizing) skips the frame.
                log::debug!("loading frame skipped: {e:?}");
                return;
            }
        }
        if let Some((app, Some(runtime))) = booting.ctx.app_and_runtime() {
            app.loading_frame(runtime, progress);
        }
        let presented = renderer.lock().map(|mut rs| rs.end_frame());
        if let Ok(Err(e)) = presented {
            log::warn!("loading frame present failed: {e:?}");
        }
    }

    /// Moves the staged startup on by one step; once it is over, hands the
    /// engine to the application.
    fn advance_startup(&mut self) {
        let Some(booting) = self.booting.as_mut() else {
            return;
        };
        if !booting.steps.is_done() {
            booting.steps.advance(&mut booting.ctx);
            return;
        }
        if let Some(stages) = booting.ctx.stages.as_mut() {
            if stages.initialize_agent(booting.next_agent) {
                booting.next_agent += 1;
                return;
            }
        }
        // Every step done and every agent initialized.
        let Some(booting) = self.booting.take() else {
            return;
        };
        if let Some(stages) = booting.ctx.stages {
            stages.finish(&mut self.engine);
        }
        let runtime = Arc::clone(self.engine.runtime());
        self.cache_renderer(&runtime);
        if let Some(app) = self.engine.app_mut() {
            app.startup_ready();
        }
        log::info!("Khora Engine: started.");
    }

    /// The staged counterpart of the one-shot bootstrap in `resumed`: runs
    /// `boot.first`, starts the rest and presents the first loading frame.
    /// The window is shown from its creation: a surface needs a shown window
    /// to offer its formats on some backends, so the renderer cannot be made
    /// before it is.
    fn begin_staged(
        &mut self,
        boot: StagedBoot,
        window: W,
        mut runtime: khora_core::Runtime,
        event_loop: &ActiveEventLoop,
    ) {
        // The application first: it may install what the rest reports through
        // (the editor's logger).
        let mut app = A::new();
        (boot.first)(
            window.as_khora_window(),
            &mut runtime,
            event_loop as &dyn Any,
        );
        self.cache_renderer(&runtime);
        app.startup_begin(&runtime);

        let mut steps: Vec<StartupStep<BootCtx<A>>> = Vec::new();
        for step in boot.steps {
            match step {
                BootStep::Main(f) => steps.push(StartupStep::Main(Box::new(move |ctx| {
                    if let Some(runtime) = ctx.runtime.as_mut() {
                        f(runtime);
                    }
                }))),
                BootStep::Blocking(prepare) => {
                    // Prepared now, from the runtime as it stands.
                    let work = prepare(&runtime);
                    steps.push(StartupStep::Blocking(Box::new(move || {
                        let apply = work();
                        Box::new(move |ctx: &mut BootCtx<A>| {
                            if let Some(runtime) = ctx.runtime.as_mut() {
                                apply(runtime);
                            }
                        })
                    })));
                }
            }
        }
        steps.push(StartupStep::Main(Box::new(|ctx: &mut BootCtx<A>| {
            if let (Some(app), Some(runtime)) = (ctx.app.take(), ctx.runtime.take()) {
                ctx.stages = Some(EngineBootstrap::begin(app, runtime));
            }
        })));
        steps.push(StartupStep::Main(Box::new(|ctx: &mut BootCtx<A>| {
            if let Some(stages) = ctx.stages.as_mut() {
                stages.setup();
            }
        })));
        steps.push(StartupStep::Main(Box::new(|ctx: &mut BootCtx<A>| {
            if let Some(stages) = ctx.stages.as_mut() {
                stages.register();
            }
        })));

        let ctx = BootCtx {
            app: Some(app),
            runtime: Some(runtime),
            stages: None,
        };
        let Some(handle) = self.tokio_runtime.as_ref().map(|rt| rt.handle().clone()) else {
            log::error!("WinitAppRunner: no tokio runtime; starting in one block");
            self.finish_in_one_block(steps, ctx);
            self.window = Some(window);
            return;
        };
        self.booting = Some(Booting {
            steps: Startup::new(steps, handle),
            ctx,
            next_agent: 0,
        });
        self.window = Some(window);
        self.present_loading_frame();
        if let Some(window) = self.window.as_ref() {
            if let Ok(winit_window) = window
                .clone_raw_window_arc()
                .downcast::<winit::window::Window>()
            {
                // The taskbar reads the icon as the window appears; setting
                // the title again makes it read it once the loop runs.
                winit_window.set_title(&winit_window.title());
            }
        }
    }

    /// Runs a staged startup's steps in a row — the fallback with no tokio
    /// runtime to run blocking work on.
    fn finish_in_one_block(&mut self, steps: Vec<StartupStep<BootCtx<A>>>, mut ctx: BootCtx<A>) {
        for step in steps {
            match step {
                StartupStep::Main(f) => f(&mut ctx),
                StartupStep::Blocking(work) => work()(&mut ctx),
            }
        }
        if let Some(mut stages) = ctx.stages.take() {
            let mut index = 0;
            while stages.initialize_agent(index) {
                index += 1;
            }
            stages.finish(&mut self.engine);
        }
        let runtime = Arc::clone(self.engine.runtime());
        self.cache_renderer(&runtime);
        if let Some(app) = self.engine.app_mut() {
            app.startup_ready();
        }
    }

    /// Runs one frame, interleaving the staged engine methods with the
    /// `EngineApp` lifecycle hooks. Sandbox-style apps (no overrides) get
    /// the same behavior as the legacy monolithic `tick`.
    fn run_frame(&mut self) {
        // The engine-level Runtime is immutable per-frame. The
        // `Arc<FrameContext>` registered at engine init has interior
        // mutability (its blackboard is a `Mutex<AnyMap>`), so each
        // frame's writes (`ColorTarget`, `DepthTarget`, …) overwrite the
        // previous frame's by replacing entries of the same type.
        let runtime_arc = Arc::clone(self.engine.runtime());
        if self.frame_context.is_none() {
            self.frame_context = runtime_arc.resources.get::<Arc<FrameContext>>().cloned();
        }

        // Stage 1: drain inputs (also marks simulation started + ticks telemetry).
        let inputs = self.engine.drain_inputs();

        // Hook: before_frame — overlay.begin_frame + shell.show_frame.
        if let Some(window) = self.window.as_ref() {
            let kwin = window.as_khora_window();
            self.engine.with_app_and_world(|app, world| {
                app.before_frame(world, &runtime_arc, kwin);
            });
        }

        // Stage 2: app.update + maintenance + extractions.
        self.engine.run_app_update(&inputs);

        // Stage 3: begin_frame on the renderer.
        let presents = self.engine.begin_render_frame(&runtime_arc);

        // Hook: before_agents — render offscreen viewport, set_render_to_viewport(true).
        self.engine.with_app_and_world(|app, world| {
            app.before_agents(world, &runtime_arc);
        });

        // Stage 4: scheduler dispatch.
        self.engine.run_scheduler(&runtime_arc);

        // Stage 5a: submit recorded agent passes. Done BEFORE `after_agents`
        // so editor overlay rendering (in `after_agents`) paints on top of
        // the 3D scene rather than getting overwritten by the agent submit.
        self.engine.submit_passes(presents);

        // Hook: after_agents — gizmos, set false, render_overlay.
        self.engine.with_app_and_world(|app, world| {
            app.after_agents(world, &runtime_arc);
        });

        // Stage 5b: present.
        self.engine.present_frame(presents);

        // Stage 6: end-of-tick Maintenance (writebacks, ECS compaction,
        // deferred cleanup). Without this, Maintenance-phase DataSystems
        // (`audio_playback_writeback`, `physics_world_writeback`,
        // `ecs_maintenance`) never run in the production winit flow.
        self.engine.run_maintenance();

        // Wait for hot-path tasks before returning so the next frame sees a
        // settled GPU state.
        if let Some(rt) = &self.tokio_runtime {
            if let Some(fctx) = &self.frame_context {
                rt.block_on(fctx.wait_for_all());
            }
        }
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

        // Create the tokio runtime for hot-path async tasks BEFORE
        // bootstrap so we can register an `Arc<FrameContext>` resource
        // into the runtime alongside the rest of the engine state.
        match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("khora-hotpath")
            .build()
        {
            Ok(rt) => {
                log::info!("WinitAppRunner: Tokio runtime created for hot-path tasks");
                self.tokio_runtime = Some(rt);
            }
            Err(e) => {
                log::warn!("WinitAppRunner: Failed to create tokio runtime: {}", e);
            }
        }

        // Build the runtime bundle and pre-populate it with windowing-tier
        // entries before handing off to the bootstrap closure.
        let mut runtime = khora_core::Runtime::new();

        // Scripting's resources, before the application's closure: an app
        // mounts its scripts there.
        crate::scripts::install(&mut runtime);

        // PRIMARY_VIEWPORT — engine-wide handle for the primary 3D viewport.
        runtime.resources.insert(PRIMARY_VIEWPORT);

        // Per-engine FrameContext (interior mutability: blackboard reused
        // across frames).
        if let Some(rt) = &self.tokio_runtime {
            let fctx = Arc::new(FrameContext::new(rt.handle().clone()));
            runtime.resources.insert(Arc::clone(&fctx));
            self.frame_context = Some(fctx);
        }

        // Insert a long-lived clone of the raw window handle so editor hooks
        // (e.g., overlay `begin_frame`) can retrieve `Arc<winit::window::Window>`
        // from resources and pass it to the overlay each frame.
        let raw_window_arc = window.clone_raw_window_arc();
        if let Ok(winit_arc) = raw_window_arc.downcast::<winit::window::Window>() {
            runtime.resources.insert(winit_arc);
        }

        if let Some(boot) = self.staged.take() {
            self.begin_staged(boot, window, runtime, event_loop);
            return;
        }

        // Call bootstrap closure — pass the active event loop opaquely so the
        // app may construct windowing-aware resources (e.g., an egui overlay
        // backed by `egui_winit::State`) without the SDK needing to know.
        let Some(bootstrap) = self.bootstrap.take() else {
            log::error!("WinitAppRunner: no bootstrap to run");
            return;
        };
        bootstrap(
            window.as_khora_window(),
            &mut runtime,
            event_loop as &dyn Any,
        );

        // Bootstrap the engine, transferring ownership of the runtime.
        // bootstrap() inserts built-in entries (AssetStore, etc.), wraps in
        // Arc, and stores the final runtime in self.engine.runtime.
        let app = A::new();
        self.engine.bootstrap(app, runtime);

        // Cache renderer for resize handling.
        if let Some(rs) = self
            .engine
            .runtime()
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
        {
            self.renderer = Some(rs.clone());
        }

        // The taskbar reads the window's icon when the window appears, while
        // the bootstrap above still holds this thread, and gets nothing back:
        // the button keeps the generic application icon. Setting the title
        // again, now that the loop is about to run, makes it read the icon.
        if let Ok(winit_window) = window
            .clone_raw_window_arc()
            .downcast::<winit::window::Window>()
        {
            winit_window.set_title(&winit_window.title());
        }

        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Give the app a chance to intercept raw events (e.g., forward to an
        // egui overlay). If consumed, do not forward to game input nor act on
        // the event further (close / resize / redraw still proceed below).
        // While a staged startup is under way the application still lives in
        // it, and its loading screen takes the events.
        let app = match self.booting.as_mut() {
            Some(booting) => booting.ctx.app_and_runtime().map(|(app, _)| app),
            None => self.engine.app_mut(),
        };
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
                if self.booting.is_some() {
                    self.present_loading_frame();
                    self.advance_startup();
                } else {
                    self.run_frame();
                }
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
        // A window closed while startup work still runs must not wait on it.
        if let Some(rt) = self.tokio_runtime.take() {
            rt.shutdown_timeout(std::time::Duration::from_secs(1));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// run_winit — entry point for winit-based applications
// ─────────────────────────────────────────────────────────────────────

/// Runs the Khora Engine with a winit event loop and a staged startup: the
/// window opens on the application's loading frames
/// ([`EngineApp::loading_frame`]) and the startup runs a step per frame
/// beneath them — `boot.first`, then `boot.steps` (blocking steps on tokio),
/// then the engine bootstrap one stage and one agent at a time — before
/// [`EngineApp::startup_ready`].
///
/// # Errors
///
/// Returns an error if the winit event loop cannot be created or fails while
/// running.
pub fn run_winit_staged<W: WindowProvider, A: EngineApp>(boot: StagedBoot) -> Result<()> {
    log::info!("Khora Engine: Starting (staged)...");

    let event_loop = EventLoop::new()?;
    let mut runner = WinitAppRunner::<W, A>::new_staged(boot);
    event_loop.run_app(&mut runner)?;
    Ok(())
}

/// Runs the Khora Engine with a winit event loop.
///
/// # Arguments
///
/// * `bootstrap` — A closure called once during initialization. Receives:
///   - `&dyn KhoraWindow` — the created window for renderer initialization
///   - `&mut ServiceRegistry` — the service registry to populate with
///     renderer, text renderer, layout system, monitors, etc.
///   - `&dyn Any` — the active winit event loop, opaque so the SDK does not
///     leak the winit type. Apps that need it (e.g., egui) downcast to
///     `&winit::event_loop::ActiveEventLoop`.
///
/// This call blocks until the window is closed, so it is the last thing `main`
/// does.
///
/// # Errors
///
/// Returns an error if the winit event loop cannot be created or fails while
/// running (e.g. the platform refuses to open a window).
///
/// # Examples
///
/// ```rust,no_run
/// use khora_sdk::prelude::*;
/// use khora_sdk::{
///     run_winit, AgentProvider, DccService, EngineApp, GameWorld, PhaseProvider,
///     Runtime, WindowConfig,
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
/// impl AgentProvider for MyGame {
///     fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
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
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any) + Send + 'static,
) -> Result<()> {
    log::info!("Khora Engine: Starting...");

    let event_loop = EventLoop::new()?;
    let mut runner = WinitAppRunner::<W, A>::new(bootstrap);
    event_loop.run_app(&mut runner)?;
    Ok(())
}
