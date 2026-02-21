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

use crate::control::gorna::AgentId;
use crate::control::gorna::{AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget};
use crate::EngineContext;
use std::any::Any;

/// The foundational interface for an Intelligent Subsystem Agent (ISA).
///
/// Each major subsystem (Rendering, Physics, etc.) implements this trait to
/// participate in the engine's dynamic resource negotiation (GORNA).
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

    /// Periodically updates the agent's internal state.
    fn update(&mut self, context: &mut EngineContext<'_>);

    /// Reports the current status and health of the agent.
    fn report_status(&self) -> AgentStatus;

    /// Executes the agent's primary function for this frame.
    /// Called after update(), this is where the agent performs its main work.
    fn execute(&mut self);

    /// Allows downcasting to concrete agent types.
    fn as_any(&self) -> &dyn Any;

    /// Allows mutable downcasting to concrete agent types.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
