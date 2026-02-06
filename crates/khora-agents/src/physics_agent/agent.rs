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
use khora_telemetry::metrics::registry::{GaugeHandle, MetricsRegistry};

/// Strategies for physics simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsStrategy {
    /// Standard high-precision physics.
    #[default]
    Standard,
}

/// Holds telemetry handles for the physics subsystem.
struct PhysicsMetrics {
    body_count: GaugeHandle,
    collider_count: GaugeHandle,
    step_time_ms: GaugeHandle,
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
    /// Telemetry metrics.
    metrics: Option<PhysicsMetrics>,
}

impl PhysicsAgent {
    /// Creates a new `PhysicsAgent` with a given provider.
    pub fn new(provider: Box<dyn PhysicsProvider>) -> Self {
        let lanes: Vec<Box<dyn PhysicsLane>> = vec![
            Box::new(StandardPhysicsLane::new()),
            Box::new(khora_lanes::physics_lane::PhysicsDebugLane::new()),
        ];

        Self {
            provider,
            lanes,
            strategy: PhysicsStrategy::Standard,
            metrics: None,
        }
    }

    /// Attaches a metrics registry to the agent for observability.
    pub fn with_telemetry(mut self, registry: &MetricsRegistry) -> Self {
        let metrics = PhysicsMetrics {
            body_count: registry
                .register_gauge(
                    "physics",
                    "body_count",
                    "Total active rigid bodies",
                    "count",
                )
                .unwrap(),
            collider_count: registry
                .register_gauge(
                    "physics",
                    "collider_count",
                    "Total active colliders",
                    "count",
                )
                .unwrap(),
            step_time_ms: registry
                .register_gauge(
                    "physics",
                    "step_time_ms",
                    "Time spent in simulation step",
                    "ms",
                )
                .unwrap(),
        };
        self.metrics = Some(metrics);
        self
    }

    /// Advances the physics simulation.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        let start = std::time::Instant::now();

        let strategy = self.strategy;
        match strategy {
            PhysicsStrategy::Standard => {
                // By indexing directly, we borrow only self.lanes,
                // allowing a simultaneous mutable borrow of self.provider.
                self.lanes[0].step(world, self.provider.as_mut(), dt);
            }
        }

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        if let Some(metrics) = &self.metrics {
            let _ = metrics
                .body_count
                .set(self.provider.get_all_bodies().len() as f64);
            let _ = metrics
                .collider_count
                .set(self.provider.get_all_colliders().len() as f64);
            let _ = metrics.step_time_ms.set(elapsed);
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

    /// Exposes raycasting from the provider.
    pub fn cast_ray(
        &self,
        ray: &khora_core::physics::Ray,
        max_toi: f32,
        solid: bool,
    ) -> Option<khora_core::physics::RaycastHit> {
        self.provider.cast_ray(ray, max_toi, solid)
    }

    /// Returns debug rendering data from the provider.
    pub fn get_debug_render_data(&self) -> (Vec<khora_core::math::Vec3>, Vec<[u32; 2]>) {
        self.provider.get_debug_render_data()
    }
}
