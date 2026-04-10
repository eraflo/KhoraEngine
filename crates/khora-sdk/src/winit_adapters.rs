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

/// Winit-specific application runner.
pub struct WinitAppRunner<W: WindowProvider, A: EngineApp> {
    window: Option<W>,
    engine: EngineCore<A>,
    renderer: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    bootstrap: Option<Box<dyn FnOnce(&dyn KhoraWindow, &mut khora_core::ServiceRegistry) + Send>>,
    tokio_runtime: Option<tokio::runtime::Runtime>,
    /// Per-frame context, recreated each frame.
    frame_context: Option<Arc<FrameContext>>,
}

impl<W: WindowProvider, A: EngineApp> WinitAppRunner<W, A> {
    /// Creates a new winit app runner with the given bootstrap closure.
    pub fn new(
        bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::ServiceRegistry) + Send + 'static,
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
}

impl<W: WindowProvider, A: EngineApp> ApplicationHandler for WinitAppRunner<W, A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Khora Engine: Initializing...");

        let window_config = A::window_config();
        let window = W::create(event_loop as &dyn Any, &window_config);

        // Build service registry
        let mut services = khora_core::ServiceRegistry::new();

        // Call bootstrap closure
        let bootstrap = self.bootstrap.take().expect("bootstrap not set");
        bootstrap(window.as_khora_window(), &mut services);

        // Create and bootstrap the engine
        let services_arc = Arc::new(services);
        let app = A::new();
        self.engine.bootstrap(app, Arc::clone(&services_arc));
        self.engine.set_services(Arc::clone(&services_arc));

        // Cache renderer for resize handling
        if let Some(rs) = self.engine.services().get::<Arc<Mutex<Box<dyn RenderSystem>>>>() {
            self.renderer = Some(rs.clone());
        }

        self.window = Some(window);

        // Create the tokio runtime for hot-path async tasks.
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
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
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
                // Create the per-frame context if tokio is available.
                if let Some(rt) = &self.tokio_runtime {
                    let fctx = FrameContext::new(rt.handle().clone());

                    // Build frame services with the FrameContext.
                    let mut frame_services = khora_core::ServiceRegistry::new();
                    frame_services.insert(Arc::clone(self.engine.services()));
                    frame_services.insert(PRIMARY_VIEWPORT);
                    let fctx_arc = Arc::new(fctx);
                    frame_services.insert(Arc::clone(&fctx_arc));

                    // Store for the renderer to access after tick.
                    self.frame_context = Some(fctx_arc);

                    // Run the engine tick with frame services.
                    let frame_services_arc = Arc::new(frame_services);
                    self.engine.tick_with_services(frame_services_arc);

                    // Wait for all hot-path tasks to complete before presenting.
                    if let Some(fctx) = &self.frame_context {
                        rt.block_on(fctx.wait_for_all());
                    }
                } else {
                    // Fallback: no tokio runtime, just tick normally.
                    self.engine.tick();
                }
            }
            _ => {
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
///
/// # Example
///
/// ```ignore
/// run_winit::<WinitWindowProvider, MyGame>(|window, services| {
///     let mut rs = WgpuRenderSystem::new();
///     rs.init(window)?;
///     services.insert(Arc::new(Mutex::new(rs as Box<dyn RenderSystem>)));
///     services.insert(Arc::new(MemoryMonitor::new("System_RAM")));
/// });
/// ```
pub fn run_winit<W: WindowProvider, A: EngineApp>(
    bootstrap: impl FnOnce(&dyn KhoraWindow, &mut khora_core::ServiceRegistry) + Send + 'static,
) -> Result<()> {
    log::info!("Khora Engine: Starting...");

    let event_loop = EventLoop::new()?;
    let mut runner = WinitAppRunner::<W, A>::new(bootstrap);
    event_loop.run_app(&mut runner)?;
    Ok(())
}
