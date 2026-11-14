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

use super::dependency::AgentDependency;
use super::execution_phase::ExecutionPhase;

/// Declares when and how an agent should execute within the frame pipeline.
///
/// Each agent provides its timing via `Agent::execution_timing()`. The
/// Scheduler uses this information to order and potentially skip agents
/// based on the current phase, budget, and dependencies.
///
/// Whether an agent runs at all in a frame is its family's state in force
/// ([`Activation`](crate::agent::Activation)), not this struct's.
#[derive(Debug, Clone)]
pub struct ExecutionTiming {
    /// The phase the agent runs in, once a frame: its place's.
    pub phase: ExecutionPhase,
    /// Priority within the phase (higher = executed earlier).
    pub priority: f32,
    /// Importance for budget management.
    pub importance: AgentImportance,
    /// Dependencies on other agents.
    pub dependencies: Vec<AgentDependency>,
}

impl Default for ExecutionTiming {
    fn default() -> Self {
        Self {
            phase: ExecutionPhase::OUTPUT,
            priority: 0.5,
            importance: AgentImportance::Important,
            dependencies: Vec::new(),
        }
    }
}

/// How critical an agent is for frame correctness — and therefore whether GORNA
/// budget arbitration may skip it under pressure.
///
/// `Critical` and `Important` are **non-negotiable**: the scheduler never drops
/// them to save frame time. Work that must be deterministic — physics in a
/// simulation, or editor chrome once it owns a non-negotiable pass — lives here.
/// Only `Optional` work is negotiable (the budget escape valve).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum AgentImportance {
    /// Must execute — non-negotiable, never skipped. Skipping causes errors or corruption.
    Critical,
    /// Should execute — non-negotiable under normal budget pressure (not skipped today).
    Important,
    /// Nice to have — **negotiable**: the first (and currently only) work the
    /// scheduler skips under budget pressure.
    Optional,
}

impl AgentImportance {
    /// Whether GORNA may skip this work under budget pressure. Only `Optional`
    /// is negotiable; `Critical` and `Important` are non-negotiable (always run).
    pub fn is_negotiable(self) -> bool {
        matches!(self, AgentImportance::Optional)
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentImportance, ExecutionPhase, ExecutionTiming};

    #[test]
    fn only_optional_is_negotiable() {
        assert!(!AgentImportance::Critical.is_negotiable());
        assert!(!AgentImportance::Important.is_negotiable());
        assert!(AgentImportance::Optional.is_negotiable());
    }

    /// **An agent runs in one phase, `OUTPUT` unless it says otherwise.** The
    /// default timing names a single phase, the one a frame's output is
    /// recorded in: an agent built with it runs once a frame, not once in
    /// every phase.
    #[test]
    fn the_default_timing_runs_in_output() {
        assert_eq!(ExecutionTiming::default().phase, ExecutionPhase::OUTPUT);
    }
}
