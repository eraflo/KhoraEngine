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
use std::sync::{Arc, Mutex};

pub use activation::{Activation, State, UnknownFamily};
pub use completion::{AgentCompletionMap, CompletionOutcome};
pub use contention::Contention;
pub use dependency::{AgentDependency, DependencyKind};
pub use execution_phase::ExecutionPhase;
pub use layer::{
    CheckedOut, LastRun, Layer, LayerRun, Place, Preparation, PreparedLayer, Terms, Wake,
    KEEP_EPOCHS,
};
pub use registration::{
    check_registrations, registered_agents, AgentRegistration, BootError, RegisteredAgent,
};
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
    /// `envelope` is `None` when the round could not price the agent: it runs
    /// at its own price, within the pin and the allowed rows.
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: Option<&Envelope>,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    );

    /// The envelope the DCC lent is withdrawn: the family was off and is on
    /// again. The agent drops what it held — its envelope, pin and allowed
    /// rows — and runs at its own price until an allocation lends it another.
    ///
    /// Defaults to doing nothing.
    fn withdraw(&mut self) {}

    /// Reports what the agent ran on its last frame: the row, its time, its
    /// health, and what its lane said of the run.
    fn report(&self) -> Report;

    /// The family's place: where its installed [`Layer`] runs every frame,
    /// the scheduler running it without the agent, and where the events that
    /// wake the agent ([`wake`](Self::wake)) gather.
    ///
    /// The scheduler asks once, when it first meets the agent, and keeps the
    /// place: on a quiet frame it neither calls nor locks the agent.
    fn place(&self) -> Arc<Mutex<Place>>;

    /// Wakes the agent for the events its place gathered, `why`, in
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
    /// Defaults to doing nothing.
    fn wake(&mut self, why: &[Wake], place: &mut Place, context: &mut EngineContext<'_>) {
        let _ = (why, place, context);
    }

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

    /// Declares what this agent competes with the others for: the deck slots it
    /// writes, and the runtime resources it reads or locks.
    ///
    /// It is what the agent reaches when it is woken — its wake runs under
    /// it. No agent reaches the `World`, so it is the only thing an agent
    /// declares. The scheduler forms its waves from the contention of each
    /// family's installed layer, which this answers for the layer running;
    /// two in one
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
}
