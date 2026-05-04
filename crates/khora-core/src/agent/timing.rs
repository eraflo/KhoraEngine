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

//! Execution timing declarations for agents.

use std::time::Duration;

use super::dependency::AgentDependency;
use super::execution_phase::ExecutionPhase;

/// Declares when and how an agent should execute within the frame pipeline.
///
/// Each agent provides its timing via `Agent::execution_timing()`. The
/// Scheduler uses this information to order and potentially skip agents
/// based on the current phase, budget, and dependencies.
///
/// **Note**: Mode filtering is done at registration time via
/// `DccService::register_agent_for_mode()`, not via this struct.
#[derive(Debug, Clone)]
pub struct ExecutionTiming {
    /// Phases where this agent CAN execute.
    pub allowed_phases: Vec<ExecutionPhase>,
    /// Default phase if GORNA doesn't specify one.
    pub default_phase: ExecutionPhase,
    /// Priority within the phase (higher = executed earlier).
    pub priority: f32,
    /// Importance for budget management.
    pub importance: AgentImportance,
    /// Fixed timestep. If None, executes every frame.
    pub fixed_timestep: Option<Duration>,
    /// Dependencies on other agents.
    pub dependencies: Vec<AgentDependency>,
}

impl Default for ExecutionTiming {
    fn default() -> Self {
        Self {
            allowed_phases: ExecutionPhase::DEFAULT_ORDER.to_vec(),
            default_phase: ExecutionPhase::OUTPUT,
            priority: 0.5,
            importance: AgentImportance::Important,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }
}

/// How critical an agent is for frame correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentImportance {
    /// Must execute. Skipping causes errors or corruption.
    Critical,
    /// Should execute. Skippable if the frame budget is exceeded.
    Important,
    /// Nice to have. First to be skipped under budget pressure.
    Optional,
}
