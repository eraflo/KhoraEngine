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
    /// Runs the frame boundary, then the app's `update` amid the data layer's
    /// invariants.
    ///
    /// The second stage: call between [`drain_inputs`](Self::drain_inputs) and
    /// [`begin_render_frame`](Self::begin_render_frame). Families switched on or off
    /// before it, in [`before_frame`](EngineApp::before_frame) or earlier, hold for
    /// the whole frame; a later request counts from the next one.
    /// See the book: concepts/the-frame.md, "The six-stage descent".
    pub fn run_app_update(&mut self, inputs: &[InputEvent]) {
        self.settle_activation();
        // The scheduler's frame start comes before the frame's first DataSystem:
        // it hands the `Boundary` systems what the background runs staged.
        if let Some(scheduler) = self.scheduler.as_mut() {
            scheduler.begin_frame(&self.runtime);
        }
        let (Some(app), Some(gw)) = (self.app.as_mut(), self.game_world.as_mut()) else {
            return;
        };
        let runtime = &self.runtime;

        // These phases run before the lanes fill the deck: a system reading it
        // here finds an empty one.
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

        // Input-driven mutations and scene events the app's update must see.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PreSimulation,
        );

        app.update(gw, inputs);

        // Hierarchy fix-ups such as transform propagation: after the update
        // moves `Transform`s, before extraction reads `GlobalTransform`.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            runtime,
            &mut deck,
            TickPhase::PostSimulation,
        );

        // GPU uploads, such as the meshes'. The Flows project the scene later,
        // in the scheduler's Substrate Pass.
        substrate::run_data_systems(
            gw.inner_world_mut(),
            &self.runtime,
            &mut deck,
            TickPhase::PreExtract,
        );
    }

    /// Settles the families switched on or off, empties the mail of those off, and
    /// tells the DCC when that set changed.
    ///
    /// See the book: reference/sdk.md, "Activation — which families run".
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
