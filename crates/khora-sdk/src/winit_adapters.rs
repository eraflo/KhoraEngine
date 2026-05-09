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
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::traits::RenderSystem;
use khora_infra::platform::window::WinitWindow;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use crate::engine::{EngineCore, PRIMARY_VIEWPORT};
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

        let mut builder = khora_infra::platform::window::WinitWindowBuilder::new()
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
            khora_infra::platform::input::translate_winit_input(winit_event)
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
    tokio_runtime: Option<tokio::runtime::Runtime>,
    /// Per-frame context, recreated each frame.
    frame_context: Option<Arc<FrameContext>>,
}

impl<W: WindowProvider, A: EngineApp> WinitAppRunner<W, A> {
    /// Creates a new winit app runner with the given bootstrap closure.
    pub fn new(
        bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any)
            + Send
            + 'static,
    ) -> Self {
        Self {
            window: None,
            engine: EngineCore::new(),
            renderer: None,
            bootstrap: Some(Box::new(bootstrap)),
            tokio_runtime: None,
            frame_context: None,
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
            self.frame_context = runtime_arc
                .resources
                .get::<Arc<FrameContext>>()
                .cloned();
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

        // Call bootstrap closure — pass the active event loop opaquely so the
        // app may construct windowing-aware resources (e.g., an egui overlay
        // backed by `egui_winit::State`) without the SDK needing to know.
        let bootstrap = self.bootstrap.take().expect("bootstrap not set");
        bootstrap(
            window.as_khora_window(),
            &mut runtime,
            event_loop as &dyn Any,
        );

        // Bootstrap the engine, transferring ownership of the runtime.
        // bootstrap() inserts built-in entries (GpuCache, etc.), wraps in
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

        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Give the app a chance to intercept raw events (e.g., forward to an
        // egui overlay). If consumed, do not forward to game input nor act on
        // the event further (close / resize / redraw still proceed below).
        let consumed_by_app =
            if let (Some(app), Some(window)) = (self.engine.app_mut(), &self.window) {
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

// ─────────────────────────────────────────────────────────────────────
// run_winit — entry point for winit-based applications
// ─────────────────────────────────────────────────────────────────────

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
/// # Example
///
/// ```ignore
/// run_winit::<WinitWindowProvider, MyGame>(|window, services, _event_loop| {
///     let mut rs = WgpuRenderSystem::new();
///     rs.init(window)?;
///     services.insert(Arc::new(Mutex::new(rs as Box<dyn RenderSystem>)));
///     services.insert(Arc::new(MemoryMonitor::new("System_RAM")));
/// });
/// ```
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::Runtime, &dyn Any)
        + Send
        + 'static,
) -> Result<()> {
    log::info!("Khora Engine: Starting...");

    let event_loop = EventLoop::new()?;
    let mut runner = WinitAppRunner::<W, A>::new(bootstrap);
    event_loop.run_app(&mut runner)?;
    Ok(())
}
