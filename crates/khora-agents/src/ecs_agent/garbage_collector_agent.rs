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

//! The Intelligent Subsystem Agent for garbage collection.
//!
//! This agent collects orphaned data locations, decides when and how much to clean
//! based on a strategy negotiated with the DCC via GORNA, and dispatches the work
//! to a `CompactionLane`.

use std::collections::VecDeque;
use std::time::Duration;

use crossbeam_channel::Sender;
use khora_core::agent::Agent;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::telemetry::event::TelemetryEvent;
use khora_data::ecs::{PageIndex, SemanticDomain, World, WorldMaintenance};
use khora_lanes::ecs_lane::{CompactionLane, GcWorkPlan};

const DEFAULT_MAX_CLEANUP_PER_FRAME: usize = 10;
const HIGH_PERFORMANCE_CLEANUP_MULTIPLIER: usize = 3;
const LOW_POWER_CLEANUP_DIVISOR: usize = 4;

/// The agent responsible for garbage collection of orphaned ECS data.
pub struct GarbageCollectorAgent {
    pending_cleanup: VecDeque<(PageIndex, SemanticDomain)>,
    pending_vacuum: VecDeque<(u32, u32)>,
    compaction_lane: CompactionLane,
    current_strategy: StrategyId,
    max_cleanup_per_frame: usize,
    last_cleanup_count: usize,
    frame_count: u64,
    telemetry_sender: Option<Sender<TelemetryEvent>>,
}

impl Agent for GarbageCollectorAgent {
    fn id(&self) -> AgentId {
        AgentId::Ecs
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let pending_count = self.pending_cleanup.len() + self.pending_vacuum.len();
        let urgency_factor = (pending_count as f32 / 100.0).min(5.0);

        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_micros(50),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_micros((100.0 * (1.0 + urgency_factor)) as u64),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_micros((500.0 * (1.0 + urgency_factor)) as u64),
                    estimated_vram: 0,
                },
            ],
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!(
            "GarbageCollectorAgent: Strategy update to {:?}",
            budget.strategy_id,
        );

        self.current_strategy = budget.strategy_id;

        self.max_cleanup_per_frame = match budget.strategy_id {
            StrategyId::LowPower => {
                (DEFAULT_MAX_CLEANUP_PER_FRAME / LOW_POWER_CLEANUP_DIVISOR).max(1)
            }
            StrategyId::Balanced => DEFAULT_MAX_CLEANUP_PER_FRAME,
            StrategyId::HighPerformance => {
                DEFAULT_MAX_CLEANUP_PER_FRAME * HIGH_PERFORMANCE_CLEANUP_MULTIPLIER
            }
            StrategyId::Custom(factor) => (factor as usize).clamp(1, 100),
        };
    }

    fn update(&mut self, context: &mut khora_core::EngineContext<'_>) {
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                self.run(world);
            }
        }
        self.frame_count += 1;
    }

    fn report_status(&self) -> AgentStatus {
        let pending = self.pending_cleanup.len() + self.pending_vacuum.len();
        let health_score = if pending == 0 {
            1.0
        } else if pending < 100 {
            0.8
        } else if pending < 500 {
            0.5
        } else {
            0.2
        };

        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!(
                "pending_cleanup={} pending_vacuum={} last_cleaned={}",
                self.pending_cleanup.len(),
                self.pending_vacuum.len(),
                self.last_cleanup_count,
            ),
        }
    }

    fn execute(&mut self) {
        // GC work is performed in update() via the tactical coordination.
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GarbageCollectorAgent {
    /// Creates a new `GarbageCollectorAgent`.
    pub fn new() -> Self {
        Self {
            pending_cleanup: VecDeque::new(),
            pending_vacuum: VecDeque::new(),
            compaction_lane: CompactionLane::new(),
            current_strategy: StrategyId::Balanced,
            max_cleanup_per_frame: DEFAULT_MAX_CLEANUP_PER_FRAME,
            last_cleanup_count: 0,
            frame_count: 0,
            telemetry_sender: None,
        }
    }

    /// Attaches a DCC sender for telemetry events.
    pub fn with_dcc_sender(mut self, sender: Sender<TelemetryEvent>) -> Self {
        self.telemetry_sender = Some(sender);
        self
    }

    /// Adds a new orphaned data location to the cleanup queue.
    pub fn queue_cleanup(&mut self, page_index: PageIndex, domain: SemanticDomain) {
        self.pending_cleanup.push_back((page_index, domain));
    }

    /// Adds a vacuum request for a page hole.
    pub fn queue_vacuum(&mut self, page_index: u32, hole_row_index: u32) {
        self.pending_vacuum.push_back((page_index, hole_row_index));
    }

    /// Runs the agent's decision-making and execution logic for one frame.
    pub fn run(&mut self, world: &mut World) {
        if self.pending_cleanup.is_empty() && self.pending_vacuum.is_empty() {
            self.last_cleanup_count = 0;
            return;
        }

        let budget = self.max_cleanup_per_frame;

        let items_to_clean: Vec<_> = self
            .pending_cleanup
            .drain(..budget.min(self.pending_cleanup.len()))
            .collect();

        let pages_to_vacuum: Vec<_> = self
            .pending_vacuum
            .drain(..budget.min(self.pending_vacuum.len()))
            .collect();

        let items_count = items_to_clean.len();
        let pages_count = pages_to_vacuum.len();
        self.last_cleanup_count = items_count + pages_count;

        if self.last_cleanup_count == 0 {
            return;
        }

        let work_plan = GcWorkPlan {
            budget,
            items_to_clean,
            pages_to_vacuum,
        };

        self.compaction_lane
            .run(world as &mut dyn WorldMaintenance, &work_plan);

        log::trace!(
            "GarbageCollectorAgent: Cleaned {} items, {} pages vacuumed (strategy={:?})",
            items_count,
            pages_count,
            self.current_strategy,
        );
    }

    /// Returns the total count of pending cleanup items.
    pub fn pending_count(&self) -> usize {
        self.pending_cleanup.len() + self.pending_vacuum.len()
    }

    /// Returns the current strategy.
    pub fn current_strategy(&self) -> StrategyId {
        self.current_strategy
    }

    /// Returns the maximum cleanup operations per frame.
    pub fn max_cleanup_per_frame(&self) -> usize {
        self.max_cleanup_per_frame
    }
}

impl Default for GarbageCollectorAgent {
    fn default() -> Self {
        Self::new()
    }
}
