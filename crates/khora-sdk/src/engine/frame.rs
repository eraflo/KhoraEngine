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
use khora_core::agent::gorna::protocol::FrameReport;
use khora_core::lane::cost::PerDevice;
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::{ClearColor, ColorTargetSize};
use khora_core::renderer::traits::RenderSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::telemetry::TelemetryEvent;
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
        self.close_frame();
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
        // The frame starts here, in every driver: from now until
        // `close_frame`, what is not a wait for the display is the frame's.
        self.frame_clock.open();
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
    ///
    /// It opens with the frame boundary: the families the app switched on or
    /// off — in its [`before_frame`](EngineApp::before_frame), or before — are
    /// settled, so every phase of the frame, the scheduler and the DCC see one
    /// state. A request made after counts from the next frame.
    pub fn run_app_update(&mut self, inputs: &[InputEvent]) {
        self.settle_activation();
        // The scheduler's frame start: the allocation lent, the staged layers
        // swapped in, and what the background runs that ended staged handed
        // to the reserve — all before the frame's first DataSystem.
        if let Some(scheduler) = self.scheduler.as_mut() {
            scheduler.begin_frame(&self.runtime);
        }
        let (Some(app), Some(gw)) = (self.app.as_mut(), self.game_world.as_mut()) else {
            return;
        };
        let runtime = &self.runtime;

        // Pre-scheduler phases run before the per-frame `OutputDeck` is
        // built. We pass a transient empty deck so `DataSystem`s that
        // happen to read it observe an empty slot — they simply no-op.
        let mut deck = khora_core::lane::OutputDeck::new();

        // The boundary: what the work that spans frames staged is applied
        // here, on the frame's World, before anything of the frame reads it.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::Boundary,
        );

        // What the startup waits on lands at the boundary: once all of it
        // has — or it waited too long — the app is told, once.
        if !self.startup_over && super::readiness::settle(runtime, std::time::Instant::now()) {
            self.startup_over = true;
            app.startup_ready();
        }

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
    /// populate the per-frame [`FrameContext`] with the frame's `Target`,
    /// its `ColorTargetSize` and its `ClearColor`.
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

    /// Stage 4 — dispatch the scheduler so all registered agents execute
    /// their phases for this frame.
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

    /// The frame boundary: makes the families requested on or off the state
    /// in force, empties the mail sent to a family while it was off — in the
    /// frame that ended or for the coming one — and tells the DCC when the
    /// families off changed, so the arbitration it runs on its own thread
    /// budgets only the families on.
    fn settle_activation(&mut self) {
        let Some(activation) = self
            .runtime
            .resources
            .get::<Arc<khora_core::agent::Activation>>()
            .cloned()
        else {
            return;
        };
        let was_off = activation.inactive();
        let was_scoped = activation.scopes();
        let changed = activation.settle();
        let mut off = activation.inactive();
        off.extend(was_off);
        khora_core::event::discard_mail_of(&off, &self.runtime);
        if changed {
            // A scope whose marker the world does not know works on nothing:
            // said once, when that scope takes effect.
            if let Some(world) = self.game_world.as_mut() {
                let components = world.inner_world().components();
                for (family, marker) in activation.scopes() {
                    if !was_scoped.contains(&(family, marker))
                        && components.key_named(marker).is_none()
                    {
                        log::warn!(
                            "the family `{family}` is scoped to `{marker}`, which no component is registered as: it works on no entity"
                        );
                    }
                }
            }
            if let Some(dcc) = &self.dcc {
                let _ = dcc.event_sender().send(
                    khora_core::telemetry::TelemetryEvent::ActivationChanged(activation.inactive()),
                );
            }
        }
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

        // Fold the agents' recorded passes (buffered in the scheduler's deck
        // instead of locking the shared graph) into the FrameGraph in the
        // order the slots' registrations declare. The topological sort
        // refines it by the declared read/write edges; insertion order is
        // the tie-breaker.
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

    /// Convenience: runs both [`submit_passes`](Self::submit_passes) and
    /// [`present_frame`](Self::present_frame) in order. Used by the default
    /// `tick` path; drivers that need to interleave hooks should call the
    /// staged methods directly.
    pub fn end_render_frame(&mut self, presents: bool) {
        self.submit_passes(presents);
        self.present_frame(presents);
    }

    /// Last stage — closes the frame's clock and, when it measured the
    /// frame, reports to the DCC how long the frame kept the host busy, the
    /// waits for the display excluded.
    pub fn close_frame(&mut self) {
        let Some(busy) = self.frame_clock.close() else {
            return;
        };
        let Some(dcc) = &self.dcc else {
            return;
        };
        // The GPU is not timed yet: its busy time stays unmeasured, never
        // zero. Best-effort, like the agents' reports.
        let report = FrameReport {
            busy: PerDevice {
                cpu: Some(busy),
                gpu: None,
                npu: None,
                other: None,
            },
        };
        let _ = dcc
            .event_sender()
            .try_send(TelemetryEvent::FrameReport(report));
    }
}

/// Prepares the renderer with the frame's camera, from the [`CameraView`] the
/// camera flow put on the bus: once a frame, before any agent records a pass.
/// An active camera whose values make no image prepares nothing.
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

/// Takes the frame's targets out of `fctx`, for a frame that acquired none.
/// The size stays: it is what cameras take their aspect from, and the
/// window's size has not changed.
fn forget_frame_targets(fctx: &khora_core::renderer::api::frame::FrameContext) {
    fctx.remove::<Target>();
}
