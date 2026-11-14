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

//! The four messages of GORNA: the DCC asks for offers, each agent offers its
//! rows, the DCC allocates an envelope to each, and each agent reports what it
//! ran.
//!
//! The DCC decides *how much* (an envelope per agent per epoch); the agent
//! decides *how* (its row within the envelope).

use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{AgentId, StrategyId};
use crate::agent::Agent;
use crate::lane::cost::PerDevice;

pub use super::frame_report::FrameReport;
pub use super::offer::{CostBasis, Offer, OfferRow};
pub use super::offer_request::OfferRequest;
pub use super::report::Report;
pub use super::why::{BindingConstraint, DecidedBy, Rejected, Why};

/// One round of negotiation: an allocation holds from its epoch until the next.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Epoch(pub u64);

/// Why the DCC opened an epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trigger {
    /// The first round.
    Initial,
    /// A heuristic reported pressure (thermal, battery, memory, …).
    Pressure,
    /// The cost model forecast the frame over its target.
    ForecastBreach,
    /// The frame governor changed the budget multiplier.
    FrameGovernor,
    /// An agent was registered or unregistered.
    AgentsChanged,
    /// A developer policy changed.
    PolicyChanged,
    /// An agent is stalled and unhealthy.
    AgentStalled,
    /// Agents reported over budget several frames in a row.
    Overruns,
    /// The periodic refresh.
    Refresh,
    /// The emergency: every envelope is its agent's floor.
    Safety,
}

/// What the DCC lends one agent for an epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// The time on each device.
    pub time: PerDevice<Duration>,
    /// The bytes held on each device.
    pub memory: PerDevice<u64>,
}

/// The DCC's decision for an epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Allocation {
    /// The epoch the allocation opens.
    pub epoch: Epoch,
    /// Why it was opened.
    pub trigger: Trigger,
    /// Each agent's envelope.
    pub envelopes: Vec<(AgentId, Envelope)>,
    /// The agents a policy pins to one row.
    pub pins: Vec<(AgentId, StrategyId)>,
    /// The agents a policy limits to some rows.
    pub allowed: Vec<(AgentId, Vec<StrategyId>)>,
    /// The cutoff of the upgrade pass, for information only.
    pub price: Option<f32>,
    /// Whether a lower frame target can take something back: some agent's
    /// plan runs above its floor — an upgrade, or a `Stable` envelope it keeps
    /// — and is not pinned there. The frame governor does not lower the
    /// multiplier without one.
    pub lever: bool,
    /// The agents whose offer came after the request's `reply_by`: each keeps
    /// the envelope it had.
    pub late: Vec<AgentId>,
}

impl Allocation {
    /// Lends `agent` what the allocation names for it — its envelope, its pin
    /// and its allowed rows — through [`Agent::apply`]; whether it named the
    /// agent. An agent it names neither an envelope, a pin nor allowed rows
    /// for is left with what it holds. One named a pin or allowed rows but no
    /// envelope — the round could not price it — is lent no envelope.
    pub fn lend_to(&self, agent: &mut dyn Agent) -> bool {
        let id = agent.id();
        let envelope = (self.envelopes.iter())
            .find(|(named, _)| *named == id)
            .map(|(_, envelope)| envelope);
        let pin = (self.pins.iter())
            .find(|(named, _)| *named == id)
            .map(|(_, row)| *row);
        let allowed = (self.allowed.iter())
            .find(|(named, _)| *named == id)
            .map(|(_, rows)| rows.as_slice());
        if envelope.is_none() && pin.is_none() && allowed.is_none() {
            return false;
        }
        agent.apply(self.epoch, envelope, pin, allowed);
        true
    }
}
