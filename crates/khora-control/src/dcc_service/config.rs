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

//! Configuration of the DCC service.

use crate::gorna::planning::PlanningConfig;
use crate::governor::GovernorConfig;

/// Configuration for the DCC Service.
#[derive(Debug, Clone)]
pub struct DccConfig {
    /// Frequency of the analysis loop in Hz.
    pub tick_rate: u32,
    /// Maximum number of telemetry events to buffer.
    /// If the buffer is full, new events are dropped.
    pub telemetry_buffer_size: usize,
    /// Timeout for acquiring locks on agents during negotiation.
    /// If an agent lock cannot be acquired within this time, the agent is skipped.
    pub agent_lock_timeout_ms: u64,
    /// Optional system-RAM budget in bytes. When set, the DCC derives
    /// [`Context::memory_pressure`](crate::dcc_context::Context::memory_pressure)
    /// from the tracking allocator's live usage and degrades frame budgets as
    /// the ceiling approaches. `None` disables the signal — chiefly useful on
    /// memory-constrained targets.
    pub memory_budget_bytes: Option<u64>,
    /// Optional VRAM budget in bytes, as
    /// [`memory_budget_bytes`](Self::memory_budget_bytes) for system RAM.
    /// When set, a VRAM report makes the available VRAM `budget − current`;
    /// otherwise the device's capacity, if the report carries one, stands in
    /// for the budget.
    pub vram_budget_bytes: Option<u64>,
    /// Tuning for the frame governor that drives
    /// [`Context::global_budget_multiplier`](crate::dcc_context::Context::global_budget_multiplier).
    pub frame_governor: GovernorConfig,
    /// The fewest ticks an epoch lasts before another trigger opens the next
    /// one; a trigger raised sooner waits for it. `Safety` never waits.
    pub min_epoch_ticks: u32,
    /// How many over-budget reports in a row from one agent raise
    /// `Trigger::Overruns`.
    pub overruns_in_a_row: u32,
    /// The planner's settings: the horizon a plan is meant to hold, the
    /// transient and the default preparation a switch costs, the risk of a
    /// frame over its target, and how many frames and reports it reads.
    pub planning: PlanningConfig,
}

impl Default for DccConfig {
    fn default() -> Self {
        Self {
            tick_rate: 20,
            telemetry_buffer_size: 1000,
            agent_lock_timeout_ms: 100,
            memory_budget_bytes: None,
            vram_budget_bytes: None,
            frame_governor: GovernorConfig::default(),
            min_epoch_ticks: 3,
            overruns_in_a_row: 3,
            planning: PlanningConfig::default(),
        }
    }
}
