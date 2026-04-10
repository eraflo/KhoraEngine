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

//! Traits for autonomous engine subsystems (Agents).

pub mod dependency;
pub mod execution_phase;
pub mod mode;
pub mod timing;

use crate::control::gorna::AgentId;
use crate::control::gorna::{AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget};
use crate::EngineContext;
use std::any::Any;

pub use dependency::{AgentDependency, DependencyCondition, DependencyKind};
pub use execution_phase::ExecutionPhase;
pub use mode::EngineMode;
pub use timing::{AgentImportance, ExecutionTiming};

/// The foundational interface for an Intelligent Subsystem Agent (ISA).
///
/// Each major subsystem (Rendering, Physics, etc.) implements this trait to
/// participate in the engine's dynamic resource negotiation (GORNA).
///
/// # Lifecycle
///
/// 1. `on_initialize(ctx)` — called **once** after registration. The agent
///    caches required services and initializes its lanes.
/// 2. `execute(ctx)` — called **every frame**. The agent selects the appropriate
///    lanes based on the current GORNA budget and dispatches their `Lane::execute()`.
/// 3. `negotiate(request)` / `apply_budget(budget)` — called by the GORNA
///    arbitrator on the DCC background thread when the system re-evaluates strategy.
/// 4. `report_status()` — polled by GORNA for health monitoring.
///
/// An agent must contain **no business logic** beyond lane selection, budget
/// negotiation, and lane dispatch. All real work belongs in [`Lane`] implementations.
pub trait Agent: Send + Sync {
    /// Returns the unique identifier for this agent.
    fn id(&self) -> AgentId;

    /// Negotiates with the DCC to determine the best execution strategy
    /// given the current global resource constraints and priorities.
    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse;

    /// Applies a resource budget issued by the DCC.
    /// The agent must adjust its internal logic (e.g., LOD, quality settings)
    /// to stay within the allocated limits.
    fn apply_budget(&mut self, budget: ResourceBudget);

    /// Reports the current status and health of the agent.
    fn report_status(&self) -> AgentStatus;

    /// Called **once** after the agent is registered with the DCC.
    ///
    /// The agent should cache services from `context.services`, initialize
    /// its lane registry, and prepare any persistent state.
    /// Default implementation is a no-op.
    fn on_initialize(&mut self, _context: &mut EngineContext<'_>) {}

    /// Called **every frame** by the engine loop.
    ///
    /// The agent selects the appropriate lanes based on the current GORNA
    /// strategy, builds a [`LaneContext`](crate::lane::LaneContext), and dispatches
    /// [`Lane::execute()`](crate::lane::Lane::execute) for each lane that should run.
    fn execute(&mut self, context: &mut EngineContext<'_>);

    /// Declares WHEN and HOW this agent should execute within the frame pipeline.
    ///
    /// The Scheduler uses this information to filter agents by phase, order them
    /// by priority, and skip optional agents under budget pressure.
    ///
    /// Default implementation returns a timing that allows execution in all phases
    /// with `Important` priority.
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming::default()
    }

    /// Allows downcasting to concrete agent types.
    fn as_any(&self) -> &dyn Any;

    /// Allows mutable downcasting to concrete agent types.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
