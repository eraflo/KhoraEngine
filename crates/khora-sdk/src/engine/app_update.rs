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

//! The app's part of a frame: the frame boundary settling which families
//! run, then the substrate phases around the application's update.

use khora_control::substrate;
use khora_data::ecs::TickPhase;
use std::sync::Arc;

use super::EngineCore;
use crate::traits::EngineApp;
use crate::InputEvent;

impl<A: EngineApp> EngineCore<A> {
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
}
