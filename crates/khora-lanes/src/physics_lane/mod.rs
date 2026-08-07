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

use khora_core::physics::{
    Collision, CollisionEvent, CollisionKind, ContactBatch, PhysicsProvider,
};

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

        // Resolved here, while the provider is in hand. A contact leaves the
        // backend naming two colliders; everything downstream wants two
        // entities, and asking the provider is `O(1)` per handle because the
        // owner is stamped on the collider itself. Leaving it to a consumer
        // would mean handing raw backend handles across the deck and hoping
        // they still resolve by the time somebody asks.
        let contacts: Vec<Collision> = provider
            .take_collision_events()
            .into_iter()
            .filter_map(|event| {
                let (kind, first, second) = match event {
                    CollisionEvent::Started(a, b) => (CollisionKind::Started, a, b),
                    CollisionEvent::Stopped(a, b) => (CollisionKind::Stopped, a, b),
                };
                // A contact involving a collider the ECS does not own — a query
                // volume, a tool's probe — has no entity to name and is not a
                // gameplay event. Dropped rather than reported half-resolved.
                Some(Collision {
                    kind,
                    a: provider.entity_of(first)?,
                    b: provider.entity_of(second)?,
                })
            })
            .collect();

        // Mark the simulation as having advanced this frame. The
        // `physics_world_writeback` DataSystem checks this slot and only
        // pulls fresh transforms from the provider when it's present —
        // unifying the Lane → Deck → DataSystem pattern across audio and
        // physics, and avoiding stale writebacks when the agent is paused.
        if let Some(deck_slot) = ctx.get::<Slot<OutputDeck>>() {
            let deck = deck_slot.get();
            *deck.slot::<khora_data::flow::PhysicsStepResult>() =
                khora_data::flow::PhysicsStepResult { dt };
            // Extended, not replaced: the agent runs this lane once per fixed
            // sub-step and there may be five in a frame, each with contacts of
            // its own. Replacing would keep only the last sub-step's.
            deck.slot::<ContactBatch>().contacts.extend(contacts);
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
