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

//! Agent dependency system for execution ordering.

use super::mode::EngineMode;
use crate::control::gorna::{AgentId, StrategyId};

/// A dependency declaration from one agent to another.
#[derive(Debug, Clone)]
pub struct AgentDependency {
    /// The agent this one depends on.
    pub target: AgentId,
    /// Type of dependency constraint.
    pub kind: DependencyKind,
    /// Optional condition. If None, the dependency is always active.
    pub condition: Option<DependencyCondition>,
}

/// The type of dependency constraint.
#[derive(Debug, Clone)]
pub enum DependencyKind {
    /// Must execute BEFORE this agent. If target is skipped, this agent is also skipped.
    Hard,
    /// Prefers to execute after the target, but can execute without it.
    Soft,
    /// Can execute in parallel (same phase). No ordering constraint.
    Parallel,
}

/// A condition that must be met for a dependency to be active.
#[derive(Debug, Clone)]
pub enum DependencyCondition {
    /// Only depends on the target if the target is active this frame.
    IfTargetActive,
    /// Only depends on the target if the current budget >= this strategy.
    IfBudgetAbove(StrategyId),
    /// Only depends on the target in this engine mode.
    IfEngineMode(EngineMode),
}
