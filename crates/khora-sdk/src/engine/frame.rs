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

//! One frame, step by step: input, maintenance, the app's update, the
//! scheduler, the render passes and presentation.

use khora_control::substrate;
use khora_core::lane::{ClearColor, ColorTarget, DepthTarget};
use khora_core::renderer::traits::RenderSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::ecs::TickPhase;
use khora_data::render::{submit_frame_graph, SharedFrameGraph};
use std::sync::{Arc, Mutex};

use super::EngineCore;
use crate::traits::EngineApp;
use crate::InputEvent;

impl<A: EngineApp> EngineCore<A> {
    /// Executes one frame: app update, ECS maintenance, scheduler.
    ///
    /// This method is called by the windowing driver (e.g. winit)
    /// each time a redraw is requested.
    pub fn tick(&mut self) {
        // Default tick path: reuse the engine-level Runtime as-is. The
        // winit driver injects per-frame resources (`FrameContext`,
        // viewport handle) by calling `tick_with_runtime` directly.
        let frame_runtime_arc = Arc::clone(&self.runtime);
        self.tick_with_runtime(frame_runtime_arc);
    }

    /// Executes one frame with a custom per-frame [`Runtime`] overlay.
    ///
    /// Used by the winit runner to inject a `FrameContext` into the
    /// per-frame Resources for cross-agent synchronization.
    ///
    /// This is a convenience wrapper that calls the staged methods in order
    /// without invoking any [`EngineApp`] lifecycle hooks. Drivers that need
    /// to interleave hooks (e.g., the editor's overlay/shell) should call the
    /// staged methods directly via the winit runner.
    pub fn tick_with_runtime(&mut self, frame_runtime_arc: Arc<Runtime>) {
        let inputs = self.drain_inputs();
        self.run_app_update(&inputs);
        let presents = self.begin_render_frame(&frame_runtime_arc);
        self.run_scheduler(&frame_runtime_arc);
        self.end_render_frame(presents);
        self.run_maintenance();
    }

    /// Stage 6 — runs the end-of-tick `Maintenance`-phase `DataSystem`s
    /// against the scheduler's per-frame [`OutputDeck`].
    ///
    /// The engine is subsystem-agnostic here: it threads the deck through
    /// the substrate dispatcher and lets each `DataSystem` drain whatever
    /// typed slot belongs to its domain (audio writeback, physics
    /// writeback, future subsystems …). No subsystem-specific code lives
    /// in this method.
    pub fn run_maintenance(&mut self) {
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        let world = gw.inner_world_mut();

        if let Some(scheduler) = self.scheduler.as_mut() {
            substrate::run_data_systems(
                world,
                &self.runtime,
                scheduler.deck_mut(),
                TickPhase::Maintenance,
            );
        } else {
            // No scheduler installed — pass a transient empty deck so
            // `DataSystem`s that read it harmlessly observe an empty slot.
            let mut deck = khora_core::lane::OutputDeck::new();
            substrate::run_data_systems(world, &self.runtime, &mut deck, TickPhase::Maintenance);
        }
    }

    /// Stage 1 — drain queued input events. Also ticks the telemetry
    /// service, and feeds the events into the engine-wide
    /// [`khora_core::platform::InputMap`] so app code can query actions
    /// (`is_pressed`, `just_pressed`) from the same frame's inputs.
    pub fn drain_inputs(&mut self) -> Vec<InputEvent> {
        if let Some(telemetry) = self.telemetry.as_mut() {
            let _ = telemetry.tick();
        }
        // Read rather than drained: this is one reader among however many
        // declare the channel — the `input_map_update` system derives the
        // action map from the same events — and taking them would leave a
        // stream that empties itself depending on who ran first.
        //
        // Deriving `InputMap` is **not** done here. It is a tick invariant, so
        // it is a registered `DataSystem` in `PreSimulation`
        // (`khora_data::ecs::systems::input_map_update`), which is what
        // `RULES.md` §3 requires of engine-tick wiring and what the phase is
        // documented for.
        self.input_events.read_for("engine_frame")
    }

    /// Stage 2 — run `app.update`, ECS maintenance, mesh sync, and scene/UI
    /// extractions. Called between [`drain_inputs`](Self::drain_inputs) and
    /// [`begin_render_frame`](Self::begin_render_frame).
    pub fn run_app_update(&mut self, inputs: &[InputEvent]) {
        let (Some(app), Some(gw)) = (self.app.as_mut(), self.game_world.as_mut()) else {
            return;
        };
        let runtime = &self.runtime;

        // Pre-scheduler phases run before the per-frame `OutputDeck` is
        // built. We pass a transient empty deck so `DataSystem`s that
        // happen to read it observe an empty slot — they simply no-op.
        let mut deck = khora_core::lane::OutputDeck::new();

        // Substrate Pass — pre-simulation invariants (input-driven mutations,
        // scene events that must be visible to agents).
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PreSimulation,
        );

        app.update(gw, inputs);

        // Substrate Pass — post-simulation invariants (hierarchy fix-ups
        // such as transform_propagation, run after app.update mutates Transforms
        // but before extraction reads GlobalTransform).
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PostSimulation,
        );

        // Substrate Pass — pre-extract. Runs:
        //  - `gpu_mesh_sync` (CPU→GPU mesh upload, replaces former proj.sync_all)
        //  - any other PreExtract DataSystem registered by users.
        // RenderFlow + UiFlow then run inside the scheduler's Substrate Pass
        // and publish their views into the LaneBus.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            &self.runtime,
            &mut deck,
            TickPhase::PreExtract,
        );
    }

    /// Stage 3 — acquire the swapchain via `RenderSystem::begin_frame` and
    /// populate the per-frame [`FrameContext`] with `ColorTarget`,
    /// `DepthTarget`, and `ClearColor`.
    ///
    /// Returns `true` when a renderer is present and `begin_frame` succeeded
    /// (driver should later call [`present_frame`](Self::present_frame)).
    /// Returns `false` only when no renderer is registered or the swapchain
    /// could not be acquired.
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
        match guard.begin_frame() {
            Ok(targets) => {
                if let Some(fctx) = &fctx {
                    fctx.insert(ColorTarget(targets.color));
                    if let Some(d) = targets.depth {
                        fctx.insert(DepthTarget(d));
                    }
                    fctx.insert(ClearColor(khora_core::math::LinearRgba::new(
                        0.1, 0.1, 0.15, 1.0,
                    )));
                }
                true
            }
            Err(e) => {
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

    /// Stage 4 — dispatch the scheduler so all registered agents execute
    /// their phases for this frame.
    pub fn run_scheduler(&mut self, frame_runtime_arc: &Arc<Runtime>) {
        self.forward_mode(frame_runtime_arc);
        let Some(gw) = self.game_world.as_mut() else {
            return;
        };
        if let Some(s) = self.scheduler.as_mut() {
            s.run_frame(gw.inner_world_mut(), frame_runtime_arc.clone());
        }
    }

    /// Tells the DCC the frame's mode when it changed, so the arbitration it
    /// runs on its own thread budgets the agents this mode runs. The
    /// scheduler reads the mode itself, this frame; the DCC learns it as soon
    /// as its next tick.
    fn forward_mode(&mut self, runtime: &Runtime) {
        let Some(mode) = runtime
            .resources
            .get::<khora_core::agent::SharedEngineMode>()
            .and_then(|shared| shared.read().ok().map(|mode| mode.clone()))
        else {
            return;
        };
        if self.forwarded_mode.as_ref() == Some(&mode) {
            return;
        }
        if let Some(dcc) = &self.dcc {
            let _ = dcc
                .event_sender()
                .send(khora_core::telemetry::TelemetryEvent::ModeChange(
                    mode.clone(),
                ));
        }
        self.forwarded_mode = Some(mode);
    }

    /// Stage 5a — submit recorded passes from the [`FrameGraph`](khora_data::render::FrameGraph) to the GPU.
    /// When `presents` is `false` (render-to-viewport mode), the frame graph
    /// is discarded instead.
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

        // Fold the agents' recorded passes (buffered in the scheduler's deck by
        // Render/Skybox/Ui/Overlay instead of locking the shared graph) into the
        // FrameGraph in a fixed layer order — scene, then the skybox background,
        // then UI, then overlay compositing. The skybox sits right after the
        // scene (it loads the scene's depth to reject geometry pixels) and
        // before the debug overlays. The topological sort refines this via the
        // declared read/write edges; insertion order is the tie-breaker.
        if let (Some(graph), Some(scheduler)) = (&frame_graph, self.scheduler.as_mut()) {
            use khora_data::render::{
                OverlayPassSlot, ScenePassSlot, SkyboxPassSlot, TransparentPassSlot, UiPassSlot,
            };
            let deck = scheduler.deck_mut();
            let scene = deck.take::<ScenePassSlot>().0;
            let skybox = deck.take::<SkyboxPassSlot>().0;
            let transparent = deck.take::<TransparentPassSlot>().0;
            let ui = deck.take::<UiPassSlot>().0;
            let overlay = deck.take::<OverlayPassSlot>().0;
            if scene.is_some()
                || skybox.is_some()
                || transparent.is_some()
                || ui.is_some()
                || overlay.is_some()
            {
                if let Ok(mut fg) = graph.lock() {
                    for pass in [scene, skybox, transparent, ui, overlay]
                        .into_iter()
                        .flatten()
                    {
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

    /// Stage 5b — call `RenderSystem::end_frame` to present the swapchain.
    /// No-op when `presents` is `false`.
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
                if let Err(e) = guard.end_frame() {
                    if e.is_fatal() {
                        log::error!("EngineCore: end_frame fatal error: {}", e);
                    } else {
                        log::debug!("EngineCore: end_frame skipped this frame: {}", e);
                    }
                }
            }
        }
    }

    /// Convenience: runs both [`submit_passes`](Self::submit_passes) and
    /// [`present_frame`](Self::present_frame) in order. Used by the default
    /// `tick` path; drivers that need to interleave hooks should call the
    /// staged methods directly.
    pub fn end_render_frame(&mut self, presents: bool) {
        self.submit_passes(presents);
        self.present_frame(presents);
    }
}
