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

/// When an agent's family runs in the frame, and how much it matters.
///
/// Whether it runs at all is its family's state in force
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

/// How critical an agent is to the frame, so whether it may be skipped under pressure.
///
/// Only `Optional` is negotiable; the scheduler never drops `Critical` or
/// `Important` work to save frame time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum AgentImportance {
    /// Must execute — non-negotiable, never skipped. Skipping causes errors or corruption.
    Critical,
    /// Should execute — non-negotiable, never skipped.
    Important,
    /// Nice to have — negotiable: the only work the scheduler skips under pressure.
    Optional,
}

impl AgentImportance {
    /// Whether GORNA may skip this work under budget pressure: only `Optional`.
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

    /// **An agent runs in one phase, `OUTPUT` unless it says otherwise.**
    #[test]
    fn the_default_timing_runs_in_output() {
        assert_eq!(ExecutionTiming::default().phase, ExecutionPhase::OUTPUT);
    }
}
