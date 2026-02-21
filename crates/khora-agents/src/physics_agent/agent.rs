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
//!
//! This agent implements the full GORNA protocol to negotiate resource budgets
//! with the DCC and adapt physics simulation quality based on system constraints.

use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use khora_core::agent::Agent;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::physics::PhysicsProvider;
use khora_core::telemetry::event::TelemetryEvent;
use khora_core::telemetry::monitoring::GpuReport;
use khora_data::ecs::World;
use khora_lanes::physics_lane::{PhysicsLane, StandardPhysicsLane};
use khora_telemetry::metrics::registry::{GaugeHandle, MetricsRegistry};

/// Scale factor to convert complexity units to estimated milliseconds.
const COST_TO_MS_SCALE: f32 = 3.0;

/// Strategies for physics simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsStrategy {
    /// Standard high-precision physics.
    #[default]
    Standard,
    /// Simplified physics for low-power mode.
    Simplified,
    /// Debug visualization mode.
    Debug,
}

/// Holds telemetry handles for the physics subsystem.
struct PhysicsMetrics {
    /// Gauge for tracking active rigid bodies.
    body_count: GaugeHandle,
    /// Gauge for tracking active colliders.
    collider_count: GaugeHandle,
    /// Gauge for tracking simulation step duration.
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
    /// Current GORNA strategy ID.
    current_strategy: StrategyId,
    /// Telemetry metrics.
    metrics: Option<PhysicsMetrics>,
    /// Sender for DCC telemetry events.
    telemetry_sender: Option<Sender<TelemetryEvent>>,
    /// Duration of the last physics step.
    last_step_time: Duration,
    /// Time budget allocated by GORNA.
    time_budget: Duration,
    /// Total number of frames simulated.
    frame_count: u64,
    /// Fixed timestep for physics simulation.
    fixed_timestep: f32,
}

impl Agent for PhysicsAgent {
    fn id(&self) -> AgentId {
        AgentId::Physics
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let body_count = self.provider.get_all_bodies().len() as u64;
        let complexity_factor = 1.0 + (body_count as f32 / 100.0).min(5.0);

        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_secs_f32(
                        (0.5 * complexity_factor * COST_TO_MS_SCALE).max(0.1) / 1000.0,
                    ),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_secs_f32(
                        (1.5 * complexity_factor * COST_TO_MS_SCALE).max(0.5) / 1000.0,
                    ),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_secs_f32(
                        (3.0 * complexity_factor * COST_TO_MS_SCALE).max(1.0) / 1000.0,
                    ),
                    estimated_vram: 0,
                },
            ],
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!(
            "PhysicsAgent: Strategy update to {:?} (time_limit={:?})",
            budget.strategy_id,
            budget.time_limit,
        );

        match budget.strategy_id {
            StrategyId::LowPower => {
                self.strategy = PhysicsStrategy::Simplified;
                self.fixed_timestep = 1.0 / 30.0;
            }
            StrategyId::Balanced => {
                self.strategy = PhysicsStrategy::Standard;
                self.fixed_timestep = 1.0 / 60.0;
            }
            StrategyId::HighPerformance => {
                self.strategy = PhysicsStrategy::Standard;
                self.fixed_timestep = 1.0 / 120.0;
            }
            StrategyId::Custom(_) => {
                log::warn!(
                    "PhysicsAgent received unsupported custom strategy. Falling back to Standard."
                );
                self.strategy = PhysicsStrategy::Standard;
                self.fixed_timestep = 1.0 / 60.0;
            }
        }

        self.current_strategy = budget.strategy_id;
        self.time_budget = budget.time_limit;
    }

    fn update(&mut self, context: &mut khora_core::EngineContext<'_>) {
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                self.step(world, self.fixed_timestep);
            }
        }
        self.emit_telemetry();
    }

    fn report_status(&self) -> AgentStatus {
        let health_score = if self.time_budget.is_zero() || self.frame_count == 0 {
            1.0
        } else {
            let ratio =
                self.time_budget.as_secs_f32() / self.last_step_time.as_secs_f32().max(0.0001);
            ratio.min(1.0)
        };

        let body_count = self.provider.get_all_bodies().len();
        let collider_count = self.provider.get_all_colliders().len();

        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: self.frame_count == 0,
            message: format!(
                "step_time={:.2}ms bodies={} colliders={}",
                self.last_step_time.as_secs_f32() * 1000.0,
                body_count,
                collider_count,
            ),
        }
    }

    fn execute(&mut self) {
        // Physics step is performed in update() via the tactical coordination.
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
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
            current_strategy: StrategyId::Balanced,
            metrics: None,
            telemetry_sender: None,
            last_step_time: Duration::ZERO,
            time_budget: Duration::ZERO,
            frame_count: 0,
            fixed_timestep: 1.0 / 60.0,
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

    /// Attaches a DCC sender for telemetry events.
    pub fn with_dcc_sender(mut self, sender: Sender<TelemetryEvent>) -> Self {
        self.telemetry_sender = Some(sender);
        self
    }

    /// Advances the physics simulation.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        let start = Instant::now();

        match self.strategy {
            PhysicsStrategy::Standard | PhysicsStrategy::Simplified => {
                self.lanes[0].step(world, self.provider.as_mut(), dt);
            }
            PhysicsStrategy::Debug => {
                if self.lanes.len() > 1 {
                    self.lanes[1].step(world, self.provider.as_mut(), dt);
                } else {
                    self.lanes[0].step(world, self.provider.as_mut(), dt);
                }
            }
        }

        self.last_step_time = start.elapsed();
        self.frame_count += 1;

        if let Some(metrics) = &self.metrics {
            let _ = metrics
                .body_count
                .set(self.provider.get_all_bodies().len() as f64);
            let _ = metrics
                .collider_count
                .set(self.provider.get_all_colliders().len() as f64);
            let _ = metrics
                .step_time_ms
                .set(self.last_step_time.as_secs_f64() * 1000.0);
        }
    }

    fn emit_telemetry(&self) {
        if let Some(sender) = &self.telemetry_sender {
            let report = GpuReport {
                frame_number: self.frame_count,
                draw_calls: 0,
                triangles_rendered: 0,
                ..Default::default()
            };
            let _ = sender.send(TelemetryEvent::GpuReport(report));
        }
    }

    /// Selects the appropriate physics lane based on the current strategy.
    pub fn select_lane(&self) -> &dyn PhysicsLane {
        match self.strategy {
            PhysicsStrategy::Standard | PhysicsStrategy::Simplified => self
                .find_lane_by_name("StandardPhysics")
                .unwrap_or_else(|| self.lanes.first().map(|b| b.as_ref()).unwrap()),
            PhysicsStrategy::Debug => self
                .find_lane_by_name("PhysicsDebug")
                .unwrap_or_else(|| self.lanes.first().map(|b| b.as_ref()).unwrap()),
        }
    }

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

    /// Returns the duration of the last step.
    pub fn last_step_time(&self) -> Duration {
        self.last_step_time
    }

    /// Returns the total number of frames simulated.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Returns the current GORNA strategy ID.
    pub fn current_strategy_id(&self) -> StrategyId {
        self.current_strategy
    }

    /// Returns the current fixed timestep.
    pub fn fixed_timestep(&self) -> f32 {
        self.fixed_timestep
    }

    /// Sets the fixed timestep.
    pub fn set_fixed_timestep(&mut self, dt: f32) {
        self.fixed_timestep = dt;
    }
}
