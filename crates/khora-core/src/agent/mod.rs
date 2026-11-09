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

pub mod activation;
mod background_checks;
pub mod completion;
pub mod contention;
pub mod dependency;
pub mod execution_phase;
pub mod gorna;
pub mod layer;
pub mod registration;
pub mod timing;

use crate::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use crate::agent::gorna::AgentId;
use crate::agent::gorna::StrategyId;
use crate::EngineContext;
use std::any::Any;
use std::sync::{Arc, Mutex};

pub use activation::{Activation, State, UnknownFamily};
pub use completion::{AgentCompletionMap, CompletionOutcome};
pub use contention::Contention;
pub use dependency::{AgentDependency, DependencyCondition, DependencyKind};
pub use execution_phase::ExecutionPhase;
pub use layer::{
    CheckedOut, LastRun, Layer, LayerRun, Place, Preparation, Prepared, Terms, Wake, KEEP_EPOCHS,
};
pub use registration::{check_registrations, AgentRegistration, BootError};
pub use timing::{AgentImportance, ExecutionTiming};

/// The foundational interface for an Intelligent Subsystem Agent (ISA).
///
/// Each major subsystem (Rendering, Physics, etc.) implements this trait to
/// participate in the engine's dynamic resource negotiation (GORNA).
///
/// # The GORNA protocol
///
/// Every epoch the DCC asks each agent for its [`offer`](Self::offer) — every
/// row it can run, priced — lends it an envelope through
/// [`apply`](Self::apply), and reads what it ran back through
/// [`report`](Self::report). The DCC decides how much; the agent decides how:
/// it chooses the row that fits the envelope.
///
/// An agent holds no business logic beyond choosing its row, answering
/// GORNA and installing its layers. All real work belongs in
/// [`Lane`](crate::lane::Lane) implementations. Every built-in family's
/// agent is the one `LaneAgent` of `khora-agents`.
pub trait Agent: Send + Sync {
    /// Returns the unique identifier for this agent.
    fn id(&self) -> AgentId;

    /// Offers every row the agent can run, each with its cost, quality and
    /// confidence, and names the floor it falls back to.
    fn offer(&mut self, request: &OfferRequest) -> Offer;

    /// Takes the envelope the DCC lent for `epoch`, the row a policy pins, and
    /// the rows a policy allows, and chooses the row that runs within them.
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: &Envelope,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    );

    /// Reports what the agent ran on its last frame: the row, its time, its
    /// health, and what its lane said of the run.
    fn report(&self) -> Report;

    /// The family's place, when the agent runs as **layers**: the scheduler
    /// then runs the place's installed [`Layer`] every frame instead of
    /// [`execute`](Self::execute), and wakes the agent through
    /// [`wake`](Self::wake) only when the place holds a [`Wake`].
    ///
    /// The scheduler asks once, when it first meets the agent, and keeps the
    /// place: on a quiet frame it neither calls nor locks the agent.
    ///
    /// Defaults to `None`: the agent runs through `execute` every frame.
    fn place(&self) -> Option<Arc<Mutex<Place>>> {
        None
    }

    /// Wakes a layered agent for the events its place gathered, `why`, in
    /// the scheduler's event pass: after the frame start, once the bus is
    /// built, before the waves. `place` is the agent's own place, already
    /// locked — the agent must not lock it again.
    ///
    /// The agent quotes and chooses its row. When the row changes it either
    /// prepares a new layer beside the installed one and
    /// [`stage`](Place::stage)s it with the time the preparation took, or
    /// [`reinstate`](Place::reinstate)s a kept layer of that row, preparing
    /// nothing. When only its envelope changed it updates the place's
    /// [`Terms`]. A staged layer is swapped in at the next frame start.
    ///
    /// Defaults to doing nothing, which is right for an agent without a
    /// place.
    fn wake(&mut self, why: &[Wake], place: &mut Place, context: &mut EngineContext<'_>) {
        let _ = (why, place, context);
    }

    /// Called **once** after the agent is registered with the DCC.
    ///
    /// The agent should cache services from `context.services`, initialize
    /// its lane registry, and prepare any persistent state.
    /// Default implementation is a no-op.
    fn on_initialize(&mut self, _context: &mut EngineContext<'_>) {}

    /// Called **every frame** by the engine loop.
    ///
    /// The agent selects the appropriate lanes based on the current GORNA
    /// strategy and runs each one that should run.
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
