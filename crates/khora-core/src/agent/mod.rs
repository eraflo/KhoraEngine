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

pub mod completion;
pub mod contention;
pub mod dependency;
pub mod execution_phase;
pub mod mode;
pub mod timing;

use crate::control::gorna::AgentId;
use crate::control::gorna::{AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget};
use crate::EngineContext;
use std::any::Any;

pub use completion::{AgentCompletionMap, AgentDone, CompletionOutcome};
pub use contention::Contention;
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
///
/// # Examples
///
/// A skeleton agent. Real agents cache their lanes in `on_initialize` and select
/// one per frame in `execute` based on the budget applied via `apply_budget`.
/// (Marked `ignore` because a compiling impl needs the GORNA request/response
/// types and an [`EngineContext`] from the running engine.)
///
/// ```ignore
/// use khora_core::agent::{Agent, ExecutionTiming};
/// use khora_core::control::gorna::{
///     AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget,
/// };
/// use khora_core::EngineContext;
/// use std::any::Any;
///
/// #[derive(Default)]
/// struct MyAgent;
///
/// impl Agent for MyAgent {
///     fn id(&self) -> AgentId { AgentId::Renderer }
///
///     fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
///         // Propose a strategy that fits `request`'s constraints.
///         NegotiationResponse::default()
///     }
///
///     fn apply_budget(&mut self, _budget: ResourceBudget) {
///         // Adjust quality / LOD to stay within the allocated budget.
///     }
///
///     fn report_status(&self) -> AgentStatus { AgentStatus::default() }
///
///     fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
///         // Pick a lane for this frame and dispatch `Lane::execute`.
///     }
///
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
/// }
/// ```
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

    /// Declares this agent's data-access footprint during [`execute`](Self::execute),
    /// so the scheduler can decide whether it is safe to run concurrently with
    /// other agents in the same phase.
    ///
    /// Defaults to [`AgentAccess::Exclusive`] — the safe fallback: the agent is
    /// assumed to need exclusive `&mut World`, so it runs serially. An agent
    /// overrides this to [`AgentAccess::Isolated`] **only** if `execute` provably
    /// touches no `World` and no shared mutable engine resource (it reads the
    /// `LaneBus` and writes solely its own `OutputDeck`).
    fn access(&self) -> AgentAccess {
        AgentAccess::Exclusive
    }

    /// Declares what this agent competes with the others for: the deck slots it
    /// writes, and the runtime resources it reads or locks.
    ///
    /// The scheduler reads it at wave-formation time. Two agents in one
    /// concurrent wave must not write the same deck slot (their private shards
    /// would collide at the merge), must not lock the same resource (they would
    /// serialise on it), and must not have one locking what another reads.
    /// Reading the same thing is fine — that is the point of saying so.
    ///
    /// It is also **enforced**, not merely compared: an agent reaches a runtime
    /// resource through [`EngineContext::resource`](crate::EngineContext::resource)
    /// or [`locked`](crate::EngineContext::locked), which refuse what this did
    /// not declare.
    ///
    /// Defaults to nothing, which is right for an agent that touches only its
    /// own state and the bus.
    fn contention(&self) -> Contention {
        Contention::none()
    }

    /// Allows downcasting to concrete agent types.
    fn as_any(&self) -> &dyn Any;

    /// Allows mutable downcasting to concrete agent types.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// An agent's data-access footprint during [`Agent::execute`], used by the
/// scheduler's parallel executor to decide which agents may run concurrently.
///
/// The default ([`Exclusive`](Self::Exclusive)) is the safe fallback: the agent
/// may touch the ECS `World` mutably (or a shared mutable resource), so it must
/// run serially. Only an agent that provably confines itself to reading the
/// `LaneBus` and writing its own `OutputDeck` should declare
/// [`Isolated`](Self::Isolated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentAccess {
    /// Needs exclusive `&mut World` (or mutates a shared resource); runs
    /// serially. The safe default.
    #[default]
    Exclusive,
    /// Touches no `World` and writes only its own `OutputDeck` — no shared
    /// mutable engine resources. Eligible for concurrent execution: the
    /// scheduler runs it with [`WorldAccess::None`](crate::WorldAccess::None)
    /// and a private deck shard, folded back into the shared deck after the
    /// concurrent wave. Any number may run in the same wave.
    Isolated,
    /// Reads the `World` immutably, never mutates it. The scheduler runs it
    /// with a shared [`WorldAccess::Shared`](crate::WorldAccess::Shared)
    /// reference, inline on the calling thread so the `World` never crosses a
    /// thread boundary.
    ///
    /// Any number may share a wave, provided their
    /// [`Contention`](super::Contention)s are disjoint. That used to be capped
    /// at one, because a `SharedWorld` agent "may write shared resources" and
    /// nothing said *which* — the cap stood in for an answer nobody could give.
    /// Now they say, so the cap is gone.
    SharedWorld,
}
