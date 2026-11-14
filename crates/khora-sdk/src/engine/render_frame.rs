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

//! The render part of a frame: acquiring the swapchain, running the
//! scheduler with the renderer prepared for the frame's camera, submitting
//! the recorded passes and presenting.

use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::{ClearColor, ColorTargetSize};
use khora_core::renderer::traits::RenderSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::{submit_frame_graph, SharedFrameGraph};
use std::sync::{Arc, Mutex};

use super::EngineCore;
use crate::traits::EngineApp;

impl<A: EngineApp> EngineCore<A> {
    /// Acquires the frame's image and publishes its `Target`, `ColorTargetSize`
    /// and `ClearColor` in the per-frame `FrameContext`.
    ///
    /// The third stage. Returns whether the frame presents: `false` when no
    /// renderer is registered, its lock is poisoned, or no image could be
    /// acquired. Pass the result to
    /// [`end_render_frame`](Self::end_render_frame).
    pub fn begin_render_frame(&mut self, frame_runtime_arc: &Arc<Runtime>) -> bool {
        let render_system = self
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        let fctx = frame_runtime_arc
            .resources
            .get::<Arc<khora_core::renderer::api::frame::FrameContext>>()
            .map(|arc| (*arc).clone());

        let Some(rs) = &render_system else {
            return false;
        };
        let Ok(mut guard) = rs.lock() else {
            return false;
        };
        // Acquiring the image blocks until the display gives one back: time
        // the frame waited, not worked.
        match self.frame_clock.wait(|| guard.begin_frame()) {
            Ok(targets) => {
                if let Some(fctx) = &fctx {
                    // What a lane declaring `#[frame] target` draws into.
                    fctx.insert(Target {
                        color: targets.color,
                        depth: targets.depth,
                        size: targets.size,
                        key: TargetKey {
                            color: targets.color_format,
                            depth: targets.depth_format,
                            samples: 1,
                        },
                    });
                    // What every camera's projection takes its aspect from.
                    fctx.insert(ColorTargetSize(targets.size));
                    fctx.insert(ClearColor(khora_core::math::LinearRgba::new(
                        0.1, 0.1, 0.15, 1.0,
                    )));
                }
                true
            }
            Err(e) => {
                // The context outlives the frame: what the last frame acquired
                // was presented, so no lane may draw into it again.
                if let Some(fctx) = &fctx {
                    forget_frame_targets(fctx);
                }
                // Fatal device errors (lost / out-of-memory) are surfaced loudly
                // so the host can decide to tear down; transient acquisition
                // skips (minimized window, surface reconfigure, timeout) are
                // expected and logged at debug to avoid per-frame spam.
                if e.is_fatal() {
                    log::error!("EngineCore: begin_frame fatal error: {}", e);
                } else {
                    log::debug!("EngineCore: begin_frame skipped this frame: {}", e);
                }
                false
            }
        }
    }

    /// Runs the scheduler's frame: the Substrate Pass, then every phase's layers.
    ///
    /// The fourth stage. The renderer is prepared with the frame's camera before
    /// any lane records a pass.
    /// See the book: concepts/the-frame.md, "The six-stage descent".
    pub fn run_scheduler(&mut self, frame_runtime_arc: &Arc<Runtime>) {
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        let render_system = self
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .cloned();
        if let Some(s) = self.scheduler.as_mut() {
            s.run_frame_with(gw.inner_world_mut(), frame_runtime_arc.clone(), |bus| {
                prepare_frame(render_system.as_ref(), bus)
            });
        }
    }

    /// Submits the passes the lanes recorded to the GPU, through the
    /// [`FrameGraph`](khora_data::render::FrameGraph).
    ///
    /// When `presents` is `false`, the recorded passes are discarded instead.
    ///
    /// # Panics
    ///
    /// Panics when the frame graph's lock is poisoned and there is something to
    /// discard (`presents` false) or to submit (`presents` true, a device
    /// installed).
    pub fn submit_passes(&mut self, presents: bool) {
        let device = self
            .runtime
            .backends
            .get::<Arc<dyn GraphicsDevice>>()
            .map(|arc| (*arc).clone());
        let frame_graph = self
            .runtime
            .resources
            .get::<SharedFrameGraph>()
            .map(|arc| (*arc).clone());

        // The lanes buffered their passes in the deck rather than lock the
        // shared graph. They join it in the slots' registered order, which the
        // graph's sort refines by the declared read/write edges.
        if let (Some(graph), Some(scheduler)) = (&frame_graph, self.scheduler.as_mut()) {
            let deck = scheduler.deck_mut();
            let recorded: Vec<_> = self
                .passes
                .iter()
                .filter_map(|pass| (pass.take)(deck))
                .collect();
            if !recorded.is_empty() {
                if let Ok(mut fg) = graph.lock() {
                    for pass in recorded {
                        fg.add_pass(pass.descriptor, pass.command_buffer);
                    }
                } else {
                    log::error!("submit_passes: FrameGraph mutex poisoned, dropping frame passes");
                }
            }
        }

        if presents {
            if let (Some(graph), Some(device)) = (&frame_graph, &device) {
                submit_frame_graph(graph, device.as_ref());
            }
        } else if let Some(graph) = &frame_graph {
            graph.lock().expect("FrameGraph mutex poisoned").clear();
        }
    }

    /// Presents the frame's image; does nothing when `presents` is `false`.
    pub fn present_frame(&mut self, presents: bool) {
        if !presents {
            return;
        }
        let render_system = self
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .map(|arc| (*arc).clone());
        if let Some(rs) = &render_system {
            if let Ok(mut guard) = rs.lock() {
                // Presenting may block on the display too.
                if let Err(e) = self.frame_clock.wait(|| guard.end_frame()) {
                    if e.is_fatal() {
                        log::error!("EngineCore: end_frame fatal error: {}", e);
                    } else {
                        log::debug!("EngineCore: end_frame skipped this frame: {}", e);
                    }
                }
            }
        }
    }

    /// Submits the recorded passes, then presents the frame.
    ///
    /// The fifth stage: [`submit_passes`](Self::submit_passes) then
    /// [`present_frame`](Self::present_frame); a driver that interleaves hooks
    /// between them calls the two itself.
    ///
    /// # Panics
    ///
    /// Panics as [`submit_passes`](Self::submit_passes) does.
    pub fn end_render_frame(&mut self, presents: bool) {
        self.submit_passes(presents);
        self.present_frame(presents);
    }
}

/// Prepares the renderer with the frame's camera, before any lane records a pass.
///
/// Reads the [`CameraView`] the camera flow put on the bus; an active camera
/// whose values make no image prepares nothing.
///
/// [`CameraView`]: khora_data::flow::CameraView
fn prepare_frame(
    render_system: Option<&Arc<Mutex<Box<dyn RenderSystem>>>>,
    bus: &khora_core::lane::LaneBus,
) {
    let Some(render_system) = render_system else {
        return;
    };
    let Some(Some(Ok(view))) = bus
        .get::<khora_data::flow::CameraView>()
        .map(|camera| camera.view.as_ref())
    else {
        return;
    };
    match render_system.lock() {
        Ok(mut render_system) => render_system.prepare_frame(view),
        Err(_) => log::error!("prepare_frame: RenderSystem mutex poisoned, frame not prepared"),
    }
}

/// Takes the frame's target out of `fctx`, for a frame that acquired none.
///
/// The size stays: cameras take their aspect from it, and the window kept its size.
fn forget_frame_targets(fctx: &khora_core::renderer::api::frame::FrameContext) {
    fctx.remove::<Target>();
}
