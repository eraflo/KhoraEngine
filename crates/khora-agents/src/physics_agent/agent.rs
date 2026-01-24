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

//! Defines the PhysicsAgent, the central orchestrator for the physics subsystem.

use khora_core::physics::PhysicsProvider;
use khora_data::ecs::World;
use khora_lanes::physics_lane::{PhysicsLane, StandardPhysicsLane};

/// Strategies for physics simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsStrategy {
    /// Standard high-precision physics.
    #[default]
    Standard,
}

/// The agent responsible for managing the physics simulation.
///
/// It acts as the Control Plane (ISA) for the physics subsystem,
/// deciding which strategies (lanes) to deploy and managing the physics world.
pub struct PhysicsAgent {
    /// The concrete physics solver provider.
    provider: Box<dyn PhysicsProvider>,
    /// Available physics lanes (strategies).
    lanes: Vec<Box<dyn PhysicsLane>>,
    /// Current selected strategy.
    strategy: PhysicsStrategy,
}

impl PhysicsAgent {
    /// Creates a new `PhysicsAgent` with a given provider.
    pub fn new(provider: Box<dyn PhysicsProvider>) -> Self {
        let lanes: Vec<Box<dyn PhysicsLane>> = vec![Box::new(StandardPhysicsLane::new())];

        Self {
            provider,
            lanes,
            strategy: PhysicsStrategy::Standard,
        }
    }

    /// Advances the physics simulation.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        let strategy = self.strategy;
        match strategy {
            PhysicsStrategy::Standard => {
                // By indexing directly, we borrow only self.lanes,
                // allowing a simultaneous mutable borrow of self.provider.
                self.lanes[0].step(world, self.provider.as_mut(), dt);
            }
        }
    }

    /// Selects the appropriate physics lane based on the current strategy.
    pub fn select_lane(&self) -> &dyn PhysicsLane {
        match self.strategy {
            PhysicsStrategy::Standard => self
                .find_lane_by_name("StandardPhysics")
                .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap()),
        }
    }

    /// Finds a lane by its strategy name.
    fn find_lane_by_name(&self, name: &str) -> Option<&dyn PhysicsLane> {
        self.lanes
            .iter()
            .find(|lane| lane.strategy_name() == name)
            .map(|boxed| boxed.as_ref())
    }
}
