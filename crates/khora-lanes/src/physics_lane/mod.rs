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

//! Physics Lane.
//!
//! After the Phase A refactor, `StandardPhysicsLane` is a thin wrapper
//! around [`PhysicsProvider::step`]: it does **no** World queries and
//! **no** structural mutations. The surrounding work is split into three
//! Substrate-Pass stages:
//!
//! 1. [`khora_data::flow::PhysicsFlow::adapt`] — AGDF detach/reattach
//!    plus the `ECS → provider` sync (`sync_to_world`-equivalent) that
//!    feeds the simulation its inputs.
//! 2. **This lane** — `provider.step(dt)`. Pure simulation, no World
//!    access at all.
//! 3. [`khora_data::ecs::systems::physics_world_writeback`] — pulls the
//!    new transforms, kinematic results, and collision events back into
//!    the World during the `Maintenance` phase.

mod native_lanes;

pub use native_lanes::*;

use khora_core::physics::PhysicsProvider;

/// The standard physics lane for industrial-grade simulation.
#[derive(Debug, Default)]
pub struct StandardPhysicsLane;

impl StandardPhysicsLane {
    /// Creates a new `StandardPhysicsLane`.
    pub fn new() -> Self {
        Self
    }
}

impl khora_core::lane::Lane for StandardPhysicsLane {
    fn strategy_name(&self) -> &'static str {
        "StandardPhysics"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Physics
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, OutputDeck, Slot};

        let dt = ctx
            .get::<khora_core::lane::PhysicsDeltaTime>()
            .ok_or(LaneError::missing("PhysicsDeltaTime"))?
            .0;
        let provider = ctx
            .get::<Slot<dyn PhysicsProvider>>()
            .ok_or(LaneError::missing("Slot<dyn PhysicsProvider>"))?
            .get();

        provider.step(dt);

        // Mark the simulation as having advanced this frame. The
        // `physics_world_writeback` DataSystem checks this slot and only
        // pulls fresh transforms from the provider when it's present —
        // unifying the Lane → Deck → DataSystem pattern across audio and
        // physics, and avoiding stale writebacks when the agent is paused.
        if let Some(deck_slot) = ctx.get::<Slot<OutputDeck>>() {
            *deck_slot
                .get()
                .slot::<khora_data::flow::PhysicsStepResult>() =
                khora_data::flow::PhysicsStepResult { dt };
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
