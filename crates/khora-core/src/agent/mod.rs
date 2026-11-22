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
mod after_checks;
mod background_checks;
mod boot_error;
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

/// A family's strategist: it answers GORNA, chooses its row and installs its layers.
///
/// It does no per-frame work: that belongs to [`Lane`](crate::lane::Lane)s.
/// Every family's agent is the one `LaneAgent` of `khora-agents`.
///
/// See the book: concepts/gorna-protocol.md, "The four messages".
pub trait Agent: Send + Sync {
    /// Returns the unique identifier for this agent.
    fn id(&self) -> AgentId;

    /// Offers every row the agent can run, each with its cost, quality and
    /// confidence, and names the floor it falls back to.
    fn offer(&mut self, request: &OfferRequest) -> Offer;

    /// Chooses the row that runs within the envelope, pin and allowed rows lent for `epoch`.
    ///
    /// `envelope` is `None` when the round could not price the agent: it runs
    /// at its own price, within the pin and the allowed rows.
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: Option<&Envelope>,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    );

    /// Drops the envelope, pin and allowed rows lent, as a family switched back on.
    ///
    /// The agent runs at its own price until an allocation lends it another.
    /// Defaults to doing nothing.
    fn withdraw(&mut self) {}

    /// Reports what the agent ran on its last frame.
    fn report(&self) -> Report;

    /// Returns the family's place, where its installed [`Layer`] runs and its wakes gather.
    ///
    /// The scheduler asks once and keeps it: on a quiet frame it neither
    /// calls nor locks the agent.
    fn place(&self) -> Arc<Mutex<Place>>;

    /// Wakes the agent for the events `why` its place gathered, to choose its row again.
    ///
    /// Called after the frame start, once the bus is built, before the waves.
    /// `place` is already locked: the agent must not lock it again. A new row
    /// is [`stage`](Place::stage)d or [`reinstate`](Place::reinstate)d; a new
    /// envelope only updates the place's [`Terms`]. Defaults to doing nothing.
    fn wake(&mut self, why: &[Wake], place: &mut Place, context: &mut EngineContext<'_>) {
        let _ = (why, place, context);
    }

    /// Declares when the agent's family runs in the frame, and how much it matters.
    ///
    /// Defaults to the `OUTPUT` phase, `Important`.
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming::default()
    }

    /// Declares the deck slots the agent reads and writes, and the runtime resources it reads or locks.
    ///
    /// Its wake runs under it: [`EngineContext::resource`](crate::EngineContext::resource)
    /// and [`locked`](crate::EngineContext::locked) refuse what it did not
    /// declare. Declaring more than it touches serialises waves for nothing.
    /// Defaults to nothing.
    ///
    /// See the book: concepts/agents-and-lanes.md, "What an agent declares".
    fn contention(&self) -> Contention {
        Contention::none()
    }
}
