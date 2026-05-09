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

//! Defines the PhysicsAgent — owns `LaneKind::Physics` lanes only.
//!
//! Per CLAD, an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA/strategy state.  The shared `PhysicsProvider` is fetched from
//! the [`ServiceRegistry`] each frame — agents are not the owners.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::PhysicsDeltaTime;
use khora_core::lane::{LaneContext, LaneRegistry, Slot};
use khora_core::physics::PhysicsProvider;
use khora_core::EngineContext;
use khora_data::ecs::World;
use khora_lanes::physics_lane::StandardPhysicsLane;

const COST_TO_MS_SCALE: f32 = 3.0;

/// Strategies for physics simulation.
///
/// Debug-overlay extraction was previously a third variant here, but it
/// is not a *strategy* of the same mission ("step the simulation") — it
/// is a side-channel projection of provider state into the `World`.
/// That work now lives in the
/// [`physics_debug_extraction`](khora_data::ecs::systems::physics_debug_extraction)
/// `DataSystem` so the simulation continues to step regardless of whether
/// the debug overlay is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsStrategy {
    /// Standard high-precision physics.
    #[default]
    Standard,
    /// Simplified physics for low-power mode.
    Simplified,
}

/// The agent responsible for managing the physics simulation.
///
/// Holds **only** its own strategy state — the `PhysicsProvider` is fetched
/// from `EngineContext::services` per frame.
pub struct PhysicsAgent {
    /// All physics lanes — the agent's strategies.
    lanes: LaneRegistry,
    /// Current selected strategy.
    strategy: PhysicsStrategy,
    /// Current GORNA strategy ID.
    current_strategy: StrategyId,
    /// Duration of the last physics step.
    last_step_time: Duration,
    /// Time budget allocated by GORNA.
    time_budget: Duration,
    /// Total frames simulated.
    frame_count: u64,
    /// Fixed timestep for physics simulation.
    fixed_timestep: f32,
    /// Number of `execute` invocations attempted.
    execute_attempts: u64,
}

impl Agent for PhysicsAgent {
    fn id(&self) -> AgentId {
        AgentId::Physics
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // We don't have access to the physics provider here (negotiate runs
        // on the DCC thread without an EngineContext).  Use a flat
        // complexity factor — refined estimates can come later via the
        // provider once GORNA supports it.
        let complexity_factor = 1.0_f32;

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
            timing_adjustment: None,
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

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        self.execute_attempts += 1;

        // Look up the physics provider from services every frame.
        let Some(provider_arc) = context
            .runtime
            .backends
            .get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()
        else {
            log::debug!("PhysicsAgent: no physics provider registered, skipping step");
            return;
        };
        let provider_arc: Arc<Mutex<Box<dyn PhysicsProvider>>> = (*provider_arc).clone();

        let Some(world_any) = context.world.as_deref_mut() else {
            return;
        };
        let Some(world) = world_any.downcast_mut::<World>() else {
            return;
        };

        let start = Instant::now();

        let mut provider_guard = match provider_arc.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("PhysicsAgent: provider mutex poisoned: {}", e);
                return;
            }
        };

        let mut ctx = LaneContext::new();
        ctx.insert(PhysicsDeltaTime(self.fixed_timestep));
        ctx.insert(Slot::new(world));
        ctx.insert(Slot::new(provider_guard.as_mut()));

        // Both strategies dispatch the same lane today; LowPower simply
        // tightens the fixed_timestep via apply_budget. A future
        // `SimplifiedPhysicsLane` strategy could fork here.
        let lane_name = "StandardPhysics";

        if let Some(lane) = self.lanes.get(lane_name) {
            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("Physics lane {} failed: {}", lane.strategy_name(), e);
            }
        }

        self.last_step_time = start.elapsed();
        self.frame_count += 1;
    }

    fn report_status(&self) -> AgentStatus {
        let health_score = if self.time_budget.is_zero() || self.frame_count == 0 {
            1.0
        } else {
            let ratio =
                self.time_budget.as_secs_f32() / self.last_step_time.as_secs_f32().max(0.0001);
            ratio.min(1.0)
        };

        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: self.execute_attempts > 0 && self.frame_count == 0,
            message: format!(
                "step_time={:.2}ms",
                self.last_step_time.as_secs_f32() * 1000.0,
            ),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            priority: 0.9,
            importance: AgentImportance::Critical,
            fixed_timestep: Some(Duration::from_secs_f32(self.fixed_timestep)),
            dependencies: Vec::new(),
        }
    }
}

impl Default for PhysicsAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(StandardPhysicsLane::new()));
        // PhysicsDebugLane was previously registered here; debug overlay
        // extraction now lives in the `physics_debug_extraction` DataSystem
        // (PostSimulation phase), running alongside the sim instead of
        // replacing it.

        Self {
            lanes,
            strategy: PhysicsStrategy::Standard,
            current_strategy: StrategyId::Balanced,
            last_step_time: Duration::ZERO,
            time_budget: Duration::ZERO,
            frame_count: 0,
            fixed_timestep: 1.0 / 60.0,
            execute_attempts: 0,
        }
    }
}
