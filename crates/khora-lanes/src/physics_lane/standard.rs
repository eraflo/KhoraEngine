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

//! The standard physics strategy: one full step of the provider per frame.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{FixedStep, Locked};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::physics::{Collision, CollisionEvent, CollisionKind, PhysicsProvider};
use khora_data::flow::PhysicsStepResult;
use khora_macros::LaneInputs;

use super::physics_family::{PhysicsFamily, PhysicsOutputs};

/// The standard physics lane: one step of the provider by the agent's fixed
/// step, and the contacts that step reported.
#[derive(Debug, Default)]
pub struct StandardPhysicsLane;

/// The one level: a simulation family may not trade its result for time.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "standard",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// What one step costs on the host, as quoted before the backend reports
/// its own timings.
const STEP_COST: Duration = Duration::from_micros(500);

/// What the physics lane reads each frame: the provider, locked for the step,
/// and the step its agent runs it at.
#[derive(LaneInputs)]
pub struct PhysicsInputs<'f> {
    /// The simulation, locked while it steps.
    #[locked]
    pub provider: Locked<'f, Box<dyn PhysicsProvider>>,
    /// The simulated time this step advances.
    #[frame]
    pub step: FixedStep,
}

impl Lane for StandardPhysicsLane {
    type Family = PhysicsFamily;
    type Inputs<'f> = PhysicsInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &PhysicsInputs<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: STEP_COST,
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut PhysicsInputs<'_>,
        out: &mut PhysicsOutputs,
    ) -> Result<LaneReport, LaneError> {
        let dt = inputs.step.0.as_secs_f32();
        let provider = &mut **inputs.provider;
        provider.step(dt);

        // Resolved here, while the provider is in hand. A contact leaves the
        // backend naming two colliders; everything downstream wants two
        // entities, and asking the provider is `O(1)` per handle because the
        // owner is stamped on the collider itself.
        let contacts = provider
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
            });

        // The step marks the simulation as having advanced this frame: the
        // `physics_world_writeback` DataSystem pulls fresh transforms only
        // when it is there. The contacts open on what the frame's earlier
        // sub-steps reported, so every sub-step's reach the game.
        out.step = PhysicsStepResult { dt };
        out.contacts.contacts.extend(contacts);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}
