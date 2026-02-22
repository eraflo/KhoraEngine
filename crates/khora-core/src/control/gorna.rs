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

//! Types and traits for the Goal-Oriented Resource Negotiation & Allocation (GORNA) protocol.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Unique identifier for engine agents with implicit priority ordering.
///
/// The order of variants defines the default execution priority (first = highest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum AgentId {
    /// The primary rendering agent (highest priority in Simulation).
    Renderer,
    /// The physics simulation agent.
    Physics,
    /// The ECS/Logic coordination agent.
    Ecs,
    /// The audio processing agent.
    Audio,
    /// The asset management agent (highest priority in Boot).
    Asset,
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Generic strategy identifier for budget allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrategyId {
    /// Minimum resource usage, lowest quality/frequency.
    LowPower,
    /// Balanced resource usage.
    Balanced,
    /// High resource usage, maximum quality/performance.
    HighPerformance,
    /// Custom ID for agent-specific strategies.
    /// Used when the predefined levels aren't sufficient.
    Custom(u32),
}

/// Hard resource constraints the DCC imposes on an Agent during negotiation.
///
/// These represent non-negotiable limits that any proposed strategy must respect.
#[derive(Debug, Clone, Default)]
pub struct ResourceConstraints {
    /// Maximum VRAM usage allowed, in bytes. `None` means unconstrained.
    pub max_vram_bytes: Option<u64>,
    /// Maximum system memory allowed, in bytes. `None` means unconstrained.
    pub max_memory_bytes: Option<u64>,
    /// If `true`, this agent is critical and must always execute (e.g. physics in Simulation).
    pub must_run: bool,
}

/// A request sent by the DCC to an Agent to negotiate resources.
#[derive(Debug, Clone)]
pub struct NegotiationRequest {
    /// The target latency for the frame or subsystem (e.g. 16.6ms).
    pub target_latency: Duration,
    /// Priority weight (0.0 to 1.0) assigned by the DCC.
    pub priority_weight: f32,
    /// Hard resource constraints that any proposed strategy must respect.
    pub constraints: ResourceConstraints,
}

/// A response from an Agent offering various execution strategies.
#[derive(Debug, Clone)]
pub struct NegotiationResponse {
    /// List of available strategies and their estimated costs.
    pub strategies: Vec<StrategyOption>,
}

/// A specific execution strategy offered by an Agent.
#[derive(Debug, Clone)]
pub struct StrategyOption {
    /// Unique identifier for the strategy.
    pub id: StrategyId,
    /// Expected cost in time.
    pub estimated_time: Duration,
    /// Expected cost in VRAM.
    pub estimated_vram: u64,
}

/// An allocated resource budget issued by the DCC to an Agent.
#[derive(Debug, Clone)]
pub struct ResourceBudget {
    /// The strategy ID to be applied.
    pub strategy_id: StrategyId,
    /// Maximum time allowed for execution.
    pub time_limit: Duration,
    /// Maximum VRAM budget in bytes, if constrained.
    pub memory_limit: Option<u64>,
    /// Additional ISA-specific parameters.
    pub extra_params: HashMap<String, String>,
}

/// A snapshot of an Agent's current health and performance.
#[derive(Debug, Clone)]
pub struct AgentStatus {
    /// The ID of the reporting agent.
    pub agent_id: AgentId,
    /// The strategy currently being executed.
    pub current_strategy: StrategyId,
    /// Health score (0.0 to 1.0). 1.0 means adhering perfectly to budget.
    pub health_score: f32,
    /// True if the agent is blocked or failed to execute.
    pub is_stalled: bool,
    /// Human-readable status message for telemetry.
    pub message: String,
}
