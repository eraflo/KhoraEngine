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
    /// Runs one frame over the engine-level runtime.
    ///
    /// A driver with per-frame resources, such as a `FrameContext`, calls
    /// [`tick_with_runtime`](Self::tick_with_runtime) instead.
    ///
    /// # Panics
    ///
    /// Panics as [`end_render_frame`](Self::end_render_frame) does.
    pub fn tick(&mut self) {
        let frame_runtime_arc = Arc::clone(&self.runtime);
        self.tick_with_runtime(frame_runtime_arc);
    }

    /// Runs one frame, its scheduler and render stages reading the per-frame
    /// [`Runtime`] `frame_runtime_arc`, which holds the `FrameContext`.
    ///
    /// Runs the staged methods in order with no [`EngineApp`] hook between
    /// them; a driver that interleaves hooks, as the editor's does, calls them
    /// itself. See the book: concepts/the-frame.md, "The six-stage descent".
    ///
    /// # Panics
    ///
    /// Panics as [`end_render_frame`](Self::end_render_frame) does.
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

    /// Runs the `Maintenance` data systems over the frame's
    /// [`OutputDeck`](khora_core::lane::OutputDeck).
    ///
    /// The sixth stage: each system drains its domain's slots of the deck, such
    /// as the physics and audio write-backs, into the world.
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
            // Without a scheduler there is no deck: the systems read an empty one.
            let mut deck = khora_core::lane::OutputDeck::new();
            substrate::run_data_systems(world, &self.runtime, &mut deck, TickPhase::Maintenance);
        }
    }

    /// Opens the frame and returns the input events queued since the last one.
    ///
    /// The first stage; it also ticks the telemetry service. The events are
    /// read, not taken: the [`InputMap`](khora_core::platform::InputMap) is
    /// derived from the same events later in the frame.
    pub fn drain_inputs(&mut self) -> Vec<InputEvent> {
        // The frame starts here, in every driver: from now until
        // `close_frame`, what is not a wait for the display is the frame's.
        self.frame_clock.open();
        if let Some(telemetry) = self.telemetry.as_mut() {
            let _ = telemetry.tick();
        }
        // Taking the events would leave the channel's other readers, such as
        // the `input_map_update` system, a stream that depends on who ran first.
        self.input_events.read_for("engine_frame")
    }

    /// Closes the frame's clock and reports to the DCC how long the frame kept
    /// the host busy.
    ///
    /// The last stage. The waits for the display are excluded; nothing is
    /// reported when the clock was not open.
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
