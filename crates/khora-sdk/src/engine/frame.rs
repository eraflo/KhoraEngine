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
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use khora_data::ecs::TickPhase;
use std::sync::Arc;

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
        if let (Some(profiles), Some(dcc)) = (self.profiles.as_mut(), self.dcc.as_ref()) {
            profiles.tick(dcc);
        }
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
